//! Tests for 4-step durability protocol, sibling temporary file commits, and fsync.

use camino::Utf8PathBuf;
use std::fs;
use tempfile::TempDir;
use woderrise_core::models::Catalog;
use woderrise_core::schema::{
    read_json_file, write_bytes_durably, write_json_file_durably,
};

#[test]
fn test_write_json_file_durably_creates_and_syncs() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let target_file = base.join("nested/dir/catalog.json");

    let catalog = Catalog::default();

    // 1. Write durably
    write_json_file_durably(&target_file, &catalog).expect("Failed to write catalog durably");

    // 2. Target file must exist
    assert!(target_file.exists());

    // 3. Parent directory must contain NO leftover .tmp files
    let parent = target_file.parent().unwrap();
    for entry in fs::read_dir(parent).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".tmp."),
            "Found unexpected leftover tmp file: {}",
            name
        );
    }

    // 4. Read back and verify equality
    let read_back: Catalog = read_json_file(&target_file).expect("Failed to read back catalog");
    assert_eq!(catalog.version, read_back.version);
    assert_eq!(catalog.repositories.len(), read_back.repositories.len());
}

#[test]
fn test_write_bytes_durably_creates_and_syncs() {
    let temp = TempDir::new().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let target_file = base.join("bytes_test/data.bin");

    let payload = b"Woderrise 4-phase durability test payload";
    write_bytes_durably(&target_file, payload).expect("Failed to write bytes durably");

    assert!(target_file.exists());
    let content = fs::read(&target_file).unwrap();
    assert_eq!(content, payload);

    // Overwrite with new content
    let new_payload = b"Completely new payload replacing old data atomically";
    write_bytes_durably(&target_file, new_payload).expect("Failed to overwrite durably");

    let new_content = fs::read(&target_file).unwrap();
    assert_eq!(new_content, new_payload);
}

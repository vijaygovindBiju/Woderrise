use std::{
    fs,
    path::PathBuf,
    time::Instant,
};
use woderrise_core::{
    compute_line_diff, sha256_bytes, sha256_file, unified_diff, BackupManager, Database,
    DiffLineKind, FileDiffAction, InstallationEngine,
};

fn temp_root(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("woderrise-chal-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn test_myers_diff_empty_and_single_char() {
    // Both empty
    let diff = compute_line_diff("", "");
    assert!(diff.is_empty(), "Empty strings should produce empty diff");

    // Old empty, new non-empty
    let diff = compute_line_diff("", "single line");
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].kind, DiffLineKind::Addition);
    assert_eq!(diff[0].left_line_no, None);
    assert_eq!(diff[0].right_line_no, Some(1));
    assert_eq!(diff[0].text, "single line");

    // Old non-empty, new empty
    let diff = compute_line_diff("single line", "");
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].kind, DiffLineKind::Deletion);
    assert_eq!(diff[0].left_line_no, Some(1));
    assert_eq!(diff[0].right_line_no, None);
    assert_eq!(diff[0].text, "single line");

    // Identical single line
    let diff = compute_line_diff("hello", "hello");
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].kind, DiffLineKind::Context);
    assert_eq!(diff[0].left_line_no, Some(1));
    assert_eq!(diff[0].right_line_no, Some(1));
    assert_eq!(diff[0].text, "hello");

    // Single character modification
    let diff = compute_line_diff("a", "b");
    assert_eq!(diff.len(), 2);
    assert_eq!(diff[0].kind, DiffLineKind::Deletion);
    assert_eq!(diff[0].text, "a");
    assert_eq!(diff[1].kind, DiffLineKind::Addition);
    assert_eq!(diff[1].text, "b");

    // Multiple character changes on same line
    let diff = compute_line_diff("word1 word2 word3", "word1 edited word3");
    assert_eq!(diff.len(), 2);
    assert_eq!(diff[0].kind, DiffLineKind::Deletion);
    assert_eq!(diff[0].text, "word1 word2 word3");
    assert_eq!(diff[1].kind, DiffLineKind::Addition);
    assert_eq!(diff[1].text, "word1 edited word3");
}

#[test]
fn test_myers_diff_identical_and_completely_disjoint() {
    // 1000 Identical lines
    let mut identical_lines = Vec::new();
    for i in 0..1000 {
        identical_lines.push(format!("configuration_entry_key_{i} = value_{i}"));
    }
    let identical_text = identical_lines.join("\n");
    let start = Instant::now();
    let diff = compute_line_diff(&identical_text, &identical_text);
    let duration = start.elapsed();
    assert_eq!(diff.len(), 1000);
    assert!(diff.iter().all(|l| l.kind == DiffLineKind::Context));
    for (i, line) in diff.iter().enumerate() {
        assert_eq!(line.left_line_no, Some(i + 1));
        assert_eq!(line.right_line_no, Some(i + 1));
    }
    println!("1000 identical lines diff elapsed: {:?}", duration);

    // 800 Completely disjoint lines (worst-case Myers search)
    let mut left_lines = Vec::new();
    let mut right_lines = Vec::new();
    for i in 0..800 {
        left_lines.push(format!("old_entry_{i}"));
        right_lines.push(format!("new_entry_{i}"));
    }
    let left_text = left_lines.join("\n");
    let right_text = right_lines.join("\n");
    let start = Instant::now();
    let diff = compute_line_diff(&left_text, &right_text);
    let duration = start.elapsed();
    assert_eq!(diff.len(), 1600);
    assert_eq!(
        diff.iter().filter(|l| l.kind == DiffLineKind::Deletion).count(),
        800
    );
    assert_eq!(
        diff.iter().filter(|l| l.kind == DiffLineKind::Addition).count(),
        800
    );
    println!("800 disjoint lines diff elapsed: {:?}", duration);
    assert!(
        duration.as_secs() < 3,
        "Disjoint diff took too long: {:?}",
        duration
    );
}

#[test]
fn test_myers_diff_trailing_newlines_and_crlf() {
    // Trailing newline vs no trailing newline
    let with_newline = "alpha\nbeta\n";
    let without_newline = "alpha\nbeta";
    let diff = compute_line_diff(with_newline, without_newline);
    assert_eq!(diff.len(), 2);
    assert!(diff.iter().all(|l| l.kind == DiffLineKind::Context));

    // CRLF vs LF
    let crlf = "window_manager: hyprland\r\ngaps_in: 5\r\ngaps_out: 10\r\n";
    let lf = "window_manager: hyprland\ngaps_in: 5\ngaps_out: 10\n";
    let diff = unified_diff(crlf, lf);
    assert_eq!(diff.len(), 3);
    assert!(diff.iter().all(|l| l.kind == DiffLineKind::Context));

    // Mixed CRLF and LF with an addition
    let old_mixed = "line1\r\nline2\nline3\r\n";
    let new_mixed = "line1\nline2\r\nline_inserted\nline3\n";
    let diff = unified_diff(old_mixed, new_mixed);
    assert_eq!(diff.len(), 4);
    assert_eq!(diff[0].kind, DiffLineKind::Context);
    assert_eq!(diff[0].text, "line1");
    assert_eq!(diff[1].kind, DiffLineKind::Context);
    assert_eq!(diff[1].text, "line2");
    assert_eq!(diff[2].kind, DiffLineKind::Addition);
    assert_eq!(diff[2].text, "line_inserted");
    assert_eq!(diff[3].kind, DiffLineKind::Context);
    assert_eq!(diff[3].text, "line3");

    // Empty lines and consecutive newlines
    let empty_lines_old = "first\n\nthird\n";
    let empty_lines_new = "first\nsecond\nthird\n";
    let diff = compute_line_diff(empty_lines_old, empty_lines_new);
    assert_eq!(diff.len(), 4);
    assert_eq!(diff[0].kind, DiffLineKind::Context);
    assert_eq!(diff[0].text, "first");
    assert_eq!(diff[1].kind, DiffLineKind::Deletion);
    assert_eq!(diff[1].text, "");
    assert_eq!(diff[2].kind, DiffLineKind::Addition);
    assert_eq!(diff[2].text, "second");
    assert_eq!(diff[3].kind, DiffLineKind::Context);
    assert_eq!(diff[3].text, "third");
}

#[test]
fn test_myers_diff_unicode_and_special_chars() {
    let old_unicode = "hyprland_config:\n  title: '🦀 Rust Hyprland 🚀'\n  comment: '日本語テスト'\n  rtl: 'مرحبا'";
    let new_unicode = "hyprland_config:\n  title: '🦀 Rust Hyprland ✨ 🚀'\n  comment: '日本語テスト'\n  rtl: 'مرحبا بالعالم'";

    let diff = compute_line_diff(old_unicode, new_unicode);
    assert_eq!(diff.len(), 6);
    assert_eq!(diff[0].kind, DiffLineKind::Context);
    assert_eq!(diff[1].kind, DiffLineKind::Deletion);
    assert_eq!(diff[1].text, "  title: '🦀 Rust Hyprland 🚀'");
    assert_eq!(diff[2].kind, DiffLineKind::Addition);
    assert_eq!(diff[2].text, "  title: '🦀 Rust Hyprland ✨ 🚀'");
    assert_eq!(diff[3].kind, DiffLineKind::Context);
    assert_eq!(diff[4].kind, DiffLineKind::Deletion);
    assert_eq!(diff[5].kind, DiffLineKind::Addition);

    // Text containing null bytes inside UTF-8
    let null_old = "config_key_1\0old\nline2";
    let null_new = "config_key_1\0new\nline2";
    let diff = compute_line_diff(null_old, null_new);
    assert_eq!(diff.len(), 3);
    assert_eq!(diff[0].kind, DiffLineKind::Deletion);
    assert_eq!(diff[1].kind, DiffLineKind::Addition);
    assert_eq!(diff[2].kind, DiffLineKind::Context);
}

#[test]
fn test_myers_diff_multi_megabyte_stress() {
    // Construct a ~3MB text file with 30,000 lines
    let mut old_lines = Vec::with_capacity(30000);
    let mut new_lines = Vec::with_capacity(30000);

    let padding = "A".repeat(90);
    for i in 0..30000 {
        if i == 15000 {
            old_lines.push(format!("entry_{i} = old_val_{padding}"));
            new_lines.push(format!("entry_{i} = new_val_{padding}"));
        } else {
            let line = format!("entry_{i} = standard_val_{padding}");
            old_lines.push(line.clone());
            new_lines.push(line);
        }
    }

    let old_text = old_lines.join("\n");
    let new_text = new_lines.join("\n");
    println!("Text sizes: old = {} bytes, new = {} bytes", old_text.len(), new_text.len());
    assert!(old_text.len() > 3_000_000);

    let start = Instant::now();
    let diff = compute_line_diff(&old_text, &new_text);
    let duration = start.elapsed();
    println!("3MB trimmed diff elapsed: {:?}", duration);

    assert_eq!(diff.len(), 30001);
    assert_eq!(diff[15000].kind, DiffLineKind::Deletion);
    assert_eq!(diff[15001].kind, DiffLineKind::Addition);
    assert!(
        duration.as_millis() < 800,
        "3MB file diff took too long: {:?}",
        duration
    );
}

#[test]
fn test_compute_plan_zero_files() {
    let pkg_dir = temp_root("plan-zero-files");
    let home = pkg_dir.join("home");
    fs::create_dir_all(&home).unwrap();

    let manifest = r#"schema_version: '1.0.0'
package:
  id: com.test.empty
  name: Empty Package
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: package with no files
install:
  files: []
"#;
    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

    let db = Database::open(pkg_dir.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(pkg_dir.join("backups")).unwrap(),
        home,
    };

    let plan = engine.compute_plan(&pkg_dir).unwrap();
    assert_eq!(plan.package_id, "com.test.empty");
    assert_eq!(plan.version, "1.0.0");
    assert!(plan.file_diffs.is_empty());
    assert_eq!(plan.will_backup_count, 0);
    assert!(!plan.has_changes());

    let _ = fs::remove_dir_all(pkg_dir);
}

#[test]
fn test_compute_plan_binary_data_handling() {
    let pkg_dir = temp_root("plan-binary");
    let home = pkg_dir.join("home");
    fs::create_dir_all(&home).unwrap();

    // 1. Non-UTF8 binary data (contains invalid UTF-8 bytes like 0xFF, 0xFE)
    let binary_data_1 = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0xFE, 0x00];
    let binary_data_2 = vec![0x7F, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00, 0xFF, 0xFF];
    // 1MB binary blob with all zeros and non-utf8 markers
    let mut binary_data_3 = vec![0x00u8; 1_000_000];
    binary_data_3[500] = 0xFF;
    binary_data_3[501] = 0xFE;

    // File 1: Added binary
    fs::write(pkg_dir.join("image.png"), &binary_data_1).unwrap();
    let h1 = sha256_bytes(&binary_data_1);

    // File 2: Existing identical binary
    let target_bin2 = home.join(".local/bin/binary2");
    fs::create_dir_all(target_bin2.parent().unwrap()).unwrap();
    fs::write(&target_bin2, &binary_data_2).unwrap();
    fs::write(pkg_dir.join("binary2"), &binary_data_2).unwrap();
    let h2 = sha256_bytes(&binary_data_2);

    // File 3: Existing modified large binary
    let target_bin3 = home.join(".local/bin/binary3");
    let old_data_3 = vec![0xFF, 0x00, 0x11, 0x22];
    fs::write(&target_bin3, &old_data_3).unwrap();
    let old_h3 = sha256_bytes(&old_data_3);
    fs::write(pkg_dir.join("binary3"), &binary_data_3).unwrap();
    let h3 = sha256_bytes(&binary_data_3);

    let manifest = format!(
        r#"schema_version: '1.0.0'
package:
  id: com.test.binary
  name: Binary Test
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: binary test
artifacts:
  - path: image.png
    sha256: {h1}
  - path: binary2
    sha256: {h2}
  - path: binary3
    sha256: {h3}
install:
  files:
    - source: image.png
      target: .local/share/image.png
      backup_before_write: true
    - source: binary2
      target: .local/bin/binary2
      backup_before_write: true
    - source: binary3
      target: .local/bin/binary3
      backup_before_write: true
"#
    );
    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

    let backups_dir = pkg_dir.join("backups");
    let db = Database::open(pkg_dir.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups_dir).unwrap(),
        home,
    };

    let plan = engine.compute_plan(&pkg_dir).unwrap();
    assert_eq!(plan.file_diffs.len(), 3);
    assert_eq!(plan.will_backup_count, 2);

    // Check File 1 (New binary)
    let f1 = &plan.file_diffs[0];
    assert_eq!(f1.action, FileDiffAction::Create);
    assert_eq!(f1.planned_backup_path, None);
    assert_eq!(f1.lines.len(), 1);
    assert_eq!(f1.lines[0].text, "[binary artifact added]");

    // Check File 2 (Identical binary)
    let f2 = &plan.file_diffs[1];
    assert_eq!(f2.action, FileDiffAction::Unchanged);
    assert_eq!(f2.planned_backup_path, Some(backups_dir.join(&h2)));
    assert_eq!(f2.lines.len(), 1);
    assert_eq!(f2.lines[0].text, "[binary artifacts identical]");

    // Check File 3 (Modified binary)
    let f3 = &plan.file_diffs[2];
    assert_eq!(f3.action, FileDiffAction::Modify);
    assert_eq!(f3.planned_backup_path, Some(backups_dir.join(&old_h3)));
    assert_eq!(f3.lines.len(), 1);
    assert_eq!(f3.lines[0].text, "[binary artifacts modified]");

    let _ = fs::remove_dir_all(pkg_dir);
}

#[test]
fn test_compute_plan_mixed_states_and_backup_flags() {
    let pkg_dir = temp_root("plan-mixed-flags");
    let home = pkg_dir.join("home");
    let backups_dir = pkg_dir.join("backups");
    fs::create_dir_all(&home).unwrap();

    // 1. Existing file modified, backup = true
    let t1 = home.join(".config/f1.txt");
    fs::create_dir_all(t1.parent().unwrap()).unwrap();
    fs::write(&t1, b"t1_old").unwrap();
    let t1_hash = sha256_file(&t1).unwrap();
    fs::write(pkg_dir.join("f1.txt"), b"t1_new").unwrap();
    let h1 = sha256_file(pkg_dir.join("f1.txt")).unwrap();

    // 2. Existing file modified, backup = false
    let t2 = home.join(".config/f2.txt");
    fs::write(&t2, b"t2_old").unwrap();
    fs::write(pkg_dir.join("f2.txt"), b"t2_new").unwrap();
    let h2 = sha256_file(pkg_dir.join("f2.txt")).unwrap();

    // 3. Existing file unchanged, backup = true
    let t3 = home.join(".config/f3.txt");
    fs::write(&t3, b"t3_same").unwrap();
    let t3_hash = sha256_file(&t3).unwrap();
    fs::write(pkg_dir.join("f3.txt"), b"t3_same").unwrap();
    let h3 = sha256_file(pkg_dir.join("f3.txt")).unwrap();

    // 4. Existing file unchanged, backup = false
    let t4 = home.join(".config/f4.txt");
    fs::write(&t4, b"t4_same").unwrap();
    fs::write(pkg_dir.join("f4.txt"), b"t4_same").unwrap();
    let h4 = sha256_file(pkg_dir.join("f4.txt")).unwrap();

    // 5. Missing file created, backup = true
    let t5 = home.join(".config/f5.txt");
    assert!(!t5.exists());
    fs::write(pkg_dir.join("f5.txt"), b"t5_created").unwrap();
    let h5 = sha256_file(pkg_dir.join("f5.txt")).unwrap();

    // 6. Missing file created, backup = false
    let t6 = home.join(".config/f6.txt");
    assert!(!t6.exists());
    fs::write(pkg_dir.join("f6.txt"), b"t6_created").unwrap();
    let h6 = sha256_file(pkg_dir.join("f6.txt")).unwrap();

    let manifest = format!(
        r#"schema_version: '1.0.0'
package:
  id: com.test.mixed
  name: Mixed Test
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: mixed test
artifacts:
  - path: f1.txt
    sha256: {h1}
  - path: f2.txt
    sha256: {h2}
  - path: f3.txt
    sha256: {h3}
  - path: f4.txt
    sha256: {h4}
  - path: f5.txt
    sha256: {h5}
  - path: f6.txt
    sha256: {h6}
install:
  files:
    - source: f1.txt
      target: .config/f1.txt
      backup_before_write: true
    - source: f2.txt
      target: .config/f2.txt
      backup_before_write: false
    - source: f3.txt
      target: .config/f3.txt
      backup_before_write: true
    - source: f4.txt
      target: .config/f4.txt
      backup_before_write: false
    - source: f5.txt
      target: .config/f5.txt
      backup_before_write: true
    - source: f6.txt
      target: .config/f6.txt
      backup_before_write: false
"#
    );
    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

    let db = Database::open(pkg_dir.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups_dir).unwrap(),
        home,
    };

    let plan = engine.compute_plan(&pkg_dir).unwrap();
    assert_eq!(plan.file_diffs.len(), 6);
    assert_eq!(plan.will_backup_count, 2);

    // f1: modified, backup Some
    assert_eq!(plan.file_diffs[0].action, FileDiffAction::Modify);
    assert_eq!(
        plan.file_diffs[0].planned_backup_path,
        Some(backups_dir.join(&t1_hash))
    );
    assert!(plan.file_diffs[0].has_changes());

    // f2: modified, backup None
    assert_eq!(plan.file_diffs[1].action, FileDiffAction::Modify);
    assert_eq!(plan.file_diffs[1].planned_backup_path, None);
    assert!(plan.file_diffs[1].has_changes());

    // f3: unchanged, backup Some
    assert_eq!(plan.file_diffs[2].action, FileDiffAction::Unchanged);
    assert_eq!(
        plan.file_diffs[2].planned_backup_path,
        Some(backups_dir.join(&t3_hash))
    );
    assert!(!plan.file_diffs[2].has_changes());

    // f4: unchanged, backup None
    assert_eq!(plan.file_diffs[3].action, FileDiffAction::Unchanged);
    assert_eq!(plan.file_diffs[3].planned_backup_path, None);
    assert!(!plan.file_diffs[3].has_changes());

    // f5: created, backup None
    assert_eq!(plan.file_diffs[4].action, FileDiffAction::Create);
    assert_eq!(plan.file_diffs[4].planned_backup_path, None);
    assert!(plan.file_diffs[4].has_changes());

    // f6: created, backup None
    assert_eq!(plan.file_diffs[5].action, FileDiffAction::Create);
    assert_eq!(plan.file_diffs[5].planned_backup_path, None);
    assert!(plan.file_diffs[5].has_changes());

    assert!(plan.has_changes());

    let _ = fs::remove_dir_all(pkg_dir);
}

#[test]
fn test_planned_backup_matches_actual_installed_snapshot() {
    let pkg_dir = temp_root("plan-vs-actual-backup");
    let home = pkg_dir.join("home");
    let data = pkg_dir.join("data");
    let backups_dir = data.join("backups");
    fs::create_dir_all(&home).unwrap();

    let target = home.join(".config/real_file.conf");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let original_bytes = b"original_content_before_installation\nkey: value\n";
    fs::write(&target, original_bytes).unwrap();
    let expected_hash = sha256_bytes(original_bytes);

    let new_bytes = b"new_installed_content\nkey: updated_value\n";
    fs::write(pkg_dir.join("payload.conf"), new_bytes).unwrap();
    let new_hash = sha256_bytes(new_bytes);

    let manifest = format!(
        r#"schema_version: '1.0.0'
package:
  id: com.test.verify.backup
  name: Verify Backup
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: test
artifacts:
  - path: payload.conf
    sha256: {new_hash}
install:
  files:
    - source: payload.conf
      target: .config/real_file.conf
      backup_before_write: true
"#
    );
    fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

    let db = Database::open(data.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(&backups_dir).unwrap(),
        home: home.clone(),
    };

    // 1. Predict backup path via compute_plan
    let plan = engine.compute_plan(&pkg_dir).unwrap();
    let planned_path = plan.file_diffs[0].planned_backup_path.clone().unwrap();
    assert_eq!(planned_path, backups_dir.join(&expected_hash));

    // 2. Perform actual installation
    let txn = engine.install(&pkg_dir, "test").unwrap();
    assert!(!txn.is_empty());

    // 3. Verify that the planned backup path actually exists on disk with the original content
    assert!(planned_path.exists(), "Planned backup path must exist after install");
    let backed_up_bytes = fs::read(&planned_path).unwrap();
    assert_eq!(backed_up_bytes, original_bytes);

    // 4. Verify DB record matches
    let db_storage: String = engine
        .db
        .conn
        .query_row(
            "SELECT storage_path FROM backup_snapshots WHERE original_path LIKE '%real_file.conf'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(PathBuf::from(db_storage), planned_path);

    let _ = fs::remove_dir_all(pkg_dir);
}

#[test]
fn test_compute_plan_error_and_safety() {
    let pkg_dir = temp_root("plan-errors");
    let home = pkg_dir.join("home");
    fs::create_dir_all(&home).unwrap();

    let db = Database::open(pkg_dir.join("journal.db")).unwrap();
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(pkg_dir.join("backups")).unwrap(),
        home: home.clone(),
    };

    // 1. Non-existent directory/manifest
    assert!(engine.compute_plan(pkg_dir.join("missing_dir")).is_err());

    // 2. Corrupt YAML
    fs::write(pkg_dir.join("woderrise.yaml"), "not: valid: yaml: :").unwrap();
    assert!(engine.compute_plan(&pkg_dir).is_err());

    // 3. Manifest missing declared source file
    let manifest_missing_source = r#"schema_version: '1.0.0'
package:
  id: com.test.missing
  name: Missing Source
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: missing source
install:
  files:
    - source: non_existent_file.conf
      target: .config/app.conf
"#;
    fs::write(pkg_dir.join("woderrise.yaml"), manifest_missing_source).unwrap();
    assert!(engine.compute_plan(&pkg_dir).is_err());

    // 4. Manifest with unsafe target traversing upwards
    let manifest_unsafe = r#"schema_version: '1.0.0'
package:
  id: com.test.unsafe
  name: Unsafe Target
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: unsafe target
install:
  files:
    - source: valid.conf
      target: ../../etc/shadow
"#;
    fs::write(pkg_dir.join("valid.conf"), "data").unwrap();
    fs::write(pkg_dir.join("woderrise.yaml"), manifest_unsafe).unwrap();
    let res = engine.compute_plan(&pkg_dir);
    assert!(res.is_err(), "Unsafe path traversal must be rejected");

    let _ = fs::remove_dir_all(pkg_dir);
}

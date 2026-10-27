mod common;

use common::TestEnv;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use woderrise_core::models::{ConfigComponent, ConflictStatus, OperationType};
use woderrise_detect::TreeEntry;
use woderrise_transaction::{
    AtomicSwapper, BackupSnapshotEngine, SecurityAuditor, StagingWorkspace,
};

#[test]
fn test_pre_install_backup_creates_exact_snapshot() {
    let env = TestEnv::new();
    let tx_id = "tx_backup_001";

    let target_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&target_dir).expect("Create target dir");
    let target_file = target_dir.join("hyprland.conf");

    let original_bytes = b"# Original Hyprland Config\nexec-once = waybar\n";
    let mut f = File::create(&target_file).expect("Create target file");
    f.write_all(original_bytes).expect("Write original bytes");
    f.sync_all().expect("Sync original bytes");

    let mut hasher = Sha256::new();
    hasher.update(original_bytes);
    let original_sha = format!("{:x}", hasher.finalize());

    // Stage new version
    let staging = StagingWorkspace::new(&env.xdg, tx_id).expect("Staging workspace");
    let new_bytes = b"# New Hyprland Config V2\n";
    let staged = staging
        .stage_file(
            camino::Utf8Path::new("hyprland.conf"),
            new_bytes,
            Some(target_file.clone()),
            "100644",
            None,
        )
        .expect("Stage file");

    let backup_engine = BackupSnapshotEngine::new(&env.xdg, tx_id).expect("Backup engine");
    let ops = backup_engine
        .create_snapshot(&[staged], ConfigComponent::Hyprland, &env.xdg)
        .expect("Create snapshot");

    assert_eq!(ops.len(), 1);
    let op = &ops[0];
    assert_eq!(op.op_type, OperationType::Overwrite);
    assert_eq!(op.target_file, target_file);
    assert_eq!(op.pre_sha256.as_deref(), Some(original_sha.as_str()));

    let backup_path = op.backup_file.as_ref().expect("Backup path recorded");
    assert!(backup_path.exists());
    let backup_bytes = fs::read(backup_path).expect("Read backup file");
    assert_eq!(backup_bytes, original_bytes);
}

#[test]
fn test_backup_handles_new_file_creation() {
    let env = TestEnv::new();
    let tx_id = "tx_backup_002";

    let staging = StagingWorkspace::new(&env.xdg, tx_id).expect("Staging workspace");
    let new_bytes = b"# Brand new config\n";
    let staged = staging
        .stage_file(
            camino::Utf8Path::new("brand_new.conf"),
            new_bytes,
            None,
            "100644",
            None,
        )
        .expect("Stage file");

    let backup_engine = BackupSnapshotEngine::new(&env.xdg, tx_id).expect("Backup engine");
    let ops = backup_engine
        .create_snapshot(&[staged], ConfigComponent::Hyprland, &env.xdg)
        .expect("Create snapshot");

    assert_eq!(ops.len(), 1);
    let op = &ops[0];
    assert_eq!(op.op_type, OperationType::Create);
    assert!(op.backup_file.is_none());
    assert!(op.pre_sha256.is_none());
    assert!(op.post_sha256.is_some());
}

#[test]
fn test_atomic_swap_replaces_cleanly_and_no_strays() {
    let env = TestEnv::new();
    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create target dir");
    let target_file = hypr_dir.join("waybar.css");

    fs::write(&target_file, b"/* Version 1 */").expect("Write initial target");

    // Sibling staging source
    let src_dir = env.xdg.cache_home.join("sources");
    fs::create_dir_all(&src_dir).expect("Create src dir");
    let src_file = src_dir.join("waybar.css");
    fs::write(&src_file, b"/* Version 2 - Atomic Swap */").expect("Write source file");

    AtomicSwapper::swap_file(&src_file, &target_file).expect("Atomic swap");

    let updated_content = fs::read_to_string(&target_file).expect("Read swapped file");
    assert_eq!(updated_content, "/* Version 2 - Atomic Swap */");

    // Verify zero stray temporary files in parent directory
    let strays = AtomicSwapper::prune_stray_tmp_files(&hypr_dir).expect("Prune strays");
    assert_eq!(strays, 0);

    for entry in fs::read_dir(&hypr_dir).expect("Read dir") {
        let entry = entry.expect("Dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".woderrise_tmp."),
            "Stray tmp file found: {}",
            name
        );
    }
}

#[test]
fn test_exdev_cross_directory_stream_copy() {
    let env = TestEnv::new();
    let cache_src = env
        .xdg
        .cache_home
        .join("staging")
        .join("test_tx")
        .join("files")
        .join("config.conf");
    if let Some(p) = cache_src.parent() {
        fs::create_dir_all(p).expect("Create cache src dir");
    }
    fs::write(&cache_src, b"EXDEV-Safe Stream Copy Content").expect("Write cache source");

    let target_dest = env.xdg.config_home.join("hypr").join("config.conf");

    AtomicSwapper::swap_file(&cache_src, &target_dest).expect("Swap across distinct paths");

    let result = fs::read_to_string(&target_dest).expect("Read target");
    assert_eq!(result, "EXDEV-Safe Stream Copy Content");
}

#[cfg(unix)]
#[test]
fn test_symlink_preservation_gnu_stow() {
    use std::os::unix::fs::symlink;

    let env = TestEnv::new();
    let tx_id = "tx_stow_001";

    // 1. Setup dotfiles repository location (e.g. ~/dotfiles/hypr/hyprland.conf)
    let dotfiles_dir = env.base_path.join("dotfiles").join("hypr");
    fs::create_dir_all(&dotfiles_dir).expect("Create dotfiles dir");
    let real_dotfile = dotfiles_dir.join("hyprland.conf");
    fs::write(&real_dotfile, b"GNU Stow Dotfile Content V1").expect("Write real dotfile");

    // 2. Setup symlink at ~/.config/hypr/hyprland.conf -> ~/dotfiles/hypr/hyprland.conf
    let hypr_dir = env.xdg.component_root(ConfigComponent::Hyprland);
    fs::create_dir_all(&hypr_dir).expect("Create hypr dir");
    let stow_symlink = hypr_dir.join("hyprland.conf");
    symlink(&real_dotfile, &stow_symlink).expect("Create stow symlink");

    // Verify it is a symlink
    let meta = fs::symlink_metadata(&stow_symlink).expect("Symlink metadata");
    assert!(meta.file_type().is_symlink());

    // 3. Security audit on tree entry should detect ExistingSymlink and resolve canonical path
    let entry = TreeEntry::new("hyprland.conf", "100644", "blob", "sha123", Some(200));
    let mapping = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
        .expect("Audit entry with host symlink");

    assert_eq!(mapping.conflict_status, ConflictStatus::ExistingSymlink);
    assert_eq!(
        fs::canonicalize(&mapping.target_destination).expect("Canonicalize mapping"),
        fs::canonicalize(&real_dotfile).expect("Canonicalize real dotfile")
    );

    // 4. Staging and Backup
    let staging = StagingWorkspace::new(&env.xdg, tx_id).expect("Staging workspace");
    let new_content = b"GNU Stow Dotfile Content V2 (Updated via Atomic Swapper)";
    let staged = staging
        .stage_file(
            camino::Utf8Path::new("hyprland.conf"),
            new_content,
            Some(mapping.target_destination.clone()),
            "100644",
            None,
        )
        .expect("Stage file");

    let backup_engine = BackupSnapshotEngine::new(&env.xdg, tx_id).expect("Backup engine");
    let ops = backup_engine
        .create_snapshot(
            std::slice::from_ref(&staged),
            ConfigComponent::Hyprland,
            &env.xdg,
        )
        .expect("Snapshot");

    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].op_type, OperationType::Overwrite);

    // 5. Execute Atomic Swap
    let staged_src = staging.files_dir.join(&staged.relative_path);
    AtomicSwapper::swap_file(&staged_src, &mapping.target_destination).expect("Swap file");

    // 6. Assertions:
    // Stow symlink STILL exists as a symlink
    let meta_after = fs::symlink_metadata(&stow_symlink).expect("Symlink metadata after swap");
    assert!(
        meta_after.file_type().is_symlink(),
        "Symlink was severed instead of preserved!"
    );

    // Reading through symlink gives updated content
    let content_via_symlink = fs::read_to_string(&stow_symlink).expect("Read through symlink");
    assert_eq!(
        content_via_symlink,
        "GNU Stow Dotfile Content V2 (Updated via Atomic Swapper)"
    );

    // Real dotfile was updated
    let content_real = fs::read_to_string(&real_dotfile).expect("Read real dotfile");
    assert_eq!(
        content_real,
        "GNU Stow Dotfile Content V2 (Updated via Atomic Swapper)"
    );
}

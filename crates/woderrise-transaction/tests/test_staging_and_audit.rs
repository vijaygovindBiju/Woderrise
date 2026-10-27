mod common;

use camino::Utf8Path;
use common::TestEnv;
use sha2::{Digest, Sha256};
use woderrise_core::error::{PathError, SecurityError, WoderriseError};
use woderrise_core::models::{
    ConfigComponent, ConflictStatus, SecurityAuditReport, StagingManifest,
};
use woderrise_detect::TreeEntry;
use woderrise_transaction::{SecurityAuditor, StagingWorkspace};

#[test]
fn test_staging_sandbox_structure_and_files() {
    let env = TestEnv::new();
    let tx_id = "tx_staging_001";
    let workspace = StagingWorkspace::new(&env.xdg, tx_id).expect("Create staging workspace");

    assert!(workspace.root_dir.exists());
    assert!(workspace.files_dir.exists());

    let content = b"# Hyprland test config\nmonitor=,preferred,auto,1\n";
    let staged = workspace
        .stage_file(
            Utf8Path::new("hyprland.conf"),
            content,
            None,
            "100644",
            Some("fake_blob_sha".to_string()),
        )
        .expect("Stage file");

    assert_eq!(staged.relative_path, "hyprland.conf");
    assert_eq!(staged.file_size, content.len() as u64);
    assert!(!staged.is_executable);
    assert!(!staged.is_symlink);

    let mut hasher = Sha256::new();
    hasher.update(content);
    let expected_sha = format!("{:x}", hasher.finalize());
    assert_eq!(staged.sha256, expected_sha);

    let staged_file_on_disk = workspace.files_dir.join("hyprland.conf");
    assert!(staged_file_on_disk.exists());
    let read_bytes = std::fs::read(&staged_file_on_disk).expect("Read staged file");
    assert_eq!(read_bytes, content);

    // Commit staging manifest
    let manifest = StagingManifest {
        schema: None,
        version: 1,
        transaction_id: tx_id.to_string(),
        component: ConfigComponent::Hyprland,
        source_repository: "user/dots".to_string(),
        source_commit_sha: "abcdef123456".to_string(),
        created_at: chrono::Utc::now(),
        staging_dir: workspace.root_dir.clone(),
        target_dir: env.xdg.component_root(ConfigComponent::Hyprland),
        files: vec![staged],
        security_audit: SecurityAuditReport {
            scripts_detected: 0,
            procedural_directives_neutralized: 0,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        },
    };

    workspace
        .commit_manifest(&manifest)
        .expect("Commit manifest");
    assert!(workspace.manifest_file.exists());

    let loaded = workspace.load_manifest().expect("Load manifest");
    assert_eq!(loaded.transaction_id, tx_id);
    assert_eq!(loaded.files.len(), 1);
    assert_eq!(loaded.files[0].relative_path, "hyprland.conf");

    // Clean up
    workspace.cleanup().expect("Cleanup staging");
    assert!(!workspace.root_dir.exists());
}

#[test]
fn test_audit_rejects_mode_120000_symlinks() {
    let env = TestEnv::new();
    let entry = TreeEntry::new("evil_link", "120000", "blob", "sha123", Some(100));

    let err = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
        .expect_err("Must reject mode 120000 symlinks (CVE-2021-21300)");

    match err {
        WoderriseError::Path(PathError::ForbiddenSymlink { path, .. }) => {
            assert_eq!(path, "evil_link");
        }
        other => panic!("Expected PathError::ForbiddenSymlink, got: {:?}", other),
    }
}

#[test]
fn test_audit_rejects_executable_modes() {
    let env = TestEnv::new();
    let entry = TreeEntry::new("run_script", "100755", "blob", "sha123", Some(100));

    let err = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
        .expect_err("Must reject executable mode 100755");

    match err {
        WoderriseError::Security(SecurityError::ScriptExecutionBlocked { path, .. }) => {
            assert_eq!(path, "run_script");
        }
        other => panic!(
            "Expected SecurityError::ScriptExecutionBlocked, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_audit_rejects_script_extensions() {
    let env = TestEnv::new();

    for script_name in &[
        "deploy.sh",
        "setup.bash",
        "helper.py",
        "script.pl",
        "build.rb",
        "binary.bin",
    ] {
        let entry = TreeEntry::new(*script_name, "100644", "blob", "sha123", Some(100));

        let err = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
            .expect_err(&format!("Must reject script extension for {}", script_name));

        match err {
            WoderriseError::Security(SecurityError::ScriptExecutionBlocked { path, .. }) => {
                assert_eq!(path, *script_name);
            }
            other => panic!(
                "Expected SecurityError::ScriptExecutionBlocked, got: {:?}",
                other
            ),
        }
    }
}

#[test]
fn test_audit_rejects_path_traversal() {
    let env = TestEnv::new();

    // 1. Parent dir traversal
    let entry_dotdot = TreeEntry::new("../escape.conf", "100644", "blob", "sha123", Some(100));
    let err = SecurityAuditor::audit_entry(&entry_dotdot, ConfigComponent::Hyprland, &env.xdg)
        .expect_err("Must reject .. traversal");
    assert!(matches!(
        err,
        WoderriseError::Path(PathError::PathTraversalAttempted { .. })
    ));

    // 2. Absolute path
    let entry_abs = TreeEntry::new("/etc/shadow", "100644", "blob", "sha123", Some(100));
    let err = SecurityAuditor::audit_entry(&entry_abs, ConfigComponent::Hyprland, &env.xdg)
        .expect_err("Must reject absolute path");
    assert!(matches!(
        err,
        WoderriseError::Path(PathError::AbsolutePathNotAllowed { .. })
    ));

    // 3. Null byte
    let entry_null = TreeEntry::new("file\0null.conf", "100644", "blob", "sha123", Some(100));
    let err = SecurityAuditor::audit_entry(&entry_null, ConfigComponent::Hyprland, &env.xdg)
        .expect_err("Must reject null byte");
    assert!(matches!(
        err,
        WoderriseError::Path(PathError::ContainsNullByte { .. })
    ));
}

#[test]
fn test_audit_rejects_resource_limit_exceeded() {
    let env = TestEnv::new();

    let entry_huge = TreeEntry::new(
        "huge.conf",
        "100644",
        "blob",
        "sha123",
        Some(15 * 1024 * 1024), // 15MB > 10MB limit
    );

    let err = SecurityAuditor::audit_entry(&entry_huge, ConfigComponent::Hyprland, &env.xdg)
        .expect_err("Must reject files exceeding 10MB limit");

    match err {
        WoderriseError::Security(SecurityError::ResourceLimitExceeded {
            limit_bytes,
            actual_bytes,
            ..
        }) => {
            assert_eq!(limit_bytes, 10 * 1024 * 1024);
            assert_eq!(actual_bytes, 15 * 1024 * 1024);
        }
        other => panic!(
            "Expected SecurityError::ResourceLimitExceeded, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_audit_preserves_hardware_monitors() {
    let env = TestEnv::new();

    let entry = TreeEntry::new("monitors.conf", "100644", "blob", "sha123", Some(500));

    let mapping = SecurityAuditor::audit_entry(&entry, ConfigComponent::Hyprland, &env.xdg)
        .expect("Auditing monitors.conf should succeed");

    assert!(mapping.is_hardware_protected);
    assert_eq!(mapping.conflict_status, ConflictStatus::HardwareProtected);
}

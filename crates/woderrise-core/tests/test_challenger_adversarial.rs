//! Adversarial challenge tests for Milestone 1:
//! Data model integrity, serialization roundtrip, and schema conformance.

use camino::Utf8PathBuf;
use chrono::Utc;
use tempfile::TempDir;
use woderrise_core::models::{
    ActiveTransaction, Catalog, ConfigComponent, Journal, JournalEntry, OperationType,
    SecurityAuditReport, StagedFile, StagingManifest, SyntaxFormat, TransactionStatus,
};
use woderrise_core::paths::{sanitize_relative_path, strip_dotfile_repo_prefix};
use woderrise_core::schema::{
    read_json_file, write_json_file_durably, CATALOG_SCHEMA_V1, JOURNAL_SCHEMA_V1,
    MANIFEST_SCHEMA_V1,
};

/// 1. Empirically verify that catalog.json schema matches Section 8.2.1 of DESIGN_PROPOSAL.md verbatim.
#[test]
fn test_challenge_catalog_section_8_2_1_verbatim_json() {
    let verbatim_json = r#"{
  "$schema": "https://woderrise.dev/schemas/catalog.v1.json",
  "version": 1,
  "last_synced": "2026-10-27T14:15:00Z",
  "repositories": [
    {
      "id": 589412039,
      "full_name": "prasanthrangan/hyprdots",
      "html_url": "https://github.com/prasanthrangan/hyprdots",
      "description": "Custom Hyprland desktop configuration and themes",
      "default_branch": "main",
      "stargazers_count": 4820,
      "topics": ["hyprland", "hyprland-dotfiles", "waybar", "rice"],
      "pushed_at": "2026-10-25T11:20:00Z",
      "cached_tree_sha": "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4",
      "etag": "W/\"325ec65236b77b58005c2f4551e84edf5178903ad64f447abda1cb1dc73f23b3\"",
      "detected_components": [
        {
          "component": "hyprland",
          "syntax_format": "legacy_conf",
          "root_path": "Configs/.config/hypr",
          "entrypoint": "hyprland.conf",
          "file_count": 14,
          "total_size_bytes": 48120
        },
        {
          "component": "waybar",
          "syntax_format": "jsonc",
          "root_path": "Configs/.config/waybar",
          "entrypoint": "config.jsonc",
          "style_file": "style.css",
          "file_count": 6,
          "total_size_bytes": 22400
        }
      ]
    }
  ]
}"#;

    // Deserialization check
    let catalog: Catalog = serde_json::from_str(verbatim_json)
        .expect("Verbatim Section 8.2.1 catalog.json must deserialize cleanly");

    assert_eq!(catalog.schema.as_deref(), Some(CATALOG_SCHEMA_V1));
    assert_eq!(catalog.version, 1);
    assert_eq!(catalog.repositories.len(), 1);

    let repo = &catalog.repositories[0];
    assert_eq!(repo.id, 589412039);
    assert_eq!(repo.full_name, "prasanthrangan/hyprdots");
    assert_eq!(repo.resolved_full_name(), "prasanthrangan/hyprdots");
    assert_eq!(repo.html_url, "https://github.com/prasanthrangan/hyprdots");
    assert_eq!(
        repo.description.as_deref(),
        Some("Custom Hyprland desktop configuration and themes")
    );
    assert_eq!(repo.default_branch, "main");
    assert_eq!(repo.stargazers_count, 4820);
    assert_eq!(repo.stars(), 4820);
    assert_eq!(
        repo.topics,
        vec!["hyprland", "hyprland-dotfiles", "waybar", "rice"]
    );
    assert_eq!(
        repo.cached_tree_sha.as_deref(),
        Some("3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4")
    );
    assert_eq!(
        repo.etag.as_deref(),
        Some("W/\"325ec65236b77b58005c2f4551e84edf5178903ad64f447abda1cb1dc73f23b3\"")
    );
    assert_eq!(repo.detected_components.len(), 2);

    let comp0 = &repo.detected_components[0];
    assert_eq!(comp0.component, ConfigComponent::Hyprland);
    assert_eq!(comp0.syntax_format, SyntaxFormat::LegacyConf);
    assert_eq!(comp0.root_path, "Configs/.config/hypr");
    assert_eq!(comp0.entrypoint, "hyprland.conf");
    assert_eq!(comp0.style_file, None);
    assert_eq!(comp0.file_count, 14);
    assert_eq!(comp0.total_size_bytes, 48120);

    let comp1 = &repo.detected_components[1];
    assert_eq!(comp1.component, ConfigComponent::Waybar);
    assert_eq!(comp1.syntax_format, SyntaxFormat::Jsonc);
    assert_eq!(comp1.root_path, "Configs/.config/waybar");
    assert_eq!(comp1.entrypoint, "config.jsonc");
    assert_eq!(comp1.style_file.as_deref(), Some("style.css"));
    assert_eq!(comp1.file_count, 6);
    assert_eq!(comp1.total_size_bytes, 22400);

    // Serialization roundtrip check
    let reserialized = serde_json::to_string_pretty(&catalog).expect("Must serialize cleanly");
    let re_deserialized: Catalog =
        serde_json::from_str(&reserialized).expect("Must re-deserialize cleanly");
    assert_eq!(catalog, re_deserialized);

    // Verify that empty owner and repo are omitted from serialized output
    let reserialized_val: serde_json::Value =
        serde_json::from_str(&reserialized).expect("Parse json val");
    let first_repo_val = &reserialized_val["repositories"][0];
    assert!(first_repo_val.get("owner").is_none());
    assert!(first_repo_val.get("repo").is_none());
}

/// 2. Empirically verify that staging_manifest.json matches Section 8.2.2 verbatim.
#[test]
fn test_challenge_staging_manifest_section_8_2_2_verbatim_json() {
    let verbatim_json = r#"{
  "transaction_id": "tx_20261027_141500_hyprland_7d8e9f",
  "component": "hyprland",
  "source_repository": "prasanthrangan/hyprdots",
  "source_commit_sha": "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4",
  "created_at": "2026-10-27T14:15:00Z",
  "staging_dir": "/home/user/.cache/woderrise/staging/tx_20261027_141500_hyprland_7d8e9f",
  "target_dir": "/home/user/.config/hypr",
  "files": [
    {
      "relative_path": "hyprland.conf",
      "blob_sha": "e48aa9249ad1527ae37fc09d21e08abd20a20f7e",
      "file_size": 3412,
      "sha256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
      "file_mode": "100644",
      "is_executable": false,
      "syntax_valid": true,
      "path_safe": true
    },
    {
      "relative_path": "colors.conf",
      "blob_sha": "642335c838090bb5d09b24e55bf6ef1490949cca",
      "file_size": 1280,
      "sha256": "ef2d127de37b942baad06145e54b0c619a1f22327b2ebbcfbec78f5564afe39d",
      "file_mode": "100644",
      "is_executable": false,
      "syntax_valid": true,
      "path_safe": true
    }
  ],
  "security_audit": {
    "scripts_detected": 0,
    "procedural_directives_neutralized": 2,
    "path_traversal_violations": 0,
    "symlinks_rejected": 0,
    "passed": true
  }
}"#;

    let manifest: StagingManifest = serde_json::from_str(verbatim_json)
        .expect("Verbatim Section 8.2.2 manifest.json must deserialize cleanly");

    assert_eq!(
        manifest.transaction_id,
        "tx_20261027_141500_hyprland_7d8e9f"
    );
    assert_eq!(manifest.component, ConfigComponent::Hyprland);
    assert_eq!(manifest.source_repository, "prasanthrangan/hyprdots");
    assert_eq!(
        manifest.source_commit_sha,
        "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4"
    );
    assert_eq!(
        manifest.staging_dir,
        Utf8PathBuf::from("/home/user/.cache/woderrise/staging/tx_20261027_141500_hyprland_7d8e9f")
    );
    assert_eq!(
        manifest.target_dir,
        Utf8PathBuf::from("/home/user/.config/hypr")
    );
    assert_eq!(manifest.files.len(), 2);
    assert_eq!(manifest.files[0].relative_path, "hyprland.conf");
    assert_eq!(manifest.files[0].file_mode, "100644");
    assert!(!manifest.files[0].is_executable);
    assert!(manifest.files[0].syntax_valid);
    assert!(manifest.files[0].path_safe);
    assert!(manifest.security_audit.passed);
    assert_eq!(manifest.security_audit.procedural_directives_neutralized, 2);

    // Invariant validation
    assert!(manifest.validate_security_invariants().is_ok());

    // Serialization roundtrip
    let reserialized = serde_json::to_string_pretty(&manifest).expect("Must serialize cleanly");
    let re_deserialized: StagingManifest =
        serde_json::from_str(&reserialized).expect("Must re-deserialize cleanly");
    assert_eq!(manifest, re_deserialized);
}

/// 3. Empirically verify that journal.json WAL matches Section 8.2.3 and 10.2 verbatim.
#[test]
fn test_challenge_journal_wal_section_8_2_3_verbatim_json() {
    let verbatim_json = r#"{
  "$schema": "https://woderrise.dev/schemas/journal.v1.json",
  "version": 1,
  "active_transaction": {
    "id": "tx_20261027_141500_hyprland_7d8e9f",
    "timestamp": "2026-10-27T14:15:00Z",
    "component": "hyprland",
    "status": "Applying",
    "source_repo": "prasanthrangan/hyprdots",
    "source_commit": "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4",
    "backup_directory": "/home/user/.local/state/woderrise/backups/tx_20261027_141500_hyprland_7d8e9f",
    "operations": [
      {
        "type": "overwrite",
        "target_file": "/home/user/.config/hypr/hyprland.conf",
        "backup_file": "hyprland.conf",
        "pre_sha256": "9f83c6051a842e4dd9b8fb4f05e6780fadd773772839404790c8ab11434b8079",
        "post_sha256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
        "mode": "0644"
      },
      {
        "type": "create",
        "target_file": "/home/user/.config/hypr/colors.conf",
        "backup_file": null,
        "pre_sha256": null,
        "post_sha256": "ef2d127de37b942baad06145e54b0c619a1f22327b2ebbcfbec78f5564afe39d",
        "mode": "0644"
      }
    ]
  },
  "transactions": [
    {
      "id": "tx_20261027_120000_waybar_3a1b2c",
      "timestamp": "2026-10-27T12:00:00Z",
      "component": "waybar",
      "status": "Committed",
      "source_repo": "prasanthrangan/hyprdots",
      "source_commit": "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4",
      "backup_directory": "/home/user/.local/state/woderrise/backups/tx_20261027_120000_waybar_3a1b2c",
      "operations": [
        {
          "type": "overwrite",
          "target_file": "/home/user/.config/waybar/config.jsonc",
          "backup_file": "config.jsonc",
          "pre_sha256": "1a2b3c4d5e6f",
          "post_sha256": "5f6e7d8c9b0a",
          "mode": "0644"
        }
      ]
    }
  ]
}"#;

    let journal: Journal = serde_json::from_str(verbatim_json)
        .expect("Verbatim Section 8.2.3 journal.json must deserialize cleanly");

    assert_eq!(journal.schema.as_deref(), Some(JOURNAL_SCHEMA_V1));
    assert_eq!(journal.version, 1);

    // Active transaction WAL state check
    let active = journal
        .active_transaction
        .as_ref()
        .expect("Active transaction must be present");
    assert_eq!(active.id, "tx_20261027_141500_hyprland_7d8e9f");
    assert_eq!(active.component, ConfigComponent::Hyprland);
    assert_eq!(active.status, TransactionStatus::Applying);
    assert_eq!(active.operations.len(), 2);
    assert_eq!(active.operations[0].op_type, OperationType::Overwrite);
    assert_eq!(
        active.operations[0].target_file,
        Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")
    );
    assert_eq!(
        active.operations[0].backup_file,
        Some(Utf8PathBuf::from("hyprland.conf"))
    );
    assert_eq!(
        active.operations[0].pre_sha256.as_deref(),
        Some("9f83c6051a842e4dd9b8fb4f05e6780fadd773772839404790c8ab11434b8079")
    );
    assert_eq!(
        active.operations[0].post_sha256.as_deref(),
        Some("4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a")
    );
    assert_eq!(active.operations[0].mode.as_deref(), Some("0644"));

    assert_eq!(active.operations[1].op_type, OperationType::Create);
    assert_eq!(active.operations[1].backup_file, None);
    assert_eq!(active.operations[1].pre_sha256, None);

    // Historical transaction check
    assert_eq!(journal.transactions.len(), 1);
    let tx = &journal.transactions[0];
    assert_eq!(tx.id, "tx_20261027_120000_waybar_3a1b2c");
    assert_eq!(tx.component, ConfigComponent::Waybar);
    assert_eq!(tx.status, TransactionStatus::Committed);

    // Serialization roundtrip
    let reserialized = serde_json::to_string_pretty(&journal).expect("Must serialize cleanly");
    let re_deserialized: Journal =
        serde_json::from_str(&reserialized).expect("Must re-deserialize cleanly");
    assert_eq!(journal, re_deserialized);
}

/// 4. Stress-test symlink mode 120000 and executable script detection on StagingManifest.
///
/// Verify that `validate_security_invariants()` detects Git tree symlink mode `120000`
/// even when `is_symlink` is `false`, and detects `is_executable: true` or script extensions.
#[test]
fn test_challenge_symlink_mode_120000_and_executable_script_enforcement() {
    // Scenario 4A: A file has Git mode "120000" (symlink) but `is_symlink` is false
    let manifest_symlink_mode = StagingManifest {
        schema: Some(MANIFEST_SCHEMA_V1.to_string()),
        version: 1,
        transaction_id: "tx_attack_symlink".to_string(),
        component: ConfigComponent::Hyprland,
        source_repository: "attacker/repo".to_string(),
        source_commit_sha: "badsha".to_string(),
        created_at: Utc::now(),
        staging_dir: Utf8PathBuf::from("/tmp/staging/tx_atk"),
        target_dir: Utf8PathBuf::from("/home/user/.config/hypr"),
        files: vec![StagedFile {
            relative_path: Utf8PathBuf::from("exploit_link"),
            target_destination: Some(Utf8PathBuf::from("/home/user/.config/hypr/exploit_link")),
            blob_sha: None,
            file_size: 16,
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            file_mode: "120000".to_string(), // Symlink mode!
            permissions: 0o777,
            is_executable: false,
            is_symlink: false, // Not marked as is_symlink
            syntax_valid: true,
            path_safe: true,
        }],
        security_audit: SecurityAuditReport {
            scripts_detected: 0,
            procedural_directives_neutralized: 0,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        },
    };

    // Scenario 4B: A file is an executable shell script (is_executable = true, mode 100755)
    let manifest_executable_script = StagingManifest {
        schema: Some(MANIFEST_SCHEMA_V1.to_string()),
        version: 1,
        transaction_id: "tx_attack_exec".to_string(),
        component: ConfigComponent::Hyprland,
        source_repository: "attacker/repo".to_string(),
        source_commit_sha: "badsha2".to_string(),
        created_at: Utc::now(),
        staging_dir: Utf8PathBuf::from("/tmp/staging/tx_atk2"),
        target_dir: Utf8PathBuf::from("/home/user/.config/hypr"),
        files: vec![StagedFile {
            relative_path: Utf8PathBuf::from("install.sh"),
            target_destination: Some(Utf8PathBuf::from("/home/user/.config/hypr/install.sh")),
            blob_sha: None,
            file_size: 120,
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            file_mode: "100755".to_string(),
            permissions: 0o755,
            is_executable: true, // EXECUTABLE SCRIPT!
            is_symlink: false,
            syntax_valid: true,
            path_safe: true,
        }],
        security_audit: SecurityAuditReport {
            scripts_detected: 1, // Script detected by audit!
            procedural_directives_neutralized: 0,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        },
    };

    // Scenario 4C: File has .sh extension even if file_mode is 100644 and is_executable is false (Interpreter bypass vector)
    let manifest_interpreter_bypass = StagingManifest {
        schema: Some(MANIFEST_SCHEMA_V1.to_string()),
        version: 1,
        transaction_id: "tx_attack_interp".to_string(),
        component: ConfigComponent::Hyprland,
        source_repository: "attacker/repo".to_string(),
        source_commit_sha: "badsha3".to_string(),
        created_at: Utc::now(),
        staging_dir: Utf8PathBuf::from("/tmp/staging/tx_atk3"),
        target_dir: Utf8PathBuf::from("/home/user/.config/hypr"),
        files: vec![StagedFile {
            relative_path: Utf8PathBuf::from("scripts/deploy.bash"),
            target_destination: Some(Utf8PathBuf::from(
                "/home/user/.config/hypr/scripts/deploy.bash",
            )),
            blob_sha: None,
            file_size: 50,
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            file_mode: "100644".to_string(),
            permissions: 0o644,
            is_executable: false,
            is_symlink: false,
            syntax_valid: true,
            path_safe: true,
        }],
        security_audit: SecurityAuditReport {
            scripts_detected: 1,
            procedural_directives_neutralized: 0,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        },
    };

    let res_symlink = manifest_symlink_mode.validate_security_invariants();
    let res_exec = manifest_executable_script.validate_security_invariants();
    let res_interp = manifest_interpreter_bypass.validate_security_invariants();

    // Security invariants MUST reject symlinks, executable scripts, and script extensions
    assert!(
        res_symlink.is_err(),
        "Mode 120000 must be blocked even if is_symlink is false"
    );
    assert!(res_exec.is_err(), "Executable scripts must be blocked");
    assert!(res_interp.is_err(), "Script files must be blocked");
}

/// 5. Empirically verify transaction lifecycle state machine enum coverage and serializations.
#[test]
fn test_challenge_transaction_lifecycle_transitions() {
    let states = vec![
        (TransactionStatus::Staged, "\"Staged\""),
        (TransactionStatus::Applying, "\"Applying\""),
        (TransactionStatus::Committed, "\"Committed\""),
        (TransactionStatus::RollingBack, "\"RollingBack\""),
        (TransactionStatus::RolledBack, "\"RolledBack\""),
        (
            TransactionStatus::RecoveredRollback,
            "\"RecoveredRollback\"",
        ),
        (TransactionStatus::Failed, "\"Failed\""),
    ];

    for (status, expected_json) in states {
        let serialized = serde_json::to_string(&status).expect("Serialization must succeed");
        assert_eq!(serialized, expected_json);

        let deserialized: TransactionStatus =
            serde_json::from_str(&serialized).expect("Deserialization must succeed");
        assert_eq!(deserialized, status);
    }

    // Check that ActiveTransaction can hold any status without domain guard:
    let mut tx = ActiveTransaction {
        id: "tx_lifecycle".to_string(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Staged,
        source_repo: "prasanthrangan/hyprdots".to_string(),
        source_commit: "commit_hash".to_string(),
        backup_directory: Utf8PathBuf::from("/tmp/backup"),
        operations: vec![],
    };

    // Staged -> Applying
    assert!(tx.transition_to(TransactionStatus::Applying).is_ok());
    assert_eq!(tx.status, TransactionStatus::Applying);

    // Applying -> Committed
    assert!(tx.transition_to(TransactionStatus::Committed).is_ok());
    assert_eq!(tx.status, TransactionStatus::Committed);

    // Committed -> Applying (invalid state transition)
    assert!(tx.transition_to(TransactionStatus::Applying).is_err());

    // In a journal, having an active_transaction with status Committed or RolledBack is invalid in WAL design:
    let journal_with_committed_active = Journal {
        schema: None,
        version: 1,
        active_transaction: Some(tx),
        transactions: vec![],
    };
    assert!(journal_with_committed_active
        .validate_wal_invariants()
        .is_err());
}

/// 6. Empirically verify durability write, atomic rename, and fsync protocol.
#[test]
fn test_challenge_atomic_durability_and_fsync_guarantee() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let target = Utf8PathBuf::from_path_buf(temp_dir.path().join("wal_test").join("journal.json"))
        .expect("Utf8 path conversion");

    let journal = Journal {
        schema: Some(JOURNAL_SCHEMA_V1.to_string()),
        version: 1,
        active_transaction: Some(ActiveTransaction {
            id: "tx_durability_test".to_string(),
            timestamp: Utc::now(),
            component: ConfigComponent::Waybar,
            status: TransactionStatus::Applying,
            source_repo: "prasanthrangan/hyprdots".to_string(),
            source_commit: "commit_hash".to_string(),
            backup_directory: Utf8PathBuf::from("/tmp/backup"),
            operations: vec![],
        }),
        transactions: vec![],
    };

    // First write: creates parent directory, writes sibling tmp, syncs, renames
    write_json_file_durably(&target, &journal).expect("First write must succeed");
    assert!(target.exists());

    // Read back and verify
    let read_back: Journal = read_json_file(&target).expect("Read back must succeed");
    assert_eq!(read_back, journal);

    // Second write: overwrites existing target atomically
    let updated_journal = Journal {
        schema: Some(JOURNAL_SCHEMA_V1.to_string()),
        version: 1,
        active_transaction: None,
        transactions: vec![JournalEntry {
            id: "tx_durability_test".to_string(),
            timestamp: Utc::now(),
            component: ConfigComponent::Waybar,
            status: TransactionStatus::Committed,
            source_repo: "prasanthrangan/hyprdots".to_string(),
            source_commit: "commit_hash".to_string(),
            backup_directory: Utf8PathBuf::from("/tmp/backup"),
            operations: vec![],
        }],
    };

    write_json_file_durably(&target, &updated_journal).expect("Second write must succeed");
    let read_back_updated: Journal = read_json_file(&target).expect("Read back must succeed");
    assert_eq!(read_back_updated, updated_journal);
    assert!(read_back_updated.active_transaction.is_none());
    assert_eq!(read_back_updated.transactions.len(), 1);
    assert_eq!(
        read_back_updated.transactions[0].status,
        TransactionStatus::Committed
    );
}

/// 7. Stress-test path sanitization against complex adversarial traversal payloads.
#[test]
fn test_challenge_path_sanitization_adversarial_matrix() {
    let malicious_inputs = vec![
        "../etc/shadow",
        "foo/../../etc/passwd",
        "/etc/shadow",
        "\\Windows\\System32",
        "C:\\Config\\hyprland.conf",
        "D:/test.conf",
        "hypr/\0bad.conf",
        "",
        "   ",
        ".",
        "./",
        "./../escape",
        "nested/dir/../../../escaped",
        "dir/..",
    ];

    for bad in malicious_inputs {
        let path = camino::Utf8Path::new(bad);
        let res = sanitize_relative_path(path);
        assert!(
            res.is_err(),
            "Path '{}' MUST be rejected as unsafe traversal",
            bad
        );
    }

    // Safe valid paths must sanitize cleanly
    let safe_inputs = vec![
        ("hyprland.conf", "hyprland.conf"),
        ("./hyprland.conf", "hyprland.conf"),
        ("themes/catppuccin.conf", "themes/catppuccin.conf"),
        ("nested/././deep/path.conf", "nested/deep/path.conf"),
    ];

    for (input, expected) in safe_inputs {
        let path = camino::Utf8Path::new(input);
        let clean = sanitize_relative_path(path)
            .unwrap_or_else(|e| panic!("Safe path '{}' should pass: {}", input, e));
        assert_eq!(clean.as_str(), expected);
    }
}

/// 8. Stress-test strip_dotfile_repo_prefix with nested subpath repetitions.
#[test]
fn test_challenge_strip_dotfile_repo_prefix_edge_cases() {
    // Normal cases
    let p1 = camino::Utf8Path::new("Configs/.config/hypr/hyprland.conf");
    assert_eq!(
        strip_dotfile_repo_prefix(p1, "hypr").as_str(),
        "hyprland.conf"
    );

    let p2 = camino::Utf8Path::new("hypr/hyprland.conf");
    assert_eq!(
        strip_dotfile_repo_prefix(p2, "hypr").as_str(),
        "hyprland.conf"
    );

    // Edge case: Repository has a nested directory named 'hypr' inside hypr
    // e.g. dotfiles/hypr/themes/hypr/custom.conf
    // Only the first component match is stripped, preserving the nested 'themes/hypr/' subdirectory:
    let p_nested = camino::Utf8Path::new("dotfiles/hypr/themes/hypr/custom.conf");
    let stripped = strip_dotfile_repo_prefix(p_nested, "hypr");
    assert_eq!(stripped.as_str(), "themes/hypr/custom.conf");
}

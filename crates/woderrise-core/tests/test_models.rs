//! Tests for domain models serialization, deserialization, Serde aliases, and security invariants.

use camino::Utf8PathBuf;
use chrono::Utc;
use woderrise_core::models::{
    ActiveTransaction, Catalog, CatalogRepository, ConfigComponent, ConflictStatus,
    DetectedComponent, FileOperationRecord, HardwareConflictNotice, Journal, JournalEntry,
    OperationType, SecurityAuditReport, StagedFile, StagingManifest, SyntaxFormat, TargetMapping,
    TransactionStatus, VerificationReport,
};

#[test]
fn test_catalog_roundtrip_and_defaults() {
    let repo = CatalogRepository {
        id: 589412039,
        owner: "prasanthrangan".to_string(),
        repo: "hyprdots".to_string(),
        full_name: "prasanthrangan/hyprdots".to_string(),
        html_url: "https://github.com/prasanthrangan/hyprdots".to_string(),
        description: Some("Custom Hyprland desktop configuration and themes".to_string()),
        stargazers_count: 4820,
        default_branch: "main".to_string(),
        topics: vec![
            "hyprland".to_string(),
            "hyprland-dotfiles".to_string(),
            "waybar".to_string(),
        ],
        pushed_at: Some(Utc::now()),
        cached_tree_sha: Some("3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4".to_string()),
        etag: Some("W/\"325ec65236b77b58\"".to_string()),
        detected_components: vec![
            DetectedComponent {
                component: ConfigComponent::HyprlandLegacy,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: "Configs/.config/hypr".to_string(),
                entrypoint: "hyprland.conf".to_string(),
                style_file: None,
                file_count: 14,
                total_size_bytes: 48120,
            },
            DetectedComponent {
                component: ConfigComponent::Waybar,
                syntax_format: SyntaxFormat::Jsonc,
                root_path: "Configs/.config/waybar".to_string(),
                entrypoint: "config.jsonc".to_string(),
                style_file: Some("style.css".to_string()),
                file_count: 6,
                total_size_bytes: 22400,
            },
        ],
    };

    let catalog = Catalog {
        schema: Some("https://woderrise.dev/schemas/catalog.v1.json".to_string()),
        version: 1,
        last_synced: Utc::now(),
        repositories: vec![repo],
    };

    let serialized = serde_json::to_string_pretty(&catalog).expect("Failed to serialize catalog");
    let deserialized: Catalog =
        serde_json::from_str(&serialized).expect("Failed to deserialize catalog");

    assert_eq!(catalog, deserialized);
    assert_eq!(deserialized.repositories[0].stars(), 4820);
    assert_eq!(
        deserialized.repositories[0].resolved_full_name(),
        "prasanthrangan/hyprdots"
    );
}

#[test]
fn test_catalog_serde_aliases_github_api() {
    let raw_github_json = r#"{
        "version": 1,
        "last_synced": "2026-10-27T14:15:00Z",
        "repositories": [
            {
                "id": 999,
                "owner": "test-owner",
                "repo": "test-repo",
                "html_url": "https://github.com/test-owner/test-repo",
                "description": null,
                "stars": 250,
                "topics": ["rice"],
                "updated_at": "2026-10-25T11:20:00Z",
                "compatible_components": [
                    {
                        "component": "hyprland",
                        "syntax_format": "legacy_conf",
                        "root_path": ".config/hypr",
                        "entrypoint": "hyprland.conf",
                        "file_count": 3,
                        "total_size_bytes": 1024
                    }
                ]
            }
        ]
    }"#;

    let catalog: Catalog =
        serde_json::from_str(raw_github_json).expect("Failed to parse alias JSON");
    assert_eq!(catalog.repositories.len(), 1);
    let repo = &catalog.repositories[0];
    assert_eq!(repo.id, 999);
    assert_eq!(repo.stargazers_count, 250);
    assert_eq!(repo.stars(), 250);
    assert_eq!(repo.resolved_full_name(), "test-owner/test-repo");
    assert!(repo.pushed_at.is_some());
    assert_eq!(repo.detected_components.len(), 1);
    assert_eq!(
        repo.detected_components[0].component,
        ConfigComponent::Hyprland
    );
}

#[test]
fn test_staging_manifest_roundtrip_and_security_invariants() {
    let manifest = StagingManifest {
        schema: Some("https://woderrise.dev/schemas/manifest.v1.json".to_string()),
        version: 1,
        transaction_id: "tx_20261027_141500_hyprland_7d8e9f".to_string(),
        component: ConfigComponent::Hyprland,
        source_repository: "prasanthrangan/hyprdots".to_string(),
        source_commit_sha: "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4".to_string(),
        created_at: Utc::now(),
        staging_dir: Utf8PathBuf::from("/tmp/staging/tx1"),
        target_dir: Utf8PathBuf::from("/home/user/.config/hypr"),
        files: vec![
            StagedFile {
                relative_path: Utf8PathBuf::from("hyprland.conf"),
                target_destination: Some(Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")),
                blob_sha: Some("e48aa9249ad1527ae37fc09d21e08abd20a20f7e".to_string()),
                file_size: 3412,
                sha256: "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a".to_string(),
                file_mode: "100644".to_string(),
                permissions: 0o644,
                is_executable: false,
                is_symlink: false,
                syntax_valid: true,
                path_safe: true,
            },
        ],
        security_audit: SecurityAuditReport {
            scripts_detected: 0,
            procedural_directives_neutralized: 2,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        },
    };

    assert!(manifest.validate_security_invariants().is_ok());

    let json = serde_json::to_string_pretty(&manifest).expect("Failed to serialize manifest");
    let deserialized: StagingManifest =
        serde_json::from_str(&json).expect("Failed to deserialize manifest");

    assert_eq!(manifest, deserialized);
}

#[test]
fn test_staging_manifest_symlink_rejection() {
    let mut manifest = StagingManifest {
        schema: None,
        version: 1,
        transaction_id: "tx_symlink_test".to_string(),
        component: ConfigComponent::Hyprland,
        source_repository: "malicious/repo".to_string(),
        source_commit_sha: "abc1234".to_string(),
        created_at: Utc::now(),
        staging_dir: Utf8PathBuf::from("/tmp/staging/tx2"),
        target_dir: Utf8PathBuf::from("/home/user/.config/hypr"),
        files: vec![StagedFile {
            relative_path: Utf8PathBuf::from("dangerous_link"),
            target_destination: Some(Utf8PathBuf::from("/home/user/.config/hypr/dangerous_link")),
            blob_sha: None,
            file_size: 16,
            sha256: "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a".to_string(),
            file_mode: "120000".to_string(),
            permissions: 0o777,
            is_executable: false,
            is_symlink: true,
            syntax_valid: true,
            path_safe: false,
        }],
        security_audit: SecurityAuditReport {
            scripts_detected: 0,
            procedural_directives_neutralized: 0,
            path_traversal_violations: 0,
            symlinks_rejected: 1,
            passed: false,
        },
    };

    assert!(manifest.validate_security_invariants().is_err());

    manifest.security_audit.passed = true;
    assert!(manifest.validate_security_invariants().is_err());
}

#[test]
fn test_journal_wal_roundtrip_active_and_committed() {
    let active_tx = ActiveTransaction {
        id: "tx_20261027_141500_hyprland_7d8e9f".to_string(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Applying,
        source_repo: "prasanthrangan/hyprdots".to_string(),
        source_commit: "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4".to_string(),
        backup_directory: Utf8PathBuf::from(
            "/home/user/.local/state/woderrise/backups/tx_20261027_141500_hyprland_7d8e9f",
        ),
        operations: vec![
            FileOperationRecord {
                op_type: OperationType::Overwrite,
                target_file: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
                backup_file: Some(Utf8PathBuf::from("hyprland.conf")),
                pre_sha256: Some(
                    "9f83c6051a842e4dd9b8fb4f05e6780fadd773772839404790c8ab11434b8079".to_string(),
                ),
                post_sha256: Some(
                    "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a".to_string(),
                ),
                mode: Some("0644".to_string()),
            },
            FileOperationRecord {
                op_type: OperationType::Create,
                target_file: Utf8PathBuf::from("/home/user/.config/hypr/colors.conf"),
                backup_file: None,
                pre_sha256: None,
                post_sha256: Some(
                    "ef2d127de37b942baad06145e54b0c619a1f22327b2ebbcfbec78f5564afe39d".to_string(),
                ),
                mode: Some("0644".to_string()),
            },
        ],
    };

    let committed_tx = JournalEntry {
        id: "tx_20261027_120000_waybar_3a1b2c".to_string(),
        timestamp: Utc::now(),
        component: ConfigComponent::Waybar,
        status: TransactionStatus::Committed,
        source_repo: "prasanthrangan/hyprdots".to_string(),
        source_commit: "3c8b0dfb5e7f8e41a67b80463513f10d57cab1a4".to_string(),
        backup_directory: Utf8PathBuf::from(
            "/home/user/.local/state/woderrise/backups/tx_20261027_120000_waybar_3a1b2c",
        ),
        operations: vec![FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file: Utf8PathBuf::from("/home/user/.config/waybar/config.jsonc"),
            backup_file: Some(Utf8PathBuf::from("config.jsonc")),
            pre_sha256: Some("1111".to_string()),
            post_sha256: Some("2222".to_string()),
            mode: Some("0644".to_string()),
        }],
    };

    let journal = Journal {
        schema: Some("https://woderrise.dev/schemas/journal.v1.json".to_string()),
        version: 1,
        active_transaction: Some(active_tx),
        transactions: vec![committed_tx],
    };

    let json = serde_json::to_string_pretty(&journal).expect("Failed to serialize journal");
    let deserialized: Journal =
        serde_json::from_str(&json).expect("Failed to deserialize journal");

    assert_eq!(journal, deserialized);
    assert_eq!(
        deserialized
            .active_transaction
            .as_ref()
            .unwrap()
            .status,
        TransactionStatus::Applying
    );
    assert_eq!(
        deserialized.active_transaction.as_ref().unwrap().operations.len(),
        2
    );
    assert_eq!(deserialized.transactions.len(), 1);
}

#[test]
fn test_verification_report_and_conflict_status() {
    let report = VerificationReport {
        transaction_id: "tx_verify_1".to_string(),
        component: ConfigComponent::Hyprland,
        total_files: 2,
        total_bytes: 4096,
        target_mappings: vec![
            TargetMapping {
                component: ConfigComponent::Hyprland,
                relative_path: Utf8PathBuf::from("hyprland.conf"),
                blob_sha: "blob_1".to_string(),
                target_destination: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
                git_file_mode: "100644".to_string(),
                expected_sha256: "hash_1".to_string(),
                is_hardware_protected: false,
                conflict_status: ConflictStatus::OverwritingExisting,
            },
            TargetMapping {
                component: ConfigComponent::Monitors,
                relative_path: Utf8PathBuf::from("monitors.conf"),
                blob_sha: "blob_2".to_string(),
                target_destination: Utf8PathBuf::from("/home/user/.config/hypr/monitors.conf"),
                git_file_mode: "100644".to_string(),
                expected_sha256: "hash_2".to_string(),
                is_hardware_protected: true,
                conflict_status: ConflictStatus::HardwareProtected,
            },
        ],
        security_audit: SecurityAuditReport {
            scripts_detected: 0,
            procedural_directives_neutralized: 1,
            path_traversal_violations: 0,
            symlinks_rejected: 0,
            passed: true,
        },
        hardware_conflicts: vec![HardwareConflictNotice {
            file_path: Utf8PathBuf::from("/home/user/.config/hypr/monitors.conf"),
            description: "Preserving local display configuration".to_string(),
            preserved_by_default: true,
        }],
        passed: true,
    };

    let json = serde_json::to_string_pretty(&report).expect("Failed to serialize report");
    assert!(json.contains("\"conflict_status\": \"overwriting_existing\""));
    assert!(json.contains("\"conflict_status\": \"hardware_protected\""));

    let deserialized: VerificationReport =
        serde_json::from_str(&json).expect("Failed to deserialize report");
    assert_eq!(report, deserialized);
}

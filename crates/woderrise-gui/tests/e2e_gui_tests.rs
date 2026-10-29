//! Milestone M6: Comprehensive End-to-End GUI Integration Tests for Woderrise.
//!
//! Opaque-box headless integration tests verifying all 4 application screens
//! (Discover, Configuration Inspection & Installation, History & Rollback, Settings & Authentication),
//! user navigation flows, LIFO rollback unwinding, dirty-state recovery, rate limit synchronization,
//! hardware display preservation invariants, cache cleanup containment, and battery clock tick scoping.
//!
//! All tests run 100% headlessly without requiring `DISPLAY` or `WAYLAND_DISPLAY` and without
//! live network access.

use camino::Utf8PathBuf;
use chrono::Utc;
use std::collections::HashSet;
use std::fs;
use std::sync::Mutex;
use tempfile::tempdir;

use woderrise_core::models::{
    CatalogRepository, ConfigComponent, ConflictStatus, DetectedComponent, FileOperationRecord,
    HardwareConflictNotice, JournalEntry, OperationType, SecurityAuditReport, StagedFile,
    SyntaxFormat, TargetMapping, TransactionStatus, VerificationReport,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{ComponentDetection, TreeEntry};
use woderrise_github::rate_limit::{RateLimitResource, RateLimitStatus};
use woderrise_github::GitTree;
use woderrise_gui::app::{App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::history::{view_history, HistoryAction};
use woderrise_gui::screens::inspect::{InspectAction, InspectTab};
use woderrise_gui::screens::install::InstallPhase;
use woderrise_gui::screens::search::{CategoryFilter, SortOption};
use woderrise_gui::screens::settings::SettingsTab;
use woderrise_gui::widgets::diff::FileDiffReport;
use woderrise_transaction::RollbackReport;

/// Global environment mutex to serialize tests that temporarily adjust XDG environment variables.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Helper to create a genuine `CatalogRepository` fixture with detected components.
fn create_test_repository(
    id: u64,
    owner: &str,
    repo: &str,
    description: &str,
    component: ConfigComponent,
    topics: Vec<&str>,
    stars: u64,
) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(description.to_string()),
        stargazers_count: stars,
        default_branch: "main".to_string(),
        topics: topics.into_iter().map(String::from).collect(),
        pushed_at: Some(Utc::now()),
        cached_tree_sha: Some("tree_sha_fixed_12345".to_string()),
        etag: None,
        detected_components: vec![DetectedComponent {
            component,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "config".to_string(),
            entrypoint: "config.conf".to_string(),
            style_file: None,
            file_count: 2,
            total_size_bytes: 4096,
        }],
    }
}

/// Helper to create a genuine `JournalEntry` fixture.
fn create_test_journal_entry(
    id: &str,
    component: ConfigComponent,
    status: TransactionStatus,
    repo: &str,
    target_file: Utf8PathBuf,
    backup_dir: Utf8PathBuf,
) -> JournalEntry {
    JournalEntry {
        id: id.to_string(),
        timestamp: Utc::now(),
        component,
        status,
        source_repo: repo.to_string(),
        source_commit: "main_sha_abc123".to_string(),
        backup_directory: backup_dir,
        operations: vec![FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file,
            backup_file: Some("backup_copy.conf".into()),
            pre_sha256: Some("sha_pre_0000".to_string()),
            post_sha256: Some("sha_post_1111".to_string()),
            mode: Some("100644".to_string()),
        }],
    }
}

/// Drain all pending commands from the test channel.
fn drain_commands(rx: &mut tokio::sync::mpsc::Receiver<GuiCommand>) {
    while rx.try_recv().is_ok() {}
}

// =========================================================================
// Test 1: Full E2E Happy Path Journey Across All 4 Screens
// Screen 1 (Discover) -> Screen 2 (Inspect & Plan) -> Screen 3 (History) -> Screen 4 (Settings)
// =========================================================================
#[tokio::test]
async fn test_e2e_complete_happy_path_journey() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    // Initial Screen is Discover (Search)
    assert_eq!(app.active_screen, Screen::Search);
    let _ = app.view();

    let sample_repo = create_test_repository(
        101,
        "hyprland-community",
        "rice-deluxe",
        "Smooth Hyprland desktop config with Waybar",
        ConfigComponent::Hyprland,
        vec!["hyprland", "rice"],
        450,
    );
    app.search_state
        .catalog_repositories
        .push(sample_repo.clone());

    // 1. Discover -> Select repository
    let _ = app.update(Message::SelectRepo(sample_repo.clone()));
    assert_eq!(app.active_screen, Screen::Inspect);

    // Assert InspectPackage command dispatched
    let inspect_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected InspectPackage command");
    match inspect_cmd {
        GuiCommand::InspectPackage { repo, .. } => {
            assert_eq!(repo, "hyprland-community/rice-deluxe");
        }
        other => panic!("Unexpected command received: {:?}", other),
    }

    // 2. Deliver TreeLoaded event with Hyprland and Monitors components
    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".to_string(),
            entrypoint: "hyprland.conf".to_string(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new(
                    "hyprland.conf",
                    "100644",
                    "blob",
                    "blob_sha_hyprland_1111",
                    Some(2048),
                ),
                TreeEntry::new(
                    "keybinds.conf",
                    "100644",
                    "blob",
                    "blob_sha_keybinds_2222",
                    Some(1024),
                ),
            ],
            file_count: 2,
            total_size_bytes: 3072,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".to_string(),
            entrypoint: "monitors.conf".to_string(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "monitors.conf",
                "100644",
                "blob",
                "blob_sha_monitors_3333",
                Some(512),
            )],
            file_count: 1,
            total_size_bytes: 512,
            hardware_notices: vec![HardwareConflictNotice {
                file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
                description: "Hardware display configuration".to_string(),
                preserved_by_default: true,
            }],
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "hyprland-community/rice-deluxe".to_string(),
        tree: GitTree {
            sha: "tree_root_sha".to_string(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections,
    }));

    // Inspect Screen assertions: hardware safety invariant upheld
    let inspect = app
        .inspect_state
        .as_ref()
        .expect("InspectState must be initialized");
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    assert!(
        !inspect
            .selected_components
            .contains(&ConfigComponent::Monitors),
        "Hardware monitor component must never be auto-selected"
    );
    assert_eq!(inspect.selected_file_count(), 2);

    // Switch across all 5 inspect tabs
    let tabs = [
        InspectTab::Overview,
        InspectTab::Components,
        InspectTab::FileTree,
        InspectTab::Readme,
        InspectTab::DiffAndInstall,
    ];
    for tab in tabs {
        let _ = app.update(Message::SelectInspectTab(tab));
        assert_eq!(
            app.inspect_state.as_ref().unwrap().active_tab,
            tab,
            "Inspect tab must switch to {:?}",
            tab
        );
    }
    let _ = app.view();

    // 3. Initiate Installation -> transition to Install screen
    let _ = app.update(Message::InitiateInstall);
    assert_eq!(app.active_screen, Screen::Install);

    let stage_cmd = cmd_rx.recv().await.expect("Expected StagePackage command");
    let tx_id = match stage_cmd {
        GuiCommand::StagePackage {
            tx_id,
            component,
            repo,
            entries,
            ..
        } => {
            assert_eq!(component, ConfigComponent::Hyprland);
            assert_eq!(repo, "hyprland-community/rice-deluxe");
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].path, "hyprland.conf");
            assert_eq!(entries[1].path, "keybinds.conf");
            tx_id
        }
        other => panic!("Unexpected command received: {:?}", other),
    };

    // 4. Staging completed -> PendingApproval phase
    let dest_hyprland = Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf");
    let dest_keybinds = Utf8PathBuf::from("/home/user/.config/hypr/keybinds.conf");
    let staged_files = vec![
        StagedFile {
            relative_path: Utf8PathBuf::from("hyprland.conf"),
            target_destination: Some(dest_hyprland.clone()),
            blob_sha: Some("blob_sha_hyprland_1111".to_string()),
            file_size: 2048,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            file_mode: "100644".to_string(),
            permissions: 0o644,
            is_executable: false,
            is_symlink: false,
            syntax_valid: true,
            path_safe: true,
        },
        StagedFile {
            relative_path: Utf8PathBuf::from("keybinds.conf"),
            target_destination: Some(dest_keybinds.clone()),
            blob_sha: Some("blob_sha_keybinds_2222".to_string()),
            file_size: 1024,
            sha256: "ca978112ca1bbdcafac231b39a23dc4da7860814961409733a9e868278c993f9".to_string(),
            file_mode: "100644".to_string(),
            permissions: 0o644,
            is_executable: false,
            is_symlink: false,
            syntax_valid: true,
            path_safe: true,
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::StagingCompleted {
        tx_id: tx_id.clone(),
        report: VerificationReport {
            transaction_id: tx_id.clone(),
            component: ConfigComponent::Hyprland,
            total_files: 2,
            total_bytes: 3072,
            target_mappings: vec![
                TargetMapping {
                    component: ConfigComponent::Hyprland,
                    relative_path: Utf8PathBuf::from("hyprland.conf"),
                    blob_sha: "blob_sha_hyprland_1111".to_string(),
                    target_destination: dest_hyprland.clone(),
                    git_file_mode: "100644".to_string(),
                    expected_sha256:
                        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
                    is_hardware_protected: false,
                    conflict_status: ConflictStatus::NewFile,
                },
                TargetMapping {
                    component: ConfigComponent::Hyprland,
                    relative_path: Utf8PathBuf::from("keybinds.conf"),
                    blob_sha: "blob_sha_keybinds_2222".to_string(),
                    target_destination: dest_keybinds.clone(),
                    git_file_mode: "100644".to_string(),
                    expected_sha256:
                        "ca978112ca1bbdcafac231b39a23dc4da7860814961409733a9e868278c993f9".to_string(),
                    is_hardware_protected: false,
                    conflict_status: ConflictStatus::NewFile,
                },
            ],
            security_audit: SecurityAuditReport {
                scripts_detected: 0,
                procedural_directives_neutralized: 0,
                path_traversal_violations: 0,
                symlinks_rejected: 0,
                passed: true,
            },
            hardware_conflicts: vec![],
            passed: true,
        },
        staged_files: staged_files.clone(),
    }));

    let install = app
        .install_state
        .as_ref()
        .expect("InstallState must be present");
    assert_eq!(install.current_phase, InstallPhase::PendingApproval);
    assert_eq!(install.staged_files.len(), 2);
    let _ = app.view();

    // 5. User confirms installation -> ApplyInstallation dispatched
    let _ = app.update(Message::ConfirmInstall);
    let apply_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected ApplyInstallation command");
    match apply_cmd {
        GuiCommand::ApplyInstallation {
            tx_id: applied_tx_id,
            component,
            repo,
            staged_files: applied_staged,
            ..
        } => {
            assert_eq!(applied_tx_id, tx_id);
            assert_eq!(component, ConfigComponent::Hyprland);
            assert_eq!(repo, "hyprland-community/rice-deluxe");
            assert_eq!(applied_staged.len(), 2);
            assert_eq!(
                applied_staged[0].target_destination,
                Some(dest_hyprland.clone())
            );
            assert_eq!(
                applied_staged[1].target_destination,
                Some(dest_keybinds.clone())
            );
        }
        other => panic!("Unexpected command received: {:?}", other),
    }

    // 6. Installation finishes -> InstallCompleted -> User clicks Done -> transitions to History
    let journal_entry = create_test_journal_entry(
        &tx_id,
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        "hyprland-community/rice-deluxe",
        dest_hyprland.clone(),
        Utf8PathBuf::from("/backups/tx_happy"),
    );

    let _ = app.update(Message::Engine(EngineEvent::InstallCompleted {
        tx_id: tx_id.clone(),
        entry: journal_entry.clone(),
    }));
    let load_on_install = cmd_rx
        .recv()
        .await
        .expect("Expected LoadHistory after InstallCompleted");
    assert!(matches!(load_on_install, GuiCommand::LoadHistory));
    assert_eq!(
        app.install_state.as_ref().unwrap().current_phase,
        InstallPhase::Completed
    );
    let _ = app.view();

    let _ = app.update(Message::InstallDone);
    assert_eq!(app.active_screen, Screen::History);
    let history_cmd = cmd_rx.recv().await.expect("Expected LoadHistory command");
    assert!(matches!(history_cmd, GuiCommand::LoadHistory));

    // 7. History loaded -> single committed transaction with rollback enabled
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![journal_entry.clone()],
        active_tx: None,
    }));

    assert_eq!(app.history_state.transactions.len(), 1);
    assert!(app.history_state.transactions[0].is_latest_committed);
    assert_eq!(app.history_state.metrics.total_transactions, 1);
    assert_eq!(app.history_state.metrics.successful_installations, 1);
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 1);

    // Open rollback confirmation modal
    let hist_item = app.history_state.transactions[0].clone();
    let _ = app.update(Message::HistoryAction(HistoryAction::OpenRollbackConfirm(
        hist_item,
    )));
    assert!(app.history_state.confirm_rollback_modal.is_some());
    let _ = app.view();

    // Execute Rollback
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: tx_id.clone(),
        force_dirty: false,
    }));
    let rollback_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected RollbackTransaction command");
    match rollback_cmd {
        GuiCommand::RollbackTransaction {
            tx_id: r_tx,
            force_dirty,
        } => {
            assert_eq!(r_tx, tx_id);
            assert!(!force_dirty);
        }
        other => panic!("Unexpected command received: {:?}", other),
    }
    assert!(app.history_state.is_rolling_back);

    // Rollback completes
    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: tx_id.clone(),
            restored_files: vec![dest_hyprland],
            removed_files: vec![dest_keybinds],
            pruned_directories: vec![],
        },
    }));
    assert!(!app.history_state.is_rolling_back);
    let load_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected LoadHistory command after rollback");
    assert!(matches!(load_cmd, GuiCommand::LoadHistory));

    // Update history state to RolledBack
    let mut rolled_back_entry = journal_entry.clone();
    rolled_back_entry.status = TransactionStatus::RolledBack;
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![rolled_back_entry],
        active_tx: None,
    }));
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 0);

    // 8. Navigate to Settings screen (Screen 4)
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);
    let limits_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected FetchRateLimits command");
    assert!(matches!(limits_cmd, GuiCommand::FetchRateLimits));

    let settings_tabs = [
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
        SettingsTab::GitHub,
    ];
    for stab in settings_tabs {
        let _ = app.update(Message::SettingsTabSelected(stab));
        assert_eq!(app.settings_state.active_tab, stab);
    }
    let _ = app.view();
}

// =========================================================================
// Test 2: Screen 1 Query, Filter, and Sort Preservation on Back Navigation
// Discover <-> Inspect
// =========================================================================
#[tokio::test]
async fn test_e2e_screen1_query_and_filter_preservation_on_back() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    // Setup sample catalog repositories
    let repo1 = create_test_repository(
        1,
        "catppuccin",
        "hyprland-mocha",
        "Hyprland Mocha Theme",
        ConfigComponent::Hyprland,
        vec!["hyprland"],
        120,
    );
    let repo2 = create_test_repository(
        2,
        "alex",
        "waybar-clean",
        "Minimal status bar",
        ConfigComponent::Waybar,
        vec!["waybar"],
        340,
    );
    let repo3 = create_test_repository(
        3,
        "power",
        "waybar-power-menu",
        "Waybar power menu widgets",
        ConfigComponent::Waybar,
        vec!["waybar"],
        210,
    );

    app.search_state =
        woderrise_gui::screens::search::SearchState::new(vec![repo1, repo2.clone(), repo3]);

    // Set search query, filter, and sort on Discover screen
    let _ = app.update(Message::SearchInputChanged("waybar".to_string()));
    let _ = app.update(Message::FilterSelected(CategoryFilter::Waybar));
    let _ = app.update(Message::SortSelected(SortOption::Stars));

    assert_eq!(app.search_state.query, "waybar");
    assert_eq!(app.search_state.active_filter, CategoryFilter::Waybar);
    assert_eq!(app.search_state.sort_option, SortOption::Stars);

    let filtered = &app.search_state.filtered_repositories;
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].full_name, "alex/waybar-clean"); // Higher stars first
    assert_eq!(filtered[1].full_name, "power/waybar-power-menu");

    // Navigate to Inspect by selecting a repository
    let _ = app.update(Message::SelectRepo(repo2));
    assert_eq!(app.active_screen, Screen::Inspect);
    let _ = app.view();

    // Click Back to return to Discover screen
    let _ = app.update(Message::InspectAction(InspectAction::Back));
    assert_eq!(app.active_screen, Screen::Search);

    // Verify search query, category filter, and sort are 100% preserved
    assert_eq!(
        app.search_state.query, "waybar",
        "Search query must be preserved on Back"
    );
    assert_eq!(
        app.search_state.active_filter,
        CategoryFilter::Waybar,
        "Category filter must be preserved on Back"
    );
    assert_eq!(
        app.search_state.sort_option,
        SortOption::Stars,
        "Sort option must be preserved on Back"
    );

    let filtered_after_back = &app.search_state.filtered_repositories;
    assert_eq!(
        filtered_after_back.len(),
        2,
        "Filtered repositories must match before navigation"
    );
    assert_eq!(filtered_after_back[0].full_name, "alex/waybar-clean");
    assert_eq!(filtered_after_back[1].full_name, "power/waybar-power-menu");
    let _ = app.view();
}

// =========================================================================
// Test 3: Screen 2 Granular File Selection, Destination Resolution, and Diff Planning
// =========================================================================
#[tokio::test]
async fn test_e2e_screen2_granular_file_selection_and_planning() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let repo = create_test_repository(
        201,
        "steven",
        "dotfiles-hypr",
        "Modular hyprland configuration",
        ConfigComponent::Hyprland,
        vec!["hyprland"],
        85,
    );
    let _ = app.update(Message::SelectRepo(repo));
    drain_commands(&mut cmd_rx);

    let detection = ComponentDetection {
        component: ConfigComponent::Hyprland,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: "hypr".to_string(),
        entrypoint: "hyprland.conf".to_string(),
        style_file: None,
        matched_entries: vec![
            TreeEntry::new(
                "hyprland.conf",
                "100644",
                "blob",
                "sha_hyprland_conf_blob",
                Some(1500),
            ),
            TreeEntry::new(
                "keybinds.conf",
                "100644",
                "blob",
                "sha_keybinds_conf_blob",
                Some(800),
            ),
            TreeEntry::new(
                "animations.conf",
                "100644",
                "blob",
                "sha_animations_conf_blob",
                Some(600),
            ),
        ],
        file_count: 3,
        total_size_bytes: 2900,
        hardware_notices: vec![],
    };

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "steven/dotfiles-hypr".to_string(),
        tree: GitTree {
            sha: "root_sha".to_string(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections: vec![detection],
    }));

    let inspect = app.inspect_state.as_mut().unwrap();
    assert_eq!(inspect.selected_file_count(), 3);
    assert!(inspect.is_file_selected("hyprland.conf", ConfigComponent::Hyprland));
    assert!(inspect.is_file_selected("keybinds.conf", ConfigComponent::Hyprland));
    assert!(inspect.is_file_selected("animations.conf", ConfigComponent::Hyprland));

    // Granularly deselect animations.conf
    let _ = app.update(Message::InspectAction(InspectAction::ToggleFile {
        path: "animations.conf".to_string(),
        component: ConfigComponent::Hyprland,
    }));

    let inspect = app.inspect_state.as_ref().unwrap();
    assert_eq!(inspect.selected_file_count(), 2);
    assert!(!inspect.is_file_selected("animations.conf", ConfigComponent::Hyprland));
    assert!(inspect.deselected_files.contains("animations.conf"));

    // Verify target path resolution with custom isolated XDG paths
    let tmp = tempdir().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    let mappings = inspect.resolve_target_mappings_with_xdg(&xdg);
    assert_eq!(mappings.len(), 2);
    let resolved_destinations: HashSet<Utf8PathBuf> =
        mappings.iter().map(|m| m.target_destination.clone()).collect();

    assert!(resolved_destinations.contains(&base.join(".config/hypr/hyprland.conf")));
    assert!(resolved_destinations.contains(&base.join(".config/hypr/keybinds.conf")));
    assert!(!resolved_destinations.contains(&base.join(".config/hypr/animations.conf")));

    // Deliver diff preview report
    let diff = FileDiffReport::compute("hyprland.conf", "# Old config\n", "# New config\n");
    let _ = app.update(Message::Engine(EngineEvent::DiffComputed {
        repo: "steven/dotfiles-hypr".to_string(),
        diffs: vec![diff],
    }));

    let inspect = app.inspect_state.as_ref().unwrap();
    assert_eq!(inspect.diff_reports.len(), 1);
    assert_eq!(inspect.diff_reports[0].file_path, "hyprland.conf");

    // Initiate install -> ensure StagePackage entries only contain the 2 selected files
    let _ = app.update(Message::InitiateInstall);
    let stage_cmd = cmd_rx.recv().await.expect("Expected StagePackage command");
    match stage_cmd {
        GuiCommand::StagePackage { entries, .. } => {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].path, "hyprland.conf");
            assert_eq!(entries[1].path, "keybinds.conf");
        }
        other => panic!("Unexpected command: {:?}", other),
    }
}

// =========================================================================
// Test 4: Hardware Display Preservation Invariant
// Monitors config must NEVER be auto-selected and must be excluded from staging
// =========================================================================
#[tokio::test]
async fn test_e2e_screen2_hardware_display_preservation_invariant() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let repo = create_test_repository(
        301,
        "pro-user",
        "multi-monitor-hypr",
        "Dual monitor rice with custom refresh rates",
        ConfigComponent::Hyprland,
        vec!["hyprland"],
        190,
    );
    let _ = app.update(Message::SelectRepo(repo));
    drain_commands(&mut cmd_rx);

    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".to_string(),
            entrypoint: "hyprland.conf".to_string(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "hyprland.conf",
                "100644",
                "blob",
                "blob_sha_hyprland_main",
                Some(3000),
            )],
            file_count: 1,
            total_size_bytes: 3000,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".to_string(),
            entrypoint: "monitors.conf".to_string(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new(
                    "monitors.conf",
                    "100644",
                    "blob",
                    "blob_sha_monitors_conf",
                    Some(400),
                ),
                TreeEntry::new(
                    "workspaces.conf",
                    "100644",
                    "blob",
                    "blob_sha_workspaces_conf",
                    Some(300),
                ),
            ],
            file_count: 2,
            total_size_bytes: 700,
            hardware_notices: vec![HardwareConflictNotice {
                file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
                description: "Hardware monitor configuration".to_string(),
                preserved_by_default: true,
            }],
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "pro-user/multi-monitor-hypr".to_string(),
        tree: GitTree {
            sha: "tree_root".to_string(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections,
    }));

    // Invariant check on initial tree load: Monitors is NOT selected
    let inspect = app.inspect_state.as_ref().unwrap();
    assert!(inspect.selected_components.contains(&ConfigComponent::Hyprland));
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "Hardware protection invariant: ConfigComponent::Monitors must NEVER be auto-selected"
    );

    // Invariant check on SelectAllComponents: Monitors remains excluded
    let _ = app.update(Message::InspectAction(InspectAction::SelectAllComponents));
    let inspect = app.inspect_state.as_ref().unwrap();
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "SelectAllComponents must strictly preserve hardware monitor exclusion"
    );

    // Initiate staging -> verify monitors.conf and workspaces.conf are excluded from entries
    let _ = app.update(Message::InitiateInstall);
    let stage_cmd = cmd_rx.recv().await.expect("Expected StagePackage command");
    match stage_cmd {
        GuiCommand::StagePackage { entries, .. } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, "hyprland.conf");
            for entry in entries {
                assert_ne!(entry.path, "monitors.conf");
                assert_ne!(entry.path, "workspaces.conf");
            }
        }
        other => panic!("Unexpected command: {:?}", other),
    }
}

// =========================================================================
// Test 5: Screen 3 Sequential Multi-Transaction LIFO Rollback Chain
// Install Tx 1 -> Install Tx 2 -> Strictly Tx 2 rollback enabled ->
// Executing Tx 2 rollback enables Tx 1 rollback -> Executing Tx 1 rollback
// =========================================================================
#[tokio::test]
async fn test_e2e_screen3_sequential_multi_tx_lifo_rollback_chain() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let tx1 = create_test_journal_entry(
        "tx_seq_01",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        "author/hypr-rice",
        Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
        Utf8PathBuf::from("/backups/tx_seq_01"),
    );

    let tx2 = create_test_journal_entry(
        "tx_seq_02",
        ConfigComponent::Waybar,
        TransactionStatus::Committed,
        "author/waybar-rice",
        Utf8PathBuf::from("/home/user/.config/waybar/config.jsonc"),
        Utf8PathBuf::from("/backups/tx_seq_02"),
    );

    // Deliver chronological journal: [tx1, tx2] (tx2 is newer)
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1.clone(), tx2.clone()],
        active_tx: None,
    }));

    assert_eq!(app.history_state.transactions.len(), 2);
    // Index 0 is tx2 (newest); Index 1 is tx1 (older)
    assert_eq!(app.history_state.transactions[0].id, "tx_seq_02");
    assert_eq!(app.history_state.transactions[1].id, "tx_seq_01");

    // Strictly tx2 has rollback enabled; tx1 is disabled by LIFO rule
    assert!(
        app.history_state.transactions[0].is_latest_committed,
        "Newest transaction tx2 must have rollback enabled"
    );
    assert!(
        !app.history_state.transactions[1].is_latest_committed,
        "Older transaction tx1 must NOT have rollback enabled while tx2 is committed"
    );

    // Execute rollback on tx2
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_seq_02".to_string(),
        force_dirty: false,
    }));
    let rollback_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected RollbackTransaction command");
    match rollback_cmd {
        GuiCommand::RollbackTransaction { tx_id, .. } => {
            assert_eq!(tx_id, "tx_seq_02");
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_seq_02".to_string(),
            restored_files: vec![Utf8PathBuf::from("/home/user/.config/waybar/config.jsonc")],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    assert!(!app.history_state.is_rolling_back);
    let load_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected LoadHistory command after tx2 rollback");
    assert!(matches!(load_cmd, GuiCommand::LoadHistory));

    // Deliver updated journal where tx2 is RolledBack and tx1 is Committed
    let mut tx2_rolled_back = tx2.clone();
    tx2_rolled_back.status = TransactionStatus::RolledBack;

    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1.clone(), tx2_rolled_back.clone()],
        active_tx: None,
    }));

    // Now tx2 rollback is disabled; tx1 rollback is dynamically enabled!
    assert_eq!(app.history_state.transactions[0].id, "tx_seq_02");
    assert!(
        !app.history_state.transactions[0].is_latest_committed,
        "Rolled back tx2 must have rollback disabled"
    );

    assert_eq!(app.history_state.transactions[1].id, "tx_seq_01");
    assert!(
        app.history_state.transactions[1].is_latest_committed,
        "Tx1 must now have rollback enabled after tx2 unwound"
    );

    // Execute rollback on tx1
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_seq_01".to_string(),
        force_dirty: false,
    }));
    let rollback_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected RollbackTransaction command for tx1");
    match rollback_cmd {
        GuiCommand::RollbackTransaction { tx_id, .. } => {
            assert_eq!(tx_id, "tx_seq_01");
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_seq_01".to_string(),
            restored_files: vec![Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    assert!(!app.history_state.is_rolling_back);
    let load_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected LoadHistory command after tx1 rollback");
    assert!(matches!(load_cmd, GuiCommand::LoadHistory));

    // Both transactions now RolledBack
    let mut tx1_rolled_back = tx1.clone();
    tx1_rolled_back.status = TransactionStatus::RolledBack;

    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1_rolled_back, tx2_rolled_back],
        active_tx: None,
    }));

    assert_eq!(app.history_state.metrics.rollback_eligible_count, 0);
    assert!(!app.history_state.transactions[0].is_latest_committed);
    assert!(!app.history_state.transactions[1].is_latest_committed);
}

// =========================================================================
// Test 6: Screen 3 Dirty State Modal and Force Rollback Workflow
// =========================================================================
#[tokio::test]
async fn test_e2e_screen3_dirty_state_modal_and_force_rollback() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let tx = create_test_journal_entry(
        "tx_dirty_01",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        "author/dirty-dotfiles",
        Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
        Utf8PathBuf::from("/backups/tx_dirty_01"),
    );

    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx],
        active_tx: None,
    }));

    // User attempts normal rollback
    let _ = app.update(Message::RequestRollbackTx {
        tx_id: "tx_dirty_01".to_string(),
        force_dirty: false,
    });
    drain_commands(&mut cmd_rx);

    // Background engine detects external edit and returns dirty error
    let dirty_error =
        "Dirty state detected: file '/home/user/.config/hypr/hyprland.conf' modified externally";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_01".to_string(),
        error: dirty_error.to_string(),
    }));

    // Assert DirtyStateModal opened with modified file path
    assert!(app.history_state.dirty_modal.is_some());
    let modal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.transaction_id, "tx_dirty_01");
    assert_eq!(modal.modified_files.len(), 1);
    assert_eq!(
        modal.modified_files[0],
        Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")
    );

    // Render history view with dirty modal open
    let _ = view_history(
        &app.history_state,
        &app.active_theme,
        Message::HistoryAction,
    );

    // User opts to force rollback despite dirty state
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_dirty_01".to_string(),
        force_dirty: true,
    }));

    // Dirty modal dismissed, is_rolling_back set to true
    assert!(app.history_state.dirty_modal.is_none());
    assert!(app.history_state.is_rolling_back);

    // GuiCommand dispatched with force_dirty: true
    let force_cmd = cmd_rx.recv().await.expect("Expected force rollback command");
    match force_cmd {
        GuiCommand::RollbackTransaction {
            tx_id, force_dirty, ..
        } => {
            assert_eq!(tx_id, "tx_dirty_01");
            assert!(force_dirty, "force_dirty must be true");
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    // Rollback finishes
    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_dirty_01".to_string(),
            restored_files: vec![Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    assert!(!app.history_state.is_rolling_back);
    let load_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected LoadHistory command after force rollback");
    assert!(matches!(load_cmd, GuiCommand::LoadHistory));
}

// =========================================================================
// Test 7: Screen 3 Empty State Navigation to Discover
// =========================================================================
#[tokio::test]
async fn test_e2e_screen3_empty_state_navigation_to_discover() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    // Navigate to History screen
    let _ = app.update(Message::NavigateTo(Screen::History));
    assert_eq!(app.active_screen, Screen::History);
    drain_commands(&mut cmd_rx);

    // Deliver empty transaction history
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![],
        active_tx: None,
    }));

    assert!(app.history_state.transactions.is_empty());
    assert_eq!(app.history_state.metrics.total_transactions, 0);

    // Verify empty state view renders successfully
    let _ = view_history(
        &app.history_state,
        &app.active_theme,
        Message::HistoryAction,
    );

    // Trigger NavigateDiscover action from empty state banner button
    let _ = app.update(Message::HistoryAction(HistoryAction::NavigateDiscover));
    assert_eq!(
        app.active_screen,
        Screen::Search,
        "Empty state button must navigate to Discover (Search) screen"
    );
    let _ = app.view();
}

// =========================================================================
// Test 8: Screen 4 PAT Authentication & Live Quota Synchronization
// Settings <-> Header <-> Discover
// =========================================================================
#[tokio::test]
async fn test_e2e_screen4_pat_auth_quota_sync_across_header_and_screens() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);
    drain_commands(&mut cmd_rx);

    // Unauthenticated initial state
    assert!(!app.settings_state.is_authenticated());
    assert!(!app.connection_state.is_authenticated);

    // 1. Enter valid PAT token
    let test_pat = "ghp_validTestPersonalAccessToken998877";
    let _ = app.update(Message::SettingsTokenChanged(test_pat.to_string()));
    assert_eq!(app.settings_state.token_input, test_pat);

    // 2. Click Save Token
    let _ = app.update(Message::SettingsSaveToken);
    assert!(app.settings_state.is_busy);

    let token_cmd = cmd_rx.recv().await.expect("Expected SetGitHubToken command");
    match token_cmd {
        GuiCommand::SetGitHubToken { token } => {
            assert_eq!(token, Some(test_pat.to_string()));
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    // 3. Deliver elevated rate limit update (authenticated tier: 5000 core, 30 search)
    let reset_core = Utc::now() + chrono::Duration::hours(1);
    let reset_search = Utc::now() + chrono::Duration::minutes(1);

    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: RateLimitStatus {
            resource: RateLimitResource::Core,
            limit: 5000,
            remaining: 4990,
            reset_time: reset_core,
            used: 10,
        },
        search: RateLimitStatus {
            resource: RateLimitResource::Search,
            limit: 30,
            remaining: 28,
            reset_time: reset_search,
            used: 2,
        },
    }));

    // Verify all models and gauges reflect the authenticated tier
    assert!(app.settings_state.is_authenticated());
    assert!(app.connection_state.is_authenticated);
    assert_eq!(app.connection_state.core_limit, 5000);
    assert_eq!(app.connection_state.core_remaining, 4990);
    assert_eq!(app.connection_state.search_limit, 30);
    assert_eq!(app.connection_state.search_remaining, 28);

    // Discover screen gauge synchronized
    assert_eq!(app.search_state.rate_limits.core_limit, 5000);
    assert_eq!(app.search_state.rate_limits.core_remaining, 4990);
    assert_eq!(app.search_state.rate_limits.search_limit, 30);
    assert_eq!(app.search_state.rate_limits.search_remaining, 28);
    let _ = app.view();

    // 4. Click Clear Token
    let _ = app.update(Message::SettingsClearToken);
    assert!(app.settings_state.is_busy);

    let clear_cmd = cmd_rx
        .recv()
        .await
        .expect("Expected SetGitHubToken None command");
    match clear_cmd {
        GuiCommand::SetGitHubToken { token } => {
            assert!(token.is_none());
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    // 5. Deliver unauthenticated rate limit update (unauthenticated tier: 60 core, 10 search)
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: RateLimitStatus {
            resource: RateLimitResource::Core,
            limit: 60,
            remaining: 58,
            reset_time: reset_core,
            used: 2,
        },
        search: RateLimitStatus {
            resource: RateLimitResource::Search,
            limit: 10,
            remaining: 9,
            reset_time: reset_search,
            used: 1,
        },
    }));

    // Verify all models and gauges revert to unauthenticated limits
    assert!(!app.settings_state.is_authenticated());
    assert!(!app.connection_state.is_authenticated);
    assert_eq!(app.connection_state.core_limit, 60);
    assert_eq!(app.connection_state.core_remaining, 58);
    assert_eq!(app.connection_state.search_limit, 10);
    assert_eq!(app.connection_state.search_remaining, 9);

    assert_eq!(app.search_state.rate_limits.core_limit, 60);
    assert_eq!(app.search_state.rate_limits.search_limit, 10);
    let _ = app.view();
}

// =========================================================================
// Test 9: Screen 4 Safe Cache Cleanup Directory Containment & Preservation
// Purges cache staging & blobs while strictly preserving config, data, and state
// =========================================================================
#[test]
fn test_e2e_screen4_safe_cache_cleanup_preserves_history_and_settings() {
    let _guard = ENV_LOCK.lock().unwrap();

    let tmp = tempdir().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);

    // 1. Establish persistent directories
    let config_dir = xdg.woderrise_config_dir();
    let data_dir = xdg.woderrise_data_dir();
    let state_dir = xdg.woderrise_state_dir();
    let cache_dir = xdg.woderrise_cache_dir();

    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    // 2. Populate critical persistent files
    let settings_file = xdg.settings_file();
    fs::write(&settings_file, b"{\"github_token\":\"ghp_secure_persistent\"}").unwrap();

    let journal_file = xdg.journal_file();
    fs::write(
        &journal_file,
        b"{\"transactions\":[{\"id\":\"tx_persistent_01\"}]}",
    )
    .unwrap();

    let backup_dir = xdg.backup_dir("tx_persistent_01").unwrap();
    fs::create_dir_all(&backup_dir).unwrap();
    let backup_snapshot = backup_dir.join("hyprland.conf.bak");
    fs::write(&backup_snapshot, b"persistent backup copy").unwrap();

    // 3. Populate cache directory with staging transactions and loose blobs
    let staging_tx = xdg.staging_dir("tx_ephemeral_to_purge").unwrap();
    fs::create_dir_all(&staging_tx).unwrap();
    let staged_file = staging_tx.join("staged_component.conf");
    fs::write(&staged_file, b"ephemeral staging bytes").unwrap();

    let loose_cache_blob = cache_dir.join("cached_avatar.png");
    fs::write(&loose_cache_blob, b"ephemeral avatar blob").unwrap();

    // Override XDG environment variables for the test process
    let orig_cache = std::env::var("XDG_CACHE_HOME").ok();
    let orig_config = std::env::var("XDG_CONFIG_HOME").ok();
    let orig_data = std::env::var("XDG_DATA_HOME").ok();
    let orig_state = std::env::var("XDG_STATE_HOME").ok();

    std::env::set_var("XDG_CACHE_HOME", xdg.cache_home.as_std_path());
    std::env::set_var("XDG_CONFIG_HOME", xdg.config_home.as_std_path());
    std::env::set_var("XDG_DATA_HOME", xdg.data_home.as_std_path());
    std::env::set_var("XDG_STATE_HOME", xdg.state_home.as_std_path());

    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Initial cache stats should reflect staged and loose files
    app.settings_state.cache_stats =
        woderrise_gui::screens::settings::calculate_directory_stats(&cache_dir).unwrap();
    assert!(app.settings_state.cache_stats.file_count >= 2);
    assert!(app.settings_state.cache_stats.total_bytes > 0);

    // 4. Trigger Clear Cache action via GUI message
    let _ = app.update(Message::SettingsClearCache);

    // Restore environment variables immediately
    match orig_cache {
        Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
        None => std::env::remove_var("XDG_CACHE_HOME"),
    }
    match orig_config {
        Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }
    match orig_data {
        Some(v) => std::env::set_var("XDG_DATA_HOME", v),
        None => std::env::remove_var("XDG_DATA_HOME"),
    }
    match orig_state {
        Some(v) => std::env::set_var("XDG_STATE_HOME", v),
        None => std::env::remove_var("XDG_STATE_HOME"),
    }

    // 5. Assert cache stats reset and notice message displayed
    assert_eq!(app.settings_state.cache_stats.file_count, 0);
    assert_eq!(app.settings_state.cache_stats.total_bytes, 0);
    assert!(app.settings_state.status_notice.is_some());
    let notice = app.settings_state.status_notice.as_ref().unwrap();
    assert!(notice.0.contains("Cache cleaned successfully"));
    assert!(!notice.1); // Not an error

    // 6. Assert ephemeral staging files removed
    assert!(
        !staging_tx.exists(),
        "Staging transaction directory must be removed"
    );
    assert!(
        !loose_cache_blob.exists(),
        "Loose cache blob must be removed"
    );

    // 7. CRITICAL CONTAINMENT INVARIANTS: Non-cache directories MUST BE 100% PRESERVED
    assert!(
        settings_file.exists(),
        "CRITICAL: settings.json was deleted by cache cleanup!"
    );
    assert_eq!(
        fs::read_to_string(&settings_file).unwrap(),
        "{\"github_token\":\"ghp_secure_persistent\"}"
    );

    assert!(
        journal_file.exists(),
        "CRITICAL: journal.json was deleted by cache cleanup!"
    );
    assert_eq!(
        fs::read_to_string(&journal_file).unwrap(),
        "{\"transactions\":[{\"id\":\"tx_persistent_01\"}]}"
    );

    assert!(
        backup_snapshot.exists(),
        "CRITICAL: backup snapshot was deleted by cache cleanup!"
    );
    assert_eq!(
        fs::read_to_string(&backup_snapshot).unwrap(),
        "persistent backup copy"
    );
}

// =========================================================================
// Test 10: Battery Conservation Clock Tick Subscription Scoping
// Clock tick is scoped strictly to Settings screen or active connection tests
// =========================================================================
#[test]
fn test_e2e_battery_clock_tick_subscription_scoping() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Screen 1: Discover (Search) -> Idle -> Clock tick must NOT be needed
    assert_eq!(app.active_screen, Screen::Search);
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must be disabled on Discover screen when idle to conserve battery"
    );

    // Screen 2: Inspect -> Idle -> Clock tick must NOT be needed
    let _ = app.update(Message::NavigateTo(Screen::Inspect));
    assert_eq!(app.active_screen, Screen::Inspect);
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must be disabled on Inspect screen when idle"
    );

    // Screen 2: Install -> Idle -> Clock tick must NOT be needed
    app.active_screen = Screen::Install;
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must be disabled on Install screen when idle"
    );

    // Screen 3: History -> Idle -> Clock tick must NOT be needed
    let _ = app.update(Message::NavigateTo(Screen::History));
    assert_eq!(app.active_screen, Screen::History);
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must be disabled on History screen when idle"
    );

    // Screen 4: Settings -> Clock tick MUST BE needed for live countdown timers
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);
    assert!(
        app.is_clock_tick_needed(),
        "Clock tick MUST be enabled on Settings screen for live quota countdowns"
    );

    // Navigate back to Discover -> Clock tick disables again
    let _ = app.update(Message::NavigateTo(Screen::Search));
    assert_eq!(app.active_screen, Screen::Search);
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must disable immediately upon navigating away from Settings"
    );

    // Active connection test triggers clock tick regardless of active screen
    let _ = app.update(Message::SettingsTestConnection);
    assert!(app.is_testing_connection);
    assert!(
        app.is_clock_tick_needed(),
        "Clock tick must be active during live connection testing"
    );

    // Connection test finishes -> Clock tick returns to idle (disabled)
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: RateLimitStatus {
            resource: RateLimitResource::Core,
            limit: 60,
            remaining: 60,
            reset_time: Utc::now(),
            used: 0,
        },
        search: RateLimitStatus {
            resource: RateLimitResource::Search,
            limit: 10,
            remaining: 10,
            reset_time: Utc::now(),
            used: 0,
        },
    }));
    assert!(!app.is_testing_connection);
    assert!(
        !app.is_clock_tick_needed(),
        "Clock tick must disable when connection test completes"
    );
}

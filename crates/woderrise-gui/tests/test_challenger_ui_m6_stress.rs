//! Adversarial empirical stress test suite for Milestone M6:
//! Comprehensive E2E UI Verification, Responsiveness & Hardening.
//!
//! Stress-tests:
//! 1. Rapid cross-screen navigation, uninitialized screen fallbacks, and out-of-order async event handling
//! 2. Multi-vector search query, category filter, and sort option preservation & clear invariants
//! 3. Deep multi-transaction sequential LIFO rollback chain with interleaved failed transactions
//! 4. Adversarial dirty-state error message parsing and forced rollback workflows
//! 5. Headless view rendering across extreme boundary states, themes, and modal combinations
//! 6. Strict hardware display protection under adversarial selection inputs
//! 7. Rapid search cancellation, debounce flooding, and channel resilience

use camino::Utf8PathBuf;
use chrono::{Duration, Utc};
use iced::Theme;

use woderrise_core::models::{
    CatalogRepository, ConfigComponent, DetectedComponent, FileOperationRecord,
    HardwareConflictNotice, JournalEntry, OperationType, SyntaxFormat, TransactionStatus,
};
use woderrise_detect::{ComponentDetection, ProceduralDirective, TreeEntry};
use woderrise_github::rate_limit::{RateLimitResource, RateLimitStatus};
use woderrise_github::GitTree;
use woderrise_gui::app::{App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::history::HistoryAction;
use woderrise_gui::screens::inspect::{InspectAction, InspectTab};
use woderrise_gui::screens::install::{InstallPhase, InstallState};
use woderrise_gui::screens::search::{CategoryFilter, SortOption};
use woderrise_transaction::RollbackReport;

fn make_test_repo(id: u64, owner: &str, repo: &str, stars: u64, comp: ConfigComponent) -> CatalogRepository {
    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(format!("Repository {}/{} for Wayland rice", owner, repo)),
        stargazers_count: stars,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".into(), "waybar".into()],
        pushed_at: Some(Utc::now()),
        cached_tree_sha: Some("sha_tree_abc".into()),
        etag: None,
        detected_components: vec![DetectedComponent {
            component: comp,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "config".into(),
            entrypoint: "config.conf".into(),
            style_file: None,
            file_count: 2,
            total_size_bytes: 2048,
        }],
    }
}

fn make_journal_tx(
    id: &str,
    comp: ConfigComponent,
    status: TransactionStatus,
    repo: &str,
    target: Utf8PathBuf,
) -> JournalEntry {
    JournalEntry {
        id: id.to_string(),
        timestamp: Utc::now(),
        component: comp,
        status,
        source_repo: repo.to_string(),
        source_commit: "commit_abc123".to_string(),
        backup_directory: Utf8PathBuf::from(format!("/backups/{}", id)),
        operations: vec![FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file: target,
            backup_file: Some("backup.conf".into()),
            pre_sha256: Some("sha_pre_000".into()),
            post_sha256: Some("sha_post_111".into()),
            mode: Some("100644".into()),
        }],
    }
}

fn drain_commands(rx: &mut tokio::sync::mpsc::Receiver<GuiCommand>) {
    while rx.try_recv().is_ok() {}
}

// =========================================================================
// 1. Stress Test: Rapid Cross-Screen Navigation & Uninitialized Screen Fallbacks
// =========================================================================
#[tokio::test]
async fn test_stress_rapid_cross_screen_navigation_and_uninitialized_fallbacks() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(100);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    // Initial screen is Search
    assert_eq!(app.active_screen, Screen::Search);
    let _ = app.view();

    // Rapid cyclical navigation across all screens, specifically testing
    // uninitialized inspect_state (None) and install_state (None)
    let screens = [
        Screen::Inspect,
        Screen::Install,
        Screen::History,
        Screen::Settings,
        Screen::Search,
        Screen::Install,
        Screen::Inspect,
        Screen::Settings,
        Screen::History,
        Screen::Search,
    ];

    for &target in &screens {
        let _ = app.update(Message::NavigateTo(target));
        assert_eq!(app.active_screen, target);
        // app.view() MUST NOT panic when inspecting or installing with None state
        let _element = app.view();
    }

    // Deliver asynchronous events while on an unrelated active screen
    let repo = make_test_repo(1, "hostile", "rice", 999, ConfigComponent::Hyprland);
    app.active_screen = Screen::Settings;

    // SearchCompleted arrives while on Settings
    let _ = app.update(Message::Engine(EngineEvent::SearchCompleted {
        query: "waybar".into(),
        results: vec![repo.clone()],
        search_quota: RateLimitStatus {
            resource: RateLimitResource::Search,
            limit: 30,
            remaining: 29,
            reset_time: Utc::now() + Duration::minutes(1),
            used: 1,
        },
    }));
    assert_eq!(app.active_screen, Screen::Settings);
    assert_eq!(app.search_state.catalog_repositories.len(), 1);

    // TreeLoaded arrives while on History
    app.active_screen = Screen::History;
    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "hostile/rice".into(),
        tree: GitTree {
            sha: "tree_sha".into(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections: vec![],
    }));
    assert_eq!(app.active_screen, Screen::History);

    // StagingProgress arrives while on Search
    app.active_screen = Screen::Search;
    let _ = app.update(Message::Engine(EngineEvent::StagingProgress {
        tx_id: "tx_mock_1".into(),
        file_path: "hyprland.conf".into(),
        index: 1,
        total: 2,
        bytes_downloaded: 1024,
    }));
    assert_eq!(app.active_screen, Screen::Search);

    let _ = app.view();
}

// =========================================================================
// 2. Stress Test: Multi-Vector Search Query, Filter, and Sort Preservation
// =========================================================================
#[tokio::test]
async fn test_stress_search_query_preservation_across_all_navigation_vectors() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let repo1 = make_test_repo(10, "hypr", "rice-alpha", 50, ConfigComponent::Hyprland);
    let repo2 = make_test_repo(20, "hypr", "rice-beta", 300, ConfigComponent::Hyprland);
    let repo3 = make_test_repo(30, "waybar", "bar-minimal", 120, ConfigComponent::Waybar);

    app.search_state = woderrise_gui::screens::search::SearchState::new(vec![
        repo1.clone(),
        repo2.clone(),
        repo3.clone(),
    ]);

    // Apply hostile query with regex chars and unicode
    let hostile_query = "hypr.*[0-9]+🦀";
    let _ = app.update(Message::SearchInputChanged(hostile_query.to_string()));
    let _ = app.update(Message::FilterSelected(CategoryFilter::Hyprland));
    let _ = app.update(Message::SortSelected(SortOption::Stars));

    assert_eq!(app.search_state.query, hostile_query);
    assert_eq!(app.search_state.active_filter, CategoryFilter::Hyprland);
    assert_eq!(app.search_state.sort_option, SortOption::Stars);

    // Navigation Vector 1: Select repo -> Inspect screen -> Back via InspectAction::Back
    let _ = app.update(Message::SelectRepo(repo2.clone()));
    assert_eq!(app.active_screen, Screen::Inspect);
    drain_commands(&mut cmd_rx);

    let _ = app.update(Message::InspectAction(InspectAction::Back));
    assert_eq!(app.active_screen, Screen::Search);
    assert_eq!(app.search_state.query, hostile_query);
    assert_eq!(app.search_state.active_filter, CategoryFilter::Hyprland);
    assert_eq!(app.search_state.sort_option, SortOption::Stars);

    // Navigation Vector 2: Select repo -> Inspect screen -> Back via Navbar NavigateTo(Search)
    let _ = app.update(Message::SelectRepo(repo1.clone()));
    assert_eq!(app.active_screen, Screen::Inspect);
    drain_commands(&mut cmd_rx);

    let _ = app.update(Message::NavigateTo(Screen::Search));
    assert_eq!(app.active_screen, Screen::Search);
    assert_eq!(app.search_state.query, hostile_query);
    assert_eq!(app.search_state.active_filter, CategoryFilter::Hyprland);
    assert_eq!(app.search_state.sort_option, SortOption::Stars);

    // Navigation Vector 3: Full loop: Discover -> History -> Settings -> Discover
    let _ = app.update(Message::NavigateTo(Screen::History));
    assert_eq!(app.active_screen, Screen::History);
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);
    let _ = app.update(Message::NavigateTo(Screen::Search));
    assert_eq!(app.active_screen, Screen::Search);
    assert_eq!(app.search_state.query, hostile_query);
    assert_eq!(app.search_state.active_filter, CategoryFilter::Hyprland);
    assert_eq!(app.search_state.sort_option, SortOption::Stars);

    // Navigation Vector 4: Escape key clear resets query, filter, and sort
    let _ = app.update(Message::SearchClear);
    assert!(app.search_state.query.is_empty());
    assert_eq!(app.search_state.active_filter, CategoryFilter::All);
    assert_eq!(app.search_state.sort_option, SortOption::Relevance);
    assert_eq!(app.search_state.filtered_repositories.len(), 3);
}

// =========================================================================
// 3. Stress Test: Deep Multi-Transaction LIFO Rollback Chain with Interleaved Failures
// Tx1 (Committed) -> Tx2 (Committed) -> Tx3 (Failed) -> Tx4 (Committed) -> Tx5 (Committed)
// =========================================================================
#[tokio::test]
async fn test_stress_deep_lifo_rollback_chain_with_interleaved_failures() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let tx1 = make_journal_tx("tx_01", ConfigComponent::Hyprland, TransactionStatus::Committed, "user/t1", "/etc/c1".into());
    let tx2 = make_journal_tx("tx_02", ConfigComponent::Waybar, TransactionStatus::Committed, "user/t2", "/etc/c2".into());
    let tx3 = make_journal_tx("tx_03", ConfigComponent::Hyprlock, TransactionStatus::Failed, "user/t3", "/etc/c3".into());
    let tx4 = make_journal_tx("tx_04", ConfigComponent::Wallpapers, TransactionStatus::Committed, "user/t4", "/etc/c4".into());
    let tx5 = make_journal_tx("tx_05", ConfigComponent::Hypridle, TransactionStatus::Committed, "user/t5", "/etc/c5".into());

    // Chronological journal order: [tx1, tx2, tx3, tx4, tx5]
    let journal = vec![tx1.clone(), tx2.clone(), tx3.clone(), tx4.clone(), tx5.clone()];
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: journal.clone(),
        active_tx: None,
    }));

    // In HistoryState, entries are reversed: Index 0 is tx5 (newest), Index 4 is tx1 (oldest)
    assert_eq!(app.history_state.transactions.len(), 5);
    assert_eq!(app.history_state.transactions[0].id, "tx_05");
    assert_eq!(app.history_state.transactions[1].id, "tx_04");
    assert_eq!(app.history_state.transactions[2].id, "tx_03");
    assert_eq!(app.history_state.transactions[3].id, "tx_02");
    assert_eq!(app.history_state.transactions[4].id, "tx_01");

    // Metrics invariant: 4 rollback eligible (tx3 is Failed, not eligible)
    assert_eq!(app.history_state.metrics.total_transactions, 5);
    assert_eq!(app.history_state.metrics.successful_installations, 4);
    assert_eq!(app.history_state.metrics.failed_operations, 1);
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 4);

    // LIFO invariant: ONLY Tx5 has is_latest_committed = true
    assert!(app.history_state.transactions[0].is_latest_committed, "tx5 must be eligible");
    assert!(!app.history_state.transactions[1].is_latest_committed, "tx4 must NOT be eligible");
    assert!(!app.history_state.transactions[2].is_latest_committed, "tx3 is failed, must NOT be eligible");
    assert!(!app.history_state.transactions[3].is_latest_committed, "tx2 must NOT be eligible");
    assert!(!app.history_state.transactions[4].is_latest_committed, "tx1 must NOT be eligible");

    // === UNWIND STEP 1: Rollback Tx5 ===
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_05".into(),
        force_dirty: false,
    }));
    let cmd = cmd_rx.recv().await.expect("cmd");
    assert!(matches!(cmd, GuiCommand::RollbackTransaction { tx_id, .. } if tx_id == "tx_05"));

    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_05".into(),
            restored_files: vec!["/etc/c5".into()],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    drain_commands(&mut cmd_rx);

    let mut tx5_rolled = tx5.clone();
    tx5_rolled.status = TransactionStatus::RolledBack;
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1.clone(), tx2.clone(), tx3.clone(), tx4.clone(), tx5_rolled.clone()],
        active_tx: None,
    }));

    // Tx4 dynamically promoted to latest committed!
    assert!(!app.history_state.transactions[0].is_latest_committed, "tx5 now rolled back");
    assert!(app.history_state.transactions[1].is_latest_committed, "tx4 must now be eligible");
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 3);

    // === UNWIND STEP 2: Rollback Tx4 ===
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_04".into(),
        force_dirty: false,
    }));
    let cmd = cmd_rx.recv().await.expect("cmd");
    assert!(matches!(cmd, GuiCommand::RollbackTransaction { tx_id, .. } if tx_id == "tx_04"));

    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_04".into(),
            restored_files: vec!["/etc/c4".into()],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    drain_commands(&mut cmd_rx);

    let mut tx4_rolled = tx4.clone();
    tx4_rolled.status = TransactionStatus::RolledBack;
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1.clone(), tx2.clone(), tx3.clone(), tx4_rolled.clone(), tx5_rolled.clone()],
        active_tx: None,
    }));

    // Tx3 was Failed! So Tx3 is skipped, and Tx2 is dynamically promoted!
    assert!(!app.history_state.transactions[1].is_latest_committed, "tx4 rolled back");
    assert!(!app.history_state.transactions[2].is_latest_committed, "tx3 failed, never eligible");
    assert!(app.history_state.transactions[3].is_latest_committed, "tx2 must be promoted over failed tx3");
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 2);

    // === UNWIND STEP 3: Rollback Tx2 ===
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_02".into(),
        force_dirty: false,
    }));
    let cmd = cmd_rx.recv().await.expect("cmd");
    assert!(matches!(cmd, GuiCommand::RollbackTransaction { tx_id, .. } if tx_id == "tx_02"));

    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_02".into(),
            restored_files: vec!["/etc/c2".into()],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    drain_commands(&mut cmd_rx);

    let mut tx2_rolled = tx2.clone();
    tx2_rolled.status = TransactionStatus::RolledBack;
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1.clone(), tx2_rolled.clone(), tx3.clone(), tx4_rolled.clone(), tx5_rolled.clone()],
        active_tx: None,
    }));

    assert!(app.history_state.transactions[4].is_latest_committed, "tx1 must now be eligible");
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 1);

    // === UNWIND STEP 4: Rollback Tx1 ===
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_01".into(),
        force_dirty: false,
    }));
    let cmd = cmd_rx.recv().await.expect("cmd");
    assert!(matches!(cmd, GuiCommand::RollbackTransaction { tx_id, .. } if tx_id == "tx_01"));

    let _ = app.update(Message::Engine(EngineEvent::RollbackCompleted {
        report: RollbackReport {
            tx_id: "tx_01".into(),
            restored_files: vec!["/etc/c1".into()],
            removed_files: vec![],
            pruned_directories: vec![],
        },
    }));
    drain_commands(&mut cmd_rx);

    let mut tx1_rolled = tx1.clone();
    tx1_rolled.status = TransactionStatus::RolledBack;
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx1_rolled, tx2_rolled, tx3, tx4_rolled, tx5_rolled],
        active_tx: None,
    }));

    // All committed transactions unwound! 0 eligible operations remain.
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 0);
    for tx in &app.history_state.transactions {
        assert!(!tx.is_latest_committed, "No transaction can have rollback enabled");
    }
}

// =========================================================================
// 4. Stress Test: Adversarial Dirty-State Error Message Parsing & Force Rollback
// =========================================================================
#[tokio::test]
async fn test_stress_dirty_state_parsing_adversarial_formats_and_force_rollback() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let tx = make_journal_tx("tx_dirty_stress", ConfigComponent::Hyprland, TransactionStatus::Committed, "u/r", "/home/u/.config/hypr/hyprland.conf".into());
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx],
        active_tx: None,
    }));

    // Case A: Dirty error without single quotes
    let err_unquoted = "Dirty state detected: modified file was altered externally by text editor";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_stress".into(),
        error: err_unquoted.into(),
    }));

    assert!(app.history_state.dirty_modal.is_some(), "Dirty modal must open even when unquoted");
    let modal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.transaction_id, "tx_dirty_stress");
    assert_eq!(modal.modified_files[0], Utf8PathBuf::from("modified_dotfile"));

    // Close modal via user cancel
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseDirtyModal));
    assert!(app.history_state.dirty_modal.is_none());

    // Case B: Standard non-dirty error (I/O error) -> MUST NOT open dirty modal, sets error banner
    let err_io = "I/O Error: Permission denied writing to /etc/protected";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_stress".into(),
        error: err_io.into(),
    }));
    assert!(app.history_state.dirty_modal.is_none());
    assert!(app.error_banner.as_ref().unwrap().contains("Permission denied"));

    // Dismiss error banner
    let _ = app.update(Message::DismissError);
    assert!(app.error_banner.is_none());

    // Case C: Dirty error with single quotes -> correctly extracts path
    let err_quoted = "Dirty state: file '/home/u/.config/hypr/hyprland.conf' modified externally";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_stress".into(),
        error: err_quoted.into(),
    }));
    assert!(app.history_state.dirty_modal.is_some());
    let modal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.modified_files[0], Utf8PathBuf::from("/home/u/.config/hypr/hyprland.conf"));

    // User executes force rollback
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_dirty_stress".into(),
        force_dirty: true,
    }));
    assert!(app.history_state.dirty_modal.is_none());
    assert!(app.history_state.is_rolling_back);

    let force_cmd = cmd_rx.recv().await.expect("force rollback cmd");
    match force_cmd {
        GuiCommand::RollbackTransaction { tx_id, force_dirty } => {
            assert_eq!(tx_id, "tx_dirty_stress");
            assert!(force_dirty);
        }
        other => panic!("Unexpected cmd: {:?}", other),
    }
}

// =========================================================================
// 5. Stress Test: Headless View Rendering Across Boundary Dimensions, Themes, & Modals
// =========================================================================
#[test]
fn test_stress_headless_view_rendering_boundary_dimensions_and_states() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // Both startup notice AND error banner simultaneously active with huge multiline strings
    app.startup_notice = Some("CRASH RECOVERY: System crashed during atomic phase 4. Auto-recovered 14 dotfiles.\nVerification report: SHA256 matches verified.".into());
    app.error_banner = Some("ADVERSARIAL ERROR: Connection dropped mid-stream.\nRetry attempt 3 of 3 failed. Host unreachable.\nRate limit status unchanged.".into());

    // 1. Render Discover with 0 repositories
    app.active_screen = Screen::Search;
    app.search_state = woderrise_gui::screens::search::SearchState::new(vec![]);
    for theme in [Theme::Dark, Theme::Light] {
        app.active_theme = theme;
        let _ = app.view();
    }

    // 2. Render Inspect with all tabs and open confirmation dialog
    let repo = make_test_repo(555, "owner", "super-rice", 1000, ConfigComponent::Hyprland);
    let mut inspect = woderrise_gui::screens::inspect::InspectState::new(repo, vec![], vec![]);
    inspect.open_confirm_dialog();
    inspect.procedural_directives = vec![
        ProceduralDirective {
            directive_type: "exec-once".into(),
            command: "waybar".into(),
            file: "hyprland.conf".into(),
            line: 10,
        },
        ProceduralDirective {
            directive_type: "exec-once".into(),
            command: "swaybg -i wallpaper.png".into(),
            file: "hyprland.conf".into(),
            line: 11,
        },
    ];
    app.inspect_state = Some(inspect);
    app.active_screen = Screen::Inspect;

    let tabs = [
        InspectTab::Overview,
        InspectTab::Components,
        InspectTab::FileTree,
        InspectTab::Readme,
        InspectTab::DiffAndInstall,
    ];
    for tab in tabs {
        app.inspect_state.as_mut().unwrap().active_tab = tab;
        let _ = app.view();
    }

    // 3. Render Install with all InstallPhase variants
    let phases = [
        InstallPhase::PendingApproval,
        InstallPhase::Staging,
        InstallPhase::Backup,
        InstallPhase::JournalWal,
        InstallPhase::AtomicSwap,
        InstallPhase::Completed,
        InstallPhase::Failed,
    ];
    let mut install = InstallState::new("tx_install_test".into(), make_test_repo(1, "o", "r", 10, ConfigComponent::Hyprland), vec![ConfigComponent::Hyprland], vec![]);
    app.active_screen = Screen::Install;
    for phase in phases {
        install.current_phase = phase;
        install.status_message = format!("Status in phase {:?}", phase);
        app.install_state = Some(install.clone());
        let _ = app.view();
    }

    // 4. Render History with inspect modal, rollback confirm modal, and dirty modal open
    app.active_screen = Screen::History;
    let hist_tx = make_journal_tx("tx_h1", ConfigComponent::Hyprland, TransactionStatus::Committed, "u/r", "/path".into());
    let hist_state = woderrise_gui::screens::history::HistoryState::from_journal_entries(&[hist_tx]);
    app.history_state = hist_state;

    // Open inspect modal
    app.history_state.inspect_modal = Some(app.history_state.transactions[0].clone());
    let _ = app.view();

    // Open rollback confirm modal
    app.history_state.inspect_modal = None;
    app.history_state.confirm_rollback_modal = Some(woderrise_gui::screens::history::RollbackConfirmState::from_item(&app.history_state.transactions[0]));
    let _ = app.view();

    // Open dirty modal
    app.history_state.confirm_rollback_modal = None;
    app.history_state.dirty_modal = Some(woderrise_gui::screens::history::DirtyStateModal {
        transaction_id: "tx_h1".into(),
        modified_files: vec!["/path1".into(), "/path2".into()],
    });
    let _ = app.view();

    // 5. Render Settings with extreme rate limit boundaries: (0, 0), (MAX, MAX)
    app.active_screen = Screen::Settings;
    app.connection_state.rate_limits = woderrise_gui::widgets::gauge::RateLimitGaugeState::new(0, 0, 0, 0);
    let _ = app.view();

    app.connection_state.rate_limits = woderrise_gui::widgets::gauge::RateLimitGaugeState::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX);
    let _ = app.view();
}

// =========================================================================
// 6. Stress Test: Strict Hardware Display Protection Under Adversarial Selection Inputs
// =========================================================================
#[tokio::test]
async fn test_stress_hardware_display_protection_under_adversarial_selection() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let repo = make_test_repo(777, "hostile-author", "hardware-takeover", 500, ConfigComponent::Hyprland);
    let _ = app.update(Message::SelectRepo(repo));
    drain_commands(&mut cmd_rx);

    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new("hyprland.conf", "100644", "blob", "sha_h", Some(1000))],
            file_count: 1,
            total_size_bytes: 1000,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "monitors.conf".into(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new("monitors.conf", "100644", "blob", "sha_m1", Some(400)),
                TreeEntry::new("workspaces.conf", "100644", "blob", "sha_m2", Some(300)),
            ],
            file_count: 2,
            total_size_bytes: 700,
            hardware_notices: vec![HardwareConflictNotice {
                file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
                description: "Hardware monitor profile".into(),
                preserved_by_default: true,
            }],
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "hostile-author/hardware-takeover".into(),
        tree: GitTree {
            sha: "root_sha".into(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections,
    }));

    let inspect = app.inspect_state.as_ref().unwrap();
    assert!(!inspect.selected_components.contains(&ConfigComponent::Monitors));

    // Adversarial action: click SelectAllComponents repeatedly
    for _ in 0..5 {
        let _ = app.update(Message::InspectAction(InspectAction::SelectAllComponents));
    }

    let inspect = app.inspect_state.as_ref().unwrap();
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "ConfigComponent::Monitors MUST NEVER be selected even after repeated SelectAllComponents"
    );

    // Initiate install -> check staged entries
    let _ = app.update(Message::InitiateInstall);
    let stage_cmd = cmd_rx.recv().await.expect("stage cmd");
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
// 7. Stress Test: Rapid Debounce Flooding and Search Cancellation Safety
// =========================================================================
#[tokio::test]
async fn test_stress_rapid_debounce_flooding_and_search_cancellation() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(100);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    // Rapidly change search input 50 times in immediate sequence
    for i in 0..50 {
        let _ = app.update(Message::SearchInputChanged(format!("query_token_{}", i)));
    }

    assert_eq!(app.search_state.query, "query_token_49");
    assert!(app.search_state.debounce_pending);

    // Fire debounce for stale query token (must be ignored)
    let _ = app.update(Message::SearchDebounceFired("query_token_10".into()));
    assert!(app.search_state.debounce_pending, "Stale debounce must not consume pending state");

    // Clear search input
    let _ = app.update(Message::SearchClear);
    assert!(!app.search_state.debounce_pending);
    assert!(app.search_state.query.is_empty());
}

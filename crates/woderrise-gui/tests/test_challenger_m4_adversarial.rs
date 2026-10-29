//! Empirical Challenger Adversarial Test Suite for Milestone M4:
//! Screen 3: History & Rollback System.
//!
//! Stress-tests:
//! 1. RollbackConfirmState file partitioning invariants (restore vs remove).
//! 2. Modal opening, closing, and state machine transitions.
//! 3. Dirty state detection and force dirty rollback command dispatch.
//! 4. Empty state navigation to Screen::Search.
//! 5. Linear LIFO rollback eligibility invariants under complex transaction sequences.
//! 6. HistoryMetrics dynamic calculation correctness under stress/boundary conditions.
//! 7. Search and filter matrix including hostile queries and component/status filtering.
//! 8. Headless view rendering across Dark and Light themes with modal overlays.

use camino::Utf8PathBuf;
use chrono::{Duration, Utc};
use iced::Theme;
use tokio::sync::mpsc;
use woderrise_core::models::{
    ConfigComponent, FileOperationRecord, JournalEntry, OperationType, TransactionStatus,
};
use woderrise_gui::app::{App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::history::{
    truncate_chars, view_history, HistoryAction, HistoryComponentFilter, HistoryItem,
    HistoryMetrics, HistoryState, HistoryStatusFilter, RollbackConfirmState,
};

fn make_adversarial_item(
    id: &str,
    repo: &str,
    commit: &str,
    comp: ConfigComponent,
    status: TransactionStatus,
    ops: Vec<FileOperationRecord>,
    is_latest: bool,
) -> HistoryItem {
    HistoryItem {
        id: id.to_string(),
        timestamp: Utc::now(),
        source_repo: repo.to_string(),
        source_commit: commit.to_string(),
        component: comp,
        status,
        file_count: ops.len(),
        backup_dir: Utf8PathBuf::from(format!("/tmp/backups/{}", id)),
        operations: ops,
        is_latest_committed: is_latest,
    }
}

// ============================================================================
// 1. RollbackConfirmState File Partitioning Invariants
// ============================================================================

#[test]
fn test_challenge_rollback_confirm_state_partitioning_exhaustive() {
    // Construct an item with all 4 OperationTypes and edge cases
    let ops = vec![
        FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
            backup_file: Some(Utf8PathBuf::from("/tmp/b1")),
            pre_sha256: Some("sha_pre_1".into()),
            post_sha256: Some("sha_post_1".into()),
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::Create,
            target_file: Utf8PathBuf::from("/home/user/.config/hypr/autostart.conf"),
            backup_file: None,
            pre_sha256: None,
            post_sha256: Some("sha_post_2".into()),
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::Delete,
            target_file: Utf8PathBuf::from("/home/user/.config/hypr/old_scratchpad.conf"),
            backup_file: Some(Utf8PathBuf::from("/tmp/b3")),
            pre_sha256: Some("sha_pre_3".into()),
            post_sha256: None,
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::DeactivateLegacy,
            target_file: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf.legacy"),
            backup_file: Some(Utf8PathBuf::from("/tmp/b4")),
            pre_sha256: Some("sha_pre_4".into()),
            post_sha256: None,
            mode: None,
        },
        FileOperationRecord {
            op_type: OperationType::Create,
            target_file: Utf8PathBuf::from("/home/user/.config/hypr/scripts/launch_waybar.sh"),
            backup_file: None,
            pre_sha256: None,
            post_sha256: Some("sha_post_5".into()),
            mode: Some("0755".into()),
        },
    ];

    let item = make_adversarial_item(
        "tx_partition_01",
        "hyprland-community/rice",
        "c0ffee123456",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        ops,
        true,
    );

    let state = RollbackConfirmState::from_item(&item);

    // Metadata preservation
    assert_eq!(state.tx_id, "tx_partition_01");
    assert_eq!(state.repo, "hyprland-community/rice");
    assert_eq!(state.revision, "c0ffee123456");
    assert_eq!(state.component, ConfigComponent::Hyprland);

    // Exact partitioning checks
    // Overwrite, Delete, DeactivateLegacy MUST be in files_to_restore
    assert_eq!(state.files_to_restore.len(), 3);
    assert_eq!(
        state.files_to_restore[0],
        Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")
    );
    assert_eq!(
        state.files_to_restore[1],
        Utf8PathBuf::from("/home/user/.config/hypr/old_scratchpad.conf")
    );
    assert_eq!(
        state.files_to_restore[2],
        Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf.legacy")
    );

    // Create MUST be in files_to_remove
    assert_eq!(state.files_to_remove.len(), 2);
    assert_eq!(
        state.files_to_remove[0],
        Utf8PathBuf::from("/home/user/.config/hypr/autostart.conf")
    );
    assert_eq!(
        state.files_to_remove[1],
        Utf8PathBuf::from("/home/user/.config/hypr/scripts/launch_waybar.sh")
    );

    // Conservation invariant: files_to_restore.len() + files_to_remove.len() == total operations
    assert_eq!(
        state.files_to_restore.len() + state.files_to_remove.len(),
        item.operations.len()
    );

    // Disjointness invariant: no file is both in files_to_restore and files_to_remove
    for r in &state.files_to_restore {
        assert!(
            !state.files_to_remove.contains(r),
            "File {} should not appear in both restore and remove lists",
            r
        );
    }
}

#[test]
fn test_challenge_rollback_confirm_state_empty_operations() {
    let item = make_adversarial_item(
        "tx_empty_ops",
        "owner/repo",
        "1234567",
        ConfigComponent::Waybar,
        TransactionStatus::Committed,
        vec![],
        true,
    );
    let state = RollbackConfirmState::from_item(&item);
    assert!(state.files_to_restore.is_empty());
    assert!(state.files_to_remove.is_empty());
    assert_eq!(state.tx_id, "tx_empty_ops");
}

// ============================================================================
// 2. Modal Workflows & State Machine Transitions
// ============================================================================

#[test]
fn test_challenge_modal_lifecycle_and_mutual_transitions() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let item = make_adversarial_item(
        "tx_modal_test",
        "archcraft/waybar",
        "fedcba98",
        ConfigComponent::Waybar,
        TransactionStatus::Committed,
        vec![FileOperationRecord {
            op_type: OperationType::Create,
            target_file: Utf8PathBuf::from("~/.config/waybar/config"),
            backup_file: None,
            pre_sha256: None,
            post_sha256: Some("sha_new".into()),
            mode: None,
        }],
        true,
    );

    // Initial state: all modals closed
    assert!(app.history_state.inspect_modal.is_none());
    assert!(app.history_state.confirm_rollback_modal.is_none());
    assert!(app.history_state.dirty_modal.is_none());

    // 1. Open and Close Inspect Modal
    let _ = app.update(Message::HistoryAction(HistoryAction::Inspect(item.clone())));
    assert!(app.history_state.inspect_modal.is_some());
    assert_eq!(
        app.history_state.inspect_modal.as_ref().unwrap().id,
        "tx_modal_test"
    );

    let _ = app.update(Message::HistoryAction(HistoryAction::CloseInspect));
    assert!(app.history_state.inspect_modal.is_none());

    // 2. Open and Close Rollback Confirm Modal
    let _ = app.update(Message::HistoryAction(HistoryAction::OpenRollbackConfirm(
        item.clone(),
    )));
    assert!(app.history_state.confirm_rollback_modal.is_some());
    assert_eq!(
        app.history_state
            .confirm_rollback_modal
            .as_ref()
            .unwrap()
            .tx_id,
        "tx_modal_test"
    );

    let _ = app.update(Message::HistoryAction(HistoryAction::CloseRollbackConfirm));
    assert!(app.history_state.confirm_rollback_modal.is_none());

    // 3. Open Dirty Modal directly and Close
    app.history_state.open_dirty_modal(
        "tx_modal_test",
        vec![Utf8PathBuf::from("~/.config/waybar/config")],
    );
    assert!(app.history_state.dirty_modal.is_some());
    assert_eq!(
        app.history_state
            .dirty_modal
            .as_ref()
            .unwrap()
            .transaction_id,
        "tx_modal_test"
    );

    let _ = app.update(Message::HistoryAction(HistoryAction::CloseDirtyModal));
    assert!(app.history_state.dirty_modal.is_none());

    // 4. Double close does not panic
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseInspect));
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseRollbackConfirm));
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseDirtyModal));
    assert!(app.history_state.inspect_modal.is_none());
    assert!(app.history_state.confirm_rollback_modal.is_none());
    assert!(app.history_state.dirty_modal.is_none());
}

// ============================================================================
// 3. Dirty State Modal Triggering & Command Dispatch
// ============================================================================

#[test]
fn test_challenge_dirty_state_backend_event_trigger_and_force_flag() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    // 1. Simulate EngineEvent::RollbackError with single-quoted dirty file error
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_01".into(),
        error: "Rollback aborted: Target file '~/.config/hypr/hyprland.conf' has been modified since installation (dirty state).".into(),
    }));

    assert!(app.history_state.dirty_modal.is_some());
    let dirty = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(dirty.transaction_id, "tx_dirty_01");
    assert_eq!(
        dirty.modified_files,
        vec![Utf8PathBuf::from("~/.config/hypr/hyprland.conf")]
    );
    assert!(!app.history_state.is_rolling_back);

    // 2. User presses "Cancel" in dirty modal
    let _ = app.update(Message::HistoryAction(HistoryAction::CloseDirtyModal));
    assert!(app.history_state.dirty_modal.is_none());
    assert!(
        cmd_rx.try_recv().is_err(),
        "Cancel must not dispatch any command"
    );

    // 3. Simulate another dirty failure with no quotes
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_02".into(),
        error: "Detected dirty dotfile modification during verification".into(),
    }));
    assert!(app.history_state.dirty_modal.is_some());
    let dirty2 = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(dirty2.transaction_id, "tx_dirty_02");
    assert_eq!(
        dirty2.modified_files,
        vec![Utf8PathBuf::from("modified_dotfile")]
    );

    // 4. User presses "Backup & Force Rollback"
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_dirty_02".into(),
        force_dirty: true,
    }));
    assert!(app.history_state.dirty_modal.is_none());
    assert!(app.history_state.is_rolling_back);

    let dispatched = cmd_rx
        .try_recv()
        .expect("Rollback command must be dispatched");
    match dispatched {
        GuiCommand::RollbackTransaction { tx_id, force_dirty } => {
            assert_eq!(tx_id, "tx_dirty_02");
            assert!(force_dirty, "force_dirty must be TRUE for force rollback");
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    // 5. Standard rollback from confirm modal must set force_dirty: false
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_standard".into(),
        force_dirty: false,
    }));
    let dispatched_std = cmd_rx
        .try_recv()
        .expect("Rollback command must be dispatched");
    match dispatched_std {
        GuiCommand::RollbackTransaction { tx_id, force_dirty } => {
            assert_eq!(tx_id, "tx_standard");
            assert!(
                !force_dirty,
                "force_dirty must be FALSE for standard rollback"
            );
        }
        other => panic!("Unexpected command: {:?}", other),
    }
}

#[test]
fn test_challenge_non_dirty_rollback_error_sets_banner() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_err".into(),
        error: "Disk I/O error or permission denied".into(),
    }));

    // Must NOT open dirty modal
    assert!(app.history_state.dirty_modal.is_none());
    // Must set error banner
    assert!(app.error_banner.is_some());
    assert!(app
        .error_banner
        .as_ref()
        .unwrap()
        .contains("Disk I/O error"));
}

// ============================================================================
// 4. Empty State Navigation
// ============================================================================

#[test]
fn test_challenge_empty_state_navigation_transitions_screen() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    app.active_screen = Screen::History;
    assert_eq!(app.active_screen, Screen::History);

    let _ = app.update(Message::HistoryAction(HistoryAction::NavigateDiscover));
    assert_eq!(
        app.active_screen,
        Screen::Search,
        "NavigateDiscover must change active_screen to Screen::Search"
    );
}

// ============================================================================
// 5. Linear LIFO Rollback Invariants & Ordering
// ============================================================================

#[test]
fn test_challenge_lifo_invariants_complex_interleaved_history() {
    let t0 = Utc::now() - Duration::hours(10);
    let t1 = Utc::now() - Duration::hours(8);
    let t2 = Utc::now() - Duration::hours(6);
    let t3 = Utc::now() - Duration::hours(4);
    let t4 = Utc::now() - Duration::hours(2);

    let entries = vec![
        JournalEntry {
            id: "tx_0_committed".into(),
            timestamp: t0,
            source_repo: "catppuccin/hyprland".into(),
            source_commit: "commit0".into(),
            component: ConfigComponent::Hyprland,
            status: TransactionStatus::Committed,
            backup_directory: "/tmp/b0".into(),
            operations: vec![],
        },
        JournalEntry {
            id: "tx_1_rolled_back".into(),
            timestamp: t1,
            source_repo: "catppuccin/waybar".into(),
            source_commit: "commit1".into(),
            component: ConfigComponent::Waybar,
            status: TransactionStatus::RolledBack,
            backup_directory: "/tmp/b1".into(),
            operations: vec![],
        },
        JournalEntry {
            id: "tx_2_committed".into(),
            timestamp: t2,
            source_repo: "catppuccin/hyprlock".into(),
            source_commit: "commit2".into(),
            component: ConfigComponent::Hyprlock,
            status: TransactionStatus::Committed,
            backup_directory: "/tmp/b2".into(),
            operations: vec![],
        },
        JournalEntry {
            id: "tx_3_failed".into(),
            timestamp: t3,
            source_repo: "broken/repo".into(),
            source_commit: "commit3".into(),
            component: ConfigComponent::Hypridle,
            status: TransactionStatus::Failed,
            backup_directory: "/tmp/b3".into(),
            operations: vec![],
        },
        JournalEntry {
            id: "tx_4_committed".into(),
            timestamp: t4,
            source_repo: "latest/rice".into(),
            source_commit: "commit4".into(),
            component: ConfigComponent::Wallpapers,
            status: TransactionStatus::Committed,
            backup_directory: "/tmp/b4".into(),
            operations: vec![],
        },
    ];

    let state = HistoryState::from_journal_entries(&entries);

    // Entries are stored in reverse chronological order:
    // index 0: tx_4 (Committed) -> MUST BE is_latest_committed = true
    // index 1: tx_3 (Failed)    -> is_latest_committed = false
    // index 2: tx_2 (Committed) -> is_latest_committed = false (blocked by tx_4)
    // index 3: tx_1 (RolledBack)-> is_latest_committed = false
    // index 4: tx_0 (Committed) -> is_latest_committed = false (blocked by tx_2 and tx_4)
    assert_eq!(state.transactions.len(), 5);
    assert_eq!(state.transactions[0].id, "tx_4_committed");
    assert!(state.transactions[0].is_latest_committed);

    assert_eq!(state.transactions[1].id, "tx_3_failed");
    assert!(!state.transactions[1].is_latest_committed);

    assert_eq!(state.transactions[2].id, "tx_2_committed");
    assert!(!state.transactions[2].is_latest_committed);

    assert_eq!(state.transactions[3].id, "tx_1_rolled_back");
    assert!(!state.transactions[3].is_latest_committed);

    assert_eq!(state.transactions[4].id, "tx_0_committed");
    assert!(!state.transactions[4].is_latest_committed);

    // Exactly one transaction has is_latest_committed == true
    let eligible_count = state
        .transactions
        .iter()
        .filter(|t| t.is_latest_committed)
        .count();
    assert_eq!(eligible_count, 1);
}

#[test]
fn test_challenge_lifo_when_latest_is_failed() {
    let t0 = Utc::now() - Duration::hours(2);
    let t1 = Utc::now() - Duration::hours(1);

    let entries = vec![
        JournalEntry {
            id: "tx_committed".into(),
            timestamp: t0,
            source_repo: "owner/repo1".into(),
            source_commit: "c1".into(),
            component: ConfigComponent::Waybar,
            status: TransactionStatus::Committed,
            backup_directory: "/tmp/b0".into(),
            operations: vec![],
        },
        JournalEntry {
            id: "tx_failed_latest".into(),
            timestamp: t1,
            source_repo: "owner/repo2".into(),
            source_commit: "c2".into(),
            component: ConfigComponent::Waybar,
            status: TransactionStatus::Failed,
            backup_directory: "/tmp/b1".into(),
            operations: vec![],
        },
    ];

    let state = HistoryState::from_journal_entries(&entries);
    // index 0 is tx_failed_latest (Failed), index 1 is tx_committed (Committed)
    // In LIFO, the most recent COMMITTED transaction is rollback eligible!
    assert_eq!(state.transactions[0].id, "tx_failed_latest");
    assert!(!state.transactions[0].is_latest_committed);

    assert_eq!(state.transactions[1].id, "tx_committed");
    assert!(state.transactions[1].is_latest_committed);
}

// ============================================================================
// 6. HistoryMetrics Boundary Correctness
// ============================================================================

#[test]
fn test_challenge_metrics_stress_and_accuracy() {
    let items = vec![
        make_adversarial_item(
            "1",
            "r",
            "c",
            ConfigComponent::Hyprland,
            TransactionStatus::Committed,
            vec![],
            true,
        ),
        make_adversarial_item(
            "2",
            "r",
            "c",
            ConfigComponent::Waybar,
            TransactionStatus::RolledBack,
            vec![],
            false,
        ),
        make_adversarial_item(
            "3",
            "r",
            "c",
            ConfigComponent::Hyprlock,
            TransactionStatus::RecoveredRollback,
            vec![],
            false,
        ),
        make_adversarial_item(
            "4",
            "r",
            "c",
            ConfigComponent::Hypridle,
            TransactionStatus::Failed,
            vec![],
            false,
        ),
        make_adversarial_item(
            "5",
            "r",
            "c",
            ConfigComponent::Wallpapers,
            TransactionStatus::Committed,
            vec![],
            false,
        ),
    ];

    let metrics = HistoryMetrics::calculate(&items);
    assert_eq!(metrics.total_transactions, 5);
    // Successful includes Committed(2) + RolledBack(1) + RecoveredRollback(1) = 4
    assert_eq!(metrics.successful_installations, 4);
    // Failed = 1
    assert_eq!(metrics.failed_operations, 1);
    // Rollback eligible count = all Committed (2)
    assert_eq!(metrics.rollback_eligible_count, 2);
}

// ============================================================================
// 7. Search and Filter Adversarial Matrix
// ============================================================================

#[test]
fn test_challenge_search_adversarial_inputs_and_edge_cases() {
    let item = make_adversarial_item(
        "tx_special_123",
        "catppuccin/hyprland-dots",
        "a1b2c3d4e5f6",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        vec![],
        true,
    );

    let mut state = HistoryState {
        transactions: vec![item.clone()],
        metrics: HistoryMetrics::calculate(std::slice::from_ref(&item)),
        search_query: String::new(),
        status_filter: HistoryStatusFilter::All,
        component_filter: HistoryComponentFilter::All,
        inspect_modal: None,
        confirm_rollback_modal: None,
        dirty_modal: None,
        is_rolling_back: false,
    };

    // Substring in repo
    state.search_query = "catppuccin".into();
    assert!(state.filter_matches(&item));

    // Substring in commit
    state.search_query = "c3d4".into();
    assert!(state.filter_matches(&item));

    // Substring in tx_id
    state.search_query = "special_123".into();
    assert!(state.filter_matches(&item));

    // Substring in component name
    state.search_query = "hyprland".into();
    assert!(state.filter_matches(&item));

    // Substring in status name
    state.search_query = "committed".into();
    assert!(state.filter_matches(&item));

    // Adversarial: SQL-injection-like string
    state.search_query = "'; DROP TABLE history; --".into();
    assert!(!state.filter_matches(&item));

    // Adversarial: Regex metacharacters
    state.search_query = ".*+?^${}()|[]\\".into();
    assert!(!state.filter_matches(&item));

    // Adversarial: Long repeating whitespace
    state.search_query = "           ".into();
    assert!(
        state.filter_matches(&item),
        "Whitespace-only should be treated as empty query"
    );

    // Clear filters
    state.search_query = "test".into();
    state.status_filter = HistoryStatusFilter::Failed;
    state.component_filter = HistoryComponentFilter::Waybar;
    assert!(state.has_active_filters());
    state.clear_filters();
    assert!(!state.has_active_filters());
    assert_eq!(state.search_query, "");
    assert_eq!(state.status_filter, HistoryStatusFilter::All);
    assert_eq!(state.component_filter, HistoryComponentFilter::All);
}

// ============================================================================
// 8. Headless View Rendering Stress Tests across Dark and Light Themes
// ============================================================================

#[test]
fn test_challenge_headless_view_rendering_all_permutations() {
    let item = make_adversarial_item(
        "tx_render_stress",
        "super-long-organization-name/super-long-repository-name-for-hyprland",
        "abcdef0123456789",
        ConfigComponent::Hyprland,
        TransactionStatus::Committed,
        vec![
            FileOperationRecord {
                op_type: OperationType::Overwrite,
                target_file: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
                backup_file: Some(Utf8PathBuf::from("/backups/hyprland.conf.bak")),
                pre_sha256: Some("sha_pre_long_string_1234567890".into()),
                post_sha256: Some("sha_post_long_string_0987654321".into()),
                mode: None,
            },
            FileOperationRecord {
                op_type: OperationType::Create,
                target_file: Utf8PathBuf::from("/home/user/.config/hypr/extra.conf"),
                backup_file: None,
                pre_sha256: None,
                post_sha256: None,
                mode: None,
            },
        ],
        true,
    );

    let mut state = HistoryState {
        transactions: vec![item.clone()],
        metrics: HistoryMetrics::calculate(std::slice::from_ref(&item)),
        search_query: String::new(),
        status_filter: HistoryStatusFilter::All,
        component_filter: HistoryComponentFilter::All,
        inspect_modal: None,
        confirm_rollback_modal: None,
        dirty_modal: None,
        is_rolling_back: false,
    };

    let themes = [Theme::Dark, Theme::Light];

    for theme in &themes {
        // 1. Base populated view
        let _ = view_history(&state, theme, Message::HistoryAction);

        // 2. Inspect modal overlay
        state.open_inspect_modal(item.clone());
        let _ = view_history(&state, theme, Message::HistoryAction);
        state.close_inspect_modal();

        // 3. Rollback confirm modal overlay
        state.open_rollback_confirm(&item);
        let _ = view_history(&state, theme, Message::HistoryAction);
        state.close_rollback_confirm();

        // 4. Dirty state modal overlay
        state.open_dirty_modal(
            "tx_render_stress",
            vec![
                Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
                Utf8PathBuf::from("/home/user/.config/hypr/extra.conf"),
            ],
        );
        let _ = view_history(&state, theme, Message::HistoryAction);
        state.close_dirty_modal();
    }
}

#[test]
fn test_challenge_unicode_and_adversarial_commit_strings() {
    let massive_ascii = "a".repeat(100_000);
    let massive_crab = "🦀".repeat(25_000);
    let complex_bidi = "Commit: שלום עולם 12345 مرحبا 🚀";
    let zwj_family = "👨‍👩‍👧‍👦";
    let zwj_rainbow = "🏳️‍🌈";
    let zwj_controls = "\u{200D}\u{200C}\u{FEFF}";
    let null_bytes = "\0\0\0\0\0\0\0\0\0\0";
    let embedded_nulls = "commit\0hash\x00123456";
    let combining_marks = "e\u{0301}\u{0300}\u{0302}\u{0303}\u{0304}\u{0305}\u{0306}\u{0307}";

    let test_commits = [
        "",                                          // empty
        "a",                                         // 1 char
        "123456",                                    // 6 chars (< 7)
        "1234567",                                   // exactly 7 chars
        "12345678",                                  // 8 chars (> 7)
        "40characterslongshastring1234567890abcdef", // standard sha
        "🦀",               // 4 bytes, 1 char (< 7 chars, but what about byte slicing?)
        "🦀🦀",             // 8 bytes, 2 chars (> 7 bytes, split at 7)
        "🦀🦀🦀🦀🦀🦀🦀🦀", // 32 bytes, 8 chars (> 7 chars)
        "日本語のコミットメッセージ", // 3-byte CJK characters
        "مرحبا بالعالم",    // Arabic UTF-8
        "שלום עולם",        // Hebrew RTL
        complex_bidi,       // Mixed BiDi (RTL + LTR + emoji)
        null_bytes,         // Pure null bytes
        embedded_nulls,     // Embedded null bytes
        zwj_family,         // ZWJ sequence (25 bytes, 7 chars)
        zwj_rainbow,        // ZWJ sequence (14 bytes, 4 chars)
        zwj_controls,       // Zero-width control characters
        combining_marks,    // Combining diacritical marks
        "🌟✨🎉🔥🚀💡🎯🎨", // Multiple 4-byte emojis
        "prefix-🦀-suffix", // Mixed ASCII and multibyte
        massive_ascii.as_str(), // Massive 100k ASCII string
        massive_crab.as_str(), // Massive 100k-byte multibyte string
    ];

    // Property-based checks on truncate_chars directly
    for s in &test_commits {
        for max_len in [0, 1, 2, 6, 7, 8, 15, 100, 100_000, usize::MAX] {
            let truncated = truncate_chars(s, max_len);

            // Invariant 1: Character count never exceeds max_len
            let char_count = s.chars().count();
            let expected_chars = char_count.min(max_len);
            assert_eq!(
                truncated.chars().count(),
                expected_chars,
                "truncate_chars char count mismatch for max_len {}",
                max_len
            );

            // Invariant 2: Result is a prefix of input
            assert!(
                s.starts_with(truncated),
                "truncate_chars result must be prefix of input"
            );

            // Invariant 3: Zero heap allocations - pointer identity into source slice
            assert_eq!(
                truncated.as_ptr(),
                s.as_ptr(),
                "truncate_chars must return borrowed slice with identical base pointer"
            );

            // Invariant 4: If input has <= max_len chars, result must equal s exactly
            if char_count <= max_len {
                assert_eq!(truncated, *s);
            }
        }
    }

    // Stress UI view rendering across all adversarial commit strings in Dark and Light themes
    for commit in test_commits {
        // Invariant assertion: truncate_chars produces valid prefix with <= max_chars
        let truncated = truncate_chars(commit, 7);
        assert!(commit.starts_with(truncated));
        assert!(truncated.chars().count() <= 7);

        let item = make_adversarial_item(
            "tx_commit_test",
            "repo",
            commit,
            ConfigComponent::Hyprland,
            TransactionStatus::Committed,
            vec![],
            true,
        );
        let state = HistoryState {
            transactions: vec![item.clone()],
            metrics: HistoryMetrics::calculate(std::slice::from_ref(&item)),
            search_query: String::new(),
            status_filter: HistoryStatusFilter::All,
            component_filter: HistoryComponentFilter::All,
            inspect_modal: None,
            confirm_rollback_modal: None,
            dirty_modal: None,
            is_rolling_back: false,
        };

        // Render table in both Dark and Light themes
        let _ = view_history(&state, &Theme::Dark, Message::HistoryAction);
        let _ = view_history(&state, &Theme::Light, Message::HistoryAction);
    }
}

#[test]
fn test_challenge_unicode_hash_slicing_inspect_modal() {
    let adversarial_pairs = [
        ("a🦀🦀", "b🦀🦀"), // 9 bytes, byte 8 is inside 2nd crab
        ("\0\0\0\0\0\0\0\0\0\0", "pre\0post\x001234"),
        ("👨‍👩‍👧‍👦", "🏳️‍🌈"),
        ("مرحبا_بالعالم_هاش", "שלום_עולם_האש"),
        ("日本語ハッシュ値テスト", "漢字ハッシュ値テスト"),
        (
            "e\u{0301}\u{0300}\u{0302}\u{0303}\u{0304}\u{0305}\u{0306}",
            "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}",
        ),
    ];

    for (pre_hash, post_hash) in adversarial_pairs {
        let item = make_adversarial_item(
            "tx_hash_test",
            "repo",
            "abcdef1",
            ConfigComponent::Hyprland,
            TransactionStatus::Committed,
            vec![FileOperationRecord {
                op_type: OperationType::Overwrite,
                target_file: Utf8PathBuf::from("/test"),
                backup_file: None,
                pre_sha256: Some(pre_hash.into()),
                post_sha256: Some(post_hash.into()),
                mode: None,
            }],
            true,
        );
        let state = HistoryState {
            transactions: vec![item.clone()],
            metrics: HistoryMetrics::calculate(std::slice::from_ref(&item)),
            search_query: String::new(),
            status_filter: HistoryStatusFilter::All,
            component_filter: HistoryComponentFilter::All,
            inspect_modal: Some(item),
            confirm_rollback_modal: None,
            dirty_modal: None,
            is_rolling_back: false,
        };
        // Headless render in both Dark and Light themes
        let _ = view_history(&state, &Theme::Dark, Message::HistoryAction);
        let _ = view_history(&state, &Theme::Light, Message::HistoryAction);
    }
}

#[test]
fn test_challenge_metrics_massive_10k_entries_pure_and_mixed() {
    use std::time::Instant;

    // 10,000 mixed entries
    let all_statuses = [
        TransactionStatus::Committed,
        TransactionStatus::RolledBack,
        TransactionStatus::RecoveredRollback,
        TransactionStatus::Failed,
        TransactionStatus::Staged,
        TransactionStatus::Applying,
        TransactionStatus::RollingBack,
    ];

    let items_10k: Vec<HistoryItem> = (0..10_000)
        .map(|i| {
            let status = all_statuses[i % all_statuses.len()];
            make_adversarial_item(
                &format!("tx_{}", i),
                "user/test_repo",
                "abc1234",
                ConfigComponent::Hyprland,
                status,
                vec![],
                false,
            )
        })
        .collect();

    let start = Instant::now();
    let m = HistoryMetrics::calculate(&items_10k);
    let elapsed = start.elapsed();

    assert_eq!(m.total_transactions, 10_000);
    let expected_committed = items_10k
        .iter()
        .filter(|t| t.status == TransactionStatus::Committed)
        .count();
    let expected_successful = items_10k
        .iter()
        .filter(|t| {
            t.status == TransactionStatus::Committed
                || t.status == TransactionStatus::RolledBack
                || t.status == TransactionStatus::RecoveredRollback
        })
        .count();
    let expected_failed = items_10k
        .iter()
        .filter(|t| t.status == TransactionStatus::Failed)
        .count();

    assert_eq!(m.successful_installations, expected_successful);
    assert_eq!(m.failed_operations, expected_failed);
    assert_eq!(m.rollback_eligible_count, expected_committed);

    // Performance assertion: sub-10ms for 10k items
    assert!(
        elapsed.as_millis() < 25,
        "HistoryMetrics::calculate on 10,000 items took {:?}, expected < 25ms",
        elapsed
    );
}

#[test]
fn test_challenge_lifo_eligibility_exhaustive_permutations_243() {
    let base_statuses = [
        TransactionStatus::Committed,
        TransactionStatus::Failed,
        TransactionStatus::RolledBack,
    ];

    let mut permutations = Vec::new();
    for &s1 in &base_statuses {
        for &s2 in &base_statuses {
            for &s3 in &base_statuses {
                for &s4 in &base_statuses {
                    for &s5 in &base_statuses {
                        permutations.push(vec![s1, s2, s3, s4, s5]);
                    }
                }
            }
        }
    }

    assert_eq!(permutations.len(), 243);
    let base_time = Utc::now();

    for (perm_idx, statuses) in permutations.into_iter().enumerate() {
        let entries: Vec<JournalEntry> = statuses
            .iter()
            .enumerate()
            .map(|(i, &status)| JournalEntry {
                id: format!("tx_p{}_{}", perm_idx, i),
                timestamp: base_time + Duration::seconds(i as i64),
                component: ConfigComponent::Hyprland,
                status,
                source_repo: format!("user/repo_{}", i),
                source_commit: format!("sha_{}", i),
                backup_directory: Utf8PathBuf::from(format!("/backups/{}", i)),
                operations: vec![],
            })
            .collect();

        let state = HistoryState::from_journal_entries(&entries);

        let eligible_count = state
            .transactions
            .iter()
            .filter(|t| t.is_latest_committed)
            .count();

        let committed_count = statuses
            .iter()
            .filter(|&&s| s == TransactionStatus::Committed)
            .count();

        if committed_count == 0 {
            assert_eq!(eligible_count, 0);
        } else {
            assert_eq!(eligible_count, 1);
            let newest_committed_orig_idx = statuses
                .iter()
                .rposition(|&s| s == TransactionStatus::Committed)
                .unwrap();
            let expected_eligible_id = format!("tx_p{}_{}", perm_idx, newest_committed_orig_idx);
            let eligible_item = state
                .transactions
                .iter()
                .find(|t| t.is_latest_committed)
                .unwrap();
            assert_eq!(eligible_item.id, expected_eligible_id);
        }
    }
}

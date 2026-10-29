//! Empirical Challenger Adversarial Test Suite for Milestone M6:
//! Comprehensive E2E UI Verification, Responsiveness & Hardening.
//!
//! Objectives:
//! 1. Boundary behaviors and adversarial error parsing in dirty-state conflict detection and force rollback.
//! 2. Safe cache cleanup boundary containment: symlink escape, loops, broken links, and target non-deletion.
//! 3. Battery conservation clock tick subscription scoping across all state machine transitions.
//! 4. Dynamic rate limit gauge synchrony, zero-limit division protection, and clamping under extreme boundaries.
//! 5. Multi-screen rapid transition stress and append-order LIFO rollback invariants.

use camino::Utf8PathBuf;
use chrono::Utc;
use iced::Theme;
use std::fs;
use std::os::unix::fs::symlink;
use std::sync::Mutex;
use tempfile::tempdir;

use woderrise_core::models::{
    ConfigComponent, FileOperationRecord, JournalEntry, OperationType, TransactionStatus,
};
use woderrise_github::rate_limit::{RateLimitResource, RateLimitStatus};
use woderrise_gui::app::{App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::history::{view_history, HistoryAction};
use woderrise_gui::screens::search::RateLimitGaugeState;
use woderrise_gui::screens::settings::{
    calculate_directory_stats, safe_cleanup_cache, view_settings, SettingsTab,
};
use woderrise_gui::widgets::gauge::RateLimitColorTier;

/// Global mutex to serialize tests that touch filesystem environment or tempdirs.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Helper to drain commands from an mpsc receiver.
fn drain_commands(rx: &mut tokio::sync::mpsc::Receiver<GuiCommand>) {
    while rx.try_recv().is_ok() {}
}

fn make_journal_entry(
    id: &str,
    status: TransactionStatus,
    target: &str,
    repo: &str,
) -> JournalEntry {
    JournalEntry {
        id: id.to_string(),
        timestamp: Utc::now(),
        component: ConfigComponent::Hyprland,
        status,
        source_repo: repo.to_string(),
        source_commit: "commit_sha_123".to_string(),
        backup_directory: Utf8PathBuf::from(format!("/backups/{}", id)),
        operations: vec![FileOperationRecord {
            op_type: OperationType::Overwrite,
            target_file: Utf8PathBuf::from(target),
            backup_file: Some("backup.bak".into()),
            pre_sha256: Some("sha_pre".to_string()),
            post_sha256: Some("sha_post".to_string()),
            mode: Some("100644".to_string()),
        }],
    }
}

// =========================================================================
// 1. Adversarial Dirty-State Error Parsing & Force Rollback Resilience
// =========================================================================

#[tokio::test]
async fn test_adversarial_dirty_state_error_parsing_and_modal_resilience() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    let tx = make_journal_entry(
        "tx_dirty_adv",
        TransactionStatus::Committed,
        "/home/user/.config/hypr/hyprland.conf",
        "author/rice",
    );

    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: vec![tx],
        active_tx: None,
    }));

    // Case A: Quoted file path with single quotes
    let error_single_quoted =
        "Dirty state detected: file '/custom/path/with spaces/hyprland.conf' modified externally";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_adv".to_string(),
        error: error_single_quoted.to_string(),
    }));

    assert!(app.history_state.dirty_modal.is_some());
    let modal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.transaction_id, "tx_dirty_adv");
    assert_eq!(
        modal.modified_files[0],
        Utf8PathBuf::from("/custom/path/with spaces/hyprland.conf")
    );

    // Case B: User closes/cancels dirty modal
    app.history_state.close_dirty_modal();
    assert!(app.history_state.dirty_modal.is_none());
    assert!(!app.history_state.is_rolling_back);

    // Case C: Unquoted or malformed dirty error message (must not panic)
    let error_unquoted = "Dirty state detected without quotes around file path";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_adv".to_string(),
        error: error_unquoted.to_string(),
    }));

    assert!(app.history_state.dirty_modal.is_some());
    let modal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.modified_files[0], Utf8PathBuf::from("modified_dotfile"));

    // Case D: Non-dirty error (e.g. permission denied) -> must route to error banner, NOT dirty modal
    app.history_state.close_dirty_modal();
    let error_permission = "Transaction rollback failed: permission denied accessing backup storage";
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        tx_id: "tx_dirty_adv".to_string(),
        error: error_permission.to_string(),
    }));

    assert!(
        app.history_state.dirty_modal.is_none(),
        "Non-dirty error must NOT open dirty modal"
    );
    assert!(app.error_banner.is_some());
    assert!(app
        .error_banner
        .as_ref()
        .unwrap()
        .contains("permission denied"));

    // Case E: Force rollback workflow execution
    app.history_state.open_dirty_modal(
        "tx_dirty_adv",
        vec![Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")],
    );
    let _ = app.update(Message::HistoryAction(HistoryAction::ExecuteRollback {
        tx_id: "tx_dirty_adv".to_string(),
        force_dirty: true,
    }));

    assert!(app.history_state.dirty_modal.is_none());
    assert!(app.history_state.is_rolling_back);

    let cmd = cmd_rx
        .recv()
        .await
        .expect("Expected force rollback command");
    match cmd {
        GuiCommand::RollbackTransaction {
            tx_id, force_dirty, ..
        } => {
            assert_eq!(tx_id, "tx_dirty_adv");
            assert!(force_dirty);
        }
        other => panic!("Unexpected command: {:?}", other),
    }

    // View rendering with dirty modal open must succeed headlessly
    app.history_state.open_dirty_modal(
        "tx_dirty_adv",
        vec![Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf")],
    );
    let _ = view_history(
        &app.history_state,
        &Theme::Dark,
        Message::HistoryAction,
    );
    let _ = view_history(
        &app.history_state,
        &Theme::Light,
        Message::HistoryAction,
    );
}

// =========================================================================
// 2. Safe Cache Cleanup Boundary Containment: Symlinks, Loops & Target Safety
// =========================================================================

#[test]
fn test_adversarial_safe_cache_cleanup_symlink_escape_and_loop_containment() {
    let _lock = TEST_LOCK.lock().unwrap();

    let tmp = tempdir().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

    // 1. Setup outside target directory simulating user sensitive dotfiles & configs
    let sensitive_home = base.join("mock_user_home");
    let sensitive_config = sensitive_home.join(".config/hypr/hyprland.conf");
    let sensitive_ssh = sensitive_home.join(".ssh/id_ed25519");
    let sensitive_bashrc = sensitive_home.join(".bashrc");

    fs::create_dir_all(sensitive_config.parent().unwrap()).unwrap();
    fs::create_dir_all(sensitive_ssh.parent().unwrap()).unwrap();

    fs::write(&sensitive_config, b"# Important Hyprland Config").unwrap();
    fs::write(&sensitive_ssh, b"CRITICAL_PRIVATE_KEY").unwrap();
    fs::write(&sensitive_bashrc, b"export PATH=$HOME/bin:$PATH").unwrap();

    // 2. Setup cache directory with adversarial symlink traps
    let cache_dir = base.join(".cache/woderrise");
    let staging_dir = cache_dir.join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    // Trap 1: Directory symlink inside staging pointing directly to sensitive user home
    let trap_dir_link = staging_dir.join("symlink_to_home");
    symlink(sensitive_home.as_std_path(), trap_dir_link.as_std_path()).unwrap();

    // Trap 2: File symlink inside staging pointing directly to sensitive ssh key
    let trap_file_link = staging_dir.join("symlink_to_key");
    symlink(sensitive_ssh.as_std_path(), trap_file_link.as_std_path()).unwrap();

    // Trap 3: Cyclical / loop symlink inside staging (points to staging itself)
    let loop_link = staging_dir.join("recursive_loop");
    symlink(staging_dir.as_std_path(), loop_link.as_std_path()).unwrap();

    // Trap 4: Broken / dangling symlink (target does not exist)
    let broken_link = staging_dir.join("dangling_link");
    symlink(
        base.join("non_existent_folder_xyz").as_std_path(),
        broken_link.as_std_path(),
    )
    .unwrap();

    // Trap 5: Directory symlink in cache root pointing to user home
    let root_symlink = cache_dir.join("rogue_root_link");
    symlink(sensitive_home.as_std_path(), root_symlink.as_std_path()).unwrap();

    // Trap 6: Loose file symlink in cache root pointing to .bashrc
    let root_bash_link = cache_dir.join("bash_link");
    symlink(sensitive_bashrc.as_std_path(), root_bash_link.as_std_path()).unwrap();

    // Legitimate staging contents: real directory with real files
    let real_staging_tx = staging_dir.join("tx_legit_staging_123");
    fs::create_dir_all(&real_staging_tx).unwrap();
    fs::write(real_staging_tx.join("stage.conf"), b"ephemeral staging").unwrap();

    // Loose cache blob in cache root
    let loose_blob = cache_dir.join("cached_catalog.json");
    fs::write(&loose_blob, b"{\"cached\":\"catalog\"}").unwrap();

    // 3. Verify directory stats calculation does NOT loop infinitely or crash on cycles/symlinks
    let stats = calculate_directory_stats(&cache_dir).expect("Stats calculation must succeed");
    assert!(stats.file_count >= 2);
    assert!(stats.total_bytes > 0);

    // 4. Execute safe_cleanup_cache
    let (cleaned_count, cleaned_bytes) =
        safe_cleanup_cache(&cache_dir).expect("safe_cleanup_cache must succeed");
    assert!(cleaned_count >= 2);
    assert!(cleaned_bytes > 0);

    // 5. Verify ephemeral and rogue symlinks were cleaned up
    assert!(!trap_dir_link.exists() && fs::symlink_metadata(trap_dir_link.as_std_path()).is_err());
    assert!(
        !trap_file_link.exists() && fs::symlink_metadata(trap_file_link.as_std_path()).is_err()
    );
    assert!(!loop_link.exists() && fs::symlink_metadata(loop_link.as_std_path()).is_err());
    assert!(!broken_link.exists() && fs::symlink_metadata(broken_link.as_std_path()).is_err());
    assert!(
        !root_symlink.exists() && fs::symlink_metadata(root_symlink.as_std_path()).is_err()
    );
    assert!(
        !root_bash_link.exists() && fs::symlink_metadata(root_bash_link.as_std_path()).is_err()
    );
    assert!(!real_staging_tx.exists());
    assert!(!loose_blob.exists());

    // 6. ABSOLUTE INVARIANT: Sensitive outside files MUST BE COMPLETELY UNTOUCHED
    assert!(
        sensitive_config.exists(),
        "CRITICAL SECURITY ESCAPE: ~/.config/hypr/hyprland.conf was deleted!"
    );
    assert_eq!(
        fs::read_to_string(&sensitive_config).unwrap(),
        "# Important Hyprland Config"
    );

    assert!(
        sensitive_ssh.exists(),
        "CRITICAL SECURITY ESCAPE: ~/.ssh/id_ed25519 was deleted!"
    );
    assert_eq!(
        fs::read_to_string(&sensitive_ssh).unwrap(),
        "CRITICAL_PRIVATE_KEY"
    );

    assert!(
        sensitive_bashrc.exists(),
        "CRITICAL SECURITY ESCAPE: ~/.bashrc was deleted!"
    );
    assert_eq!(
        fs::read_to_string(&sensitive_bashrc).unwrap(),
        "export PATH=$HOME/bin:$PATH"
    );

    // 7. Non-existent cache directory must return (0, 0) cleanly
    let non_existent = base.join("path_does_not_exist_at_all");
    let (c, b) = safe_cleanup_cache(&non_existent).expect("Must return (0,0) for missing dir");
    assert_eq!(c, 0);
    assert_eq!(b, 0);
}

// =========================================================================
// 3. Battery Conservation Clock Tick Subscription Scoping State Machine
// =========================================================================

#[test]
fn test_adversarial_battery_clock_tick_subscription_state_machine() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    // 1. Initial State: Discover (Search) -> Idle
    assert_eq!(app.active_screen, Screen::Search);
    assert!(!app.is_clock_tick_needed());

    // 2. Discover -> Inspect -> Idle
    let _ = app.update(Message::NavigateTo(Screen::Inspect));
    assert_eq!(app.active_screen, Screen::Inspect);
    assert!(!app.is_clock_tick_needed());

    // 3. Inspect -> Install -> Idle
    app.active_screen = Screen::Install;
    assert_eq!(app.active_screen, Screen::Install);
    assert!(!app.is_clock_tick_needed());

    // 4. Install -> History -> Idle
    let _ = app.update(Message::NavigateTo(Screen::History));
    assert_eq!(app.active_screen, Screen::History);
    assert!(!app.is_clock_tick_needed());

    // 5. History -> Settings -> Clock tick is REQUIRED
    let _ = app.update(Message::NavigateTo(Screen::Settings));
    assert_eq!(app.active_screen, Screen::Settings);
    assert!(app.is_clock_tick_needed());

    // 6. Across all Settings tabs, clock tick remains active
    let tabs = [
        SettingsTab::GitHub,
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
    ];
    for tab in tabs {
        let _ = app.update(Message::SettingsTabSelected(tab));
        assert!(
            app.is_clock_tick_needed(),
            "Clock tick must remain active on Settings tab {:?}",
            tab
        );
    }

    // 7. Navigating away from Settings immediately disables clock tick
    let _ = app.update(Message::NavigateTo(Screen::Search));
    assert_eq!(app.active_screen, Screen::Search);
    assert!(!app.is_clock_tick_needed());

    // 8. Connection test started from Discover screen triggers clock tick
    let _ = app.update(Message::SettingsTestConnection);
    assert!(app.is_testing_connection);
    assert!(app.is_clock_tick_needed());

    // 9. When connection test completes, clock tick disables again on Discover screen
    let now = Utc::now();
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: RateLimitStatus {
            resource: RateLimitResource::Core,
            limit: 60,
            remaining: 60,
            reset_time: now,
            used: 0,
        },
        search: RateLimitStatus {
            resource: RateLimitResource::Search,
            limit: 10,
            remaining: 10,
            reset_time: now,
            used: 0,
        },
    }));
    assert!(!app.is_testing_connection);
    assert!(!app.is_clock_tick_needed());

    // 10. Delivering ClockTick on idle screen must be safe no-op
    let _ = app.update(Message::ClockTick);
    assert_eq!(app.active_screen, Screen::Search);
    assert!(!app.is_clock_tick_needed());
}

// =========================================================================
// 4. Dynamic Rate Limit Gauge Synchrony & Extreme Boundary Resilience
// =========================================================================

#[test]
fn test_adversarial_rate_limit_gauge_extreme_boundaries_and_sync() {
    // 1. Boundary: Zero limit (division by zero risk)
    let gauge_zero = RateLimitGaugeState::new(0, 0, 0, 0);
    assert_eq!(gauge_zero.core_limit, 1, "Limit must be clamped to min 1");
    assert_eq!(gauge_zero.search_limit, 1, "Limit must be clamped to min 1");
    assert_eq!(gauge_zero.core_percentage(), 0.0);
    assert_eq!(gauge_zero.search_percentage(), 0.0);
    assert_eq!(gauge_zero.core_tier(), RateLimitColorTier::Red);
    assert_eq!(gauge_zero.search_tier(), RateLimitColorTier::Red);

    // 2. Boundary: remaining > limit (clamped to 1.0)
    let gauge_overflow = RateLimitGaugeState::new(200, 100, 50, 10);
    assert_eq!(gauge_overflow.core_percentage(), 1.0);
    assert_eq!(gauge_overflow.search_percentage(), 1.0);
    assert_eq!(gauge_overflow.core_tier(), RateLimitColorTier::Green);
    assert_eq!(gauge_overflow.search_tier(), RateLimitColorTier::Green);

    // 3. Boundary: exact tier boundaries
    // 50% threshold: exactly 50% is Amber (> 50% is Green)
    assert_eq!(RateLimitGaugeState::calc_tier(50, 100), RateLimitColorTier::Amber);
    assert_eq!(RateLimitGaugeState::calc_tier(51, 100), RateLimitColorTier::Green);
    // 10% threshold: exactly 10% is Amber (< 10% is Red)
    assert_eq!(RateLimitGaugeState::calc_tier(10, 100), RateLimitColorTier::Amber);
    assert_eq!(RateLimitGaugeState::calc_tier(9, 100), RateLimitColorTier::Red);

    // 4. App synchrony across screens
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    let reset_time = Utc::now() + chrono::Duration::minutes(45);
    let _ = app.update(Message::Engine(EngineEvent::RateLimitsUpdated {
        core: RateLimitStatus {
            resource: RateLimitResource::Core,
            limit: 5000,
            remaining: 2500, // exactly 50% -> Amber
            reset_time,
            used: 2500,
        },
        search: RateLimitStatus {
            resource: RateLimitResource::Search,
            limit: 30,
            remaining: 2, // 2/30 = 6.6% -> Red
            reset_time,
            used: 28,
        },
    }));

    // Check App connection_state
    assert_eq!(app.connection_state.core_limit, 5000);
    assert_eq!(app.connection_state.core_remaining, 2500);
    assert_eq!(app.connection_state.search_limit, 30);
    assert_eq!(app.connection_state.search_remaining, 2);

    // Check SearchState synchrony
    assert_eq!(app.search_state.rate_limits.core_limit, 5000);
    assert_eq!(app.search_state.rate_limits.core_remaining, 2500);
    assert_eq!(app.search_state.rate_limits.search_limit, 30);
    assert_eq!(app.search_state.rate_limits.search_remaining, 2);

    assert_eq!(
        app.search_state.rate_limits.core_tier(),
        RateLimitColorTier::Amber
    );
    assert_eq!(
        app.search_state.rate_limits.search_tier(),
        RateLimitColorTier::Red
    );

    // Check SettingsState reset time
    assert_eq!(app.settings_state.core_reset, Some(reset_time));
    assert_eq!(app.settings_state.search_reset, Some(reset_time));

    // Render views with extreme boundaries: zero crash
    let _ = app.view();
    let _ = view_settings(
        &app.settings_state,
        &app.connection_state.rate_limits,
        &Theme::Dark,
        Message::SettingsAction,
    );
    let _ = view_settings(
        &app.settings_state,
        &app.connection_state.rate_limits,
        &Theme::Light,
        Message::SettingsAction,
    );
}

// =========================================================================
// 5. Multi-Screen Rapid Transition Stress & Append-Order LIFO Invariants
// =========================================================================

#[tokio::test]
async fn test_adversarial_rapid_navigation_and_out_of_order_history_invariants() {
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);
    drain_commands(&mut cmd_rx);

    // Rapid screen bouncing
    for _ in 0..10 {
        let _ = app.update(Message::NavigateTo(Screen::Search));
        let _ = app.update(Message::NavigateTo(Screen::History));
        let _ = app.update(Message::NavigateTo(Screen::Settings));
        let _ = app.update(Message::NavigateTo(Screen::Inspect));
    }
    drain_commands(&mut cmd_rx);

    // Setup chronological journal entries in WAL append order:
    // tx_1 was committed first (oldest), tx_2 second, tx_3 third (newest).
    let tx_1 = make_journal_entry(
        "tx_1_old",
        TransactionStatus::Committed,
        "/path/old",
        "repo/old",
    );
    let tx_2 = make_journal_entry(
        "tx_2_mid",
        TransactionStatus::Committed,
        "/path/mid",
        "repo/mid",
    );
    let tx_3 = make_journal_entry(
        "tx_3_new",
        TransactionStatus::Committed,
        "/path/new",
        "repo/new",
    );

    // WAL journal delivers entries in append order: [tx_1, tx_2, tx_3]
    let journal_entries = vec![tx_1, tx_2, tx_3];
    let _ = app.update(Message::Engine(EngineEvent::HistoryLoaded {
        transactions: journal_entries,
        active_tx: None,
    }));

    // In History table, items are reversed so newest transaction is at index 0
    assert_eq!(app.history_state.transactions.len(), 3);
    assert_eq!(
        app.history_state.transactions[0].id, "tx_3_new",
        "LIFO display must place latest committed transaction at index 0"
    );
    assert_eq!(app.history_state.transactions[1].id, "tx_2_mid");
    assert_eq!(app.history_state.transactions[2].id, "tx_1_old");

    // Strictly tx_3_new has rollback enabled
    assert!(
        app.history_state.transactions[0].is_latest_committed,
        "Strictly latest committed transaction must have rollback privilege"
    );
    assert!(!app.history_state.transactions[1].is_latest_committed);
    assert!(!app.history_state.transactions[2].is_latest_committed);

    // Derived metrics invariant: all 3 committed transactions are eventually rollback-eligible
    assert_eq!(app.history_state.metrics.total_transactions, 3);
    assert_eq!(app.history_state.metrics.successful_installations, 3);
    assert_eq!(app.history_state.metrics.rollback_eligible_count, 3);
}

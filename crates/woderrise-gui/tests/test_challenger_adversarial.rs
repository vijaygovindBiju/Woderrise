//! Empirical Challenger Adversarial Test Suite for Milestone 4:
//! State Invariants, Transaction Transitions, Hardware Display Protection,
//! Directive Approval Gating, Strict LIFO Rollback, Dirty-State Modals, and Crash Recovery.

use camino::Utf8PathBuf;
use chrono::Utc;
use std::fs;
use woderrise_core::error::TransactionError;
use woderrise_core::models::{
    CatalogRepository, ConfigComponent, DetectedComponent, HardwareConflictNotice, JournalEntry,
    SyntaxFormat, TransactionStatus,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{ComponentDetection, ProceduralDirective};
use woderrise_gui::app::{App, EngineEvent, Message};
use woderrise_gui::screens::history::HistoryState;
use woderrise_gui::screens::inspect::InspectState;
use woderrise_gui::screens::install::InstallState;
use woderrise_transaction::{JournalManager, TransactionEngine};

fn create_sample_catalog_repo(
    owner: &str,
    repo: &str,
    components: Vec<ConfigComponent>,
) -> CatalogRepository {
    CatalogRepository {
        id: 42,
        owner: owner.into(),
        repo: repo.into(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some("Adversarial test dotfile repository".into()),
        stargazers_count: 50,
        default_branch: "main".into(),
        topics: vec!["hyprland".into()],
        pushed_at: None,
        cached_tree_sha: None,
        etag: None,
        detected_components: components
            .into_iter()
            .map(|c| DetectedComponent {
                component: c,
                syntax_format: SyntaxFormat::LegacyConf,
                root_path: "config".into(),
                entrypoint: c.default_entrypoint().into(),
                style_file: None,
                file_count: 2,
                total_size_bytes: 4096,
            })
            .collect(),
    }
}

/// Challenge 1: Hardware Display Preservation Invariant
///
/// Invariant: `monitors.conf`, `workspaces.conf`, and `hardware.conf` (represented by `ConfigComponent::Monitors`)
/// MUST NEVER be automatically selected by default in the inspect screen.
/// Overwrite requires explicit opt-in by the user.
#[test]
fn test_adversarial_hardware_preservation_auto_selection_in_inspect() {
    let repo = create_sample_catalog_repo(
        "adversary",
        "rice-with-monitors",
        vec![
            ConfigComponent::Hyprland,
            ConfigComponent::Waybar,
            ConfigComponent::Monitors,
        ],
    );

    let hw_notice = HardwareConflictNotice {
        file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
        description: "Hardware display configuration detected in monitors.conf".into(),
        preserved_by_default: true,
    };

    // 1. Check initialization from CatalogRepository
    let inspect = InspectState::new(repo.clone(), Vec::new(), vec![hw_notice.clone()]);

    // Hyprland and Waybar may be selected by default
    assert!(
        inspect
            .selected_components
            .contains(&ConfigComponent::Hyprland),
        "Hyprland should be selectable"
    );
    assert!(
        inspect
            .selected_components
            .contains(&ConfigComponent::Waybar),
        "Waybar should be selectable"
    );

    // CRITICAL EMPIRICAL CHALLENGE:
    // monitors.conf must NOT be auto-selected by default!
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "Hardware protection violation: ConfigComponent::Monitors was automatically selected by default in InspectState!"
    );
}

/// Challenge 1b: Hardware Display Preservation via TreeLoaded event in App
#[test]
fn test_adversarial_hardware_preservation_in_tree_loaded_event() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    let repo = create_sample_catalog_repo(
        "adversary",
        "rice-with-monitors",
        vec![ConfigComponent::Hyprland],
    );

    let _ = app.update(Message::SelectRepo(repo));

    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![],
            file_count: 1,
            total_size_bytes: 1024,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "monitors.conf".into(),
            style_file: None,
            matched_entries: vec![],
            file_count: 1,
            total_size_bytes: 512,
            hardware_notices: vec![HardwareConflictNotice {
                file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
                description: "Hardware monitors detected".into(),
                preserved_by_default: true,
            }],
        },
    ];

    // Deliver TreeLoaded with detected monitors.conf
    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "adversary/rice-with-monitors".into(),
        tree: woderrise_github::GitTree {
            sha: "tree123".into(),
            url: None,
            tree: vec![],
            truncated: false,
        },
        detections,
    }));

    let inspect = app
        .inspect_state
        .as_ref()
        .expect("Inspect state must exist");

    // CRITICAL EMPIRICAL CHALLENGE:
    // TreeLoaded must NOT auto-check ConfigComponent::Monitors into selected_components!
    assert!(
        !inspect.selected_components.contains(&ConfigComponent::Monitors),
        "Hardware protection violation: TreeLoaded event auto-selected ConfigComponent::Monitors into selected_components!"
    );
}

/// Challenge 2: LIFO Rollback Ordering & is_latest_committed Invariant
///
/// Invariant: In standard chronological journal history `[tx_01, tx_02, tx_03]`:
/// - `tx_03` (newest committed) MUST have `is_latest_committed == true`
/// - `tx_01` (oldest committed) MUST have `is_latest_committed == false`
/// - `tx_02` (middle committed) MUST have `is_latest_committed == false`
#[test]
fn test_adversarial_lifo_rollback_chronological_ordering() {
    let t1 = JournalEntry {
        id: "tx_01_oldest".into(),
        timestamp: Utc::now() - chrono::Duration::hours(2),
        component: ConfigComponent::Hyprland,
        status: TransactionStatus::Committed,
        source_repo: "catppuccin/hyprland".into(),
        source_commit: "c1".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx_01"),
        operations: vec![],
    };

    let t2 = JournalEntry {
        id: "tx_02_middle".into(),
        timestamp: Utc::now() - chrono::Duration::hours(1),
        component: ConfigComponent::Waybar,
        status: TransactionStatus::Committed,
        source_repo: "alex/waybar".into(),
        source_commit: "c2".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx_02"),
        operations: vec![],
    };

    let t3 = JournalEntry {
        id: "tx_03_newest".into(),
        timestamp: Utc::now(),
        component: ConfigComponent::Wallpapers,
        status: TransactionStatus::Committed,
        source_repo: "artist/nord".into(),
        source_commit: "c3".into(),
        backup_directory: Utf8PathBuf::from("/backups/tx_03"),
        operations: vec![],
    };

    // get_history() returns journal.transactions in chronological order: [t1 (oldest), t2 (middle), t3 (newest)]
    let history = HistoryState::from_journal_entries(&[t1, t2, t3]);

    // Find each transaction in history
    let item_t1 = history
        .transactions
        .iter()
        .find(|t| t.id == "tx_01_oldest")
        .expect("tx_01 must exist");
    let item_t2 = history
        .transactions
        .iter()
        .find(|t| t.id == "tx_02_middle")
        .expect("tx_02 must exist");
    let item_t3 = history
        .transactions
        .iter()
        .find(|t| t.id == "tx_03_newest")
        .expect("tx_03 must exist");

    // CRITICAL EMPIRICAL CHALLENGES:
    // Only the newest committed transaction (tx_03) can be rolled back!
    assert!(
        item_t3.is_latest_committed,
        "LIFO order violation: Newest committed transaction 'tx_03_newest' did NOT get is_latest_committed = true!"
    );
    assert!(
        !item_t2.is_latest_committed,
        "LIFO order violation: Middle transaction 'tx_02_middle' was incorrectly granted rollback privilege!"
    );
    assert!(
        !item_t1.is_latest_committed,
        "LIFO order violation: Oldest transaction 'tx_01_oldest' was incorrectly granted rollback privilege!"
    );
}

/// Challenge 3: Dirty-State Detection Modal Confirmation Popup Invariant
///
/// Invariant: When `RollbackEngine::verify_dirty_state` encounters a modified file,
/// it raises `TransactionError::DirtyStateDetected`.
/// The GUI engine event handler MUST detect this error and open `dirty_modal`.
#[test]
fn test_adversarial_dirty_state_error_modal_trigger() {
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, None);

    let dirty_err = TransactionError::DirtyStateDetected {
        file: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
        expected_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        actual_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
    };

    let error_string = dirty_err.to_string();
    assert!(
        error_string.starts_with("Dirty state detected"),
        "Sanity check: formatted error string starts with 'Dirty state detected'"
    );

    // Deliver RollbackError to App
    let _ = app.update(Message::Engine(EngineEvent::RollbackError {
        error: error_string.clone(),
        tx_id: "tx_test_dirty".into(),
    }));

    // CRITICAL EMPIRICAL CHALLENGE:
    // dirty_modal MUST be opened to warn the user and require explicit confirmation!
    assert!(
        app.history_state.dirty_modal.is_some(),
        "Dirty-state modal failure: dirty_modal was None! Instead error_banner was set to: {:?}",
        app.error_banner
    );

    let modal = app.history_state.dirty_modal.as_ref().unwrap();
    assert_eq!(modal.transaction_id, "tx_test_dirty");
}

/// Challenge 4: Procedural Directive Approval Gating
///
/// Invariant: All procedural directives detected in configurations (`exec-once`, `exec`, `on-click`)
/// MUST be unapproved (neutralized) by default (`approved == false`).
/// No procedural command may be approved without explicit user action.
#[test]
fn test_adversarial_directive_approval_gating() {
    let repo = create_sample_catalog_repo("user", "my-rice", vec![ConfigComponent::Hyprland]);

    let directives = vec![
        ProceduralDirective {
            directive_type: "exec-once".into(),
            command: "waybar &".into(),
            file: "hyprland.conf".into(),
            line: 15,
        },
        ProceduralDirective {
            directive_type: "exec".into(),
            command: "swww init".into(),
            file: "hyprland.conf".into(),
            line: 22,
        },
        ProceduralDirective {
            directive_type: "on-click".into(),
            command: "alacritty -e btop".into(),
            file: "config.jsonc".into(),
            line: 45,
        },
    ];

    let mut install = InstallState::new(
        "tx_directives_test".into(),
        repo,
        vec![ConfigComponent::Hyprland],
        directives,
    );

    // Invariant: all directives MUST be unapproved by default
    assert_eq!(install.directive_approvals.len(), 3);
    for (i, d) in install.directive_approvals.iter().enumerate() {
        assert!(
            !d.approved,
            "Directive approval gate violation: Directive {} ('{}') was approved by default!",
            i, d.directive.command
        );
    }

    // Explicit user toggles
    install.toggle_directive_approval(0);
    assert!(install.directive_approvals[0].approved);
    assert!(!install.directive_approvals[1].approved);
    assert!(!install.directive_approvals[2].approved);

    // User untoggles
    install.toggle_directive_approval(0);
    assert!(!install.directive_approvals[0].approved);
}

/// Challenge 5: WAL Startup Crash Recovery Flow
///
/// Invariant: Incomplete WAL transactions in state `Applying` must trigger crash recovery
/// and display the startup notification banner in the GUI.
#[test]
fn test_adversarial_wal_crash_recovery_startup_flow() {
    let test_dir = Utf8PathBuf::from(format!("/tmp/woderrise_test_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&test_dir).expect("create test dir");
    let xdg = XdgPaths::custom(&test_dir);

    let journal_mgr = JournalManager::new(&xdg).expect("Journal manager");
    let tx_id = "tx_crashed_mid_flight";
    let backup_dir = xdg.backup_dir(tx_id).expect("Backup dir");

    // Begin Applying in WAL (simulates crash before commit)
    journal_mgr
        .begin_applying(
            tx_id,
            ConfigComponent::Hyprland,
            "catppuccin/hyprland",
            "deadbeef",
            &backup_dir,
            Vec::new(),
        )
        .expect("Begin applying");

    let tx_engine = TransactionEngine::new(xdg.clone()).expect("TransactionEngine");

    // Execute startup recovery
    let outcome = tx_engine
        .recover_on_startup()
        .expect("Recovery must succeed");
    assert!(
        outcome.is_some(),
        "Recovery outcome should be Some for Applying status"
    );

    let rec = outcome.unwrap();
    assert_eq!(rec.tx_id, tx_id);
    assert_eq!(rec.recovered_status, TransactionStatus::RecoveredRollback);

    // Now test GUI App startup notice integration
    let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(cmd_tx, Some(rec));

    assert!(
        app.startup_notice.is_some(),
        "Startup notice should be populated on crash recovery"
    );
    let notice_text = app.startup_notice.as_ref().unwrap();
    assert!(notice_text.contains(tx_id));

    // User dismisses startup notice
    let _ = app.update(Message::DismissStartupNotice);
    assert!(
        app.startup_notice.is_none(),
        "Startup notice should be cleared after dismissal"
    );

    let _ = fs::remove_dir_all(&test_dir);
}

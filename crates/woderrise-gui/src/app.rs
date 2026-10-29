//! Elm architecture application root, state machine, and Tokio channel bridge for Woderrise.

use camino::Utf8PathBuf;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Border, Color, Element, Length, Task, Theme};
use std::sync::Mutex;
use tracing::info;

use crate::theme;

use woderrise_core::models::{
    ActiveTransaction, CatalogRepository, ConfigComponent, ConflictStatus, DetectedComponent,
    JournalEntry, StagedFile, TargetMapping, VerificationReport,
};
use woderrise_detect::{ComponentDetection, TreeEntry};
use woderrise_github::{GitTree, RateLimitStatus};
use woderrise_transaction::{RecoveryOutcome, RollbackReport};

use crate::screens::history::{view_history, HistoryAction, HistoryState};
use chrono::{DateTime, Utc};

use crate::screens::inspect::{view_inspect, InspectAction, InspectState, InspectTab};
use crate::screens::install::{view_install, InstallPhase, InstallState};
use crate::screens::search::{view_search, CategoryFilter, SearchState, SortOption};
use crate::screens::settings::{
    calculate_directory_stats, format_bytes, safe_cleanup_cache, view_settings, CacheStats,
    SettingsAction, SettingsState, SettingsTab, ThemeMode, UiDensity,
};
use crate::widgets::diff::FileDiffReport;
use crate::widgets::gauge::{view_gauge, RateLimitGaugeState};

pub use crate::screens::settings::{format_reset_countdown, format_reset_duration};

/// Authoritative live connection and quota state for navigation header and Screen 4.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveConnectionState {
    pub core_remaining: u32,
    pub core_limit: u32,
    pub core_reset: DateTime<Utc>,
    pub search_remaining: u32,
    pub search_limit: u32,
    pub search_reset: DateTime<Utc>,
    pub is_authenticated: bool,
    pub username: Option<String>,
    pub last_checked: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub rate_limits: RateLimitGaugeState,
}

impl Default for LiveConnectionState {
    fn default() -> Self {
        Self {
            core_remaining: 60,
            core_limit: 60,
            core_reset: Utc::now() + chrono::Duration::hours(1),
            search_remaining: 10,
            search_limit: 10,
            search_reset: Utc::now() + chrono::Duration::minutes(1),
            is_authenticated: false,
            username: None,
            last_checked: None,
            last_error: None,
            rate_limits: RateLimitGaugeState::new(60, 60, 10, 10),
        }
    }
}

impl LiveConnectionState {
    pub fn to_gauge_state(&self) -> RateLimitGaugeState {
        RateLimitGaugeState {
            core_remaining: self.core_remaining,
            core_limit: self.core_limit,
            search_remaining: self.search_remaining,
            search_limit: self.search_limit,
        }
    }
}

/// Commands dispatched from GUI event loop to Tokio background worker.
#[derive(Debug)]
pub enum GuiCommand {
    /// Remote GitHub search with cancellation token.
    Search {
        query: String,
        token: tokio_util::sync::CancellationToken,
    },
    /// Refresh local catalog cache.
    RefreshCatalog,
    /// Inspect repository: fetch Git Tree and detect dotfile components.
    InspectPackage {
        repo: String,
        commit_sha: String,
        token: tokio_util::sync::CancellationToken,
    },
    /// Stream single file blob from Fastly CDN (0 REST quota).
    FetchFileContent {
        repo: String,
        commit_sha: String,
        path: String,
        blob_sha: Option<String>,
    },
    /// Stream remote files and compute Myers diff against local destination paths.
    ComputeDiff {
        repo: String,
        commit_sha: String,
        mappings: Vec<TargetMapping>,
    },
    /// Download and stage dotfiles into isolated ephemeral sandbox.
    StagePackage {
        tx_id: String,
        component: ConfigComponent,
        repo: String,
        commit_sha: String,
        entries: Vec<TreeEntry>,
    },
    /// Execute 4-phase atomic installation transaction.
    ApplyInstallation {
        tx_id: String,
        component: ConfigComponent,
        repo: String,
        commit_sha: String,
        staged_files: Vec<StagedFile>,
    },
    /// Cancel and clean up ephemeral staging sandbox.
    CancelStaging { tx_id: String },
    /// Load journal transaction history and active in-flight status.
    LoadHistory,
    /// Execute 1-click LIFO rollback for latest committed transaction.
    RollbackLatest { force_dirty: bool },
    /// Execute rollback for specific transaction ID.
    RollbackTransaction { tx_id: String, force_dirty: bool },
    /// Query current GitHub REST and Search rate limit statuses.
    FetchRateLimits,
    /// Update or clear GitHub personal access token in client.
    SetGitHubToken { token: Option<String> },
}

/// Events emitted from Tokio background worker to GUI event loop.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    // Search & Catalog
    SearchStarted {
        query: String,
    },
    SearchCompleted {
        query: String,
        results: Vec<CatalogRepository>,
        search_quota: RateLimitStatus,
    },
    SearchError {
        query: String,
        error: String,
    },
    RepoPreInspected {
        repo: String,
        components: Vec<DetectedComponent>,
    },

    // Inspect & Tree Resolution
    InspectStarted {
        repo: String,
    },
    TreeLoaded {
        repo: String,
        tree: GitTree,
        detections: Vec<ComponentDetection>,
    },
    FileContentFetched {
        repo: String,
        path: String,
        content: Vec<u8>,
    },
    DiffComputed {
        repo: String,
        diffs: Vec<FileDiffReport>,
    },
    InspectError {
        repo: String,
        error: String,
    },

    // Staging & Installation Progress
    StagingProgress {
        tx_id: String,
        file_path: String,
        index: usize,
        total: usize,
        bytes_downloaded: u64,
    },
    StagingCompleted {
        tx_id: String,
        report: VerificationReport,
        staged_files: Vec<StagedFile>,
    },
    StagingError {
        tx_id: String,
        error: String,
    },
    InstallProgress {
        tx_id: String,
        phase: InstallPhase,
        step_description: String,
        percent: u8,
    },
    InstallCompleted {
        tx_id: String,
        entry: JournalEntry,
    },
    InstallError {
        tx_id: String,
        phase: InstallPhase,
        error: String,
    },

    // History & Rollback
    HistoryLoaded {
        transactions: Vec<JournalEntry>,
        active_tx: Option<ActiveTransaction>,
    },
    RollbackProgress {
        tx_id: String,
        step_description: String,
        percent: u8,
    },
    RollbackCompleted {
        report: RollbackReport,
    },
    RollbackError {
        tx_id: String,
        error: String,
    },

    // System Diagnostics
    RateLimitsUpdated {
        core: RateLimitStatus,
        search: RateLimitStatus,
    },
    RateLimitsFetchFailed {
        error: String,
    },
    TokenVerificationFailed {
        error: String,
    },
    StartupRecoveryFinished {
        outcome: Option<RecoveryOutcome>,
    },
    PortalThemeChanged {
        dark_mode: bool,
    },
}

/// Active top-level screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Search,
    Inspect,
    Install,
    History,
    Settings,
}

/// Messages handled by the Iced application update loop.
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    NavigateTo(Screen),

    // Search screen interactions
    SearchInputChanged(String),
    SearchDebounceFired(String),
    SearchSubmitted,
    SearchClear,
    FilterSelected(CategoryFilter),
    SortSelected(SortOption),
    SelectRepo(CatalogRepository),

    // Inspect screen interactions
    SelectInspectTab(InspectTab),
    ToggleComponent(ConfigComponent),
    InitiateInstall,
    InspectAction(InspectAction),

    // Install screen interactions
    ToggleDirectiveApproval(usize),
    ConfirmInstall,
    CancelInstall,
    InstallDone,

    // History & Rollback interactions
    HistoryAction(HistoryAction),
    RequestRollbackLatest { force_dirty: bool },
    RequestRollbackTx { tx_id: String, force_dirty: bool },
    CloseDirtyModal,

    // Settings screen interactions
    SettingsAction(SettingsAction),
    SettingsTabSelected(SettingsTab),
    SettingsThemeSelected(ThemeMode),
    SettingsDensitySelected(UiDensity),
    SettingsTimeoutSelected(u64),
    SettingsClearCache,
    SettingsTokenChanged(String),
    SettingsToggleMask,
    SettingsSaveToken,
    SettingsClearToken,
    SettingsTestConnection,
    SettingsRefreshStatus,

    // Theming & Alerts
    ToggleDarkMode(bool),
    DismissError,
    DismissStartupNotice,

    // Periodic 1-second clock tick for live countdown timers
    ClockTick,

    // Async Channel Deliveries
    Engine(EngineEvent),
    Noop,
}

static EVENT_RECEIVER: Mutex<Option<tokio::sync::mpsc::Receiver<EngineEvent>>> = Mutex::new(None);

/// Stores the engine event receiver for subscription consumption.
pub fn set_engine_event_receiver(rx: tokio::sync::mpsc::Receiver<EngineEvent>) {
    if let Ok(mut lock) = EVENT_RECEIVER.lock() {
        *lock = Some(rx);
    }
}

fn engine_subscription() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(100, async |mut output| {
        use iced::futures::SinkExt;
        let rx_opt = EVENT_RECEIVER
            .lock()
            .ok()
            .and_then(|mut guard| guard.take());
        if let Some(mut rx) = rx_opt {
            while let Some(event) = rx.recv().await {
                let _ = output.send(Message::Engine(event)).await;
            }
        }
    })
}

/// The top-level Woderrise Elm architecture App state.
pub struct App {
    pub active_screen: Screen,
    pub search_state: SearchState,
    pub inspect_state: Option<InspectState>,
    pub install_state: Option<InstallState>,
    pub history_state: HistoryState,
    pub settings_state: SettingsState,
    pub connection_state: LiveConnectionState,
    pub active_theme: Theme,
    pub error_banner: Option<String>,
    pub startup_notice: Option<String>,
    pub cmd_tx: tokio::sync::mpsc::Sender<GuiCommand>,
    pub search_cancel_token: Option<tokio_util::sync::CancellationToken>,
    pub inspect_cancel_token: Option<tokio_util::sync::CancellationToken>,
    pub is_testing_connection: bool,
    pub pending_token_save: Option<Option<String>>,
}

impl App {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<GuiCommand>,
        recovery_outcome: Option<RecoveryOutcome>,
    ) -> Self {
        let startup_notice = recovery_outcome.map(|outcome| {
            format!(
                "Startup Crash Recovery: Interrupted transaction '{}' was automatically rolled back. Restored {} files.",
                outcome.tx_id, outcome.restored_files.len()
            )
        });

        // Trigger initial history & rate limit loads
        let _ = cmd_tx.try_send(GuiCommand::LoadHistory);
        let _ = cmd_tx.try_send(GuiCommand::FetchRateLimits);
        let _ = cmd_tx.try_send(GuiCommand::RefreshCatalog);

        let current_token = woderrise_core::paths::XdgPaths::from_env()
            .ok()
            .and_then(|paths| woderrise_core::models::load_settings(&paths.settings_file()).ok())
            .and_then(|s| s.github_token);

        let is_auth = current_token
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false);

        let mut settings_state = SettingsState::new(current_token);

        if let Ok(paths) = woderrise_core::paths::XdgPaths::from_env() {
            if let Ok(stats) = calculate_directory_stats(&paths.woderrise_cache_dir()) {
                settings_state.cache_stats = stats;
            }
        }

        Self {
            active_screen: Screen::Search,
            search_state: SearchState::new(Vec::new()),
            inspect_state: None,
            install_state: None,
            history_state: HistoryState::from_journal_entries(&[]),
            settings_state,
            connection_state: LiveConnectionState {
                is_authenticated: is_auth,
                ..Default::default()
            },
            active_theme: Theme::Dark,
            error_banner: None,
            startup_notice,
            cmd_tx,
            search_cancel_token: None,
            inspect_cancel_token: None,
            is_testing_connection: false,
            pending_token_save: None,
        }
    }

    pub fn title(&self) -> String {
        "Woderrise — Wayland Configuration Store".to_string()
    }

    pub fn theme(&self) -> Theme {
        self.active_theme.clone()
    }

    /// Returns true if the 1-second periodic clock tick subscription should be active.
    ///
    /// Active strictly when:
    /// - The user is on the Settings screen (where live API quota countdowns are displayed), OR
    /// - A GitHub connection test is actively running (`is_testing_connection == true`).
    ///
    /// Returns `false` on Discover, Inspect, Install, and History screens when idle,
    /// eliminating continuous event-loop wakeups, GPU redraws, and battery drain.
    pub fn is_clock_tick_needed(&self) -> bool {
        matches!(self.active_screen, Screen::Settings) || self.is_testing_connection
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs = vec![
            iced::Subscription::run(engine_subscription),
            iced::keyboard::listen().filter_map(|event| match event {
                iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                } => Some(Message::SearchClear),
                _ => None,
            }),
        ];

        // 1-second clock tick subscription is scoped strictly to Settings or active connection tests.
        // Prevents continuous wakeups, view rebuilds, and battery drain when on Discover, Inspect, or History.
        if self.is_clock_tick_needed() {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::ClockTick),
            );
        }

        iced::Subscription::batch(subs)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavigateTo(screen) => {
                self.active_screen = screen;
                if screen == Screen::History {
                    let _ = self.cmd_tx.try_send(GuiCommand::LoadHistory);
                } else if screen == Screen::Settings {
                    if let Ok(paths) = woderrise_core::paths::XdgPaths::from_env() {
                        if let Ok(stats) = calculate_directory_stats(&paths.woderrise_cache_dir()) {
                            self.settings_state.cache_stats = stats;
                        }
                    }
                    let _ = self.cmd_tx.try_send(GuiCommand::FetchRateLimits);
                }
            }

            Message::SearchInputChanged(query) => {
                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                self.search_state.set_query(query.clone(), now_ms);

                if let Some(token) = self.search_cancel_token.take() {
                    token.cancel();
                }

                if query.trim().is_empty() {
                    self.search_state.is_searching = false;
                    let _ = self.cmd_tx.try_send(GuiCommand::RefreshCatalog);
                    return Task::none();
                } else {
                    let query_clone = query;
                    return Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                            query_clone
                        },
                        Message::SearchDebounceFired,
                    );
                }
            }

            Message::SearchDebounceFired(debounced_query) => {
                let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                if self.search_state.query == debounced_query
                    && self.search_state.should_fire_debounce(now_ms, 300)
                    && !debounced_query.trim().is_empty()
                {
                    self.search_state.clear_debounce();
                    let token = tokio_util::sync::CancellationToken::new();
                    self.search_cancel_token = Some(token.clone());
                    self.search_state.is_searching = true;
                    let _ = self.cmd_tx.try_send(GuiCommand::Search {
                        query: debounced_query,
                        token,
                    });
                }
            }

            Message::SearchSubmitted => {
                if !self.search_state.query.trim().is_empty() {
                    self.search_state.clear_debounce();
                    let token = tokio_util::sync::CancellationToken::new();
                    self.search_cancel_token = Some(token.clone());
                    self.search_state.is_searching = true;
                    let _ = self.cmd_tx.try_send(GuiCommand::Search {
                        query: self.search_state.query.clone(),
                        token,
                    });
                }
            }

            Message::SearchClear => {
                self.search_state.clear();
                if let Some(token) = self.search_cancel_token.take() {
                    token.cancel();
                }
                let _ = self.cmd_tx.try_send(GuiCommand::RefreshCatalog);
            }

            Message::FilterSelected(filter) => {
                self.search_state.set_filter(filter);
            }

            Message::SortSelected(sort) => {
                self.search_state.set_sort_option(sort);
            }

            Message::SelectRepo(repo) => {
                let token = tokio_util::sync::CancellationToken::new();
                self.inspect_cancel_token = Some(token.clone());

                let mut inspect = InspectState::new(repo.clone(), Vec::new(), Vec::new());
                inspect.is_loading = true;
                self.inspect_state = Some(inspect);
                self.active_screen = Screen::Inspect;

                let _ = self.cmd_tx.try_send(GuiCommand::InspectPackage {
                    repo: repo.resolved_full_name(),
                    commit_sha: repo.default_branch,
                    token,
                });
            }

            Message::SelectInspectTab(tab) => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.active_tab = tab;
                }
            }

            Message::ToggleComponent(comp) => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.toggle_component(comp);
                }
            }

            Message::InspectAction(action) => match action {
                InspectAction::Back => {
                    self.active_screen = Screen::Search;
                }
                InspectAction::SelectTab(tab) => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.active_tab = tab;
                        if tab == InspectTab::DiffAndInstall && inspect.diff_reports.is_empty() {
                            let mappings = inspect.resolve_target_mappings();
                            let overwrites: Vec<_> = mappings
                                .into_iter()
                                .filter(|m| {
                                    m.conflict_status == ConflictStatus::OverwritingExisting
                                })
                                .collect();
                            if !overwrites.is_empty() {
                                let _ = self.cmd_tx.try_send(GuiCommand::ComputeDiff {
                                    repo: inspect.repository.resolved_full_name(),
                                    commit_sha: inspect.repository.default_branch.clone(),
                                    mappings: overwrites,
                                });
                            }
                        }
                    }
                }
                InspectAction::ToggleComponent(comp) => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.toggle_component(comp);
                    }
                }
                InspectAction::ToggleFile { path, component } => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.toggle_file(&path, component);
                    }
                }
                InspectAction::SelectAllComponents => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.select_all_components();
                    }
                }
                InspectAction::DeselectAllComponents => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.deselect_all_components();
                    }
                }
                InspectAction::ToggleExpandComponent(comp) => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.toggle_expand_component(comp);
                    }
                }
                InspectAction::TreeFilterChanged(filter) => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.tree_filter = filter;
                    }
                }
                InspectAction::InspectFile(path) => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.preview_file = Some(path.clone());
                        inspect.preview_content = None;
                        inspect.is_fetching_file = true;
                        let blob_sha = inspect
                            .full_tree_entries
                            .iter()
                            .find(|e| e.path == path)
                            .map(|e| e.sha.clone());
                        let _ = self.cmd_tx.try_send(GuiCommand::FetchFileContent {
                            repo: inspect.repository.resolved_full_name(),
                            commit_sha: inspect.repository.default_branch.clone(),
                            path,
                            blob_sha,
                        });
                    }
                }
                InspectAction::OpenConfirmDialog => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.open_confirm_dialog();
                    }
                }
                InspectAction::CloseConfirmDialog => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.close_confirm_dialog();
                    }
                }
                InspectAction::ToggleDirective(idx) => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.toggle_directive_approval(idx);
                    }
                }
                InspectAction::ConfirmInstall => {
                    if let Some(inspect) = &mut self.inspect_state {
                        inspect.close_confirm_dialog();
                    }
                    return self.update(Message::InitiateInstall);
                }
                InspectAction::OpenExternalUrl(url) => {
                    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                }
            },

            Message::InitiateInstall => {
                if let Some(inspect) = &self.inspect_state {
                    let tx_id = format!("tx_{}", uuid::Uuid::new_v4().simple());
                    let selected: Vec<ConfigComponent> =
                        inspect.selected_components.iter().copied().collect();

                    let mut install = InstallState::new(
                        tx_id.clone(),
                        inspect.repository.clone(),
                        selected.clone(),
                        inspect.procedural_directives.clone(),
                    );
                    for &idx in &inspect.approved_directives {
                        if let Some(approval) = install.directive_approvals.get_mut(idx) {
                            approval.approved = true;
                        }
                    }
                    self.install_state = Some(install);
                    self.active_screen = Screen::Install;

                    // Collect matched tree entries for selected components, excluding deselected_files
                    let mut entries = Vec::new();
                    for detection in &inspect.raw_detections {
                        if inspect.selected_components.contains(&detection.component) {
                            if !detection.matched_entries.is_empty() {
                                for entry in &detection.matched_entries {
                                    if !inspect.deselected_files.contains(&entry.path) {
                                        entries.push(entry.clone());
                                    }
                                }
                            } else {
                                if !inspect.deselected_files.contains(&detection.entrypoint) {
                                    entries.push(TreeEntry::new(
                                        detection.entrypoint.clone(),
                                        "100644",
                                        "blob",
                                        "0000000000000000000000000000000000000000",
                                        Some(detection.total_size_bytes),
                                    ));
                                }
                                if let Some(style) = &detection.style_file {
                                    if !inspect.deselected_files.contains(style) {
                                        entries.push(TreeEntry::new(
                                            style.clone(),
                                            "100644",
                                            "blob",
                                            "0000000000000000000000000000000000000000",
                                            None,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    if entries.is_empty() {
                        for comp in &inspect.detected_components {
                            if inspect.selected_components.contains(&comp.component) {
                                if !inspect.deselected_files.contains(&comp.entrypoint) {
                                    entries.push(TreeEntry::new(
                                        comp.entrypoint.clone(),
                                        "100644",
                                        "blob",
                                        "0000000000000000000000000000000000000000",
                                        Some(comp.total_size_bytes),
                                    ));
                                }
                                if let Some(style) = &comp.style_file {
                                    if !inspect.deselected_files.contains(style) {
                                        entries.push(TreeEntry::new(
                                            style.clone(),
                                            "100644",
                                            "blob",
                                            "0000000000000000000000000000000000000000",
                                            None,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    entries.sort_by(|a, b| a.path.cmp(&b.path));
                    entries.dedup_by(|a, b| a.path == b.path);

                    let primary_comp = selected
                        .first()
                        .copied()
                        .unwrap_or(ConfigComponent::Hyprland);

                    let _ = self.cmd_tx.try_send(GuiCommand::StagePackage {
                        tx_id,
                        component: primary_comp,
                        repo: inspect.repository.resolved_full_name(),
                        commit_sha: inspect.repository.default_branch.clone(),
                        entries,
                    });
                }
            }

            Message::ToggleDirectiveApproval(index) => {
                if let Some(install) = &mut self.install_state {
                    install.toggle_directive_approval(index);
                }
            }

            Message::ConfirmInstall => {
                if let Some(install) = &mut self.install_state {
                    let primary_comp = install
                        .selected_components
                        .first()
                        .copied()
                        .unwrap_or(ConfigComponent::Hyprland);

                    install.advance_phase(
                        InstallPhase::Backup,
                        0.35,
                        "Creating pre-install backup snapshot...",
                    );

                    let staged_files = install.staged_files.clone();

                    let _ = self.cmd_tx.try_send(GuiCommand::ApplyInstallation {
                        tx_id: install.transaction_id.clone(),
                        component: primary_comp,
                        repo: install.repository.resolved_full_name(),
                        commit_sha: install.repository.default_branch.clone(),
                        staged_files,
                    });
                }
            }

            Message::CancelInstall => {
                if let Some(install) = &self.install_state {
                    let _ = self.cmd_tx.try_send(GuiCommand::CancelStaging {
                        tx_id: install.transaction_id.clone(),
                    });
                }
                self.install_state = None;
                self.active_screen = Screen::Inspect;
            }

            Message::InstallDone => {
                self.install_state = None;
                self.active_screen = Screen::History;
                let _ = self.cmd_tx.try_send(GuiCommand::LoadHistory);
            }

            Message::HistoryAction(action) => match action {
                HistoryAction::SearchChanged(q) => {
                    self.history_state.search_query = q;
                }
                HistoryAction::StatusFilterSelected(sf) => {
                    self.history_state.status_filter = sf;
                }
                HistoryAction::ComponentFilterSelected(cf) => {
                    self.history_state.component_filter = cf;
                }
                HistoryAction::ClearFilters => {
                    self.history_state.clear_filters();
                }
                HistoryAction::Inspect(item) => {
                    self.history_state.open_inspect_modal(item);
                }
                HistoryAction::CloseInspect => {
                    self.history_state.close_inspect_modal();
                }
                HistoryAction::OpenRollbackConfirm(item) => {
                    self.history_state.open_rollback_confirm(&item);
                }
                HistoryAction::CloseRollbackConfirm => {
                    self.history_state.close_rollback_confirm();
                }
                HistoryAction::ExecuteRollback { tx_id, force_dirty } => {
                    self.history_state.close_rollback_confirm();
                    self.history_state.is_rolling_back = true;
                    self.history_state.close_dirty_modal();
                    let _ = self
                        .cmd_tx
                        .try_send(GuiCommand::RollbackTransaction { tx_id, force_dirty });
                }
                HistoryAction::CloseDirtyModal => {
                    self.history_state.close_dirty_modal();
                }
                HistoryAction::NavigateDiscover => {
                    self.active_screen = Screen::Search;
                }
            },

            Message::RequestRollbackLatest { force_dirty } => {
                self.history_state.is_rolling_back = true;
                let _ = self
                    .cmd_tx
                    .try_send(GuiCommand::RollbackLatest { force_dirty });
            }

            Message::RequestRollbackTx { tx_id, force_dirty } => {
                self.history_state.is_rolling_back = true;
                self.history_state.close_dirty_modal();
                let _ = self
                    .cmd_tx
                    .try_send(GuiCommand::RollbackTransaction { tx_id, force_dirty });
            }

            Message::CloseDirtyModal => {
                self.history_state.close_dirty_modal();
            }

            Message::ClockTick => {
                // Second-by-second UI refresh for live countdown timers
            }

            Message::SettingsAction(action) => match action {
                SettingsAction::TabSelected(tab) => {
                    return self.update(Message::SettingsTabSelected(tab));
                }
                SettingsAction::TokenChanged(token) => {
                    return self.update(Message::SettingsTokenChanged(token));
                }
                SettingsAction::ToggleMask => {
                    return self.update(Message::SettingsToggleMask);
                }
                SettingsAction::SaveToken => {
                    return self.update(Message::SettingsSaveToken);
                }
                SettingsAction::ClearToken => {
                    return self.update(Message::SettingsClearToken);
                }
                SettingsAction::TestConnection => {
                    return self.update(Message::SettingsTestConnection);
                }
                SettingsAction::RefreshStatus => {
                    return self.update(Message::SettingsRefreshStatus);
                }
                SettingsAction::ThemeSelected(mode) => {
                    return self.update(Message::SettingsThemeSelected(mode));
                }
                SettingsAction::DensitySelected(density) => {
                    return self.update(Message::SettingsDensitySelected(density));
                }
                SettingsAction::TimeoutSelected(secs) => {
                    return self.update(Message::SettingsTimeoutSelected(secs));
                }
                SettingsAction::ClearCache => {
                    return self.update(Message::SettingsClearCache);
                }
                SettingsAction::RefreshCatalog => {
                    let _ = self.cmd_tx.try_send(GuiCommand::RefreshCatalog);
                }
            },

            Message::SettingsTabSelected(tab) => {
                self.settings_state.set_tab(tab);
            }

            Message::SettingsThemeSelected(mode) => {
                self.settings_state.set_theme_mode(mode);
                match mode {
                    ThemeMode::Dark => self.active_theme = Theme::Dark,
                    ThemeMode::Light => self.active_theme = Theme::Light,
                    ThemeMode::System => {
                        return Task::perform(crate::theme::fetch_portal_theme(), |theme| {
                            Message::ToggleDarkMode(theme == Theme::Dark)
                        });
                    }
                }
            }

            Message::SettingsDensitySelected(density) => {
                self.settings_state.set_ui_density(density);
            }

            Message::SettingsTimeoutSelected(secs) => {
                self.settings_state.set_timeout(secs);
            }

            Message::SettingsClearCache => {
                self.settings_state.is_clearing_cache = true;
                let cache_dir = woderrise_core::paths::XdgPaths::from_env()
                    .map(|p| p.woderrise_cache_dir())
                    .unwrap_or_else(|_| camino::Utf8PathBuf::from("/tmp/woderrise_cache"));
                match safe_cleanup_cache(&cache_dir) {
                    Ok((count, bytes)) => {
                        self.settings_state.cache_stats = CacheStats::default();
                        self.settings_state.is_clearing_cache = false;
                        self.settings_state.set_notice(
                            format!(
                                "✓ Cache cleaned successfully. Freed {} across {} items.",
                                format_bytes(bytes),
                                count
                            ),
                            false,
                        );
                    }
                    Err(e) => {
                        self.settings_state.is_clearing_cache = false;
                        self.settings_state
                            .set_notice(format!("Failed to clean cache: {}", e), true);
                    }
                }
            }

            Message::SettingsTokenChanged(token) => {
                self.settings_state.set_token_input(token);
            }

            Message::SettingsToggleMask => {
                self.settings_state.toggle_mask();
            }

            Message::SettingsSaveToken => {
                let token = self.settings_state.token_input.trim().to_string();
                if token.is_empty() {
                    self.settings_state.set_notice(
                        "Token cannot be empty. Use 'Clear Token' to remove an existing token.",
                        true,
                    );
                } else {
                    self.settings_state.is_busy = true;
                    self.is_testing_connection = false;
                    self.pending_token_save = Some(Some(token.clone()));
                    self.settings_state
                        .set_notice("Saving token and verifying with GitHub API...", false);
                    let _ = self
                        .cmd_tx
                        .try_send(GuiCommand::SetGitHubToken { token: Some(token) });
                }
            }

            Message::SettingsClearToken => {
                self.settings_state.token_input.clear();
                self.settings_state.is_busy = true;
                self.is_testing_connection = false;
                self.pending_token_save = Some(None);
                self.settings_state.set_notice(
                    "Clearing token and reverting to unauthenticated limits...",
                    false,
                );
                let _ = self
                    .cmd_tx
                    .try_send(GuiCommand::SetGitHubToken { token: None });
            }

            Message::SettingsTestConnection => {
                self.settings_state.is_busy = true;
                self.is_testing_connection = true;
                self.pending_token_save = None;
                self.settings_state.set_notice(
                    "Testing GitHub API connection and refreshing rate limit quotas...",
                    false,
                );
                let _ = self.cmd_tx.try_send(GuiCommand::FetchRateLimits);
            }

            Message::SettingsRefreshStatus => {
                self.settings_state.is_busy = true;
                self.is_testing_connection = true;
                self.pending_token_save = None;
                self.settings_state
                    .set_notice("Refreshing GitHub API rate limit quotas...", false);
                let _ = self.cmd_tx.try_send(GuiCommand::FetchRateLimits);
            }

            Message::ToggleDarkMode(dark) => {
                self.active_theme = if dark { Theme::Dark } else { Theme::Light };
            }

            Message::DismissError => {
                self.error_banner = None;
            }

            Message::DismissStartupNotice => {
                self.startup_notice = None;
            }

            Message::Engine(event) => {
                self.handle_engine_event(event);
            }

            Message::Noop => {}
        }

        Task::none()
    }

    fn handle_engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::SearchStarted { .. } => {
                self.search_state.is_searching = true;
                if let Some(err) = &self.error_banner {
                    if err.starts_with("GitHub Search Error:") {
                        self.error_banner = None;
                    }
                }
            }

            EngineEvent::SearchCompleted {
                results,
                search_quota,
                ..
            } => {
                self.search_state.is_searching = false;
                if let Some(err) = &self.error_banner {
                    if err.starts_with("GitHub Search Error:") {
                        self.error_banner = None;
                    }
                }
                self.search_state.catalog_repositories = results;
                self.search_state.rate_limits.search_remaining = search_quota.remaining;
                self.search_state.rate_limits.search_limit = search_quota.limit;
                self.connection_state.search_remaining = search_quota.remaining;
                self.connection_state.search_limit = search_quota.limit;
                self.connection_state.search_reset = search_quota.reset_time;
                self.connection_state.last_checked = Some(Utc::now());
                self.connection_state.rate_limits.search_remaining = search_quota.remaining;
                self.connection_state.rate_limits.search_limit = search_quota.limit;
                self.settings_state.search_reset = Some(search_quota.reset_time);
                self.search_state.apply_filter();
            }

            EngineEvent::SearchError { error, .. } => {
                self.search_state.is_searching = false;
                self.error_banner = Some(format!("GitHub Search Error: {}", error));
            }

            EngineEvent::RepoPreInspected { repo, components } => {
                self.search_state.update_repo_components(&repo, components);
            }

            EngineEvent::InspectStarted { .. } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.is_loading = true;
                }
            }

            EngineEvent::TreeLoaded {
                tree, detections, ..
            } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.is_loading = false;
                    let mut tree_entries: Vec<TreeEntry> = tree
                        .tree
                        .iter()
                        .map(|e| {
                            TreeEntry::new(
                                e.path.clone(),
                                e.mode.clone(),
                                e.entry_type.clone(),
                                e.sha.clone(),
                                e.size,
                            )
                        })
                        .collect();
                    if tree_entries.is_empty() {
                        for d in &detections {
                            tree_entries.extend(d.matched_entries.clone());
                        }
                    }
                    tree_entries.sort_by(|a, b| a.path.cmp(&b.path));
                    tree_entries.dedup_by(|a, b| a.path == b.path);
                    inspect.full_tree_entries = tree_entries;
                    inspect.detected_components = detections.iter().map(|d| d.into()).collect();
                    inspect.raw_detections = detections.clone();
                    for d in &detections {
                        if d.component != ConfigComponent::Monitors {
                            inspect.selected_components.insert(d.component);
                        }
                        inspect.hardware_notices.extend(d.hardware_notices.clone());
                    }
                }
            }

            EngineEvent::FileContentFetched { path, content, .. } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.is_fetching_file = false;
                    if path.to_lowercase().ends_with("readme.md") {
                        if let Ok(src) = String::from_utf8(content.clone()) {
                            inspect.set_markdown(src);
                        }
                    }
                    inspect.preview_file = Some(path);
                    inspect.preview_content = String::from_utf8(content).ok();
                }
            }

            EngineEvent::DiffComputed { diffs, .. } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.diff_reports = diffs;
                }
            }

            EngineEvent::InspectError { error, .. } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.is_loading = false;
                }
                self.error_banner = Some(format!("Inspect Error: {}", error));
            }

            EngineEvent::StagingProgress {
                file_path,
                index,
                total,
                ..
            } => {
                if let Some(install) = &mut self.install_state {
                    install.advance_phase(
                        InstallPhase::Staging,
                        (index as f32 / total.max(1) as f32) * 0.25,
                        format!("Streaming '{}' ({}/{})", file_path, index, total),
                    );
                }
            }

            EngineEvent::StagingCompleted { staged_files, .. } => {
                if let Some(install) = &mut self.install_state {
                    install.staged_files = staged_files.clone();
                    install.installed_files = staged_files
                        .iter()
                        .map(|s| s.relative_path.clone())
                        .collect();
                    install.advance_phase(
                        InstallPhase::PendingApproval,
                        0.25,
                        "Staging verified. Ready for approval.",
                    );
                }
            }

            EngineEvent::StagingError { error, .. } => {
                if let Some(install) = &mut self.install_state {
                    install.fail(format!("Staging failed: {}", error));
                }
                self.error_banner = Some(format!("Staging error: {}", error));
            }

            EngineEvent::InstallProgress {
                phase,
                step_description,
                percent,
                ..
            } => {
                if let Some(install) = &mut self.install_state {
                    install.advance_phase(phase, percent as f32 / 100.0, step_description);
                }
            }

            EngineEvent::InstallCompleted { .. } => {
                if let Some(install) = &mut self.install_state {
                    install.advance_phase(
                        InstallPhase::Completed,
                        1.0,
                        "Installation committed successfully!",
                    );
                }
                let _ = self.cmd_tx.try_send(GuiCommand::LoadHistory);
            }

            EngineEvent::InstallError { error, .. } => {
                if let Some(install) = &mut self.install_state {
                    install.fail(format!("Installation failed: {}", error));
                }
                self.error_banner = Some(format!("Installation error: {}", error));
            }

            EngineEvent::HistoryLoaded { transactions, .. } => {
                self.history_state = HistoryState::from_journal_entries(&transactions);
            }

            EngineEvent::RollbackProgress {
                step_description, ..
            } => {
                info!("Rollback progress: {}", step_description);
            }

            EngineEvent::RollbackCompleted { report } => {
                self.history_state.is_rolling_back = false;
                info!("Rollback completed: {:?}", report.tx_id);
                let _ = self.cmd_tx.try_send(GuiCommand::LoadHistory);
            }

            EngineEvent::RollbackError { error, tx_id } => {
                self.history_state.is_rolling_back = false;
                let err_lower = error.to_lowercase();
                if err_lower.contains("dirty") || err_lower.contains("modified") {
                    let file_path = if let Some(start) = error.find('\'') {
                        if let Some(end) = error[start + 1..].find('\'') {
                            Utf8PathBuf::from(&error[start + 1..start + 1 + end])
                        } else {
                            Utf8PathBuf::from("modified_dotfile")
                        }
                    } else {
                        Utf8PathBuf::from("modified_dotfile")
                    };
                    self.history_state.open_dirty_modal(tx_id, vec![file_path]);
                } else {
                    self.error_banner = Some(format!("Rollback failed: {}", error));
                }
            }

            EngineEvent::RateLimitsUpdated { core, search } => {
                self.connection_state.core_remaining = core.remaining;
                self.connection_state.core_limit = core.limit;
                self.connection_state.core_reset = core.reset_time;
                self.connection_state.search_remaining = search.remaining;
                self.connection_state.search_limit = search.limit;
                self.connection_state.search_reset = search.reset_time;
                self.connection_state.is_authenticated = self.settings_state.is_authenticated();
                self.connection_state.last_checked = Some(Utc::now());
                self.connection_state.last_error = None;

                self.settings_state.core_reset = Some(core.reset_time);
                self.settings_state.search_reset = Some(search.reset_time);
                self.settings_state.last_checked = Some(Utc::now());

                self.connection_state.rate_limits = RateLimitGaugeState::new(
                    core.remaining,
                    core.limit,
                    search.remaining,
                    search.limit,
                );

                self.search_state.set_rate_limits(
                    core.remaining,
                    core.limit,
                    search.remaining,
                    search.limit,
                );
                if self.is_testing_connection {
                    self.is_testing_connection = false;
                    self.settings_state.is_busy = false;
                    self.settings_state.set_notice(
                        format!(
                            "Connection verified. Active limits — Search: {}/{} remaining, Core: {}/{} remaining.",
                            search.remaining, search.limit, core.remaining, core.limit
                        ),
                        false,
                    );
                } else if let Some(saved) = self.pending_token_save.take() {
                    self.settings_state.set_saved(saved);
                    self.connection_state.is_authenticated = self.settings_state.is_authenticated();
                } else if self.settings_state.is_busy {
                    let token = if self.settings_state.token_input.trim().is_empty() {
                        None
                    } else {
                        Some(self.settings_state.token_input.trim().to_string())
                    };
                    self.settings_state.set_saved(token);
                    self.connection_state.is_authenticated = self.settings_state.is_authenticated();
                } else if self.active_screen == Screen::Settings {
                    self.settings_state.set_notice(
                        format!(
                            "Rate limits active — Search: {}/{} remaining, Core: {}/{} remaining.",
                            search.remaining, search.limit, core.remaining, core.limit
                        ),
                        false,
                    );
                }
            }

            EngineEvent::RateLimitsFetchFailed { error } => {
                self.connection_state.last_error = Some(error.clone());
                if self.is_testing_connection {
                    self.is_testing_connection = false;
                    self.settings_state.is_busy = false;
                    self.settings_state
                        .set_notice(format!("Connection test failed: {}", error), true);
                    self.error_banner =
                        Some(format!("GitHub API connection test failed: {}", error));
                }
            }

            EngineEvent::TokenVerificationFailed { error } => {
                self.is_testing_connection = false;
                self.pending_token_save = None;
                self.settings_state.is_busy = false;
                self.settings_state
                    .set_notice(format!("Verification failed: {}", error), true);
                self.error_banner = Some(format!("GitHub token verification failed: {}", error));
            }

            EngineEvent::StartupRecoveryFinished { outcome } => {
                if let Some(outcome) = outcome {
                    self.startup_notice = Some(format!(
                        "Interrupted transaction '{}' recovered and rolled back ({} files restored).",
                        outcome.tx_id,
                        outcome.restored_files.len()
                    ));
                }
            }

            EngineEvent::PortalThemeChanged { dark_mode } => {
                self.active_theme = if dark_mode { Theme::Dark } else { Theme::Light };
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let tokens = theme::tokens(&self.active_theme);

        // 1. Navigation Header Bar
        let brand = row![
            text("🌊").size(18),
            text("WODERRISE").size(16).color(tokens.text_primary),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let is_search = self.active_screen == Screen::Search;
        let is_history = self.active_screen == Screen::History;
        let is_settings = self.active_screen == Screen::Settings;

        let nav_search = button(text("Discover").size(13))
            .padding([6, 14])
            .on_press(Message::NavigateTo(Screen::Search))
            .style(move |_theme, status| theme::button_tab(tokens, is_search, status));

        let nav_history = button(text("History & Rollback").size(13))
            .padding([6, 14])
            .on_press(Message::NavigateTo(Screen::History))
            .style(move |_theme, status| theme::button_tab(tokens, is_history, status));

        let nav_settings = button(text("⚙ Settings").size(13))
            .padding([6, 14])
            .on_press(Message::NavigateTo(Screen::Settings))
            .style(move |_theme, status| theme::button_tab(tokens, is_settings, status));

        let nav_pill_bar = container(row![nav_search, nav_history, nav_settings].spacing(4))
            .padding(3)
            .style(move |_theme| container::Style {
                background: Some(tokens.bg_base.into()),
                border: Border {
                    color: tokens.border_subtle,
                    width: 1.0,
                    radius: theme::radius::PILL,
                },
                ..Default::default()
            });

        let rate_gauge = view_gauge(&self.connection_state.rate_limits);

        let is_dark = tokens == theme::ThemeTokens::dark();
        let theme_toggle_btn = button(
            row![
                text(if is_dark { "☀️" } else { "🌙" }).size(12),
                text(if is_dark { "Light" } else { "Dark" })
                    .size(12)
                    .color(tokens.text_primary),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([5, 12])
        .on_press(Message::ToggleDarkMode(!is_dark))
        .style(move |_theme, status| theme::button_secondary(tokens, status));

        let header = container(
            row![
                brand,
                Space::new().width(Length::Fixed(12.0)),
                nav_pill_bar,
                Space::new().width(Length::Fill),
                row![rate_gauge, theme_toggle_btn]
                    .spacing(12)
                    .align_y(Alignment::Center),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_surface.into()),
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        });

        // 2. Alert / Notice Banners
        let mut alerts_col = column![].spacing(6);

        if let Some(notice) = &self.startup_notice {
            let dismiss_btn = button(text("✕").size(12))
                .padding([2, 6])
                .on_press(Message::DismissStartupNotice)
                .style(move |_theme, status| theme::button_secondary(tokens, status));

            let notice_bar = container(
                row![
                    text(format!("ℹ️ {}", notice))
                        .size(12)
                        .color(tokens.warning),
                    dismiss_btn,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([6, 14])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(
                    Color::from_rgba(tokens.warning.r, tokens.warning.g, tokens.warning.b, 0.15)
                        .into(),
                ),
                border: Border {
                    color: tokens.warning,
                    width: 1.0,
                    radius: theme::radius::SM,
                },
                ..Default::default()
            });

            alerts_col = alerts_col.push(notice_bar);
        }

        if let Some(err) = &self.error_banner {
            let dismiss_btn = button(text("✕").size(12))
                .padding([2, 6])
                .on_press(Message::DismissError)
                .style(move |_theme, status| theme::button_secondary(tokens, status));

            let error_bar = container(
                row![
                    text(format!("❌ {}", err)).size(12).color(tokens.danger),
                    dismiss_btn,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([6, 14])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(
                    Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.15)
                        .into(),
                ),
                border: Border {
                    color: tokens.danger,
                    width: 1.0,
                    radius: theme::radius::SM,
                },
                ..Default::default()
            });

            alerts_col = alerts_col.push(error_bar);
        }

        // 3. Screen Body
        let body: Element<Message> = match self.active_screen {
            Screen::Search => view_search(
                &self.search_state,
                &self.active_theme,
                Message::SearchInputChanged,
                Message::SearchSubmitted,
                Message::SearchClear,
                Message::FilterSelected,
                Message::SortSelected,
                Message::SelectRepo,
            ),
            Screen::Inspect => {
                if let Some(inspect) = &self.inspect_state {
                    view_inspect(inspect, &self.active_theme, Message::InspectAction)
                } else {
                    view_search(
                        &self.search_state,
                        &self.active_theme,
                        Message::SearchInputChanged,
                        Message::SearchSubmitted,
                        Message::SearchClear,
                        Message::FilterSelected,
                        Message::SortSelected,
                        Message::SelectRepo,
                    )
                }
            }
            Screen::Install => {
                if let Some(install) = &self.install_state {
                    view_install(
                        install,
                        Message::ToggleDirectiveApproval,
                        Message::ConfirmInstall,
                        Message::CancelInstall,
                        Message::RequestRollbackLatest { force_dirty: false },
                        Message::InstallDone,
                    )
                } else {
                    view_search(
                        &self.search_state,
                        &self.active_theme,
                        Message::SearchInputChanged,
                        Message::SearchSubmitted,
                        Message::SearchClear,
                        Message::FilterSelected,
                        Message::SortSelected,
                        Message::SelectRepo,
                    )
                }
            }
            Screen::History => view_history(
                &self.history_state,
                &self.active_theme,
                Message::HistoryAction,
            ),
            Screen::Settings => view_settings(
                &self.settings_state,
                &self.connection_state.rate_limits,
                &self.active_theme,
                Message::SettingsAction,
            ),
        };

        container(column![header, alerts_col, body].spacing(0))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| theme::container_base(tokens))
            .into()
    }
}

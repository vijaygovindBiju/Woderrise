//! Elm architecture application root, state machine, and Tokio channel bridge for Woderrise.

use camino::Utf8PathBuf;
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Task, Theme};
use std::sync::Mutex;
use tracing::info;

use woderrise_core::models::{
    ActiveTransaction, CatalogRepository, ConfigComponent, JournalEntry, StagedFile, TargetMapping,
    VerificationReport,
};
use woderrise_detect::{ComponentDetection, TreeEntry};
use woderrise_github::{GitTree, RateLimitStatus};
use woderrise_transaction::{RecoveryOutcome, RollbackReport};

use crate::screens::history::{view_history, HistoryState};
use crate::screens::inspect::{view_inspect, InspectState, InspectTab};
use crate::screens::install::{view_install, InstallPhase, InstallState};
use crate::screens::search::{view_search, CategoryFilter, SearchState};
use crate::widgets::diff::FileDiffReport;
use crate::widgets::gauge::view_gauge;

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
    FilterSelected(CategoryFilter),
    SelectRepo(CatalogRepository),

    // Inspect screen interactions
    SelectInspectTab(InspectTab),
    ToggleComponent(ConfigComponent),
    InitiateInstall,

    // Install screen interactions
    ToggleDirectiveApproval(usize),
    ConfirmInstall,
    CancelInstall,
    InstallDone,

    // History & Rollback interactions
    RequestRollbackLatest { force_dirty: bool },
    RequestRollbackTx { tx_id: String, force_dirty: bool },
    CloseDirtyModal,

    // Theming & Alerts
    ToggleDarkMode(bool),
    DismissError,
    DismissStartupNotice,

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
    pub active_theme: Theme,
    pub error_banner: Option<String>,
    pub startup_notice: Option<String>,
    pub cmd_tx: tokio::sync::mpsc::Sender<GuiCommand>,
    pub search_cancel_token: Option<tokio_util::sync::CancellationToken>,
    pub inspect_cancel_token: Option<tokio_util::sync::CancellationToken>,
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

        Self {
            active_screen: Screen::Search,
            search_state: SearchState::new(Vec::new()),
            inspect_state: None,
            install_state: None,
            history_state: HistoryState::from_journal_entries(&[]),
            active_theme: Theme::Dark,
            error_banner: None,
            startup_notice,
            cmd_tx,
            search_cancel_token: None,
            inspect_cancel_token: None,
        }
    }

    pub fn title(&self) -> String {
        "Woderrise — Wayland Configuration Store".to_string()
    }

    pub fn theme(&self) -> Theme {
        self.active_theme.clone()
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        iced::Subscription::run(engine_subscription)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavigateTo(screen) => {
                self.active_screen = screen;
                if screen == Screen::History {
                    let _ = self.cmd_tx.try_send(GuiCommand::LoadHistory);
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
                    let q = query.clone();
                    return Task::perform(
                        async move {
                            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                            q
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

            Message::FilterSelected(filter) => {
                self.search_state.set_filter(filter);
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

            Message::InitiateInstall => {
                if let Some(inspect) = &self.inspect_state {
                    let tx_id = format!("tx_{}", uuid::Uuid::new_v4().simple());
                    let selected: Vec<ConfigComponent> =
                        inspect.selected_components.iter().copied().collect();

                    let install = InstallState::new(
                        tx_id.clone(),
                        inspect.repository.clone(),
                        selected.clone(),
                        inspect.procedural_directives.clone(),
                    );
                    self.install_state = Some(install);
                    self.active_screen = Screen::Install;

                    // Collect matched tree entries for selected components
                    let mut entries = Vec::new();
                    for detection in &inspect.raw_detections {
                        if inspect.selected_components.contains(&detection.component) {
                            if !detection.matched_entries.is_empty() {
                                entries.extend(detection.matched_entries.clone());
                            } else {
                                entries.push(TreeEntry::new(
                                    detection.entrypoint.clone(),
                                    "100644",
                                    "blob",
                                    "0000000000000000000000000000000000000000",
                                    Some(detection.total_size_bytes),
                                ));
                                if let Some(style) = &detection.style_file {
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
                    if entries.is_empty() {
                        for comp in &inspect.detected_components {
                            if inspect.selected_components.contains(&comp.component) {
                                entries.push(TreeEntry::new(
                                    comp.entrypoint.clone(),
                                    "100644",
                                    "blob",
                                    "0000000000000000000000000000000000000000",
                                    Some(comp.total_size_bytes),
                                ));
                                if let Some(style) = &comp.style_file {
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
            }

            EngineEvent::SearchCompleted {
                results,
                search_quota,
                ..
            } => {
                self.search_state.is_searching = false;
                self.search_state.catalog_repositories = results;
                self.search_state.rate_limits.search_remaining = search_quota.remaining;
                self.search_state.rate_limits.search_limit = search_quota.limit;
                self.search_state.apply_filter();
            }

            EngineEvent::SearchError { error, .. } => {
                self.search_state.is_searching = false;
                self.error_banner = Some(format!("GitHub Search Error: {}", error));
            }

            EngineEvent::InspectStarted { .. } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.is_loading = true;
                }
            }

            EngineEvent::TreeLoaded {
                tree: _,
                detections,
                ..
            } => {
                if let Some(inspect) = &mut self.inspect_state {
                    inspect.is_loading = false;
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
                self.search_state.set_rate_limits(
                    core.remaining,
                    core.limit,
                    search.remaining,
                    search.limit,
                );
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
        // 1. Navigation Header Bar
        let brand = text("WODERRISE")
            .size(17)
            .color(Color::from_rgb(0.55, 0.75, 0.98));

        let nav_search = button(text("Discover").size(13))
            .padding([6, 12])
            .on_press(Message::NavigateTo(Screen::Search));
        let nav_history = button(text("History & Rollback").size(13))
            .padding([6, 12])
            .on_press(Message::NavigateTo(Screen::History));

        let rate_gauge = view_gauge(&self.search_state.rate_limits);

        let is_dark = self.active_theme == Theme::Dark;
        let theme_toggle_btn = button(text(if is_dark { "☀️ Light" } else { "🌙 Dark" }).size(12))
            .padding([4, 10])
            .on_press(Message::ToggleDarkMode(!is_dark));

        let header = container(
            row![
                brand,
                row![nav_search, nav_history].spacing(8),
                row![rate_gauge, theme_toggle_btn]
                    .spacing(12)
                    .align_y(Alignment::Center),
            ]
            .spacing(18)
            .align_y(Alignment::Center),
        )
        .padding([10, 16])
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.09, 0.1, 0.15, 0.98).into()),
            border: iced::Border {
                color: Color::from_rgba(0.25, 0.28, 0.4, 0.4),
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
                .on_press(Message::DismissStartupNotice);

            let notice_bar = container(
                row![
                    text(format!("ℹ️ {}", notice))
                        .size(12)
                        .color(Color::from_rgb(0.95, 0.85, 0.4)),
                    dismiss_btn,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([6, 14])
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.3, 0.25, 0.1, 0.4).into()),
                border: iced::Border {
                    color: Color::from_rgb(0.8, 0.65, 0.2),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

            alerts_col = alerts_col.push(notice_bar);
        }

        if let Some(err) = &self.error_banner {
            let dismiss_btn = button(text("✕").size(12))
                .padding([2, 6])
                .on_press(Message::DismissError);

            let error_bar = container(
                row![
                    text(format!("❌ {}", err))
                        .size(12)
                        .color(Color::from_rgb(0.96, 0.45, 0.45)),
                    dismiss_btn,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([6, 14])
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.4, 0.1, 0.12, 0.4).into()),
                border: iced::Border {
                    color: Color::from_rgb(0.85, 0.3, 0.3),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

            alerts_col = alerts_col.push(error_bar);
        }

        // 3. Screen Body
        let body: Element<Message> = match self.active_screen {
            Screen::Search => view_search(
                &self.search_state,
                Message::SearchInputChanged,
                Message::SearchSubmitted,
                Message::FilterSelected,
                Message::SelectRepo,
            ),
            Screen::Inspect => {
                if let Some(inspect) = &self.inspect_state {
                    view_inspect(
                        inspect,
                        Message::NavigateTo(Screen::Search),
                        Message::ToggleComponent,
                        Message::SelectInspectTab,
                        Message::InitiateInstall,
                    )
                } else {
                    view_search(
                        &self.search_state,
                        Message::SearchInputChanged,
                        Message::SearchSubmitted,
                        Message::FilterSelected,
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
                        Message::SearchInputChanged,
                        Message::SearchSubmitted,
                        Message::FilterSelected,
                        Message::SelectRepo,
                    )
                }
            }
            Screen::History => view_history(
                &self.history_state,
                |force| Message::RequestRollbackLatest { force_dirty: force },
                |tx_id, force| Message::RequestRollbackTx {
                    tx_id,
                    force_dirty: force,
                },
                Message::CloseDirtyModal,
            ),
        };

        column![header, alerts_col, body].spacing(0).into()
    }
}

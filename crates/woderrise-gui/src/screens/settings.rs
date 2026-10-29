//! Settings, GitHub Authentication, Appearance & Cache Management Screen.
//!
//! Provides an interface organized in a responsive two-column category layout:
//! - **GitHub Tab**: Personal Access Token configuration with strict POSIX 0600 storage,
//!   live connection status, elevated rate limits (Search & Core REST) with color tiers
//!   and live countdown timers.
//! - **Appearance Tab**: System, Dark, Light theme preferences and UI density selection
//!   with live WCAG AAA contrast ratio calculation badges.
//! - **Network & Cache Tab**: $XDG_CACHE_HOME/woderrise usage metrics, safe cache cleanup
//!   strictly preserving backups, WAL journals, and settings, and network timeout bounds.
//! - **About Tab**: Version provenance, dual license, system environment diagnostics
//!   and XDG directory layout inspection.

use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use iced::widget::{
    button, column, container, progress_bar, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Color, Element, Length, Theme};
use serde::{Deserialize, Serialize};

use crate::theme::{self, ThemeTokens};
use crate::widgets::gauge::RateLimitGaugeState;

/// Category navigation tabs for the Settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SettingsTab {
    #[default]
    GitHub,
    Appearance,
    NetworkCache,
    About,
}

impl SettingsTab {
    pub fn label(&self) -> &'static str {
        match self {
            Self::GitHub => "GitHub & Quotas",
            Self::Appearance => "Appearance",
            Self::NetworkCache => "Network & Cache",
            Self::About => "About & Diagnostics",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::GitHub => "🐙",
            Self::Appearance => "🎨",
            Self::NetworkCache => "🌐",
            Self::About => "ℹ️",
        }
    }
}

/// User theme preference options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::System => "System (Auto)",
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::System => "🖥️",
            Self::Dark => "🌙",
            Self::Light => "☀️",
        }
    }
}

/// Interface density preferences across Discover, Inspect, and History screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UiDensity {
    Compact,
    #[default]
    Default,
    Comfortable,
}

impl UiDensity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Default => "Default",
            Self::Comfortable => "Comfortable",
        }
    }
}

/// Live metrics for disk space occupied in `$XDG_CACHE_HOME/woderrise`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub file_count: usize,
    pub total_bytes: u64,
}

impl CacheStats {
    /// Formats total bytes into a human-readable string (e.g., "14.2 MB", "512 B").
    pub fn formatted_size(&self) -> String {
        format_bytes(self.total_bytes)
    }
}

/// Formats raw byte count into human-readable B, KB, MB, GB.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// System diagnostics and environment metadata for the About tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutDiagnostics {
    pub app_version: String,
    pub os_name: String,
    pub arch: String,
    pub display_server: String,
    pub config_dir: Utf8PathBuf,
    pub data_dir: Utf8PathBuf,
    pub cache_dir: Utf8PathBuf,
    pub state_dir: Utf8PathBuf,
}

impl AboutDiagnostics {
    pub fn collect(xdg: &woderrise_core::paths::XdgPaths) -> Self {
        let display_server = if std::env::var("WAYLAND_DISPLAY").is_ok() {
            format!(
                "Wayland ({})",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_default()
            )
        } else if std::env::var("DISPLAY").is_ok() {
            format!("X11 ({})", std::env::var("DISPLAY").unwrap_or_default())
        } else {
            "Headless (No Display Server)".to_string()
        };

        Self {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os_name: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            display_server,
            config_dir: xdg.woderrise_config_dir(),
            data_dir: xdg.woderrise_data_dir(),
            cache_dir: xdg.woderrise_cache_dir(),
            state_dir: xdg.woderrise_state_dir(),
        }
    }
}

/// Action events dispatched by the Settings screen.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsAction {
    TabSelected(SettingsTab),
    TokenChanged(String),
    ToggleMask,
    SaveToken,
    ClearToken,
    TestConnection,
    RefreshStatus,
    ThemeSelected(ThemeMode),
    DensitySelected(UiDensity),
    TimeoutSelected(u64),
    ClearCache,
    RefreshCatalog,
}

/// Comprehensive state for Screen 4 — Settings.
#[derive(Debug, Clone)]
pub struct SettingsState {
    // Navigation
    pub active_tab: SettingsTab,

    // GitHub & Auth Section
    pub token_input: String,
    pub is_masked: bool,
    pub is_busy: bool,
    pub status_notice: Option<(String, bool)>,
    pub saved_token: Option<String>,
    pub username: Option<String>,
    pub core_reset: Option<DateTime<Utc>>,
    pub search_reset: Option<DateTime<Utc>>,
    pub last_checked: Option<DateTime<Utc>>,

    // Appearance Section
    pub theme_mode: ThemeMode,
    pub ui_density: UiDensity,

    // Network & Cache Section
    pub cache_stats: CacheStats,
    pub request_timeout_secs: u64,
    pub is_clearing_cache: bool,

    // About Section
    pub diagnostics: Option<AboutDiagnostics>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new(None)
    }
}

impl SettingsState {
    /// Creates a new `SettingsState` initializing input from the current saved token if present.
    pub fn new(current_token: Option<String>) -> Self {
        let xdg = woderrise_core::paths::XdgPaths::from_env().ok();
        Self::with_xdg(current_token, xdg.as_ref())
    }

    /// Creates a `SettingsState` with an explicit XdgPaths instance for diagnostics.
    pub fn with_xdg(
        current_token: Option<String>,
        xdg: Option<&woderrise_core::paths::XdgPaths>,
    ) -> Self {
        let diagnostics = xdg.map(AboutDiagnostics::collect);
        Self {
            active_tab: SettingsTab::GitHub,
            token_input: current_token.clone().unwrap_or_default(),
            is_masked: true,
            is_busy: false,
            status_notice: None,
            saved_token: current_token,
            username: None,
            core_reset: None,
            search_reset: None,
            last_checked: None,
            theme_mode: ThemeMode::System,
            ui_density: UiDensity::Default,
            cache_stats: CacheStats::default(),
            request_timeout_secs: 10,
            is_clearing_cache: false,
            diagnostics,
        }
    }

    /// Update the token input field text and clear any stale status notice.
    pub fn set_token_input(&mut self, token: String) {
        self.token_input = token;
        self.status_notice = None;
    }

    /// Toggle password masking for the token input field.
    pub fn toggle_mask(&mut self) {
        self.is_masked = !self.is_masked;
    }

    /// Set a status notice banner.
    pub fn set_notice(&mut self, message: impl Into<String>, is_error: bool) {
        self.status_notice = Some((message.into(), is_error));
    }

    /// Clear the current status notice.
    pub fn clear_notice(&mut self) {
        self.status_notice = None;
    }

    /// Marks the given token as successfully saved and verified.
    pub fn set_saved(&mut self, token: Option<String>) {
        self.saved_token = token.clone();
        if let Some(ref t) = token {
            self.token_input = t.clone();
            self.set_notice(
                "✓ Personal Access Token saved and verified. Elevated rate limits active.",
                false,
            );
        } else {
            self.token_input.clear();
            self.set_notice(
                "✓ Personal Access Token cleared. Reverted to unauthenticated rate limits.",
                false,
            );
        }
        self.is_busy = false;
    }

    /// Returns a masked display string of the saved or active token.
    pub fn masked_display(&self) -> String {
        match &self.saved_token {
            Some(token) if !token.trim().is_empty() => mask_token(token),
            _ => "None (Unauthenticated)".to_string(),
        }
    }

    /// Whether a token is currently saved and authenticated.
    pub fn is_authenticated(&self) -> bool {
        self.saved_token
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false)
    }

    /// Switch active settings category tab.
    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
        self.status_notice = None;
    }

    /// Set the application theme mode preference.
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
    }

    /// Set the UI density preference.
    pub fn set_ui_density(&mut self, density: UiDensity) {
        self.ui_density = density;
    }

    /// Set the network request timeout bounded between 5s and 60s.
    pub fn set_timeout(&mut self, secs: u64) {
        self.request_timeout_secs = secs.clamp(5, 60);
    }

    /// Update cached storage metrics.
    pub fn update_cache_stats(&mut self, stats: CacheStats) {
        self.cache_stats = stats;
    }
}

/// Helper to mask sensitive token characters (e.g., `ghp_••••••••1234`).
pub fn mask_token(token: &str) -> String {
    let t = token.trim();
    if t.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= 8 {
        return "••••••••".to_string();
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{}••••••••{}", prefix, suffix)
}

/// Format raw countdown duration into compact human-readable string without prefix (e.g. "45 min", "23s", "now").
pub fn format_reset_duration(reset_time: DateTime<Utc>) -> String {
    let now = Utc::now();
    if reset_time <= now {
        "now".to_string()
    } else {
        let duration = reset_time - now;
        let total_secs = duration.num_seconds();
        let mins = duration.num_minutes();
        if mins >= 60 {
            format!("{}h {}m", mins / 60, mins % 60)
        } else if mins > 0 {
            format!("{} min", mins)
        } else {
            format!("{}s", total_secs.max(1))
        }
    }
}

/// Format an ISO reset timestamp into a live relative countdown string (e.g., "resets in 45 min", "resets in 23s", "resets now").
pub fn format_reset_countdown(reset_time: DateTime<Utc>) -> String {
    let duration = format_reset_duration(reset_time);
    if duration == "now" {
        "resets now".to_string()
    } else {
        format!("resets in {}", duration)
    }
}

/// Recursively calculates file count and total size in bytes for a directory.
pub fn calculate_directory_stats(dir: &Utf8Path) -> io::Result<CacheStats> {
    if !dir.exists() {
        return Ok(CacheStats::default());
    }

    let mut count = 0;
    let mut total_bytes = 0;
    visit_dir_stats(dir.as_std_path(), &mut count, &mut total_bytes)?;

    Ok(CacheStats {
        file_count: count,
        total_bytes,
    })
}

fn visit_dir_stats(
    dir: &std::path::Path,
    count: &mut usize,
    total_bytes: &mut u64,
) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                let _ = visit_dir_stats(&entry.path(), count, total_bytes);
            } else if file_type.is_file() {
                *count += 1;
                if let Ok(meta) = entry.metadata() {
                    *total_bytes += meta.len();
                }
            }
        }
    }
    Ok(())
}

/// Safely cleans up ephemeral cache files in `$XDG_CACHE_HOME/woderrise`.
///
/// # Invariants
/// - NEVER modifies or deletes `~/.config/woderrise/` (settings.json).
/// - NEVER modifies or deletes `~/.local/share/woderrise/` (journal.json, catalog.json).
/// - NEVER modifies or deletes `~/.local/state/woderrise/` (backup snapshot archives).
/// - Only purges `~/.cache/woderrise/staging/` subdirectories and loose cache files.
pub fn safe_cleanup_cache(cache_dir: &Utf8Path) -> io::Result<(usize, u64)> {
    if !cache_dir.exists() {
        return Ok((0, 0));
    }

    let stats_before = calculate_directory_stats(cache_dir)?;
    let mut removed_files = 0;
    let mut freed_bytes = 0;

    let staging_dir = cache_dir.join("staging");
    let staging_std = staging_dir.as_std_path();

    // 1. Inspect staging directory using symlink_metadata (DO NOT FOLLOW SYMLINKS)
    if let Ok(meta) = fs::symlink_metadata(staging_std) {
        if meta.file_type().is_symlink() {
            // Adversarial or stray symlink: unlink the symlink itself; NEVER traverse it!
            if fs::remove_file(staging_std).is_err() {
                let _ = fs::remove_dir(staging_std);
            }
        } else if meta.file_type().is_dir() {
            // Legitimate physical directory: purge child subdirectories and files
            if let Ok(entries) = fs::read_dir(staging_std) {
                for entry in entries {
                    let Ok(entry) = entry else { continue };
                    let path = entry.path();
                    let Ok(ft) = entry.file_type() else { continue };

                    if ft.is_symlink() {
                        // Unlink symlink directly without traversing
                        if fs::remove_file(&path).is_err() {
                            let _ = fs::remove_dir(&path);
                        }
                    } else if ft.is_dir() {
                        // Real subdirectory: recursive purge is safe
                        let _ = fs::remove_dir_all(&path);
                    } else if ft.is_file() {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        } else {
            // If staging is a regular file or other non-dir entity, unlink it
            let _ = fs::remove_file(staging_std);
        }
    }

    // 2. Remove loose cached blob files and rogue symlinks in cache root, preserving the directory itself
    if let Ok(entries) = fs::read_dir(cache_dir.as_std_path()) {
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };

            // Never remove the staging directory itself in the loose files pass
            if entry.file_name() == "staging" {
                continue;
            }

            if ft.is_symlink() {
                // Remove loose symlinks (file, dir, or broken) without traversing
                if fs::remove_file(&path).is_err() {
                    let _ = fs::remove_dir(&path);
                }
            } else if ft.is_file() {
                // Real loose file: accumulate stats and unlink
                if let Ok(meta) = entry.metadata() {
                    freed_bytes += meta.len();
                    removed_files += 1;
                }
                let _ = fs::remove_file(&path);
            }
        }
    }

    let _ = (removed_files, freed_bytes);
    Ok((stats_before.file_count, stats_before.total_bytes))
}

/// Renders the responsive two-column Settings screen view.
pub fn view_settings<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    rate_limits: &'a RateLimitGaugeState,
    theme: &'a Theme,
    on_action: impl Fn(SettingsAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let tokens = theme::tokens(theme);

    // Left Column: Category Navigation Sidebar (fixed 220px width)
    let sidebar = view_settings_sidebar(state, tokens, on_action);

    // Right Column: Active Tab Content (Fill width & height)
    let content = match state.active_tab {
        SettingsTab::GitHub => view_github_tab(state, rate_limits, tokens, on_action),
        SettingsTab::Appearance => view_appearance_tab(state, tokens, on_action),
        SettingsTab::NetworkCache => view_network_cache_tab(state, tokens, on_action),
        SettingsTab::About => view_about_tab(state, tokens),
    };

    let main_panel = container(scrollable(content).width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill);

    row![sidebar, main_panel]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Renders the left-hand category sidebar navigation.
fn view_settings_sidebar<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    tokens: ThemeTokens,
    on_action: impl Fn(SettingsAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let title = row![
        text("⚙").size(16),
        text("Settings").size(15).color(tokens.text_primary),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let tabs = [
        SettingsTab::GitHub,
        SettingsTab::Appearance,
        SettingsTab::NetworkCache,
        SettingsTab::About,
    ];

    let mut tab_buttons = column![title].spacing(10);

    for tab in tabs {
        let is_active = state.active_tab == tab;
        let btn = button(
            row![text(tab.icon()).size(14), text(tab.label()).size(13),]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .on_press(on_action(SettingsAction::TabSelected(tab)))
        .style(move |_theme, status| theme::button_tab(tokens, is_active, status));

        tab_buttons = tab_buttons.push(btn);
    }

    container(tab_buttons)
        .width(Length::Fixed(220.0))
        .height(Length::Fill)
        .padding([20, 16])
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_sidebar.into()),
            border: iced::Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: iced::border::Radius::default(),
            },
            ..Default::default()
        })
        .into()
}

/// Renders the GitHub Authentication and API Quotas tab.
fn view_github_tab<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    rate_limits: &'a RateLimitGaugeState,
    tokens: ThemeTokens,
    on_action: impl Fn(SettingsAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    // 1. Header Section
    let header = column![
        text("GitHub Authentication & API Quotas")
            .size(20)
            .color(tokens.text_primary),
        text("Configure an optional GitHub Personal Access Token (PAT) for elevated rate limits and reliable dotfile discovery.")
            .size(13)
            .color(tokens.text_secondary),
    ]
    .spacing(4);

    // 2. Status Notice Banner
    let mut notice_element: Option<Element<'a, Message>> = None;
    if let Some((msg, is_error)) = &state.status_notice {
        let (border_col, bg_col, text_col) = if *is_error {
            (
                tokens.danger,
                Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.15),
                tokens.danger,
            )
        } else {
            (
                tokens.success,
                Color::from_rgba(tokens.success.r, tokens.success.g, tokens.success.b, 0.15),
                tokens.success,
            )
        };

        let notice_box = container(text(msg).size(13).color(text_col))
            .padding([10, 14])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg_col.into()),
                border: iced::Border {
                    color: border_col,
                    width: 1.0,
                    radius: theme::radius::MD,
                },
                ..Default::default()
            });

        notice_element = Some(notice_box.into());
    }

    // 3. Elevated Rate Limits Explanation Card
    let info_card = container(
        column![
            text("Elevated API Rate Limits")
                .size(15)
                .color(tokens.accent_primary),
            Space::new().height(Length::Fixed(4.0)),
            text("By default, GitHub restricts anonymous requests to prevent API abuse. Supplying a Personal Access Token unlocks higher tier quotas:")
                .size(13)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(6.0)),
            column![
                row![
                    text("• Search API:").size(13).color(tokens.text_primary),
                    text(" 10 requests / minute (unauthenticated) ➔ ").size(13).color(tokens.text_muted),
                    text("30 requests / minute (authenticated)").size(13).color(tokens.success),
                ].align_y(Alignment::Center),
                row![
                    text("• Core REST API:").size(13).color(tokens.text_primary),
                    text(" 60 requests / hour (unauthenticated) ➔ ").size(13).color(tokens.text_muted),
                    text("5,000 requests / hour (authenticated)").size(13).color(tokens.success),
                ].align_y(Alignment::Center),
                row![
                    text("• POSIX Security:").size(13).color(tokens.text_primary),
                    text(" Token is saved durably in ~/.config/woderrise/settings.json with strict ").size(13).color(tokens.text_muted),
                    text("0600 permissions").size(13).color(tokens.warning),
                    text(" (read/write by owner only).").size(13).color(tokens.text_muted),
                ].align_y(Alignment::Center),
                row![
                    text("• Required Scopes:").size(13).color(tokens.text_primary),
                    text(" No private repository permissions needed. Fine-grained tokens only require read-only access to public repositories.").size(13).color(tokens.text_muted),
                ].align_y(Alignment::Center),
            ].spacing(5),
        ]
        .spacing(6),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    // 4. Current Connection Status & Live Quotas Card
    let auth_badge = if state.is_authenticated() {
        let mut b_row = row![
            text("●").size(14).color(tokens.success),
            text(" Authenticated").size(13).color(tokens.success),
            Space::new().width(Length::Fixed(8.0)),
            text(format!("(Active Token: {})", state.masked_display()))
                .size(12)
                .color(tokens.text_secondary),
        ]
        .align_y(Alignment::Center);

        if let Some(ref user) = state.username {
            b_row = b_row.push(Space::new().width(Length::Fixed(8.0)));
            b_row = b_row.push(
                text(format!("@{}", user))
                    .size(12)
                    .color(tokens.accent_primary),
            );
        }
        b_row
    } else {
        row![
            text("○").size(14).color(tokens.warning),
            text(" Unauthenticated (Anonymous public access)")
                .size(13)
                .color(tokens.warning),
        ]
        .align_y(Alignment::Center)
    };

    // Live countdown strings
    let search_countdown = if let Some(reset) = state.search_reset {
        format_reset_countdown(reset)
    } else {
        "resets in 1 min".to_string()
    };

    let core_countdown = if let Some(reset) = state.core_reset {
        format_reset_countdown(reset)
    } else {
        "resets in 60 min".to_string()
    };

    let search_tier_color = rate_limits.search_tier().color();
    let core_tier_color = rate_limits.core_tier().color();

    let search_quota_box = column![
        row![
            text("Search API Quota").size(13).color(tokens.text_primary),
            Space::new().width(Length::Fill),
            text(format!(
                "{}/{} remaining",
                rate_limits.search_remaining, rate_limits.search_limit
            ))
            .size(13)
            .color(tokens.text_secondary),
        ]
        .align_y(Alignment::Center),
        progress_bar(0.0..=1.0, rate_limits.search_percentage())
            .girth(6)
            .style(move |_theme| progress_bar::Style {
                background: Color::from_rgba(
                    tokens.border_card.r,
                    tokens.border_card.g,
                    tokens.border_card.b,
                    0.4
                )
                .into(),
                bar: search_tier_color.into(),
                border: iced::border::rounded(3),
            }),
        row![
            text(format!("Status: {}", rate_limits.search_tier().label()))
                .size(11)
                .color(search_tier_color),
            Space::new().width(Length::Fill),
            text(search_countdown).size(11).color(tokens.text_muted),
        ],
    ]
    .spacing(6);

    let core_quota_box = column![
        row![
            text("Core REST API Quota")
                .size(13)
                .color(tokens.text_primary),
            Space::new().width(Length::Fill),
            text(format!(
                "{}/{} remaining",
                rate_limits.core_remaining, rate_limits.core_limit
            ))
            .size(13)
            .color(tokens.text_secondary),
        ]
        .align_y(Alignment::Center),
        progress_bar(0.0..=1.0, rate_limits.core_percentage())
            .girth(6)
            .style(move |_theme| progress_bar::Style {
                background: Color::from_rgba(
                    tokens.border_card.r,
                    tokens.border_card.g,
                    tokens.border_card.b,
                    0.4
                )
                .into(),
                bar: core_tier_color.into(),
                border: iced::border::rounded(3),
            }),
        row![
            text(format!("Status: {}", rate_limits.core_tier().label()))
                .size(11)
                .color(core_tier_color),
            Space::new().width(Length::Fill),
            text(core_countdown).size(11).color(tokens.text_muted),
        ],
    ]
    .spacing(6);

    let quotas_col = column![
        row![
            text("Current Connection Status")
                .size(15)
                .color(tokens.text_primary),
            Space::new().width(Length::Fill),
            auth_badge,
        ]
        .align_y(Alignment::Center),
        Space::new().height(Length::Fixed(8.0)),
        row![
            container(search_quota_box).width(Length::FillPortion(1)),
            Space::new().width(Length::Fixed(16.0)),
            container(core_quota_box).width(Length::FillPortion(1)),
        ],
    ]
    .spacing(6);

    let status_card = container(quotas_col)
        .padding([14, 18])
        .width(Length::Fill)
        .style(move |_theme| theme::container_card(tokens));

    // 5. Token Management Card
    let input = text_input("Enter Personal Access Token (ghp_...)", &state.token_input)
        .secure(state.is_masked)
        .on_input(move |s| on_action(SettingsAction::TokenChanged(s)))
        .padding(10)
        .size(13)
        .width(Length::Fill)
        .style(move |_theme, status| theme::text_input(tokens, status));

    let toggle_mask_btn = button(
        text(if state.is_masked {
            "👁️ Show"
        } else {
            "🔒 Hide"
        })
        .size(12),
    )
    .padding([8, 12])
    .on_press(on_action(SettingsAction::ToggleMask))
    .style(move |_theme, status| theme::button_secondary(tokens, status));

    let input_row = row![input, toggle_mask_btn]
        .spacing(8)
        .align_y(Alignment::Center);

    let save_btn = button(
        text(if state.is_busy {
            "Saving..."
        } else {
            "Save Token"
        })
        .size(13),
    )
    .padding([8, 16])
    .on_press_maybe(if state.is_busy {
        None
    } else {
        Some(on_action(SettingsAction::SaveToken))
    })
    .style(move |_theme, status| theme::button_primary(tokens, status));

    let clear_btn = button(text("Clear Token").size(13))
        .padding([8, 16])
        .on_press_maybe(if state.is_busy {
            None
        } else {
            Some(on_action(SettingsAction::ClearToken))
        })
        .style(move |_theme, status| theme::button_danger(tokens, status));

    let test_btn = button(text("Test Connection").size(13))
        .padding([8, 16])
        .on_press_maybe(if state.is_busy {
            None
        } else {
            Some(on_action(SettingsAction::TestConnection))
        })
        .style(move |_theme, status| theme::button_secondary(tokens, status));

    let refresh_btn = button(text("Refresh Status").size(13))
        .padding([8, 16])
        .on_press_maybe(if state.is_busy {
            None
        } else {
            Some(on_action(SettingsAction::RefreshStatus))
        })
        .style(move |_theme, status| theme::button_secondary(tokens, status));

    let actions_row = row![save_btn, clear_btn, test_btn, refresh_btn]
        .spacing(10)
        .align_y(Alignment::Center);

    let token_card = container(
        column![
            text("Manage GitHub Token")
                .size(15)
                .color(tokens.text_primary),
            text("Enter a token generated from GitHub: Settings ➔ Developer settings ➔ Personal access tokens.")
                .size(12)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(4.0)),
            input_row,
            Space::new().height(Length::Fixed(4.0)),
            actions_row,
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    let mut content = column![header].spacing(16).padding([20, 24]);
    if let Some(notice) = notice_element {
        content = content.push(notice);
    }
    content = content.push(info_card).push(status_card).push(token_card);

    content.into()
}

/// Renders the Appearance Tab: theme modes, UI density, and WCAG contrast badge.
fn view_appearance_tab<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    tokens: ThemeTokens,
    on_action: impl Fn(SettingsAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let header = column![
        text("Appearance & Interface Preferences")
            .size(20)
            .color(tokens.text_primary),
        text("Configure theme palette, interface density, and verify contrast accessibility.")
            .size(13)
            .color(tokens.text_secondary),
    ]
    .spacing(4);

    // 1. Theme Mode Card
    let theme_modes = [ThemeMode::System, ThemeMode::Dark, ThemeMode::Light];
    let mut theme_buttons = row![].spacing(10);

    for mode in theme_modes {
        let is_selected = state.theme_mode == mode;
        let btn = button(
            row![text(mode.icon()).size(14), text(mode.label()).size(13),]
                .spacing(6)
                .align_y(Alignment::Center),
        )
        .padding([8, 16])
        .on_press(on_action(SettingsAction::ThemeSelected(mode)))
        .style(move |_theme, status| theme::button_tab(tokens, is_selected, status));

        theme_buttons = theme_buttons.push(btn);
    }

    // Live WCAG contrast ratio calculation
    let contrast = theme::contrast_ratio(tokens.text_primary, tokens.bg_base);
    let contrast_badge = container(
        row![
            text("✓").size(12).color(tokens.success),
            text(format!("Contrast: {:.1}:1 (WCAG AAA Compliant)", contrast))
                .size(12)
                .color(tokens.success),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(move |_theme| container::Style {
        background: Some(
            Color::from_rgba(tokens.success.r, tokens.success.g, tokens.success.b, 0.12).into(),
        ),
        border: iced::Border {
            color: tokens.success,
            width: 1.0,
            radius: theme::radius::PILL,
        },
        ..Default::default()
    });

    let theme_card = container(
        column![
            row![
                text("Theme Mode")
                    .size(15)
                    .color(tokens.text_primary),
                Space::new().width(Length::Fill),
                contrast_badge,
            ]
            .align_y(Alignment::Center),
            text("System mode automatically synchronizes with your Wayland desktop color scheme via XDG Desktop Portal.")
                .size(12)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(6.0)),
            theme_buttons,
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    // 2. UI Density Card
    let densities = [
        UiDensity::Compact,
        UiDensity::Default,
        UiDensity::Comfortable,
    ];
    let mut density_buttons = row![].spacing(10);

    for density in densities {
        let is_selected = state.ui_density == density;
        let btn = button(text(density.label()).size(13))
            .padding([8, 16])
            .on_press(on_action(SettingsAction::DensitySelected(density)))
            .style(move |_theme, status| theme::button_tab(tokens, is_selected, status));

        density_buttons = density_buttons.push(btn);
    }

    let density_card = container(
        column![
            text("Interface Density")
                .size(15)
                .color(tokens.text_primary),
            text("Adjust card spacing, table row heights, and layout padding across Discover, Inspect, and History views.")
                .size(12)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(6.0)),
            density_buttons,
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    // 3. Typography & Contrast Sample Card
    let preview_card = container(
        column![
            text("Typography & Color Tokens Preview")
                .size(15)
                .color(tokens.text_primary),
            text("Semantic palette verified against WCAG 2.1 AAA accessibility thresholds.")
                .size(12)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(6.0)),
            row![
                container(
                    column![
                        text("Monospace Configuration Path")
                            .size(11)
                            .color(tokens.text_muted),
                        text("~/.config/hypr/hyprland.conf")
                            .size(13)
                            .color(tokens.accent_primary),
                    ]
                    .spacing(4)
                )
                .padding([8, 12])
                .style(move |_theme| container::Style {
                    background: Some(tokens.bg_surface.into()),
                    border: iced::Border {
                        color: tokens.border_subtle,
                        width: 1.0,
                        radius: theme::radius::SM,
                    },
                    ..Default::default()
                }),
                Space::new().width(Length::Fixed(12.0)),
                container(
                    column![
                        text("Primary Text").size(13).color(tokens.text_primary),
                        text("Secondary Text").size(12).color(tokens.text_secondary),
                        text("Muted Text").size(11).color(tokens.text_muted),
                    ]
                    .spacing(2)
                )
                .padding([8, 12])
                .style(move |_theme| container::Style {
                    background: Some(tokens.bg_surface.into()),
                    border: iced::Border {
                        color: tokens.border_subtle,
                        width: 1.0,
                        radius: theme::radius::SM,
                    },
                    ..Default::default()
                }),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    column![header, theme_card, density_card, preview_card]
        .spacing(16)
        .padding([20, 24])
        .into()
}

/// Renders the Network & Cache Tab: disk stats, safe cleanup, and timeouts.
fn view_network_cache_tab<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    tokens: ThemeTokens,
    on_action: impl Fn(SettingsAction) -> Message + Copy + 'a,
) -> Element<'a, Message> {
    let header = column![
        text("Network & Cache Storage")
            .size(20)
            .color(tokens.text_primary),
        text("Manage local disk storage, clean temporary staging sandboxes safely, and configure timeouts.")
            .size(13)
            .color(tokens.text_secondary),
    ]
    .spacing(4);

    // 1. Cache Storage Card
    let cache_dir_display = state
        .diagnostics
        .as_ref()
        .map(|d| d.cache_dir.as_str())
        .unwrap_or("~/.cache/woderrise");

    let clear_cache_btn = button(
        text(if state.is_clearing_cache {
            "Cleaning Cache..."
        } else {
            "Clear Cache"
        })
        .size(13),
    )
    .padding([8, 16])
    .on_press_maybe(if state.is_clearing_cache {
        None
    } else {
        Some(on_action(SettingsAction::ClearCache))
    })
    .style(move |_theme, status| theme::button_secondary(tokens, status));

    let cache_card = container(
        column![
            row![
                text("Cache Storage & Disk Usage")
                    .size(15)
                    .color(tokens.text_primary),
                Space::new().width(Length::Fill),
                clear_cache_btn,
            ]
            .align_y(Alignment::Center),
            text(format!(
                "Directory: {} ({} across {} cached items)",
                cache_dir_display,
                state.cache_stats.formatted_size(),
                state.cache_stats.file_count
            ))
            .size(13)
            .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(4.0)),
            text("Safely cleans ephemeral transaction sandboxes (`staging/`) and temporary downloaded assets without modifying configuration (`settings.json`), pre-install backups, or transaction journal records.")
                .size(12)
                .color(tokens.text_muted),
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    // 2. Network Request Timeout Card
    let timeouts = [5u64, 10, 15, 30];
    let mut timeout_buttons = row![].spacing(10);

    for t in timeouts {
        let is_selected = state.request_timeout_secs == t;
        let label = if t == 10 {
            "10s (Default)".to_string()
        } else {
            format!("{}s", t)
        };
        let btn = button(text(label).size(13))
            .padding([8, 16])
            .on_press(on_action(SettingsAction::TimeoutSelected(t)))
            .style(move |_theme, status| theme::button_tab(tokens, is_selected, status));

        timeout_buttons = timeout_buttons.push(btn);
    }

    let refresh_catalog_btn = button(text("Refresh Dotfile Catalog").size(13))
        .padding([8, 16])
        .on_press(on_action(SettingsAction::RefreshCatalog))
        .style(move |_theme, status| theme::button_secondary(tokens, status));

    let network_card = container(
        column![
            text("Request Timeout & Remote Operations")
                .size(15)
                .color(tokens.text_primary),
            text("Maximum timeout duration for GitHub API and remote blob fetching requests.")
                .size(12)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(6.0)),
            timeout_buttons,
            Space::new().height(Length::Fixed(12.0)),
            row![
                refresh_catalog_btn,
                Space::new().width(Length::Fixed(12.0)),
                text("Re-indexes static catalog registry and cached dotfile feeds.")
                    .size(12)
                    .color(tokens.text_muted),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    column![header, cache_card, network_card]
        .spacing(16)
        .padding([20, 24])
        .into()
}

/// Renders the About Tab: version branding, licenses, and system diagnostics.
fn view_about_tab<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    tokens: ThemeTokens,
) -> Element<'a, Message> {
    let header = column![
        text("About Woderrise").size(20).color(tokens.text_primary),
        text("System environment diagnostics, application provenance, and licensing.")
            .size(13)
            .color(tokens.text_secondary),
    ]
    .spacing(4);

    // 1. Branding Card
    let brand_card = container(
        column![
            row![
                text("🌊").size(22),
                text("WODERRISE")
                    .size(18)
                    .color(tokens.accent_primary),
                Space::new().width(Length::Fixed(8.0)),
                container(
                    text(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .size(12)
                        .color(tokens.text_primary),
                )
                .padding([2, 8])
                .style(move |_theme| container::Style {
                    background: Some(tokens.bg_surface.into()),
                    border: iced::Border {
                        color: tokens.border_subtle,
                        width: 1.0,
                        radius: theme::radius::PILL,
                    },
                    ..Default::default()
                }),
            ]
            .align_y(Alignment::Center),
            Space::new().height(Length::Fixed(4.0)),
            text("A polished, crash-resilient Linux desktop dotfile & configuration manager for Wayland (Hyprland, Waybar, Hyprlock, Hypridle, Wallpapers, GTK).")
                .size(13)
                .color(tokens.text_secondary),
            Space::new().height(Length::Fixed(4.0)),
            row![
                text("License:").size(12).color(tokens.text_muted),
                Space::new().width(Length::Fixed(6.0)),
                text("MIT or Apache-2.0 Dual License").size(12).color(tokens.text_primary),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    // 2. System Diagnostics Card
    let diag_content = if let Some(ref d) = state.diagnostics {
        let rows = [
            ("Application Version", d.app_version.as_str()),
            ("Operating System", d.os_name.as_str()),
            ("Architecture", d.arch.as_str()),
            ("Display Server", d.display_server.as_str()),
            ("Config Directory", d.config_dir.as_str()),
            ("Data Directory", d.data_dir.as_str()),
            ("Cache Directory", d.cache_dir.as_str()),
            ("State Directory", d.state_dir.as_str()),
        ];

        let mut diag_col = column![].spacing(6);
        for (label, value) in rows {
            let row_item = row![
                container(text(label).size(12).color(tokens.text_secondary))
                    .width(Length::Fixed(180.0)),
                text(value).size(12).color(tokens.text_primary),
            ]
            .align_y(Alignment::Center);
            diag_col = diag_col.push(row_item);
        }
        diag_col
    } else {
        column![text("XDG diagnostics unavailable.")
            .size(12)
            .color(tokens.text_muted),]
    };

    let diagnostics_card = container(
        column![
            text("System Diagnostics & Environment Paths")
                .size(15)
                .color(tokens.text_primary),
            Space::new().height(Length::Fixed(6.0)),
            diag_content,
        ]
        .spacing(8),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(move |_theme| theme::container_card(tokens));

    column![header, brand_card, diagnostics_card]
        .spacing(16)
        .padding([20, 24])
        .into()
}

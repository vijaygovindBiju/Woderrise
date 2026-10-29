//! Linear LIFO transaction history and rollback management screen.
//!
//! Enforces linear LIFO rollback: only the latest committed transaction
//! can be rolled back. Provides dynamic metrics, search and filtering,
//! a structured audit table, full operation inspection, and guarded rollback confirmation.

use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, stack, text, text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Length, Theme};
use woderrise_core::models::{
    ConfigComponent, FileOperationRecord, JournalEntry, OperationType, TransactionStatus,
};

use crate::theme::{self, ThemeTokens};
use crate::widgets::card::format_relative_time;

/// A single entry in the transaction history view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryItem {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub source_repo: String,
    pub source_commit: String,
    pub component: ConfigComponent,
    pub status: TransactionStatus,
    pub file_count: usize,
    pub backup_dir: Utf8PathBuf,
    pub operations: Vec<FileOperationRecord>,
    pub is_latest_committed: bool,
}

/// State for dirty file modifications modal dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirtyStateModal {
    pub transaction_id: String,
    pub modified_files: Vec<Utf8PathBuf>,
}

/// Calculated summary metrics derived from persistent journal entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistoryMetrics {
    pub total_transactions: usize,
    pub successful_installations: usize,
    pub failed_operations: usize,
    pub rollback_eligible_count: usize,
}

impl HistoryMetrics {
    pub fn calculate(items: &[HistoryItem]) -> Self {
        let total = items.len();
        let successful = items
            .iter()
            .filter(|t| {
                t.status == TransactionStatus::Committed
                    || t.status == TransactionStatus::RolledBack
                    || t.status == TransactionStatus::RecoveredRollback
            })
            .count();
        let failed = items
            .iter()
            .filter(|t| t.status == TransactionStatus::Failed)
            .count();
        let rollback_eligible = items
            .iter()
            .filter(|t| t.status == TransactionStatus::Committed)
            .count();

        Self {
            total_transactions: total,
            successful_installations: successful,
            failed_operations: failed,
            rollback_eligible_count: rollback_eligible,
        }
    }
}

/// Filter criteria for transaction status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryStatusFilter {
    #[default]
    All,
    Committed,
    RolledBack,
    Failed,
}

impl HistoryStatusFilter {
    pub const ALL: [HistoryStatusFilter; 4] = [
        HistoryStatusFilter::All,
        HistoryStatusFilter::Committed,
        HistoryStatusFilter::RolledBack,
        HistoryStatusFilter::Failed,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            HistoryStatusFilter::All => "All Status",
            HistoryStatusFilter::Committed => "Committed",
            HistoryStatusFilter::RolledBack => "Rolled Back",
            HistoryStatusFilter::Failed => "Failed",
        }
    }

    pub fn matches(&self, status: TransactionStatus) -> bool {
        match self {
            HistoryStatusFilter::All => true,
            HistoryStatusFilter::Committed => status == TransactionStatus::Committed,
            HistoryStatusFilter::RolledBack => {
                status == TransactionStatus::RolledBack
                    || status == TransactionStatus::RecoveredRollback
            }
            HistoryStatusFilter::Failed => status == TransactionStatus::Failed,
        }
    }
}

impl std::fmt::Display for HistoryStatusFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Filter criteria for configuration component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryComponentFilter {
    #[default]
    All,
    Hyprland,
    Waybar,
    Hyprlock,
    Hypridle,
    Wallpapers,
    GtkTheme,
}

impl HistoryComponentFilter {
    pub const ALL: [HistoryComponentFilter; 7] = [
        HistoryComponentFilter::All,
        HistoryComponentFilter::Hyprland,
        HistoryComponentFilter::Waybar,
        HistoryComponentFilter::Hyprlock,
        HistoryComponentFilter::Hypridle,
        HistoryComponentFilter::Wallpapers,
        HistoryComponentFilter::GtkTheme,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            HistoryComponentFilter::All => "All Components",
            HistoryComponentFilter::Hyprland => "Hyprland",
            HistoryComponentFilter::Waybar => "Waybar",
            HistoryComponentFilter::Hyprlock => "Hyprlock",
            HistoryComponentFilter::Hypridle => "Hypridle",
            HistoryComponentFilter::Wallpapers => "Wallpapers",
            HistoryComponentFilter::GtkTheme => "GTK Theme",
        }
    }

    pub fn matches(&self, component: ConfigComponent) -> bool {
        match self {
            HistoryComponentFilter::All => true,
            HistoryComponentFilter::Hyprland => component == ConfigComponent::Hyprland,
            HistoryComponentFilter::Waybar => component == ConfigComponent::Waybar,
            HistoryComponentFilter::Hyprlock => component == ConfigComponent::Hyprlock,
            HistoryComponentFilter::Hypridle => component == ConfigComponent::Hypridle,
            HistoryComponentFilter::Wallpapers => component == ConfigComponent::Wallpapers,
            HistoryComponentFilter::GtkTheme => component == ConfigComponent::GtkTheme,
        }
    }
}

impl std::fmt::Display for HistoryComponentFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// State for the guarded rollback confirmation modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackConfirmState {
    pub tx_id: String,
    pub repo: String,
    pub revision: String,
    pub component: ConfigComponent,
    pub files_to_restore: Vec<Utf8PathBuf>,
    pub files_to_remove: Vec<Utf8PathBuf>,
}

impl RollbackConfirmState {
    pub fn from_item(item: &HistoryItem) -> Self {
        let mut files_to_restore = Vec::new();
        let mut files_to_remove = Vec::new();

        for op in &item.operations {
            match op.op_type {
                OperationType::Overwrite
                | OperationType::Delete
                | OperationType::DeactivateLegacy => {
                    files_to_restore.push(op.target_file.clone());
                }
                OperationType::Create => {
                    files_to_remove.push(op.target_file.clone());
                }
            }
        }

        Self {
            tx_id: item.id.clone(),
            repo: item.source_repo.clone(),
            revision: item.source_commit.clone(),
            component: item.component,
            files_to_restore,
            files_to_remove,
        }
    }
}

/// User interactions originating from the history screen.
#[derive(Debug, Clone)]
pub enum HistoryAction {
    SearchChanged(String),
    StatusFilterSelected(HistoryStatusFilter),
    ComponentFilterSelected(HistoryComponentFilter),
    ClearFilters,
    Inspect(HistoryItem),
    CloseInspect,
    OpenRollbackConfirm(HistoryItem),
    CloseRollbackConfirm,
    ExecuteRollback { tx_id: String, force_dirty: bool },
    CloseDirtyModal,
    NavigateDiscover,
}

/// State for the History screen.
#[derive(Debug, Clone)]
pub struct HistoryState {
    pub transactions: Vec<HistoryItem>,
    pub metrics: HistoryMetrics,
    pub search_query: String,
    pub status_filter: HistoryStatusFilter,
    pub component_filter: HistoryComponentFilter,
    pub inspect_modal: Option<HistoryItem>,
    pub confirm_rollback_modal: Option<RollbackConfirmState>,
    pub dirty_modal: Option<DirtyStateModal>,
    pub is_rolling_back: bool,
}

impl HistoryState {
    pub fn from_journal_entries(entries: &[JournalEntry]) -> Self {
        let mut items = Vec::with_capacity(entries.len());
        for entry in entries.iter().rev() {
            items.push(HistoryItem {
                id: entry.id.clone(),
                timestamp: entry.timestamp,
                source_repo: entry.source_repo.clone(),
                source_commit: entry.source_commit.clone(),
                component: entry.component,
                status: entry.status,
                file_count: entry.operations.len(),
                backup_dir: entry.backup_directory.clone(),
                operations: entry.operations.clone(),
                is_latest_committed: false,
            });
        }

        // LIFO order: first committed transaction at top index gets rollback privilege
        let first_committed_idx = items
            .iter()
            .position(|t| t.status == TransactionStatus::Committed);
        for (i, tx) in items.iter_mut().enumerate() {
            tx.is_latest_committed = Some(i) == first_committed_idx;
        }

        let metrics = HistoryMetrics::calculate(&items);

        Self {
            transactions: items,
            metrics,
            search_query: String::new(),
            status_filter: HistoryStatusFilter::All,
            component_filter: HistoryComponentFilter::All,
            inspect_modal: None,
            confirm_rollback_modal: None,
            dirty_modal: None,
            is_rolling_back: false,
        }
    }

    pub fn has_active_filters(&self) -> bool {
        !self.search_query.trim().is_empty()
            || self.status_filter != HistoryStatusFilter::All
            || self.component_filter != HistoryComponentFilter::All
    }

    pub fn clear_filters(&mut self) {
        self.search_query.clear();
        self.status_filter = HistoryStatusFilter::All;
        self.component_filter = HistoryComponentFilter::All;
    }

    pub fn filter_matches(&self, item: &HistoryItem) -> bool {
        if !self.status_filter.matches(item.status) {
            return false;
        }
        if !self.component_filter.matches(item.component) {
            return false;
        }

        let q = self.search_query.trim().to_lowercase();
        if !q.is_empty() {
            let status_str = match item.status {
                TransactionStatus::Committed => "committed",
                TransactionStatus::RolledBack => "rolled back rollback",
                TransactionStatus::RecoveredRollback => "recovered rollback",
                TransactionStatus::Failed => "failed",
                TransactionStatus::Staged => "staged",
                TransactionStatus::Applying => "applying",
                TransactionStatus::RollingBack => "rolling back",
            };
            let comp_str = format!("{:?}", item.component).to_lowercase();
            let repo_match = item.source_repo.to_lowercase().contains(&q);
            let commit_match = item.source_commit.to_lowercase().contains(&q);
            let id_match = item.id.to_lowercase().contains(&q);
            let comp_match = comp_str.contains(&q);
            let status_match = status_str.contains(&q);

            if !repo_match && !commit_match && !id_match && !comp_match && !status_match {
                return false;
            }
        }
        true
    }

    pub fn filtered_transactions(&self) -> Vec<&HistoryItem> {
        self.transactions
            .iter()
            .filter(|item| self.filter_matches(item))
            .collect()
    }

    pub fn open_inspect_modal(&mut self, item: HistoryItem) {
        self.inspect_modal = Some(item);
    }

    pub fn close_inspect_modal(&mut self) {
        self.inspect_modal = None;
    }

    pub fn open_rollback_confirm(&mut self, item: &HistoryItem) {
        self.confirm_rollback_modal = Some(RollbackConfirmState::from_item(item));
    }

    pub fn close_rollback_confirm(&mut self) {
        self.confirm_rollback_modal = None;
    }

    pub fn open_dirty_modal(&mut self, tx_id: impl Into<String>, modified: Vec<Utf8PathBuf>) {
        self.dirty_modal = Some(DirtyStateModal {
            transaction_id: tx_id.into(),
            modified_files: modified,
        });
    }

    pub fn close_dirty_modal(&mut self) {
        self.dirty_modal = None;
    }
}

/// Safely truncates a string slice to at most `max_chars` Unicode code points
/// without heap allocation and without slicing across multi-byte UTF-8 boundaries.
#[inline]
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    s.char_indices()
        .nth(max_chars)
        .map(|(idx, _)| &s[..idx])
        .unwrap_or(s)
}

/// Alias for [`truncate_chars`].
#[inline]
pub fn safe_truncate(s: &str, max_chars: usize) -> &str {
    truncate_chars(s, max_chars)
}

/// Renders the upgraded history screen view.
pub fn view_history<'a, Message: Clone + 'a>(
    state: &'a HistoryState,
    theme: &'a Theme,
    on_action: impl Fn(HistoryAction) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let tokens = theme::tokens(theme);
    let on_search = on_action.clone();
    let on_status = on_action.clone();
    let on_comp = on_action.clone();

    // 1. Header Column
    let header = column![
        text("Transaction History & Rollback")
            .size(22)
            .color(tokens.text_primary),
        text("Audit trail of dotfile configurations with linear LIFO rollback safety.")
            .size(13)
            .color(tokens.text_secondary),
    ]
    .spacing(4);

    // 2. Overview Metrics Bar (4 Cards)
    let total_card = render_metric_card(
        tokens,
        "Total Operations",
        &state.metrics.total_transactions.to_string(),
        "🔄",
        tokens.border_card,
    );
    let success_card = render_metric_card(
        tokens,
        "Successful",
        &state.metrics.successful_installations.to_string(),
        "✅",
        tokens.success,
    );
    let failed_card = render_metric_card(
        tokens,
        "Failed",
        &state.metrics.failed_operations.to_string(),
        "⚠️",
        tokens.danger,
    );
    let rollback_card = render_metric_card(
        tokens,
        "Can Rollback",
        &state.metrics.rollback_eligible_count.to_string(),
        "⏪",
        tokens.warning,
    );

    let metrics_row = row![total_card, success_card, failed_card, rollback_card]
        .spacing(12)
        .width(Length::Fill);

    // 3. Search & Filter Bar
    let search_input = text_input(
        "Search repo, commit, component, status...",
        &state.search_query,
    )
    .on_input(move |q| on_search(HistoryAction::SearchChanged(q)))
    .padding([8, 12])
    .size(13)
    .width(Length::FillPortion(3))
    .style(move |_theme, status| theme::text_input(tokens, status));

    let status_picker = pick_list(
        &HistoryStatusFilter::ALL[..],
        Some(state.status_filter),
        move |sf| on_status(HistoryAction::StatusFilterSelected(sf)),
    )
    .padding([8, 10])
    .text_size(12);

    let component_picker = pick_list(
        &HistoryComponentFilter::ALL[..],
        Some(state.component_filter),
        move |cf| on_comp(HistoryAction::ComponentFilterSelected(cf)),
    )
    .padding([8, 10])
    .text_size(12);

    let mut filter_row = row![search_input, status_picker, component_picker]
        .spacing(10)
        .align_y(Alignment::Center);

    if state.has_active_filters() {
        let clear_btn = button(text("✕ Clear Filters").size(12))
            .padding([7, 12])
            .on_press(on_action(HistoryAction::ClearFilters))
            .style(move |_theme, status| theme::button_secondary(tokens, status));
        filter_row = filter_row.push(clear_btn);
    }

    let filtered_items = state.filtered_transactions();

    // 4. Content Area
    let content: Element<'a, Message> = if state.transactions.is_empty() {
        // Genuine Empty State
        container(
            column![
                text("📦").size(40),
                text("No Transaction History")
                    .size(18)
                    .color(tokens.text_primary),
                text("Dotfile configurations installed from Discover will appear here with instant, safe LIFO rollback support.")
                    .size(13)
                    .color(tokens.text_secondary),
                Space::new().height(8),
                button(text("Explore Dotfiles in Discover").size(13))
                    .padding([8, 16])
                    .on_press(on_action(HistoryAction::NavigateDiscover))
                    .style(move |_theme, status| theme::button_primary(tokens, status)),
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .padding(48)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .style(move |_theme| theme::container_card(tokens))
        .into()
    } else if filtered_items.is_empty() {
        // No match for active filters
        container(
            column![
                text("🔍").size(36),
                text("No Matching Transactions")
                    .size(16)
                    .color(tokens.text_primary),
                text("No recorded transactions match your current search and filter criteria.")
                    .size(13)
                    .color(tokens.text_secondary),
                Space::new().height(6),
                button(text("✕ Clear Filters").size(12))
                    .padding([6, 14])
                    .on_press(on_action(HistoryAction::ClearFilters))
                    .style(move |_theme, status| theme::button_secondary(tokens, status)),
            ]
            .spacing(10)
            .align_x(Alignment::Center),
        )
        .padding(36)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .style(move |_theme| theme::container_card(tokens))
        .into()
    } else {
        // Formatted Tabular View
        let table_header = container(
            row![
                text("DATE & TIME")
                    .size(11)
                    .color(tokens.text_muted)
                    .width(Length::FillPortion(2)),
                text("REPOSITORY")
                    .size(11)
                    .color(tokens.text_muted)
                    .width(Length::FillPortion(3)),
                text("COMPONENT")
                    .size(11)
                    .color(tokens.text_muted)
                    .width(Length::FillPortion(2)),
                text("FILES")
                    .size(11)
                    .color(tokens.text_muted)
                    .width(Length::FillPortion(1)),
                text("STATUS")
                    .size(11)
                    .color(tokens.text_muted)
                    .width(Length::FillPortion(2)),
                text("ACTIONS")
                    .size(11)
                    .color(tokens.text_muted)
                    .width(Length::FillPortion(2)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([8, 14])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_surface.into()),
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: theme::radius::SM,
            },
            ..Default::default()
        });

        let mut rows_col = column![].spacing(6);

        for item in &filtered_items {
            let (status_color, status_text) = match item.status {
                TransactionStatus::Committed => (tokens.success, "● Committed"),
                TransactionStatus::RolledBack => (tokens.accent_primary, "● Rolled Back"),
                TransactionStatus::RecoveredRollback => (tokens.warning, "● Recovered"),
                TransactionStatus::Failed => (tokens.danger, "● Failed"),
                TransactionStatus::Staged => (tokens.warning, "● Staged"),
                TransactionStatus::Applying => (tokens.accent_hover, "● Applying"),
                TransactionStatus::RollingBack => (tokens.danger, "● Rolling Back"),
            };

            let date_col = column![
                text(item.timestamp.format("%Y-%m-%d %H:%M").to_string())
                    .size(12)
                    .color(tokens.text_primary),
                text(format_relative_time(Some(item.timestamp)))
                    .size(10)
                    .color(tokens.text_muted),
            ]
            .spacing(2)
            .width(Length::FillPortion(2));

            let short_commit = truncate_chars(&item.source_commit, 7);

            let repo_row = row![
                text(&item.source_repo).size(13).color(tokens.text_primary),
                container(
                    text(format!("@{}", short_commit))
                        .size(10)
                        .color(tokens.accent_primary)
                )
                .padding([2, 6])
                .style(move |_theme| theme::badge(tokens.accent_subtle, tokens.accent_primary)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .width(Length::FillPortion(3));

            let comp_badge = container(
                text(format!("{:?}", item.component))
                    .size(11)
                    .color(tokens.text_primary),
            )
            .padding([3, 8])
            .style(move |_theme| theme::badge(tokens.bg_surface, tokens.text_primary))
            .width(Length::FillPortion(2));

            let files_text = text(format!("{} files", item.file_count))
                .size(12)
                .color(tokens.text_secondary)
                .width(Length::FillPortion(1));

            let status_badge = container(text(status_text).size(11).color(status_color))
                .padding([3, 8])
                .style(move |_theme| {
                    theme::badge(
                        Color::from_rgba(status_color.r, status_color.g, status_color.b, 0.15),
                        status_color,
                    )
                })
                .width(Length::FillPortion(2));

            let view_btn = button(text("View").size(11))
                .padding([4, 10])
                .on_press(on_action(HistoryAction::Inspect((*item).clone())))
                .style(move |_theme, status| theme::button_secondary(tokens, status));

            let mut actions_row = row![view_btn].spacing(6).align_y(Alignment::Center);

            if item.is_latest_committed {
                let rollback_btn = button(text("Rollback").size(11))
                    .padding([4, 10])
                    .on_press(on_action(HistoryAction::OpenRollbackConfirm(
                        (*item).clone(),
                    )))
                    .style(move |_theme, status| theme::button_danger(tokens, status));
                actions_row = actions_row.push(rollback_btn);
            } else if item.status == TransactionStatus::Committed {
                actions_row = actions_row.push(
                    container(text("Locked (LIFO)").size(10).color(tokens.text_muted))
                        .padding([3, 6])
                        .style(move |_theme| theme::badge(tokens.bg_surface, tokens.text_muted)),
                );
            }

            let actions_container = container(actions_row).width(Length::FillPortion(2));

            let row_container = container(
                row![
                    date_col,
                    repo_row,
                    comp_badge,
                    files_text,
                    status_badge,
                    actions_container,
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([10, 14])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(tokens.bg_card.into()),
                border: Border {
                    color: tokens.border_card,
                    width: 1.0,
                    radius: theme::radius::SM,
                },
                ..Default::default()
            });

            rows_col = rows_col.push(row_container);
        }

        let range_label = format!(
            "Showing 1-{} of {} transactions",
            filtered_items.len(),
            state.transactions.len()
        );

        let footer = row![
            text(range_label).size(12).color(tokens.text_muted),
            Space::new().width(Length::Fill),
        ]
        .padding([4, 8])
        .align_y(Alignment::Center);

        column![table_header, scrollable(rows_col), footer]
            .spacing(10)
            .into()
    };

    let main_view = column![header, metrics_row, filter_row, content]
        .spacing(16)
        .padding(20)
        .width(Length::Fill);

    // 5. Modal Dialogs Handling
    if let Some(confirm_modal) = &state.confirm_rollback_modal {
        let modal_overlay = render_rollback_confirm_modal(tokens, confirm_modal, &on_action);
        stack![main_view, modal_overlay].into()
    } else if let Some(dirty) = &state.dirty_modal {
        let modal_overlay = render_dirty_state_modal(tokens, dirty, &on_action);
        stack![main_view, modal_overlay].into()
    } else if let Some(inspect_item) = &state.inspect_modal {
        let modal_overlay = render_inspect_modal(tokens, inspect_item, &on_action);
        stack![main_view, modal_overlay].into()
    } else {
        main_view.into()
    }
}

/// Helper to render an individual overview metric card.
fn render_metric_card<'a, Message: 'a>(
    tokens: ThemeTokens,
    label: &'static str,
    value: &str,
    icon: &'static str,
    border_accent: Color,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(label).size(11).color(tokens.text_secondary),
                text(value.to_string()).size(22).color(tokens.text_primary),
            ]
            .spacing(2),
            Space::new().width(Length::Fill),
            text(icon).size(22),
        ]
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Length::FillPortion(1))
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_card.into()),
        border: Border {
            color: border_accent,
            width: 1.0,
            radius: theme::radius::MD,
        },
        ..Default::default()
    })
    .into()
}

/// Helper to render the guarded rollback confirmation modal.
fn render_rollback_confirm_modal<'a, Message: Clone + 'a>(
    tokens: ThemeTokens,
    modal: &'a RollbackConfirmState,
    on_action: &impl Fn(HistoryAction) -> Message,
) -> Element<'a, Message> {
    let title = text("Confirm Rollback Operation")
        .size(18)
        .color(tokens.danger);

    let target_card = container(
        column![
            row![
                text("Target Repository: ")
                    .size(12)
                    .color(tokens.text_muted),
                text(&modal.repo).size(12).color(tokens.text_primary),
            ]
            .spacing(6),
            row![
                text("Commit Revision: ").size(12).color(tokens.text_muted),
                text(&modal.revision).size(12).color(tokens.accent_primary),
                Space::new().width(12),
                text("Component: ").size(12).color(tokens.text_muted),
                text(format!("{:?}", modal.component))
                    .size(12)
                    .color(tokens.text_primary),
            ]
            .spacing(6),
        ]
        .spacing(4),
    )
    .padding(12)
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_surface.into()),
        border: Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: theme::radius::SM,
        },
        ..Default::default()
    });

    let mut details_col = column![].spacing(10);

    if !modal.files_to_restore.is_empty() {
        let mut restore_list = column![text(format!(
            "Files to Restore ({}):",
            modal.files_to_restore.len()
        ))
        .size(12)
        .color(tokens.warning),]
        .spacing(4);

        for path in &modal.files_to_restore {
            restore_list = restore_list.push(
                text(format!("⟲ {}", path))
                    .size(11)
                    .color(tokens.text_secondary),
            );
        }
        details_col = details_col.push(restore_list);
    }

    if !modal.files_to_remove.is_empty() {
        let mut remove_list = column![text(format!(
            "Files to Remove ({}):",
            modal.files_to_remove.len()
        ))
        .size(12)
        .color(tokens.danger),]
        .spacing(4);

        for path in &modal.files_to_remove {
            remove_list = remove_list.push(
                text(format!("✕ {}", path))
                    .size(11)
                    .color(tokens.text_secondary),
            );
        }
        details_col = details_col.push(remove_list);
    }

    let warning_box = container(
        row![
            text("🛡️").size(16),
            text("Local modifications made post-install are safeguarded; rollback will abort if dirty state is detected.")
                .size(11)
                .color(tokens.text_secondary),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(Color::from_rgba(tokens.warning.r, tokens.warning.g, tokens.warning.b, 0.1).into()),
        border: Border {
            color: tokens.warning,
            width: 1.0,
            radius: theme::radius::SM,
        },
        ..Default::default()
    });

    let confirm_btn = button(text("Confirm Rollback").size(12))
        .padding([8, 16])
        .on_press(on_action(HistoryAction::ExecuteRollback {
            tx_id: modal.tx_id.clone(),
            force_dirty: false,
        }))
        .style(move |_theme, status| theme::button_danger(tokens, status));

    let cancel_btn = button(text("Cancel").size(12))
        .padding([8, 16])
        .on_press(on_action(HistoryAction::CloseRollbackConfirm))
        .style(move |_theme, status| theme::button_secondary(tokens, status));

    let action_row = row![confirm_btn, cancel_btn].spacing(12);

    let modal_box = container(
        column![
            title,
            target_card,
            scrollable(details_col).height(Length::Fixed(160.0)),
            warning_box,
            action_row,
        ]
        .spacing(14),
    )
    .padding(24)
    .max_width(620)
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_card.into()),
        border: Border {
            color: tokens.danger,
            width: 1.5,
            radius: theme::radius::LG,
        },
        ..Default::default()
    });

    container(modal_box)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Color::from_rgba(0.02, 0.04, 0.08, 0.82).into()),
            ..Default::default()
        })
        .into()
}

/// Helper to render the preserved dirty state conflict modal.
fn render_dirty_state_modal<'a, Message: Clone + 'a>(
    tokens: ThemeTokens,
    modal: &'a DirtyStateModal,
    on_action: &impl Fn(HistoryAction) -> Message,
) -> Element<'a, Message> {
    let title = text("⚠️ Modified Files Detected")
        .size(18)
        .color(tokens.danger);

    let subtitle = text("The following files have been modified since installation. Rollback has been halted to prevent overwriting your changes:")
        .size(12)
        .color(tokens.text_secondary);

    let mut mod_files = column![].spacing(4);
    for path in &modal.modified_files {
        mod_files = mod_files.push(text(format!("• {}", path)).size(11).color(tokens.danger));
    }

    let tx_id = modal.transaction_id.clone();
    let backup_force_btn = button(text("Backup & Force Rollback").size(12))
        .padding([8, 16])
        .on_press(on_action(HistoryAction::ExecuteRollback {
            tx_id,
            force_dirty: true,
        }))
        .style(move |_theme, status| theme::button_danger(tokens, status));

    let cancel_btn = button(text("Cancel").size(12))
        .padding([8, 16])
        .on_press(on_action(HistoryAction::CloseDirtyModal))
        .style(move |_theme, status| theme::button_secondary(tokens, status));

    let modal_box = container(
        column![
            title,
            subtitle,
            scrollable(mod_files).height(Length::Fixed(120.0)),
            row![backup_force_btn, cancel_btn].spacing(12),
        ]
        .spacing(14),
    )
    .padding(24)
    .max_width(560)
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_card.into()),
        border: Border {
            color: tokens.danger,
            width: 2.0,
            radius: theme::radius::LG,
        },
        ..Default::default()
    });

    container(modal_box)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Color::from_rgba(0.02, 0.04, 0.08, 0.82).into()),
            ..Default::default()
        })
        .into()
}

/// Helper to render the operation inspection modal.
fn render_inspect_modal<'a, Message: Clone + 'a>(
    tokens: ThemeTokens,
    item: &'a HistoryItem,
    on_action: &impl Fn(HistoryAction) -> Message,
) -> Element<'a, Message> {
    let title = text("Transaction Audit Details")
        .size(18)
        .color(tokens.text_primary);

    let meta_card = container(
        column![
            row![
                text("Transaction ID: ").size(11).color(tokens.text_muted),
                text(&item.id).size(11).color(tokens.text_primary),
            ]
            .spacing(6),
            row![
                text("Repository: ").size(11).color(tokens.text_muted),
                text(&item.source_repo).size(11).color(tokens.text_primary),
                Space::new().width(12),
                text("Commit: ").size(11).color(tokens.text_muted),
                text(&item.source_commit)
                    .size(11)
                    .color(tokens.accent_primary),
                Space::new().width(12),
                text("Component: ").size(11).color(tokens.text_muted),
                text(format!("{:?}", item.component))
                    .size(11)
                    .color(tokens.text_primary),
            ]
            .spacing(6),
            row![
                text("Timestamp: ").size(11).color(tokens.text_muted),
                text(item.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .size(11)
                    .color(tokens.text_secondary),
                Space::new().width(12),
                text("Backup Path: ").size(11).color(tokens.text_muted),
                text(item.backup_dir.as_str())
                    .size(11)
                    .color(tokens.text_secondary),
            ]
            .spacing(6),
        ]
        .spacing(4),
    )
    .padding(12)
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_surface.into()),
        border: Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: theme::radius::SM,
        },
        ..Default::default()
    });

    let ops_header = text(format!(
        "File Operations ({} records):",
        item.operations.len()
    ))
    .size(13)
    .color(tokens.text_primary);

    let op_table_header = container(
        row![
            text("OPERATION")
                .size(10)
                .color(tokens.text_muted)
                .width(Length::FillPortion(2)),
            text("TARGET FILE")
                .size(10)
                .color(tokens.text_muted)
                .width(Length::FillPortion(4)),
            text("BACKUP SNAPSHOT")
                .size(10)
                .color(tokens.text_muted)
                .width(Length::FillPortion(3)),
            text("SHA256 (PRE / POST)")
                .size(10)
                .color(tokens.text_muted)
                .width(Length::FillPortion(3)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_surface.into()),
        border: Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: theme::radius::SM,
        },
        ..Default::default()
    });

    let mut op_rows = column![].spacing(4);

    for op in &item.operations {
        let (op_color, op_label) = match op.op_type {
            OperationType::Create => (tokens.success, "+ Create"),
            OperationType::Overwrite => (tokens.warning, "~ Overwrite"),
            OperationType::Delete => (tokens.danger, "- Delete"),
            OperationType::DeactivateLegacy => (tokens.accent_primary, "⊘ Deactivate"),
        };

        let op_badge = container(text(op_label).size(10).color(op_color))
            .padding([2, 6])
            .style(move |_theme| {
                theme::badge(
                    Color::from_rgba(op_color.r, op_color.g, op_color.b, 0.15),
                    op_color,
                )
            })
            .width(Length::FillPortion(2));

        let target_text = text(op.target_file.as_str())
            .size(11)
            .color(tokens.text_primary)
            .width(Length::FillPortion(4));

        let backup_str = op.backup_file.as_ref().map(|p| p.as_str()).unwrap_or("-");
        let backup_text = text(backup_str)
            .size(10)
            .color(tokens.text_muted)
            .width(Length::FillPortion(3));

        let pre_hash = op.pre_sha256.as_deref().unwrap_or("-");
        let post_hash = op.post_sha256.as_deref().unwrap_or("-");
        let short_pre = truncate_chars(pre_hash, 8);
        let short_post = truncate_chars(post_hash, 8);

        let hash_col = column![
            text(format!("pre:  {}", short_pre))
                .size(9)
                .color(tokens.text_muted),
            text(format!("post: {}", short_post))
                .size(9)
                .color(tokens.text_muted),
        ]
        .spacing(1)
        .width(Length::FillPortion(3));

        let row_box = container(
            row![op_badge, target_text, backup_text, hash_col]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_surface.into()),
            border: Border {
                color: tokens.border_subtle,
                width: 0.5,
                radius: theme::radius::SM,
            },
            ..Default::default()
        });

        op_rows = op_rows.push(row_box);
    }

    let close_btn = button(text("Close").size(12))
        .padding([8, 18])
        .on_press(on_action(HistoryAction::CloseInspect))
        .style(move |_theme, status| theme::button_secondary(tokens, status));

    let modal_box = container(
        column![
            title,
            meta_card,
            ops_header,
            op_table_header,
            scrollable(op_rows).height(Length::Fixed(220.0)),
            row![Space::new().width(Length::Fill), close_btn].align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(24)
    .max_width(740)
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_card.into()),
        border: Border {
            color: tokens.border_card,
            width: 1.5,
            radius: theme::radius::LG,
        },
        ..Default::default()
    });

    container(modal_box)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Color::from_rgba(0.02, 0.04, 0.08, 0.82).into()),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_chars_ascii_and_edge_cases() {
        assert_eq!(truncate_chars("", 0), "");
        assert_eq!(truncate_chars("", 7), "");
        assert_eq!(truncate_chars("a", 7), "a");
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("123456", 7), "123456");
        assert_eq!(truncate_chars("1234567", 7), "1234567");
        assert_eq!(truncate_chars("12345678", 7), "1234567");
        assert_eq!(
            truncate_chars("40characterslongshastring1234567890abcdef", 7),
            "40chara"
        );
        assert_eq!(truncate_chars("abcdef12345", 8), "abcdef12");
        assert_eq!(truncate_chars("hello", 0), "");
    }

    #[test]
    fn test_truncate_chars_unicode_multibyte_safety() {
        assert_eq!(truncate_chars("🦀", 0), "");
        assert_eq!(truncate_chars("🦀", 1), "🦀");
        assert_eq!(truncate_chars("🦀", 7), "🦀");
        assert_eq!(truncate_chars("🦀🦀", 7), "🦀🦀");
        assert_eq!(truncate_chars("🦀🦀🦀", 2), "🦀🦀");
        assert_eq!(truncate_chars("🦀🦀🦀🦀🦀🦀🦀", 7), "🦀🦀🦀🦀🦀🦀🦀");
        assert_eq!(truncate_chars("🦀🦀🦀🦀🦀🦀🦀🦀", 7), "🦀🦀🦀🦀🦀🦀🦀");
        assert_eq!(truncate_chars("a🦀🦀", 2), "a🦀");
        assert_eq!(truncate_chars("a🦀🦀", 8), "a🦀🦀");
        assert_eq!(truncate_chars("prefix-🦀-suffix", 8), "prefix-🦀");
        assert_eq!(truncate_chars("v1.0-🚀-feature", 7), "v1.0-🚀-");
        assert_eq!(truncate_chars("日本語と英語", 3), "日本語");
        assert_eq!(safe_truncate("12345678", 7), "1234567");
        assert_eq!(safe_truncate("🦀🦀", 1), "🦀");
    }
}

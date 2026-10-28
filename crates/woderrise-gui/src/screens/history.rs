//! Linear LIFO transaction history and rollback management screen.
//!
//! Enforces linear LIFO rollback: only the latest committed transaction
//! can be rolled back. Handles dirty-state confirmation modals if local files
//! were modified post-install.

use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};
use woderrise_core::models::{
    ConfigComponent, FileOperationRecord, JournalEntry, TransactionStatus,
};

/// A single entry in the transaction history view.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct DirtyStateModal {
    pub transaction_id: String,
    pub modified_files: Vec<Utf8PathBuf>,
}

/// State for the History screen.
#[derive(Debug, Clone)]
pub struct HistoryState {
    pub transactions: Vec<HistoryItem>,
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

        Self {
            transactions: items,
            dirty_modal: None,
            is_rolling_back: false,
        }
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

/// Renders the history screen view.
pub fn view_history<'a, Message: Clone + 'a>(
    state: &'a HistoryState,
    on_rollback_latest: impl Fn(bool) -> Message + 'a,
    on_rollback_tx: impl Fn(String, bool) -> Message + 'a,
    on_close_modal: Message,
) -> Element<'a, Message> {
    let header = column![
        text("Transaction History & Rollback").size(20),
        text("Audit trail of all dotfile installations with guaranteed 1-click LIFO reversal.")
            .size(13)
            .color(Color::from_rgb(0.7, 0.72, 0.8)),
    ]
    .spacing(4);

    if state.transactions.is_empty() {
        let empty_msg = container(
            column![
                text("No transaction history found.").size(15),
                text(
                    "Installations you perform will be logged here with complete rollback support."
                )
                .size(12)
                .color(Color::from_rgb(0.6, 0.62, 0.7)),
            ]
            .spacing(6)
            .align_x(Alignment::Center),
        )
        .padding(40)
        .width(Length::Fill)
        .align_x(Alignment::Center);

        return column![header, empty_msg].spacing(20).padding(20).into();
    }

    let mut list_col = column![].spacing(12);

    for item in &state.transactions {
        let (status_color, status_text) = match item.status {
            TransactionStatus::Committed => (Color::from_rgb(0.3, 0.85, 0.4), "Committed"),
            TransactionStatus::RolledBack => (Color::from_rgb(0.5, 0.7, 0.95), "Rolled Back"),
            TransactionStatus::RecoveredRollback => {
                (Color::from_rgb(0.95, 0.65, 0.3), "Recovered Rollback")
            }
            TransactionStatus::Failed => (Color::from_rgb(0.95, 0.35, 0.35), "Failed"),
            TransactionStatus::Staged => (Color::from_rgb(0.8, 0.8, 0.4), "Staged"),
            TransactionStatus::Applying => (Color::from_rgb(0.8, 0.6, 0.9), "Applying"),
            TransactionStatus::RollingBack => (Color::from_rgb(0.9, 0.6, 0.4), "Rolling Back"),
        };

        let status_badge = container(text(status_text).size(11).color(status_color))
            .padding([2, 8])
            .style(move |_theme| container::Style {
                background: Some(
                    Color::from_rgba(status_color.r, status_color.g, status_color.b, 0.15).into(),
                ),
                border: iced::Border {
                    color: status_color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        let card_header = row![
            text(&item.id)
                .size(14)
                .color(Color::from_rgb(0.9, 0.92, 0.98)),
            status_badge,
            text(item.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .size(11)
                .color(Color::from_rgb(0.55, 0.58, 0.7)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let details = row![
            text(format!("Repo: {}", item.source_repo))
                .size(12)
                .color(Color::from_rgb(0.7, 0.75, 0.85)),
            text(format!("Component: {:?}", item.component))
                .size(12)
                .color(Color::from_rgb(0.7, 0.75, 0.85)),
            text(format!("Files: {}", item.file_count))
                .size(12)
                .color(Color::from_rgb(0.7, 0.75, 0.85)),
        ]
        .spacing(16);

        // Strict LIFO rollback button
        let action_row: Option<Element<'a, Message>> = if item.is_latest_committed {
            let rollback_btn = button(text("1-Click Rollback").size(12))
                .padding([4, 12])
                .on_press(on_rollback_latest(false));
            Some(row![rollback_btn].into())
        } else if item.status == TransactionStatus::Committed {
            Some(
                text("Rollback unavailable (Must rollback newer transactions first)")
                    .size(11)
                    .color(Color::from_rgb(0.55, 0.55, 0.65))
                    .into(),
            )
        } else {
            None
        };

        let mut card_col = column![card_header, details].spacing(8);
        if let Some(action) = action_row {
            card_col = card_col.push(action);
        }

        let card = container(card_col)
            .padding(12)
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.12, 0.13, 0.2, 0.95).into()),
                border: iced::Border {
                    color: Color::from_rgba(0.3, 0.35, 0.5, 0.4),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        list_col = list_col.push(card);
    }

    let mut main_col = column![header].spacing(16);

    // Modal overlay for dirty state if active
    if let Some(modal) = &state.dirty_modal {
        let mut mod_files = column![
            text("⚠️ Modified Files Detected")
                .size(16)
                .color(Color::from_rgb(0.95, 0.4, 0.4)),
            text("The following files have been modified since installation:")
                .size(12)
                .color(Color::from_rgb(0.8, 0.82, 0.9)),
        ]
        .spacing(6);

        for path in &modal.modified_files {
            mod_files = mod_files.push(
                text(format!("• {}", path))
                    .size(11)
                    .color(Color::from_rgb(0.9, 0.6, 0.6)),
            );
        }

        let tx_id = modal.transaction_id.clone();
        let backup_force_btn = button(text("Backup & Force Rollback").size(12))
            .padding([6, 12])
            .on_press(on_rollback_tx(tx_id.clone(), true));
        let cancel_modal_btn = button(text("Cancel").size(12))
            .padding([6, 12])
            .on_press(on_close_modal);

        let modal_box = container(
            column![
                mod_files,
                row![backup_force_btn, cancel_modal_btn].spacing(10),
            ]
            .spacing(14)
            .padding(16),
        )
        .style(|_theme| container::Style {
            background: Some(Color::from_rgb(0.14, 0.15, 0.22).into()),
            border: iced::Border {
                color: Color::from_rgb(0.8, 0.3, 0.3),
                width: 2.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .max_width(500);

        let modal_container = container(modal_box)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .padding(20);

        main_col = main_col.push(modal_container);
    }

    main_col = main_col.push(scrollable(list_col));

    main_col.padding(20).into()
}

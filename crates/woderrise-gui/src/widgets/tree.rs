//! Complete hierarchical and segmented Git tree widget.
//!
//! Visualizes files and directories from Git trees with icons, file modes,
//! byte sizes, search filtering, and selectable inspection rows.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Border, Color, Element, Length, Theme};
use woderrise_detect::TreeEntry;

use crate::theme;

/// Formats a byte size into human-readable string (B, KB, MB).
pub fn format_tree_size(bytes: Option<u64>) -> String {
    match bytes {
        Some(b) if b >= 1_048_576 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        Some(b) if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) => format!("{} B", b),
        None => "".to_string(),
    }
}

/// Renders a Git tree list with selection and inspection triggers.
pub fn view_tree<'a, Message: Clone + 'a>(
    entries: &'a [TreeEntry],
    selected_path: Option<&'a str>,
    filter: &str,
    theme: &Theme,
    on_select: impl Fn(String) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let tokens = theme::tokens(theme);
    let filter_clean = filter.trim().to_lowercase();

    let filtered: Vec<&TreeEntry> = entries
        .iter()
        .filter(|e| {
            if filter_clean.is_empty() {
                true
            } else {
                e.path.to_lowercase().contains(&filter_clean)
            }
        })
        .collect();

    if filtered.is_empty() {
        let msg = if entries.is_empty() {
            "No files in tree or tree still loading..."
        } else {
            "No files match filter."
        };
        return container(text(msg).size(13).color(tokens.text_muted))
            .padding(16)
            .width(Length::Fill)
            .into();
    }

    let mut list = column![].spacing(3);

    for entry in filtered {
        let is_selected = selected_path == Some(entry.path.as_str());

        let icon = if entry.is_symlink() {
            "🔗"
        } else if entry.is_executable() {
            "⚡"
        } else if entry.is_tree() {
            "📁"
        } else {
            "📄"
        };

        let size_str = format_tree_size(entry.size);
        let path_text = text(&entry.path).size(12).color(if is_selected {
            tokens.accent_primary
        } else {
            tokens.text_primary
        });

        let mut meta_row = row![text(&entry.mode).size(11).color(tokens.text_muted),].spacing(6);

        if !size_str.is_empty() {
            meta_row = meta_row.push(
                text(format!("• {}", size_str))
                    .size(11)
                    .color(tokens.text_muted),
            );
        }

        let item_row = row![
            text(icon).size(12),
            path_text,
            Space::new().width(Length::Fill),
            meta_row,
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let path_clone = entry.path.clone();
        let select_fn = on_select.clone();

        let item_btn = button(item_row)
            .padding([5, 8])
            .width(Length::Fill)
            .on_press(select_fn(path_clone))
            .style(move |_theme, status| {
                if is_selected {
                    button::Style {
                        background: Some(tokens.accent_subtle.into()),
                        border: Border {
                            color: tokens.accent_primary,
                            width: 1.0,
                            radius: theme::radius::SM,
                        },
                        text_color: tokens.text_primary,
                        ..Default::default()
                    }
                } else {
                    match status {
                        button::Status::Hovered => button::Style {
                            background: Some(tokens.bg_card_hover.into()),
                            border: Border {
                                color: tokens.border_subtle,
                                width: 1.0,
                                radius: theme::radius::SM,
                            },
                            text_color: tokens.text_primary,
                            ..Default::default()
                        },
                        _ => button::Style {
                            background: Some(Color::TRANSPARENT.into()),
                            border: Border {
                                color: Color::TRANSPARENT,
                                width: 1.0,
                                radius: theme::radius::SM,
                            },
                            text_color: tokens.text_primary,
                            ..Default::default()
                        },
                    }
                }
            });

        list = list.push(item_btn);
    }

    scrollable(list).width(Length::Fill).into()
}

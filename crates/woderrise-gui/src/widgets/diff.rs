//! Syntax-highlighted unified diff renderer using `similar`.
//!
//! Visualizes dotfile line additions (+), deletions (-), and context lines
//! with clean styling matching Wayland desktop ricing aesthetics.

use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Border, Color, Element, Length, Theme};
use similar::{ChangeTag, TextDiff};

use crate::theme;

/// A single line in a diff representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub tag: ChangeTag,
    pub content: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

/// A complete diff report for a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiffReport {
    pub file_path: String,
    pub lines: Vec<DiffLine>,
    pub additions: usize,
    pub deletions: usize,
}

impl FileDiffReport {
    /// Computes a unified diff between old content (local file) and new content (remote dotfile).
    pub fn compute(file_path: impl Into<String>, old_content: &str, new_content: &str) -> Self {
        let file_path = file_path.into();
        let diff = TextDiff::from_lines(old_content, new_content);
        let mut lines = Vec::new();
        let mut additions = 0;
        let mut deletions = 0;

        let mut old_idx = 1;
        let mut new_idx = 1;

        for change in diff.iter_all_changes() {
            let tag = change.tag();
            let (old_line, new_line) = match tag {
                ChangeTag::Delete => {
                    let cur = old_idx;
                    old_idx += 1;
                    deletions += 1;
                    (Some(cur), None)
                }
                ChangeTag::Insert => {
                    let cur = new_idx;
                    new_idx += 1;
                    additions += 1;
                    (None, Some(cur))
                }
                ChangeTag::Equal => {
                    let cur_old = old_idx;
                    let cur_new = new_idx;
                    old_idx += 1;
                    new_idx += 1;
                    (Some(cur_old), Some(cur_new))
                }
            };

            lines.push(DiffLine {
                tag,
                content: change.value().trim_end_matches(['\r', '\n']).to_string(),
                old_line,
                new_line,
            });
        }

        Self {
            file_path,
            lines,
            additions,
            deletions,
        }
    }
}

/// Renders a `FileDiffReport` as an Iced Element with default dark theme.
pub fn view_diff<'a, Message: 'a>(report: &'a FileDiffReport) -> Element<'a, Message> {
    view_diff_themed(report, &Theme::Dark)
}

/// Renders a `FileDiffReport` as an Iced Element styled with the specified theme tokens.
pub fn view_diff_themed<'a, Message: 'a>(
    report: &'a FileDiffReport,
    theme: &Theme,
) -> Element<'a, Message> {
    let tokens = theme::tokens(theme);

    let header = container(
        row![
            text(&report.file_path).size(13).color(tokens.text_primary),
            Space::new().width(Length::Fill),
            text(format!("+{}", report.additions))
                .size(12)
                .color(tokens.success),
            text(format!("-{}", report.deletions))
                .size(12)
                .color(tokens.danger),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center),
    )
    .padding([8, 12])
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_surface.into()),
        border: Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: Border {
                radius: theme::radius::SM,
                ..Default::default()
            }
            .radius,
        },
        ..Default::default()
    });

    let mut diff_column = column![header].spacing(2);

    for line in &report.lines {
        let (sign, text_color, bg_color) = match line.tag {
            ChangeTag::Insert => (
                "+",
                tokens.success,
                Color::from_rgba(tokens.success.r, tokens.success.g, tokens.success.b, 0.12),
            ),
            ChangeTag::Delete => (
                "-",
                tokens.danger,
                Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.12),
            ),
            ChangeTag::Equal => (" ", tokens.text_secondary, Color::TRANSPARENT),
        };

        let old_num = line.old_line.map(|n| n.to_string()).unwrap_or_default();
        let new_num = line.new_line.map(|n| n.to_string()).unwrap_or_default();

        let line_row = row![
            text(format!("{:>4}", old_num))
                .size(11)
                .color(tokens.text_muted),
            text(format!("{:>4}", new_num))
                .size(11)
                .color(tokens.text_muted),
            text(sign).size(12).color(text_color),
            text(&line.content).size(12).color(text_color),
        ]
        .spacing(8);

        let styled_line = container(line_row)
            .padding([2, 8])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg_color.into()),
                ..Default::default()
            });

        diff_column = diff_column.push(styled_line);
    }

    container(scrollable(diff_column))
        .padding(6)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_card.into()),
            border: Border {
                color: tokens.border_card,
                width: 1.0,
                radius: theme::radius::MD,
            },
            ..Default::default()
        })
        .into()
}

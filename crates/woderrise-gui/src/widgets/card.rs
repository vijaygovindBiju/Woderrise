//! Repository card widget for dotfile discovery grid.
//!
//! Inspired by the modern desktop software presentation, displaying
//! metadata, star counters, component badges, relative update timestamps,
//! and call-to-action buttons styled with unified design system tokens.

use iced::border::Radius;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Border, Color, Element, Length, Theme};
use woderrise_core::models::CatalogRepository;

/// Formats a push timestamp into a human-readable relative time string.
pub fn format_relative_time(pushed_at: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let Some(pushed) = pushed_at else {
        return "Updated recently".to_string();
    };
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(pushed);
    let seconds = duration.num_seconds();

    if seconds < 60 {
        "Updated just now".to_string()
    } else if seconds < 3600 {
        let mins = (seconds / 60).max(1);
        if mins == 1 {
            "Updated 1 min ago".to_string()
        } else {
            format!("Updated {} mins ago", mins)
        }
    } else if seconds < 86400 {
        let hours = (seconds / 3600).max(1);
        if hours == 1 {
            "Updated 1 hour ago".to_string()
        } else {
            format!("Updated {} hours ago", hours)
        }
    } else if seconds < 86400 * 30 {
        let days = (seconds / 86400).max(1);
        if days == 1 {
            "Updated 1 day ago".to_string()
        } else {
            format!("Updated {} days ago", days)
        }
    } else if seconds < 86400 * 365 {
        let months = (seconds / (86400 * 30)).max(1);
        if months == 1 {
            "Updated 1 month ago".to_string()
        } else {
            format!("Updated {} months ago", months)
        }
    } else {
        let years = (seconds / (86400 * 365)).max(1);
        if years == 1 {
            "Updated 1 year ago".to_string()
        } else {
            format!("Updated {} years ago", years)
        }
    }
}

/// Formats a star count into an authentic abbreviated representation (e.g. `★ 16.4k`).
pub fn format_star_count(stars: u64) -> String {
    if stars >= 1_000 {
        format!("★ {:.1}k", stars as f64 / 1000.0)
    } else {
        format!("★ {}", stars)
    }
}

/// Renders a repository card using default dark theme tokens.
pub fn view_card<'a, Message: Clone + 'a>(
    repo: &'a CatalogRepository,
    on_inspect: Message,
) -> Element<'a, Message> {
    view_card_themed(repo, &Theme::Dark, on_inspect)
}

/// Renders a repository card styled with the specified theme tokens.
pub fn view_card_themed<'a, Message: Clone + 'a>(
    repo: &'a CatalogRepository,
    theme: &Theme,
    on_inspect: Message,
) -> Element<'a, Message> {
    let tokens = crate::theme::tokens(theme);

    // 1. Top Preview Banner with authentic star count badge
    let star_label = format_star_count(repo.stargazers_count);

    let star_badge = container(text(star_label).size(11).color(tokens.star_gold))
        .padding([3, 8])
        .style(move |_theme| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
            border: Border {
                radius: crate::theme::radius::PILL,
                ..Default::default()
            },
            ..Default::default()
        });

    let preview_banner = container(
        row![Space::new().width(Length::Fill), star_badge].align_y(iced::Alignment::Start),
    )
    .padding(8)
    .width(Length::Fill)
    .height(Length::Fixed(64.0))
    .style(move |theme| {
        let t = crate::theme::tokens(theme);
        container::Style {
            background: Some(
                Color::from_rgba(
                    t.accent_primary.r,
                    t.accent_primary.g,
                    t.accent_primary.b,
                    0.12,
                )
                .into(),
            ),
            border: Border {
                radius: Radius {
                    top_left: 10.0,
                    top_right: 10.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
                ..Default::default()
            },
            ..Default::default()
        }
    });

    // 2. Repository Name and Owner Handle
    let repo_name = repo.resolved_repo();
    let display_repo = if repo_name.is_empty() {
        repo.resolved_full_name()
    } else {
        repo_name.to_string()
    };
    let title = text(display_repo).size(14).color(tokens.text_primary);

    let owner_handle = repo.resolved_owner();
    let owner_text = if owner_handle.is_empty() {
        String::new()
    } else {
        format!("@{}", owner_handle)
    };
    let owner = text(owner_text).size(11).color(tokens.text_muted);

    let title_row =
        row![title, Space::new().width(Length::Fixed(6.0)), owner].align_y(iced::Alignment::Center);

    // 3. Clean 2-line clamped description
    let raw_desc = repo
        .description
        .as_deref()
        .unwrap_or("No description provided");
    let desc_truncated = if raw_desc.chars().count() > 95 {
        format!("{}...", raw_desc.chars().take(92).collect::<String>())
    } else {
        raw_desc.to_string()
    };
    let description = text(desc_truncated).size(12).color(tokens.text_secondary);

    // 4. Detected component badge row wrapping up to 4 tags
    let mut badge_row = row![].spacing(6);
    for comp in repo.detected_components.iter().take(4) {
        let badge = container(
            text(comp.component.display_name())
                .size(10)
                .color(tokens.accent_primary),
        )
        .padding([2, 6])
        .style(move |theme| {
            let t = crate::theme::tokens(theme);
            container::Style {
                background: Some(t.accent_subtle.into()),
                border: Border {
                    color: t.accent_subtle,
                    width: 1.0,
                    radius: crate::theme::radius::SM,
                },
                ..Default::default()
            }
        });

        badge_row = badge_row.push(badge);
    }

    // 5. Card footer: Relative Update Time + File Count + Primary Inspect Button
    let time_str = format_relative_time(repo.pushed_at);
    let total_files: usize = repo.detected_components.iter().map(|c| c.file_count).sum();
    let files_label = if total_files == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", total_files)
    };
    let meta_str = format!("{} • {}", time_str, files_label);

    let inspect_btn = button(text("Inspect").size(12))
        .padding([5, 14])
        .on_press(on_inspect)
        .style(move |theme, status| {
            let t = crate::theme::tokens(theme);
            crate::theme::button_primary(t, status)
        });

    let footer = row![
        text(meta_str).size(11).color(tokens.text_muted),
        Space::new().width(Length::Fill),
        inspect_btn,
    ]
    .align_y(iced::Alignment::Center);

    // 6. Assemble Full Card Content with standardized 280.0px height
    let card_content = column![
        preview_banner,
        column![
            title_row,
            description,
            badge_row.wrap(),
            Space::new().height(Length::Fill),
            footer,
        ]
        .spacing(8)
        .padding(12),
    ];

    container(card_content)
        .width(Length::Fill)
        .height(Length::Fixed(280.0))
        .style(move |theme| {
            let t = crate::theme::tokens(theme);
            crate::theme::container_card(t)
        })
        .into()
}

//! Repository card widget for dotfile discovery grid.
//!
//! Inspired by the modern Snap Store / GNOME Software presentation, displaying
//! metadata, star counters, component badges, and call-to-action buttons.

use iced::widget::{button, column, container, row, text};
use iced::{Color, Element, Length};
use woderrise_core::models::CatalogRepository;

/// Renders a repository card.
pub fn view_card<'a, Message: Clone + 'a>(
    repo: &'a CatalogRepository,
    on_inspect: Message,
) -> Element<'a, Message> {
    let title = text(repo.resolved_full_name())
        .size(15)
        .color(Color::from_rgb(0.9, 0.92, 0.98));

    let stars = text(format!("★ {}", repo.stargazers_count))
        .size(12)
        .color(Color::from_rgb(0.95, 0.85, 0.35));

    let header = row![title, stars]
        .spacing(8)
        .align_y(iced::Alignment::Center);

    let desc_text = repo
        .description
        .as_deref()
        .unwrap_or("No description provided");
    let description = text(desc_text)
        .size(12)
        .color(Color::from_rgb(0.7, 0.72, 0.8));

    // Component badges and file counts
    let mut badge_row = row![].spacing(6);
    let total_files: usize = repo.detected_components.iter().map(|c| c.file_count).sum();

    for comp in &repo.detected_components {
        let label = if comp.file_count > 0 {
            let plural = if comp.file_count == 1 {
                "file"
            } else {
                "files"
            };
            format!(
                "{} ({} {})",
                comp.component.display_name(),
                comp.file_count,
                plural
            )
        } else {
            comp.component.display_name().to_string()
        };

        let badge = container(
            text(label)
                .size(10)
                .color(Color::from_rgb(0.55, 0.75, 0.98)),
        )
        .padding([2, 6])
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.2, 0.3, 0.5, 0.3).into()),
            border: iced::Border {
                color: Color::from_rgba(0.4, 0.6, 0.9, 0.4),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        badge_row = badge_row.push(badge);
    }

    let inspect_btn = button(text("Inspect").size(12))
        .padding([4, 12])
        .on_press(on_inspect);

    let mut footer = row![inspect_btn]
        .spacing(10)
        .align_y(iced::Alignment::Center);
    if total_files > 0 {
        let plural = if total_files == 1 { "file" } else { "files" };
        footer = footer.push(
            text(format!("Total: {} {}", total_files, plural))
                .size(11)
                .color(Color::from_rgb(0.65, 0.70, 0.85)),
        );
    }

    let mut card_content = column![header, description].spacing(10);
    if !repo.detected_components.is_empty() {
        card_content = card_content.push(badge_row.wrap());
    }
    card_content = card_content.push(footer);

    container(card_content)
        .padding(14)
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.12, 0.13, 0.19, 0.95).into()),
            border: iced::Border {
                color: Color::from_rgba(0.3, 0.35, 0.48, 0.4),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        })
        .into()
}

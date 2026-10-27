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

    // Component badges
    let mut badge_row = row![].spacing(6);
    for comp in &repo.detected_components {
        let badge = container(
            text(format!("{:?}", comp.component))
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
    let badge_row = badge_row.wrap();

    let inspect_btn = button(text("Inspect").size(12))
        .padding([4, 12])
        .on_press(on_inspect);

    let card_content = column![
        header,
        description,
        badge_row,
        row![inspect_btn].align_y(iced::Alignment::Center)
    ]
    .spacing(10);

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

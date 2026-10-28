//! Settings and GitHub Personal Access Token management screen.
//!
//! Provides an interface to configure an optional GitHub Personal Access Token (PAT)
//! stored durably in `~/.config/woderrise/settings.json` with POSIX 0600 permissions,
//! activating elevated rate limits (Search 10/min -> 30/min, Core 60/hr -> 5,000/hr).

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Color, Element, Length};

use crate::widgets::gauge::RateLimitGaugeState;

/// State for the Settings screen.
#[derive(Debug, Clone)]
pub struct SettingsState {
    /// Active text in the token input field.
    pub token_input: String,
    /// Whether the token is masked in the input field.
    pub is_masked: bool,
    /// Whether an asynchronous save, clear, or test operation is in progress.
    pub is_busy: bool,
    /// Feedback status message: (message_text, is_error).
    pub status_notice: Option<(String, bool)>,
    /// Last verified or durably saved token.
    pub saved_token: Option<String>,
}

impl SettingsState {
    /// Creates a new `SettingsState` initializing input from the current saved token if present.
    pub fn new(current_token: Option<String>) -> Self {
        Self {
            token_input: current_token.clone().unwrap_or_default(),
            is_masked: true,
            is_busy: false,
            status_notice: None,
            saved_token: current_token,
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

/// Renders the Settings screen view.
pub fn view_settings<'a, Message: Clone + 'a>(
    state: &'a SettingsState,
    rate_limits: &'a RateLimitGaugeState,
    on_token_changed: impl Fn(String) -> Message + 'a,
    on_toggle_mask: Message,
    on_save_token: Message,
    on_clear_token: Message,
    on_test_connection: Message,
) -> Element<'a, Message> {
    // 1. Header Section
    let header = column![
        text("Settings: GitHub Authentication & API Quotas").size(20),
        text("Configure an optional GitHub Personal Access Token (PAT) for elevated rate limits and reliable dotfile discovery.")
            .size(13)
            .color(Color::from_rgb(0.65, 0.68, 0.8)),
    ]
    .spacing(4);

    // 2. Elevated Rate Limits Explanation Card
    let info_card = container(
        column![
            text("Elevated API Rate Limits")
                .size(15)
                .color(Color::from_rgb(0.55, 0.75, 0.98)),
            Space::new().height(Length::Fixed(4.0)),
            text("By default, GitHub restricts anonymous requests to prevent API abuse. Supplying a Personal Access Token unlocks higher tier quotas:")
                .size(13)
                .color(Color::from_rgb(0.8, 0.82, 0.9)),
            Space::new().height(Length::Fixed(6.0)),
            column![
                row![
                    text("• Search API:").size(13).color(Color::from_rgb(0.9, 0.9, 0.95)),
                    text(" 10 requests / minute (unauthenticated) ➔ ").size(13).color(Color::from_rgb(0.7, 0.72, 0.8)),
                    text("30 requests / minute (authenticated)").size(13).color(Color::from_rgb(0.4, 0.85, 0.5)),
                ].align_y(Alignment::Center),
                row![
                    text("• Core REST API:").size(13).color(Color::from_rgb(0.9, 0.9, 0.95)),
                    text(" 60 requests / hour (unauthenticated) ➔ ").size(13).color(Color::from_rgb(0.7, 0.72, 0.8)),
                    text("5,000 requests / hour (authenticated)").size(13).color(Color::from_rgb(0.4, 0.85, 0.5)),
                ].align_y(Alignment::Center),
                row![
                    text("• POSIX Security:").size(13).color(Color::from_rgb(0.9, 0.9, 0.95)),
                    text(" Token is saved durably in ~/.config/woderrise/settings.json with strict ").size(13).color(Color::from_rgb(0.7, 0.72, 0.8)),
                    text("0600 permissions").size(13).color(Color::from_rgb(0.95, 0.85, 0.4)),
                    text(" (read/write by owner only).").size(13).color(Color::from_rgb(0.7, 0.72, 0.8)),
                ].align_y(Alignment::Center),
                row![
                    text("• Required Scopes:").size(13).color(Color::from_rgb(0.9, 0.9, 0.95)),
                    text(" No private repository permissions needed. Fine-grained tokens only require read-only access to public repositories.").size(13).color(Color::from_rgb(0.7, 0.72, 0.8)),
                ].align_y(Alignment::Center),
            ].spacing(5),
        ]
        .spacing(6),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(Color::from_rgba(0.12, 0.15, 0.25, 0.6).into()),
        border: iced::Border {
            color: Color::from_rgba(0.3, 0.35, 0.5, 0.4),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 3. Current Authentication & Rate Limit Status Card
    let auth_badge = if state.is_authenticated() {
        row![
            text("●").size(14).color(Color::from_rgb(0.4, 0.85, 0.5)),
            text(" Authenticated")
                .size(13)
                .color(Color::from_rgb(0.4, 0.85, 0.5)),
            Space::new().width(Length::Fixed(8.0)),
            text(format!("(Active Token: {})", state.masked_display()))
                .size(12)
                .color(Color::from_rgb(0.7, 0.72, 0.8)),
        ]
        .align_y(Alignment::Center)
    } else {
        row![
            text("○").size(14).color(Color::from_rgb(0.95, 0.85, 0.4)),
            text(" Unauthenticated (Anonymous public access)")
                .size(13)
                .color(Color::from_rgb(0.95, 0.85, 0.4)),
        ]
        .align_y(Alignment::Center)
    };

    let quota_row = row![
        column![
            text("Search API Quota")
                .size(12)
                .color(Color::from_rgb(0.65, 0.68, 0.8)),
            text(format!(
                "{}/{} remaining",
                rate_limits.search_remaining, rate_limits.search_limit
            ))
            .size(14)
            .color(Color::from_rgb(0.95, 0.95, 0.98)),
        ]
        .spacing(2),
        Space::new().width(Length::Fixed(24.0)),
        column![
            text("Core REST API Quota")
                .size(12)
                .color(Color::from_rgb(0.65, 0.68, 0.8)),
            text(format!(
                "{}/{} remaining",
                rate_limits.core_remaining, rate_limits.core_limit
            ))
            .size(14)
            .color(Color::from_rgb(0.95, 0.95, 0.98)),
        ]
        .spacing(2),
    ]
    .align_y(Alignment::Center);

    let status_card = container(
        column![
            row![
                text("Current Connection Status")
                    .size(15)
                    .color(Color::from_rgb(0.95, 0.95, 0.98)),
                Space::new().width(Length::Fill),
                auth_badge,
            ]
            .align_y(Alignment::Center),
            Space::new().height(Length::Fixed(6.0)),
            quota_row,
        ]
        .spacing(6),
    )
    .padding([14, 18])
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(Color::from_rgba(0.1, 0.12, 0.18, 0.7).into()),
        border: iced::Border {
            color: Color::from_rgba(0.25, 0.28, 0.4, 0.4),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });

    // 4. Token Configuration Card
    let input = text_input("Enter Personal Access Token (ghp_...)", &state.token_input)
        .secure(state.is_masked)
        .on_input(on_token_changed)
        .padding(10)
        .size(13)
        .width(Length::Fill);

    let toggle_mask_btn = button(
        text(if state.is_masked {
            "👁️ Show"
        } else {
            "🔒 Hide"
        })
        .size(12),
    )
    .padding([8, 12])
    .on_press(on_toggle_mask);

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
        Some(on_save_token)
    })
    .style(|_theme, status| {
        let bg = match status {
            button::Status::Hovered => Color::from_rgb(0.35, 0.65, 0.95),
            _ => Color::from_rgb(0.25, 0.55, 0.88),
        };
        button::Style {
            background: Some(bg.into()),
            text_color: Color::from_rgb(0.98, 0.98, 0.98),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    let clear_btn = button(text("Clear Token").size(13))
        .padding([8, 16])
        .on_press_maybe(if state.is_busy {
            None
        } else {
            Some(on_clear_token)
        })
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => Color::from_rgba(0.5, 0.2, 0.2, 0.6),
                _ => Color::from_rgba(0.4, 0.15, 0.15, 0.4),
            };
            button::Style {
                background: Some(bg.into()),
                text_color: Color::from_rgb(0.95, 0.65, 0.65),
                border: iced::Border {
                    color: Color::from_rgba(0.7, 0.3, 0.3, 0.5),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }
        });

    let test_btn = button(text("Test Connection").size(13))
        .padding([8, 16])
        .on_press_maybe(if state.is_busy {
            None
        } else {
            Some(on_test_connection)
        })
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => Color::from_rgba(0.3, 0.35, 0.5, 0.4),
                _ => Color::from_rgba(0.2, 0.22, 0.32, 0.3),
            };
            button::Style {
                background: Some(bg.into()),
                text_color: Color::from_rgb(0.85, 0.88, 0.95),
                border: iced::Border {
                    color: Color::from_rgba(0.4, 0.45, 0.6, 0.4),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }
        });

    let actions_row = row![save_btn, clear_btn, test_btn]
        .spacing(10)
        .align_y(Alignment::Center);

    let mut token_card_col = column![
        text("Manage GitHub Token")
            .size(15)
            .color(Color::from_rgb(0.95, 0.95, 0.98)),
        text("Enter a token generated from GitHub: Settings ➔ Developer settings ➔ Personal access tokens.")
            .size(12)
            .color(Color::from_rgb(0.65, 0.68, 0.8)),
        Space::new().height(Length::Fixed(4.0)),
        input_row,
        Space::new().height(Length::Fixed(4.0)),
        actions_row,
    ]
    .spacing(8);

    // 5. Status Notice Feedback
    if let Some((msg, is_error)) = &state.status_notice {
        let (border_col, bg_col, text_col) = if *is_error {
            (
                Color::from_rgb(0.85, 0.3, 0.3),
                Color::from_rgba(0.4, 0.1, 0.12, 0.4),
                Color::from_rgb(0.96, 0.5, 0.5),
            )
        } else {
            (
                Color::from_rgb(0.3, 0.8, 0.4),
                Color::from_rgba(0.1, 0.35, 0.15, 0.4),
                Color::from_rgb(0.5, 0.95, 0.6),
            )
        };

        let notice_box = container(text(msg).size(13).color(text_col))
            .padding([8, 14])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg_col.into()),
                border: iced::Border {
                    color: border_col,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        token_card_col = token_card_col.push(Space::new().height(Length::Fixed(4.0)));
        token_card_col = token_card_col.push(notice_box);
    }

    let token_card = container(token_card_col)
        .padding([14, 18])
        .width(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.1, 0.12, 0.18, 0.7).into()),
            border: iced::Border {
                color: Color::from_rgba(0.25, 0.28, 0.4, 0.4),
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

    let content = column![header, info_card, status_card, token_card]
        .spacing(16)
        .padding([20, 24]);

    scrollable(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

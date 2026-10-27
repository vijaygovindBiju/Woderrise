//! Dual rate limit gauge widget for Core REST and Search API pools.
//!
//! Color tiers:
//! - Green: > 50% quota remaining
//! - Amber: 10% - 50% quota remaining
//! - Red: < 10% quota remaining (or limit == 0)

use iced::widget::{column, container, progress_bar, row, text};
use iced::{Color, Element};

/// Tier classification for GitHub API rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitColorTier {
    Green, // > 50%
    Amber, // 10% - 50%
    Red,   // < 10%
}

impl RateLimitColorTier {
    pub fn color(&self) -> Color {
        match self {
            Self::Green => Color::from_rgb(0.35, 0.85, 0.45),
            Self::Amber => Color::from_rgb(0.95, 0.75, 0.25),
            Self::Red => Color::from_rgb(0.95, 0.35, 0.35),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Green => "Normal",
            Self::Amber => "Moderate",
            Self::Red => "Low Quota",
        }
    }
}

/// State representation for GitHub API quotas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimitGaugeState {
    pub core_remaining: u32,
    pub core_limit: u32,
    pub search_remaining: u32,
    pub search_limit: u32,
}

impl Default for RateLimitGaugeState {
    fn default() -> Self {
        Self {
            core_remaining: 60,
            core_limit: 60,
            search_remaining: 10,
            search_limit: 10,
        }
    }
}

impl RateLimitGaugeState {
    pub fn new(
        core_remaining: u32,
        core_limit: u32,
        search_remaining: u32,
        search_limit: u32,
    ) -> Self {
        Self {
            core_remaining,
            core_limit: core_limit.max(1),
            search_remaining,
            search_limit: search_limit.max(1),
        }
    }

    pub fn core_percentage(&self) -> f32 {
        (self.core_remaining as f32 / self.core_limit as f32).clamp(0.0, 1.0)
    }

    pub fn search_percentage(&self) -> f32 {
        (self.search_remaining as f32 / self.search_limit as f32).clamp(0.0, 1.0)
    }

    pub fn core_tier(&self) -> RateLimitColorTier {
        Self::calc_tier(self.core_remaining, self.core_limit)
    }

    pub fn search_tier(&self) -> RateLimitColorTier {
        Self::calc_tier(self.search_remaining, self.search_limit)
    }

    pub fn calc_tier(remaining: u32, limit: u32) -> RateLimitColorTier {
        if limit == 0 {
            return RateLimitColorTier::Red;
        }
        let pct = remaining as f64 / limit as f64;
        if pct > 0.50 {
            RateLimitColorTier::Green
        } else if pct >= 0.10 {
            RateLimitColorTier::Amber
        } else {
            RateLimitColorTier::Red
        }
    }

    pub fn is_search_low(&self) -> bool {
        self.search_remaining < 2
    }
}

/// Renders the dual rate limit gauge widget.
pub fn view_gauge<'a, Message: 'a>(state: &'a RateLimitGaugeState) -> Element<'a, Message> {
    let core_tier = state.core_tier();
    let search_tier = state.search_tier();

    let core_widget = column![
        row![
            text("Core API:")
                .size(11)
                .color(Color::from_rgb(0.7, 0.7, 0.7)),
            text(format!("{}/{}", state.core_remaining, state.core_limit))
                .size(11)
                .color(core_tier.color()),
        ]
        .spacing(4),
        progress_bar(0.0..=1.0, state.core_percentage())
            .girth(4)
            .style(move |_theme| progress_bar::Style {
                background: Color::from_rgba(0.2, 0.2, 0.3, 0.5).into(),
                bar: core_tier.color().into(),
                border: iced::border::rounded(2),
            }),
    ]
    .spacing(2)
    .width(90);

    let search_widget = column![
        row![
            text("Search API:")
                .size(11)
                .color(Color::from_rgb(0.7, 0.7, 0.7)),
            text(format!("{}/{}", state.search_remaining, state.search_limit))
                .size(11)
                .color(search_tier.color()),
        ]
        .spacing(4),
        progress_bar(0.0..=1.0, state.search_percentage())
            .girth(4)
            .style(move |_theme| progress_bar::Style {
                background: Color::from_rgba(0.2, 0.2, 0.3, 0.5).into(),
                bar: search_tier.color().into(),
                border: iced::border::rounded(2),
            }),
    ]
    .spacing(2)
    .width(90);

    let mut gauge_row = row![core_widget, search_widget].spacing(12);

    if state.is_search_low() {
        let warning = container(
            text("⚠️ Throttled")
                .size(10)
                .color(Color::from_rgb(0.95, 0.4, 0.4)),
        )
        .padding([2, 6])
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.6, 0.1, 0.1, 0.2).into()),
            border: iced::Border {
                color: Color::from_rgb(0.8, 0.3, 0.3),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        gauge_row = gauge_row.push(warning);
    }

    container(gauge_row)
        .padding([4, 8])
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba(0.12, 0.12, 0.18, 0.8).into()),
            border: iced::Border {
                color: Color::from_rgba(0.3, 0.3, 0.4, 0.5),
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

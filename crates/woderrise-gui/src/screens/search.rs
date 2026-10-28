//! Search and catalog discovery screen.
//!
//! Provides two-tier search (instant local filtering + debounced remote GitHub query),
//! category filter pills, responsive card grid layout, and API rate limit monitoring.

use iced::widget::{
    button, column, container, responsive, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Color, Element, Length};
use woderrise_core::models::{CatalogRepository, ConfigComponent};

pub use crate::widgets::gauge::{RateLimitColorTier, RateLimitGaugeState};

/// Available category filter pills for dotfile components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CategoryFilter {
    #[default]
    All,
    Hyprland,
    Waybar,
    Hyprlock,
    Hypridle,
    Wallpapers,
    Gtk,
}

impl CategoryFilter {
    pub const ALL: [CategoryFilter; 7] = [
        Self::All,
        Self::Hyprland,
        Self::Waybar,
        Self::Hyprlock,
        Self::Hypridle,
        Self::Wallpapers,
        Self::Gtk,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Hyprland => "Hyprland",
            Self::Waybar => "Waybar",
            Self::Hyprlock => "Hyprlock",
            Self::Hypridle => "Hypridle",
            Self::Wallpapers => "Wallpapers",
            Self::Gtk => "GTK",
        }
    }

    pub fn matches(&self, repo: &CatalogRepository) -> bool {
        match self {
            Self::All => true,
            Self::Hyprland => {
                if !repo.detected_components.is_empty() {
                    repo.detected_components.iter().any(|c| {
                        matches!(
                            c.component,
                            ConfigComponent::Hyprland
                                | ConfigComponent::HyprlandLegacy
                                | ConfigComponent::HyprlandLua
                        )
                    })
                } else {
                    let text = format!(
                        "{} {} {}",
                        repo.resolved_full_name(),
                        repo.description.as_deref().unwrap_or(""),
                        repo.topics.join(" ")
                    )
                    .to_lowercase();
                    text.contains("hypr")
                }
            }
            Self::Waybar => {
                if !repo.detected_components.is_empty() {
                    repo.detected_components
                        .iter()
                        .any(|c| c.component == ConfigComponent::Waybar)
                } else {
                    let text = format!(
                        "{} {} {}",
                        repo.resolved_full_name(),
                        repo.description.as_deref().unwrap_or(""),
                        repo.topics.join(" ")
                    )
                    .to_lowercase();
                    text.contains("waybar")
                }
            }
            Self::Hyprlock => {
                if !repo.detected_components.is_empty() {
                    repo.detected_components
                        .iter()
                        .any(|c| c.component == ConfigComponent::Hyprlock)
                } else {
                    let text = format!(
                        "{} {} {}",
                        repo.resolved_full_name(),
                        repo.description.as_deref().unwrap_or(""),
                        repo.topics.join(" ")
                    )
                    .to_lowercase();
                    text.contains("lock")
                }
            }
            Self::Hypridle => {
                if !repo.detected_components.is_empty() {
                    repo.detected_components
                        .iter()
                        .any(|c| c.component == ConfigComponent::Hypridle)
                } else {
                    let text = format!(
                        "{} {} {}",
                        repo.resolved_full_name(),
                        repo.description.as_deref().unwrap_or(""),
                        repo.topics.join(" ")
                    )
                    .to_lowercase();
                    text.contains("idle")
                }
            }
            Self::Wallpapers => {
                if !repo.detected_components.is_empty() {
                    repo.detected_components
                        .iter()
                        .any(|c| c.component == ConfigComponent::Wallpapers)
                } else {
                    let text = format!(
                        "{} {} {}",
                        repo.resolved_full_name(),
                        repo.description.as_deref().unwrap_or(""),
                        repo.topics.join(" ")
                    )
                    .to_lowercase();
                    text.contains("wallpaper")
                        || text.contains("swww")
                        || text.contains("hyprpaper")
                }
            }
            Self::Gtk => {
                if !repo.detected_components.is_empty() {
                    repo.detected_components
                        .iter()
                        .any(|c| c.component == ConfigComponent::GtkTheme)
                } else {
                    let text = format!(
                        "{} {} {}",
                        repo.resolved_full_name(),
                        repo.description.as_deref().unwrap_or(""),
                        repo.topics.join(" ")
                    )
                    .to_lowercase();
                    text.contains("gtk") || text.contains("theme")
                }
            }
        }
    }
}

/// State for the Search / Catalog screen.
#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub active_filter: CategoryFilter,
    pub catalog_repositories: Vec<CatalogRepository>,
    pub filtered_repositories: Vec<CatalogRepository>,
    pub is_searching: bool,
    pub rate_limits: RateLimitGaugeState,
    pub last_input_time_ms: u64,
    pub debounce_pending: bool,
}

impl SearchState {
    pub fn new(catalog_repositories: Vec<CatalogRepository>) -> Self {
        let mut s = Self {
            query: String::new(),
            active_filter: CategoryFilter::All,
            catalog_repositories,
            filtered_repositories: Vec::new(),
            is_searching: false,
            rate_limits: RateLimitGaugeState::new(60, 60, 10, 10),
            last_input_time_ms: 0,
            debounce_pending: false,
        };
        s.apply_filter();
        s
    }

    pub fn set_query(&mut self, query: String, current_time_ms: u64) {
        self.query = query;
        self.last_input_time_ms = current_time_ms;
        self.debounce_pending = true;
        self.apply_filter();
    }

    pub fn set_filter(&mut self, filter: CategoryFilter) {
        self.active_filter = filter;
        self.apply_filter();
    }

    pub fn set_rate_limits(
        &mut self,
        core_rem: u32,
        core_lim: u32,
        search_rem: u32,
        search_lim: u32,
    ) {
        self.rate_limits = RateLimitGaugeState::new(core_rem, core_lim, search_rem, search_lim);
    }

    pub fn apply_filter(&mut self) {
        let q = self.query.to_lowercase().trim().to_string();
        self.filtered_repositories = self
            .catalog_repositories
            .iter()
            .filter(|repo| {
                if !self.active_filter.matches(repo) {
                    return false;
                }
                if q.is_empty() {
                    return true;
                }
                repo.resolved_full_name().to_lowercase().contains(&q)
                    || repo
                        .description
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || repo.topics.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
    }

    pub fn should_fire_debounce(&self, current_time_ms: u64, debounce_window_ms: u64) -> bool {
        self.debounce_pending
            && (current_time_ms.saturating_sub(self.last_input_time_ms) >= debounce_window_ms)
    }

    pub fn clear_debounce(&mut self) {
        self.debounce_pending = false;
    }
}

/// Renders the search screen view.
pub fn view_search<'a, Message: Clone + 'a>(
    state: &'a SearchState,
    on_input_changed: impl Fn(String) -> Message + 'a,
    on_submit: Message,
    on_filter: impl Fn(CategoryFilter) -> Message + 'a,
    on_inspect: impl Fn(CatalogRepository) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    // 1. Search bar with text input and submit button
    let input = text_input("Search dotfiles, authors, components...", &state.query)
        .on_input(on_input_changed)
        .on_submit(on_submit.clone())
        .padding(10)
        .size(14)
        .width(Length::Fill);

    let search_label = if state.is_searching {
        "Searching..."
    } else {
        "Search GitHub"
    };

    let search_button = button(text(search_label).size(13))
        .padding([8, 16])
        .on_press(on_submit);

    let search_bar = row![input, search_button]
        .spacing(10)
        .align_y(Alignment::Center);

    // 2. Category filter pills
    let mut filter_row = row![].spacing(8);
    for filter in CategoryFilter::ALL {
        let is_active = filter == state.active_filter;
        let pill_btn = button(text(filter.label()).size(12))
            .padding([5, 12])
            .on_press(on_filter(filter))
            .style(move |_theme, status| {
                if is_active {
                    button::Style {
                        background: Some(Color::from_rgb(0.54, 0.71, 0.98).into()),
                        text_color: Color::from_rgb(0.08, 0.08, 0.12),
                        border: iced::Border {
                            radius: 14.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                } else {
                    let bg = match status {
                        button::Status::Hovered => Color::from_rgba(0.3, 0.35, 0.5, 0.3),
                        _ => Color::from_rgba(0.2, 0.22, 0.32, 0.4),
                    };
                    button::Style {
                        background: Some(bg.into()),
                        text_color: Color::from_rgb(0.85, 0.88, 0.95),
                        border: iced::Border {
                            color: Color::from_rgba(0.4, 0.45, 0.6, 0.3),
                            width: 1.0,
                            radius: 14.0.into(),
                        },
                        ..Default::default()
                    }
                }
            });

        filter_row = filter_row.push(pill_btn);
    }
    let filter_row = filter_row.wrap();

    // 3. Responsive card grid
    let repos: &'a [CatalogRepository] = &state.filtered_repositories;
    let inspect_fn = on_inspect.clone();
    let grid = responsive(move |size| {
        if repos.is_empty() {
            return container(
                column![
                    text("No configurations found.")
                        .size(16)
                        .color(Color::from_rgb(0.65, 0.68, 0.8)),
                    text("Try adjusting your search query or switching category filters.")
                        .size(13)
                        .color(Color::from_rgb(0.5, 0.52, 0.65)),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(40)
            .align_x(Alignment::Center)
            .into();
        }

        let padding = 16.0;
        let spacing = 14.0;
        let card_min_width = 280.0;
        let available_width = (size.width - padding * 2.0).max(card_min_width);
        let columns = ((available_width + spacing) / (card_min_width + spacing))
            .floor()
            .max(1.0) as usize;

        let mut grid_col = column![].spacing(spacing);

        for chunk in repos.chunks(columns) {
            let mut grid_row = row![].spacing(spacing);
            for repo in chunk {
                let card = crate::widgets::card::view_card(repo, inspect_fn(repo.clone()));
                grid_row = grid_row.push(container(card).width(Length::FillPortion(1)));
            }
            // Fill empty slots in the last row to maintain grid alignment
            let remainder = columns - chunk.len();
            for _ in 0..remainder {
                grid_row = grid_row.push(container(Space::new()).width(Length::FillPortion(1)));
            }
            grid_col = grid_col.push(grid_row);
        }

        scrollable(grid_col).into()
    });

    column![search_bar, filter_row, grid]
        .spacing(16)
        .padding(16)
        .into()
}

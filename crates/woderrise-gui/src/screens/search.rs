//! Search and catalog discovery screen.
//!
//! Provides two-tier search (instant local filtering + debounced remote GitHub query),
//! category filter counts, responsive card grid layout, sorting options, and API rate limit monitoring.

use iced::widget::{
    button, column, container, responsive, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Length, Theme};
use woderrise_core::models::{Catalog, CatalogRepository, ConfigComponent, DetectedComponent};

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

    /// Counts how many repositories match this filter in the given collection.
    pub fn count_in_catalog(&self, repos: &[CatalogRepository]) -> usize {
        repos.iter().filter(|r| self.matches(r)).count()
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

/// Available sorting options for discovered dotfile repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOption {
    #[default]
    Relevance,
    Stars,
    RecentlyUpdated,
    Name,
}

impl SortOption {
    pub const ALL: [SortOption; 4] = [
        Self::Relevance,
        Self::Stars,
        Self::RecentlyUpdated,
        Self::Name,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Relevance => "Relevance",
            Self::Stars => "Stars",
            Self::RecentlyUpdated => "Recently Updated",
            Self::Name => "Name",
        }
    }
}

/// Computes responsive grid column count adapting to available desktop width.
///
/// Returns:
/// - 2 columns for narrow widths (< 960px)
/// - 3 columns for standard desktop widths (960px - 1399px)
/// - 4 columns for wide desktop widths (>= 1400px)
pub fn calculate_grid_columns(width: f32) -> usize {
    if width < 960.0 {
        2
    } else if width < 1400.0 {
        3
    } else {
        4
    }
}

/// State for the Search / Catalog screen.
#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub active_filter: CategoryFilter,
    pub sort_option: SortOption,
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
            sort_option: SortOption::default(),
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

    pub fn set_sort_option(&mut self, sort_option: SortOption) {
        self.sort_option = sort_option;
        self.apply_filter();
    }

    /// Resets the search query, active filter, and sort option, restoring full results.
    pub fn clear(&mut self) {
        self.query.clear();
        self.active_filter = CategoryFilter::All;
        self.sort_option = SortOption::Relevance;
        self.is_searching = false;
        self.debounce_pending = false;
        self.apply_filter();
    }

    /// Counts how many repositories in the given catalog match the currently active filter.
    pub fn count_in_catalog(&self, repos: &[CatalogRepository]) -> usize {
        self.active_filter.count_in_catalog(repos)
    }

    /// Counts how many repositories in this state's catalog match the specified filter.
    pub fn category_count(&self, filter: CategoryFilter) -> usize {
        filter.count_in_catalog(&self.catalog_repositories)
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
        let q = self.query.trim();
        let mut candidate_repos: Vec<CatalogRepository> = self
            .catalog_repositories
            .iter()
            .filter(|repo| self.active_filter.matches(repo))
            .cloned()
            .collect();

        if !q.is_empty() {
            let temp_catalog = Catalog {
                repositories: candidate_repos,
                ..Default::default()
            };
            candidate_repos = woderrise_github::filter_local_catalog(&temp_catalog, q);
        }

        match self.sort_option {
            SortOption::Relevance => {
                if q.is_empty() {
                    candidate_repos.sort_by_key(|r| std::cmp::Reverse(r.stargazers_count));
                }
            }
            SortOption::Stars => {
                candidate_repos.sort_by_key(|r| std::cmp::Reverse(r.stargazers_count));
            }
            SortOption::RecentlyUpdated => {
                candidate_repos.sort_by_key(|r| std::cmp::Reverse(r.pushed_at));
            }
            SortOption::Name => {
                candidate_repos.sort_by(|a, b| {
                    a.resolved_repo()
                        .to_lowercase()
                        .cmp(&b.resolved_repo().to_lowercase())
                });
            }
        }

        self.filtered_repositories = candidate_repos;
    }

    pub fn should_fire_debounce(&self, current_time_ms: u64, debounce_window_ms: u64) -> bool {
        self.debounce_pending
            && (current_time_ms.saturating_sub(self.last_input_time_ms) >= debounce_window_ms)
    }

    pub fn clear_debounce(&mut self) {
        self.debounce_pending = false;
    }

    /// Update detected components for a repository by full name or owner/repo format,
    /// triggering an instant update to filtered repositories.
    pub fn update_repo_components(&mut self, repo_name: &str, components: Vec<DetectedComponent>) {
        let repo_lower = repo_name.to_lowercase();
        let mut updated = false;
        for r in &mut self.catalog_repositories {
            if r.resolved_full_name().to_lowercase() == repo_lower
                || format!("{}/{}", r.owner, r.repo).to_lowercase() == repo_lower
            {
                r.detected_components = components.clone();
                updated = true;
            }
        }
        if updated {
            self.apply_filter();
        }
    }
}

/// Renders the search and discover screen view.
#[allow(clippy::too_many_arguments)]
pub fn view_search<'a, Message: Clone + 'a>(
    state: &'a SearchState,
    theme: &Theme,
    on_input_changed: impl Fn(String) -> Message + 'a,
    on_submit: Message,
    on_clear: Message,
    on_filter: impl Fn(CategoryFilter) -> Message + 'a,
    on_sort: impl Fn(SortOption) -> Message + 'a,
    on_inspect: impl Fn(CatalogRepository) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let tokens = crate::theme::tokens(theme);

    // --- 1. Left Sidebar: Categories & Sort Options ---
    let cat_title = text("CATEGORIES").size(11).color(tokens.text_muted);

    let mut cat_col = column![cat_title].spacing(4);
    for filter in CategoryFilter::ALL {
        let is_active = filter == state.active_filter;
        let count = filter.count_in_catalog(&state.catalog_repositories);
        let label_str = if filter == CategoryFilter::All {
            format!("All Components [{}]", count)
        } else {
            format!("{} [{}]", filter.label(), count)
        };

        let btn = button(
            row![
                text(label_str).size(12).color(if is_active {
                    Color::WHITE
                } else {
                    tokens.text_primary
                }),
                Space::new().width(Length::Fill),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([7, 10])
        .on_press(on_filter(filter))
        .style(move |_theme, status| {
            if is_active {
                crate::theme::button_primary(tokens, status)
            } else {
                match status {
                    button::Status::Hovered => button::Style {
                        background: Some(tokens.bg_card_hover.into()),
                        text_color: tokens.text_primary,
                        border: Border {
                            radius: crate::theme::radius::SM,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    _ => button::Style {
                        background: Some(Color::TRANSPARENT.into()),
                        text_color: tokens.text_secondary,
                        border: Border {
                            radius: crate::theme::radius::SM,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                }
            }
        });

        cat_col = cat_col.push(btn);
    }

    let sort_title = text("SORT BY").size(11).color(tokens.text_muted);

    let mut sort_col = column![sort_title].spacing(4);
    for sort_opt in SortOption::ALL {
        let is_active = sort_opt == state.sort_option;
        let btn = button(
            row![
                text(sort_opt.label()).size(12).color(if is_active {
                    Color::WHITE
                } else {
                    tokens.text_primary
                }),
                Space::new().width(Length::Fill),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([7, 10])
        .on_press(on_sort(sort_opt))
        .style(move |_theme, status| {
            if is_active {
                crate::theme::button_primary(tokens, status)
            } else {
                match status {
                    button::Status::Hovered => button::Style {
                        background: Some(tokens.bg_card_hover.into()),
                        text_color: tokens.text_primary,
                        border: Border {
                            radius: crate::theme::radius::SM,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    _ => button::Style {
                        background: Some(Color::TRANSPARENT.into()),
                        text_color: tokens.text_secondary,
                        border: Border {
                            radius: crate::theme::radius::SM,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                }
            }
        });

        sort_col = sort_col.push(btn);
    }

    let sidebar_content = column![cat_col, Space::new().height(Length::Fixed(16.0)), sort_col,]
        .spacing(12)
        .padding(16);

    let sidebar = container(scrollable(sidebar_content))
        .width(Length::Fixed(240.0))
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_sidebar.into()),
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: crate::theme::radius::MD,
            },
            ..Default::default()
        });

    // --- 2. Right Main Content Area ---
    // Search bar: Text input, clear button ("✕" when query non-empty), and Search GitHub button
    let input = text_input("Search dotfiles, themes, configs...", &state.query)
        .on_input(on_input_changed)
        .on_submit(on_submit.clone())
        .padding(10)
        .size(13)
        .width(Length::Fill)
        .style(move |_theme, status| crate::theme::text_input(tokens, status));

    let mut search_row = row![input].spacing(8).align_y(Alignment::Center);

    if !state.query.is_empty() {
        let clear_btn = button(text("✕").size(13))
            .padding([8, 12])
            .on_press(on_clear.clone())
            .style(move |_theme, status| crate::theme::button_secondary(tokens, status));
        search_row = search_row.push(clear_btn);
    }

    let search_label = if state.is_searching {
        "Searching..."
    } else {
        "Search GitHub"
    };

    let search_btn = button(text(search_label).size(13))
        .padding([8, 16])
        .on_press(on_submit)
        .style(move |_theme, status| crate::theme::button_primary(tokens, status));

    let search_bar = search_row.push(search_btn);

    let rate_warning_opt = if state.rate_limits.is_search_low() {
        let msg = format!(
            "⚠️ GitHub Search rate limit low ({} of {} searches remaining).",
            state.rate_limits.search_remaining, state.rate_limits.search_limit
        );
        Some(
            container(row![text(msg).size(12).color(tokens.warning)].align_y(Alignment::Center))
                .padding([8, 12])
                .width(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(
                        Color::from_rgba(
                            tokens.warning.r,
                            tokens.warning.g,
                            tokens.warning.b,
                            0.12,
                        )
                        .into(),
                    ),
                    border: Border {
                        color: tokens.warning,
                        width: 1.0,
                        radius: crate::theme::radius::SM,
                    },
                    ..Default::default()
                }),
        )
    } else {
        None
    };

    // Content view: Loading, Empty, or Responsive Grid
    let repos: &'a [CatalogRepository] = &state.filtered_repositories;
    let inspect_fn = on_inspect.clone();
    let active_theme = theme.clone();

    let content_area: Element<'a, Message> = if state.is_searching {
        container(
            column![
                text("⟳").size(32).color(tokens.accent_primary),
                text("Searching GitHub...")
                    .size(16)
                    .color(tokens.text_primary),
                text("Looking for matching repositories and components...")
                    .size(12)
                    .color(tokens.text_muted),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .padding(40)
        .into()
    } else if repos.is_empty() {
        container(
            column![
                text("🔍").size(36),
                text("No configurations found")
                    .size(16)
                    .color(tokens.text_primary),
                text("Try adjusting your search query, clearing filters, or searching GitHub.")
                    .size(12)
                    .color(tokens.text_muted),
                button(text("Reset Search & Filters").size(12))
                    .padding([8, 16])
                    .on_press(on_clear)
                    .style(move |_theme, status| crate::theme::button_primary(tokens, status)),
            ]
            .spacing(12)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .padding(40)
        .into()
    } else {
        responsive(move |size| {
            let columns = calculate_grid_columns(size.width);
            let spacing = 14.0;
            let mut grid_col = column![].spacing(spacing);

            for chunk in repos.chunks(columns) {
                let mut grid_row = row![].spacing(spacing);
                for repo in chunk {
                    let card = crate::widgets::card::view_card_themed(
                        repo,
                        &active_theme,
                        inspect_fn(repo.clone()),
                    );
                    grid_row = grid_row.push(container(card).width(Length::FillPortion(1)));
                }
                // Fill empty slots in the last row to maintain grid alignment
                let remainder = columns - chunk.len();
                for _ in 0..remainder {
                    grid_row = grid_row.push(
                        container(Space::new())
                            .width(Length::FillPortion(1))
                            .height(Length::Fixed(280.0)),
                    );
                }
                grid_col = grid_col.push(grid_row);
            }

            scrollable(grid_col).into()
        })
        .into()
    };

    let mut main_col = column![search_bar].spacing(14);
    if let Some(warning) = rate_warning_opt {
        main_col = main_col.push(warning);
    }
    main_col = main_col.push(content_area);

    let main_content = container(main_col)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(16);

    row![sidebar, main_content]
        .spacing(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

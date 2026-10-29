//! Responsive 5-tab Inspect and Installation Detail view.
//!
//! Provides granular component selection, individual file-level selection,
//! hardware display preservation invariants, pre-flight installation planning
//! with conflict status detection, Myers diff preview, and explicit confirmation dialog.

use iced::widget::{
    button, checkbox, column, container, markdown, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Border, Color, Element, Length, Theme};
use std::collections::HashSet;

use woderrise_core::models::{
    CatalogRepository, ConfigComponent, ConflictStatus, DetectedComponent, HardwareConflictNotice,
    TargetMapping,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{ComponentDetection, ProceduralDirective, TreeEntry};

use crate::theme;
use crate::widgets::card::{format_relative_time, format_star_count};
use crate::widgets::diff::{view_diff_themed, FileDiffReport};
use crate::widgets::tree::{format_tree_size, view_tree};

/// 5 organized tabs for repository detail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectTab {
    #[default]
    Overview,
    Components,
    FileTree,
    Readme,
    DiffAndInstall,
}

impl InspectTab {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Components,
        Self::FileTree,
        Self::Readme,
        Self::DiffAndInstall,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Components => "Components",
            Self::FileTree => "File Tree",
            Self::Readme => "README",
            Self::DiffAndInstall => "Diff & Install",
        }
    }
}

/// User interactions originating from the inspect screen.
#[derive(Debug, Clone)]
pub enum InspectAction {
    Back,
    SelectTab(InspectTab),
    ToggleComponent(ConfigComponent),
    ToggleFile {
        path: String,
        component: ConfigComponent,
    },
    SelectAllComponents,
    DeselectAllComponents,
    ToggleExpandComponent(ConfigComponent),
    TreeFilterChanged(String),
    InspectFile(String),
    OpenConfirmDialog,
    CloseConfirmDialog,
    ToggleDirective(usize),
    ConfirmInstall,
    OpenExternalUrl(String),
}

/// State for the Inspect screen.
#[derive(Debug, Clone)]
pub struct InspectState {
    pub repository: CatalogRepository,
    pub detected_components: Vec<DetectedComponent>,
    pub selected_components: HashSet<ConfigComponent>,
    pub deselected_files: HashSet<String>,
    pub full_tree_entries: Vec<TreeEntry>,
    pub raw_detections: Vec<ComponentDetection>,
    pub procedural_directives: Vec<ProceduralDirective>,
    pub hardware_notices: Vec<HardwareConflictNotice>,
    pub active_tab: InspectTab,
    pub markdown_source: Option<String>,
    pub parsed_markdown: Vec<markdown::Item>,
    pub preview_file: Option<String>,
    pub preview_content: Option<String>,
    pub is_fetching_file: bool,
    pub is_loading: bool,
    pub diff_reports: Vec<FileDiffReport>,
    pub show_confirm_dialog: bool,
    pub tree_filter: String,
    pub expanded_components: HashSet<ConfigComponent>,
    pub approved_directives: HashSet<usize>,
}

impl InspectState {
    pub fn new(
        repository: CatalogRepository,
        directives: Vec<ProceduralDirective>,
        hardware_notices: Vec<HardwareConflictNotice>,
    ) -> Self {
        // Invariant: ConfigComponent::Monitors must NEVER be auto-selected by default!
        let mut selected = HashSet::new();
        let mut expanded = HashSet::new();

        for comp in &repository.detected_components {
            if comp.component != ConfigComponent::Monitors {
                selected.insert(comp.component);
            }
            expanded.insert(comp.component);
        }

        Self {
            detected_components: repository.detected_components.clone(),
            repository,
            selected_components: selected,
            deselected_files: HashSet::new(),
            full_tree_entries: Vec::new(),
            raw_detections: Vec::new(),
            procedural_directives: directives,
            hardware_notices,
            active_tab: InspectTab::Overview,
            markdown_source: None,
            parsed_markdown: Vec::new(),
            preview_file: None,
            preview_content: None,
            is_fetching_file: false,
            is_loading: false,
            diff_reports: Vec::new(),
            show_confirm_dialog: false,
            tree_filter: String::new(),
            expanded_components: expanded,
            approved_directives: HashSet::new(),
        }
    }

    pub fn set_markdown(&mut self, src: String) {
        self.parsed_markdown = markdown::parse(&src).collect();
        self.markdown_source = Some(src);
    }

    pub fn toggle_component(&mut self, component: ConfigComponent) {
        if self.selected_components.contains(&component) {
            self.selected_components.remove(&component);
        } else {
            self.selected_components.insert(component);
        }
    }

    pub fn select_all_components(&mut self) {
        for comp in &self.detected_components {
            // Preserve hardware monitor protection invariant
            if comp.component != ConfigComponent::Monitors {
                self.selected_components.insert(comp.component);
            }
        }
        for d in &self.raw_detections {
            if d.component != ConfigComponent::Monitors {
                self.selected_components.insert(d.component);
            }
        }
        self.deselected_files.clear();
    }

    pub fn deselect_all_components(&mut self) {
        self.selected_components.clear();
    }

    pub fn is_file_selected(&self, path: &str, component: ConfigComponent) -> bool {
        self.selected_components.contains(&component) && !self.deselected_files.contains(path)
    }

    pub fn toggle_file(&mut self, path: &str, component: ConfigComponent) {
        if self.is_file_selected(path, component) {
            self.deselected_files.insert(path.to_string());
        } else {
            self.deselected_files.remove(path);
            self.selected_components.insert(component);
        }
    }

    pub fn toggle_expand_component(&mut self, component: ConfigComponent) {
        if self.expanded_components.contains(&component) {
            self.expanded_components.remove(&component);
        } else {
            self.expanded_components.insert(component);
        }
    }

    pub fn toggle_directive_approval(&mut self, index: usize) {
        if self.approved_directives.contains(&index) {
            self.approved_directives.remove(&index);
        } else {
            self.approved_directives.insert(index);
        }
    }

    pub fn open_confirm_dialog(&mut self) {
        self.show_confirm_dialog = true;
    }

    pub fn close_confirm_dialog(&mut self) {
        self.show_confirm_dialog = false;
    }

    pub fn is_install_allowed(&self) -> bool {
        !self.selected_components.is_empty() && self.selected_file_count() > 0 && !self.is_loading
    }

    pub fn selected_file_count(&self) -> usize {
        let mut count = 0;
        let mut counted_components = HashSet::new();

        for d in &self.raw_detections {
            if self.selected_components.contains(&d.component) {
                counted_components.insert(d.component);
                if !d.matched_entries.is_empty() {
                    for entry in &d.matched_entries {
                        if !self.deselected_files.contains(&entry.path) {
                            count += 1;
                        }
                    }
                } else {
                    if !self.deselected_files.contains(&d.entrypoint) {
                        count += 1;
                    }
                    if let Some(style) = &d.style_file {
                        if !self.deselected_files.contains(style) {
                            count += 1;
                        }
                    }
                }
            }
        }

        for comp in &self.detected_components {
            if self.selected_components.contains(&comp.component)
                && !counted_components.contains(&comp.component)
            {
                let deselected_for_comp = self
                    .deselected_files
                    .iter()
                    .filter(|p| p.starts_with(&comp.root_path) || *p == &comp.entrypoint)
                    .count();
                count += comp.file_count.saturating_sub(deselected_for_comp);
            }
        }

        count
    }

    pub fn selected_total_bytes(&self) -> u64 {
        let mut bytes = 0;
        let mut counted_components = HashSet::new();

        for d in &self.raw_detections {
            if self.selected_components.contains(&d.component) {
                counted_components.insert(d.component);
                if !d.matched_entries.is_empty() {
                    for entry in &d.matched_entries {
                        if !self.deselected_files.contains(&entry.path) {
                            bytes += entry.size.unwrap_or(0);
                        }
                    }
                } else {
                    bytes += d.total_size_bytes;
                }
            }
        }

        for comp in &self.detected_components {
            if self.selected_components.contains(&comp.component)
                && !counted_components.contains(&comp.component)
            {
                bytes += comp.total_size_bytes;
            }
        }

        bytes
    }

    /// Resolves target destinations and conflict status for all selected files using `XdgPaths`.
    pub fn resolve_target_mappings_with_xdg(&self, xdg: &XdgPaths) -> Vec<TargetMapping> {
        let mut mappings = Vec::new();

        for d in &self.raw_detections {
            if !self.selected_components.contains(&d.component) {
                continue;
            }
            if !d.matched_entries.is_empty() {
                for entry in &d.matched_entries {
                    if self.deselected_files.contains(&entry.path) {
                        continue;
                    }
                    if let Ok(m) =
                        woderrise_transaction::SecurityAuditor::audit_entry(entry, d.component, xdg)
                    {
                        mappings.push(m);
                    }
                }
            } else {
                let entry = TreeEntry::new(
                    d.entrypoint.clone(),
                    "100644",
                    "blob",
                    "0000000000000000000000000000000000000000",
                    Some(d.total_size_bytes),
                );
                if !self.deselected_files.contains(&entry.path) {
                    if let Ok(m) = woderrise_transaction::SecurityAuditor::audit_entry(
                        &entry,
                        d.component,
                        xdg,
                    ) {
                        mappings.push(m);
                    }
                }
            }
        }

        if mappings.is_empty() {
            for comp in &self.detected_components {
                if !self.selected_components.contains(&comp.component) {
                    continue;
                }
                let entry = TreeEntry::new(
                    comp.entrypoint.clone(),
                    "100644",
                    "blob",
                    "0000000000000000000000000000000000000000",
                    Some(comp.total_size_bytes),
                );
                if !self.deselected_files.contains(&entry.path) {
                    if let Ok(m) = woderrise_transaction::SecurityAuditor::audit_entry(
                        &entry,
                        comp.component,
                        xdg,
                    ) {
                        mappings.push(m);
                    }
                }
                if let Some(style) = &comp.style_file {
                    if !self.deselected_files.contains(style) {
                        let entry = TreeEntry::new(
                            style.clone(),
                            "100644",
                            "blob",
                            "0000000000000000000000000000000000000000",
                            None,
                        );
                        if let Ok(m) = woderrise_transaction::SecurityAuditor::audit_entry(
                            &entry,
                            comp.component,
                            xdg,
                        ) {
                            mappings.push(m);
                        }
                    }
                }
            }
        }

        mappings
    }

    /// Resolves target destinations using the default environment XDG paths.
    pub fn resolve_target_mappings(&self) -> Vec<TargetMapping> {
        if let Ok(xdg) = XdgPaths::from_env() {
            self.resolve_target_mappings_with_xdg(&xdg)
        } else {
            Vec::new()
        }
    }
}

/// Renders the inspect view screen.
pub fn view_inspect<'a, Message: Clone + 'a>(
    state: &'a InspectState,
    theme: &'a Theme,
    on_action: impl Fn(InspectAction) -> Message + Clone + 'a,
) -> Element<'a, Message> {
    let tokens = theme::tokens(theme);

    // =========================================================================
    // 1. Full-Width Responsive Header Card
    // =========================================================================
    let back_btn = button(
        row![
            text("←").size(13),
            text("Back to Results").size(12).color(tokens.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .on_press(on_action(InspectAction::Back))
    .style(move |_theme, status| theme::button_secondary(tokens, status));

    let github_url = state.repository.html_url.clone();
    let on_gh = on_action.clone();
    let view_gh_btn = button(
        row![
            text("View on GitHub").size(12).color(tokens.text_primary),
            text("↗").size(12).color(tokens.text_muted),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .on_press(on_gh(InspectAction::OpenExternalUrl(github_url)))
    .style(move |_theme, status| theme::button_secondary(tokens, status));

    let top_nav_row =
        row![back_btn, Space::new().width(Length::Fill), view_gh_btn].align_y(Alignment::Center);

    let title_text = format!("@{} / {}", state.repository.owner, state.repository.repo);
    let title = text(title_text).size(20).color(tokens.text_primary);

    let star_label = format_star_count(state.repository.stargazers_count);
    let star_pill = container(text(star_label).size(12).color(tokens.star_gold))
        .padding([3, 10])
        .style(move |_theme| container::Style {
            background: Some(
                Color::from_rgba(
                    tokens.star_gold.r,
                    tokens.star_gold.g,
                    tokens.star_gold.b,
                    0.12,
                )
                .into(),
            ),
            border: Border {
                color: Color::from_rgba(
                    tokens.star_gold.r,
                    tokens.star_gold.g,
                    tokens.star_gold.b,
                    0.35,
                ),
                width: 1.0,
                radius: theme::radius::PILL,
            },
            ..Default::default()
        });

    let branch_pill = container(
        text(format!("Branch: {}", state.repository.default_branch))
            .size(11)
            .color(tokens.text_secondary),
    )
    .padding([3, 8])
    .style(move |_theme| container::Style {
        background: Some(tokens.bg_surface.into()),
        border: Border {
            color: tokens.border_subtle,
            width: 1.0,
            radius: theme::radius::PILL,
        },
        ..Default::default()
    });

    let updated_str = format_relative_time(state.repository.pushed_at);
    let updated_text = text(updated_str).size(12).color(tokens.text_muted);

    let title_row = row![title, star_pill, branch_pill, updated_text]
        .spacing(12)
        .align_y(Alignment::Center);

    let desc_str = state
        .repository
        .description
        .as_deref()
        .unwrap_or("No description provided for this repository.");
    let description = text(desc_str).size(13).color(tokens.text_secondary);

    let mut badge_row = row![].spacing(6).align_y(Alignment::Center);
    for comp in &state.detected_components {
        let comp_name = format!("{:?}", comp.component);
        let pill = container(text(comp_name).size(11).color(tokens.accent_primary))
            .padding([2, 8])
            .style(move |_theme| theme::badge(tokens.accent_subtle, tokens.accent_primary));
        badge_row = badge_row.push(pill);
    }

    let security_notice = text("Remote content • No scripts executed • Displays protected")
        .size(11)
        .color(tokens.text_muted);

    let badges_and_notice_row = row![badge_row, Space::new().width(Length::Fill), security_notice,]
        .align_y(Alignment::Center);

    let header_card =
        container(column![top_nav_row, title_row, description, badges_and_notice_row].spacing(10))
            .padding([14, 18])
            .width(Length::Fill)
            .style(move |_theme| theme::container_surface(tokens));

    // =========================================================================
    // 2. Full-Width Horizontal Tab Bar
    // =========================================================================
    let mut tab_items = row![].spacing(6);
    let sel_files_count = state.selected_file_count();

    for tab in InspectTab::ALL {
        let is_active = state.active_tab == tab;
        let on_tab = on_action.clone();

        let label = match tab {
            InspectTab::Overview => "Overview".to_string(),
            InspectTab::Components => {
                format!("Components ({})", state.detected_components.len())
            }
            InspectTab::FileTree => {
                let tree_len = if !state.full_tree_entries.is_empty() {
                    state.full_tree_entries.len()
                } else {
                    state
                        .raw_detections
                        .iter()
                        .map(|d| d.matched_entries.len())
                        .sum()
                };
                format!("File Tree ({})", tree_len)
            }
            InspectTab::Readme => "README".to_string(),
            InspectTab::DiffAndInstall => {
                format!("Diff & Install ({})", sel_files_count)
            }
        };

        let tab_btn = button(text(label).size(13))
            .padding([7, 16])
            .on_press(on_tab(InspectAction::SelectTab(tab)))
            .style(move |_theme, status| theme::button_tab(tokens, is_active, status));

        tab_items = tab_items.push(tab_btn);
    }

    let tab_bar = container(tab_items)
        .padding([4, 8])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_base.into()),
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: theme::radius::PILL,
            },
            ..Default::default()
        });

    // =========================================================================
    // 3. Tab Body Views
    // =========================================================================
    let tab_content: Element<'a, Message> = match state.active_tab {
        // ---------------------------------------------------------------------
        // TAB 1: OVERVIEW
        // ---------------------------------------------------------------------
        InspectTab::Overview => {
            // Left Column: Environment & Architecture Summary
            let env_title = text("Target Environment & Architecture")
                .size(15)
                .color(tokens.text_primary);

            let env_desc = text(
                "This repository provides desktop configuration files for the Wayland ecosystem. \
                Configurations are validated against strict path-containment policies before installation.",
            )
            .size(12)
            .color(tokens.text_secondary);

            let stats_row = row![
                column![
                    text("Detected Components")
                        .size(11)
                        .color(tokens.text_muted),
                    text(format!("{}", state.detected_components.len()))
                        .size(15)
                        .color(tokens.accent_primary),
                ]
                .spacing(2),
                column![
                    text("Total Files").size(11).color(tokens.text_muted),
                    text(format!(
                        "{}",
                        state
                            .detected_components
                            .iter()
                            .map(|c| c.file_count)
                            .sum::<usize>()
                    ))
                    .size(15)
                    .color(tokens.accent_primary),
                ]
                .spacing(2),
                column![
                    text("Target Directory").size(11).color(tokens.text_muted),
                    text("~/.config").size(15).color(tokens.text_primary),
                ]
                .spacing(2),
            ]
            .spacing(24);

            let mut left_col = column![env_title, env_desc, stats_row].spacing(14);

            // Hardware Preservation Notice
            let hw_notice_box = container(
                column![
                    row![
                        text("🛡️").size(14),
                        text("Hardware Display Preservation")
                            .size(13)
                            .color(tokens.warning),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    text(
                        "Monitor and workspace configurations (monitors.conf, workspaces.conf) are \
                        excluded from default installation to protect your monitor layout and refresh rates.",
                    )
                    .size(12)
                    .color(tokens.text_secondary),
                ]
                .spacing(6),
            )
            .padding(12)
            .style(move |_theme| container::Style {
                background: Some(Color::from_rgba(tokens.warning.r, tokens.warning.g, tokens.warning.b, 0.1).into()),
                border: Border {
                    color: Color::from_rgba(tokens.warning.r, tokens.warning.g, tokens.warning.b, 0.35),
                    width: 1.0,
                    radius: theme::radius::MD,
                },
                ..Default::default()
            });

            left_col = left_col.push(hw_notice_box);

            // Security Directives Notice
            if !state.procedural_directives.is_empty() {
                let directive_count = state.procedural_directives.len();
                let sec_notice_box = container(
                    column![
                        row![
                            text("⚠️").size(14),
                            text(format!(
                                "Security Review: {} Inline Procedural Directives",
                                directive_count
                            ))
                            .size(13)
                            .color(tokens.danger),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                        text(
                            "Commands (exec, exec-once, on-click) detected in configuration files are \
                            safely neutralized by default. You can review and approve them during installation.",
                        )
                        .size(12)
                        .color(tokens.text_secondary),
                    ]
                    .spacing(6),
                )
                .padding(12)
                .style(move |_theme| container::Style {
                    background: Some(Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.1).into()),
                    border: Border {
                        color: Color::from_rgba(tokens.danger.r, tokens.danger.g, tokens.danger.b, 0.35),
                        width: 1.0,
                        radius: theme::radius::MD,
                    },
                    ..Default::default()
                });
                left_col = left_col.push(sec_notice_box);
            }

            let left_pane = container(left_col)
                .padding(16)
                .width(Length::FillPortion(1))
                .style(move |_theme| theme::container_card(tokens));

            // Right Column: "Detected Components" Card
            let comp_title = text("Detected Components")
                .size(15)
                .color(tokens.text_primary);

            let sel_comps_count = state.selected_components.len();
            let sel_bytes = state.selected_total_bytes();
            let badge_str = format!(
                "{} selected ({} files, {})",
                sel_comps_count,
                sel_files_count,
                format_tree_size(Some(sel_bytes))
            );

            let count_pill = container(text(badge_str).size(11).color(tokens.accent_primary))
                .padding([3, 10])
                .style(move |_theme| theme::badge(tokens.accent_subtle, tokens.accent_primary));

            let comp_header = row![comp_title, Space::new().width(Length::Fill), count_pill]
                .align_y(Alignment::Center);

            let on_all = on_action.clone();
            let on_none = on_action.clone();
            let select_all_btn = button(text("Select All").size(11))
                .padding([4, 10])
                .on_press(on_all(InspectAction::SelectAllComponents))
                .style(move |_theme, status| theme::button_secondary(tokens, status));

            let deselect_all_btn = button(text("Deselect All").size(11))
                .padding([4, 10])
                .on_press(on_none(InspectAction::DeselectAllComponents))
                .style(move |_theme, status| theme::button_secondary(tokens, status));

            let bulk_actions = row![select_all_btn, deselect_all_btn].spacing(8);

            let mut comp_rows = column![].spacing(8);

            for comp in &state.detected_components {
                let is_checked = state.selected_components.contains(&comp.component);
                let is_monitors = comp.component == ConfigComponent::Monitors;
                let c = comp.component;
                let on_toggle = on_action.clone();

                let cb = checkbox(is_checked)
                    .label(format!("{:?}", comp.component))
                    .size(14)
                    .on_toggle(move |_| on_toggle(InspectAction::ToggleComponent(c)));

                let comp_meta = text(format!(
                    "{} files • {}",
                    comp.file_count,
                    format_tree_size(Some(comp.total_size_bytes))
                ))
                .size(12)
                .color(tokens.text_muted);

                let status_pill = if is_monitors {
                    container(text("🛡️ Protected").size(11).color(tokens.warning))
                        .padding([2, 8])
                        .style(move |_theme| {
                            theme::badge(
                                Color::from_rgba(
                                    tokens.warning.r,
                                    tokens.warning.g,
                                    tokens.warning.b,
                                    0.15,
                                ),
                                tokens.warning,
                            )
                        })
                } else {
                    container(text("✓ Valid").size(11).color(tokens.success))
                        .padding([2, 8])
                        .style(move |_theme| {
                            theme::badge(
                                Color::from_rgba(
                                    tokens.success.r,
                                    tokens.success.g,
                                    tokens.success.b,
                                    0.15,
                                ),
                                tokens.success,
                            )
                        })
                };

                let comp_item = container(
                    row![cb, Space::new().width(Length::Fill), comp_meta, status_pill]
                        .spacing(12)
                        .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .style(move |_theme| container::Style {
                    background: Some(tokens.bg_surface.into()),
                    border: Border {
                        color: if is_checked {
                            tokens.border_focus
                        } else {
                            tokens.border_subtle
                        },
                        width: 1.0,
                        radius: theme::radius::SM,
                    },
                    ..Default::default()
                });

                comp_rows = comp_rows.push(comp_item);
            }

            let on_continue = on_action.clone();
            let mut continue_btn = button(
                row![
                    text("Continue to Installation").size(13),
                    text("→").size(14),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([10, 20]);

            if state.is_install_allowed() {
                continue_btn = continue_btn
                    .on_press(on_continue(InspectAction::SelectTab(
                        InspectTab::DiffAndInstall,
                    )))
                    .style(move |_theme, status| theme::button_primary(tokens, status));
            } else {
                continue_btn = continue_btn.style(move |_theme, _status| button::Style {
                    background: Some(tokens.bg_card.into()),
                    text_color: tokens.text_muted,
                    border: Border {
                        color: tokens.border_subtle,
                        width: 1.0,
                        radius: theme::radius::MD,
                    },
                    ..Default::default()
                });
            }

            let right_col = column![
                comp_header,
                bulk_actions,
                comp_rows,
                Space::new().height(Length::Fixed(8.0)),
                continue_btn,
            ]
            .spacing(12);

            let right_pane = container(right_col)
                .padding(16)
                .width(Length::FillPortion(1))
                .style(move |_theme| theme::container_card(tokens));

            row![left_pane, right_pane].spacing(16).into()
        }

        // ---------------------------------------------------------------------
        // TAB 2: COMPONENTS (Collapsible & File-Level Selection)
        // ---------------------------------------------------------------------
        InspectTab::Components => {
            let mut list = column![].spacing(14);

            for comp in &state.detected_components {
                let is_expanded = state.expanded_components.contains(&comp.component);
                let is_comp_checked = state.selected_components.contains(&comp.component);
                let c = comp.component;

                let on_toggle_c = on_action.clone();
                let cb = checkbox(is_comp_checked)
                    .label(format!("{:?}", comp.component))
                    .size(15)
                    .on_toggle(move |_| on_toggle_c(InspectAction::ToggleComponent(c)));

                let on_exp = on_action.clone();
                let chevron = if is_expanded { "▼" } else { "▶" };
                let exp_btn = button(text(chevron).size(12).color(tokens.text_muted))
                    .padding([4, 8])
                    .on_press(on_exp(InspectAction::ToggleExpandComponent(c)))
                    .style(move |_theme, status| theme::button_secondary(tokens, status));

                let comp_meta_str = format!(
                    "{} files • {}",
                    comp.file_count,
                    format_tree_size(Some(comp.total_size_bytes))
                );

                let header_row = row![
                    exp_btn,
                    cb,
                    Space::new().width(Length::Fill),
                    text(comp_meta_str).size(12).color(tokens.text_muted),
                ]
                .spacing(10)
                .align_y(Alignment::Center);

                let mut comp_panel = column![header_row].spacing(10);

                if is_expanded {
                    let mut files_list = column![].spacing(6);

                    // Find matched entries from raw detections
                    let matched: Vec<TreeEntry> = state
                        .raw_detections
                        .iter()
                        .find(|d| d.component == comp.component)
                        .map(|d| d.matched_entries.clone())
                        .unwrap_or_default();

                    let files: Vec<TreeEntry> = if !matched.is_empty() {
                        matched
                    } else {
                        vec![TreeEntry::new(
                            comp.entrypoint.clone(),
                            "100644",
                            "blob",
                            "0000000000000000000000000000000000000000",
                            Some(comp.total_size_bytes),
                        )]
                    };

                    for file in files {
                        let is_file_chk = state.is_file_selected(&file.path, comp.component);
                        let file_path = file.path.clone();
                        let file_path_for_inspect = file.path.clone();
                        let on_file_tog = on_action.clone();
                        let on_insp = on_action.clone();

                        let file_cb = checkbox(is_file_chk)
                            .label(file_path.clone())
                            .size(13)
                            .on_toggle(move |_| {
                                on_file_tog(InspectAction::ToggleFile {
                                    path: file_path.clone(),
                                    component: c,
                                })
                            });

                        let file_meta = text(format!(
                            "Mode: {} • {}",
                            file.mode,
                            format_tree_size(file.size)
                        ))
                        .size(11)
                        .color(tokens.text_muted);

                        let inspect_btn = button(text("Inspect File").size(11))
                            .padding([3, 8])
                            .on_press(on_insp(InspectAction::InspectFile(file_path_for_inspect)))
                            .style(move |_theme, status| theme::button_secondary(tokens, status));

                        let file_row = row![
                            Space::new().width(Length::Fixed(24.0)),
                            file_cb,
                            Space::new().width(Length::Fill),
                            file_meta,
                            inspect_btn,
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center);

                        files_list = files_list.push(file_row);
                    }

                    comp_panel = comp_panel.push(container(files_list).padding([8, 12]).style(
                        move |_theme| container::Style {
                            background: Some(tokens.bg_surface.into()),
                            border: Border {
                                color: tokens.border_subtle,
                                width: 1.0,
                                radius: theme::radius::SM,
                            },
                            ..Default::default()
                        },
                    ));
                }

                let panel_container = container(comp_panel)
                    .padding(14)
                    .width(Length::Fill)
                    .style(move |_theme| theme::container_card(tokens));

                list = list.push(panel_container);
            }

            scrollable(list).width(Length::Fill).into()
        }

        // ---------------------------------------------------------------------
        // TAB 3: FILE TREE & CONTENT INSPECTOR
        // ---------------------------------------------------------------------
        InspectTab::FileTree => {
            let on_filter = on_action.clone();
            let filter_input = text_input("Filter repository files...", &state.tree_filter)
                .on_input(move |s| on_filter(InspectAction::TreeFilterChanged(s)))
                .padding([6, 10])
                .style(move |_theme, status| theme::text_input(tokens, status));

            let on_tree_sel = on_action.clone();
            let tree_view = view_tree(
                &state.full_tree_entries,
                state.preview_file.as_deref(),
                &state.tree_filter,
                theme,
                move |path| on_tree_sel(InspectAction::InspectFile(path)),
            );

            let left_tree_pane = container(column![filter_input, tree_view].spacing(10))
                .padding(12)
                .width(Length::FillPortion(2))
                .style(move |_theme| theme::container_card(tokens));

            // Right Pane: File Content Previewer
            let preview_header_text = state
                .preview_file
                .as_deref()
                .unwrap_or("No file selected for inspection");

            let preview_header = container(
                row![
                    text("📄").size(14),
                    text(preview_header_text)
                        .size(13)
                        .color(tokens.text_primary),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(tokens.bg_surface.into()),
                border: Border {
                    color: tokens.border_subtle,
                    width: 1.0,
                    radius: theme::radius::SM,
                },
                ..Default::default()
            });

            let preview_body: Element<'a, Message> = if state.is_fetching_file {
                container(
                    text("Streaming file blob from Fastly CDN (0 REST quota)...")
                        .size(13)
                        .color(tokens.text_muted),
                )
                .padding(24)
                .into()
            } else if let Some(content) = &state.preview_content {
                let mut lines_col = column![].spacing(2);
                for (i, line) in content.lines().enumerate() {
                    let line_row = row![
                        text(format!("{:>4}", i + 1))
                            .size(11)
                            .color(tokens.text_muted),
                        text(line).size(12).color(tokens.text_primary),
                    ]
                    .spacing(12);

                    lines_col = lines_col.push(line_row);
                }
                scrollable(lines_col.padding(12)).into()
            } else {
                container(
                    text("Select any file on the left tree to inspect its raw content.")
                        .size(13)
                        .color(tokens.text_muted),
                )
                .padding(24)
                .into()
            };

            let right_viewer_pane = container(column![preview_header, preview_body].spacing(8))
                .padding(12)
                .width(Length::FillPortion(3))
                .style(move |_theme| theme::container_card(tokens));

            row![left_tree_pane, right_viewer_pane].spacing(16).into()
        }

        // ---------------------------------------------------------------------
        // TAB 4: README
        // ---------------------------------------------------------------------
        InspectTab::Readme => {
            if !state.parsed_markdown.is_empty() {
                let settings = markdown::Settings::with_style(markdown::Style::from_palette(
                    if tokens == theme::ThemeTokens::dark() {
                        iced::theme::Palette::DARK
                    } else {
                        iced::theme::Palette::LIGHT
                    },
                ));
                let on_act = on_action.clone();
                let md_widget = markdown::view(&state.parsed_markdown, settings)
                    .map(move |_| on_act(InspectAction::Back));

                container(scrollable(container(md_widget).padding(16)))
                    .width(Length::Fill)
                    .style(move |_theme| theme::container_card(tokens))
                    .into()
            } else if state.is_loading {
                container(
                    text("Loading README from repository...")
                        .size(13)
                        .color(tokens.text_muted),
                )
                .padding(32)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .style(move |_theme| theme::container_card(tokens))
                .into()
            } else {
                container(
                    text("No README.md found in this repository.")
                        .size(13)
                        .color(tokens.text_muted),
                )
                .padding(32)
                .width(Length::Fill)
                .center_x(Length::Fill)
                .style(move |_theme| theme::container_card(tokens))
                .into()
            }
        }

        // ---------------------------------------------------------------------
        // TAB 5: DIFF & INSTALLATION PLANNING
        // ---------------------------------------------------------------------
        InspectTab::DiffAndInstall => {
            let mappings = state.resolve_target_mappings();
            let mut new_files = 0;
            let mut overwrite_files = 0;
            let mut symlink_files = 0;
            let mut hw_files = 0;

            for m in &mappings {
                match m.conflict_status {
                    ConflictStatus::NewFile => new_files += 1,
                    ConflictStatus::OverwritingExisting => overwrite_files += 1,
                    ConflictStatus::ExistingSymlink => symlink_files += 1,
                    ConflictStatus::HardwareProtected => hw_files += 1,
                    ConflictStatus::Unmodified => {}
                }
            }

            // Summary Pills Row
            let plan_title = text("Pre-Flight Installation Plan & Conflict Audit")
                .size(16)
                .color(tokens.text_primary);

            let status_pills = row![
                container(
                    text(format!("{} New Files", new_files))
                        .size(11)
                        .color(tokens.success)
                )
                .padding([3, 8])
                .style(move |_theme| theme::badge(
                    Color::from_rgba(tokens.success.r, tokens.success.g, tokens.success.b, 0.15),
                    tokens.success,
                )),
                container(
                    text(format!("{} Overwrites", overwrite_files))
                        .size(11)
                        .color(tokens.warning)
                )
                .padding([3, 8])
                .style(move |_theme| theme::badge(
                    Color::from_rgba(tokens.warning.r, tokens.warning.g, tokens.warning.b, 0.15),
                    tokens.warning,
                )),
                container(
                    text(format!("{} Symlinks", symlink_files))
                        .size(11)
                        .color(tokens.accent_primary)
                )
                .padding([3, 8])
                .style(move |_theme| theme::badge(tokens.accent_subtle, tokens.accent_primary)),
                container(
                    text(format!("{} Hardware Protected", hw_files))
                        .size(11)
                        .color(tokens.text_muted)
                )
                .padding([3, 8])
                .style(move |_theme| theme::badge(tokens.bg_surface, tokens.text_muted)),
            ]
            .spacing(8);

            // Backup & Rollback Plan Notice
            let backup_notice = container(
                column![
                    row![
                        text("🛡️").size(14),
                        text("Guaranteed Pre-Install Backup & LIFO Rollback")
                            .size(13)
                            .color(tokens.accent_primary),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    text(
                        "An exact pre-install snapshot of all overwritten files will be archived to \
                        ~/.local/state/woderrise/backups/<tx_id>/ before any modifications occur. \
                        You can immediately restore your configuration with 1-click rollback from History.",
                    )
                    .size(12)
                    .color(tokens.text_secondary),
                ]
                .spacing(6),
            )
            .padding(12)
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(tokens.bg_surface.into()),
                border: Border {
                    color: tokens.border_subtle,
                    width: 1.0,
                    radius: theme::radius::MD,
                },
                ..Default::default()
            });

            // Destination Mappings Table
            let mut mappings_col = column![].spacing(6);

            for m in &mappings {
                let (badge_text, badge_color, badge_bg) = match m.conflict_status {
                    ConflictStatus::NewFile => (
                        "🟢 New File",
                        tokens.success,
                        Color::from_rgba(
                            tokens.success.r,
                            tokens.success.g,
                            tokens.success.b,
                            0.15,
                        ),
                    ),
                    ConflictStatus::OverwritingExisting => (
                        "🟡 Overwrite",
                        tokens.warning,
                        Color::from_rgba(
                            tokens.warning.r,
                            tokens.warning.g,
                            tokens.warning.b,
                            0.15,
                        ),
                    ),
                    ConflictStatus::ExistingSymlink => {
                        ("🔗 Symlink", tokens.accent_primary, tokens.accent_subtle)
                    }
                    ConflictStatus::HardwareProtected => (
                        "🛡️ Protected",
                        tokens.warning,
                        Color::from_rgba(
                            tokens.warning.r,
                            tokens.warning.g,
                            tokens.warning.b,
                            0.15,
                        ),
                    ),
                    ConflictStatus::Unmodified => {
                        ("⚪ Unmodified", tokens.text_muted, tokens.bg_surface)
                    }
                };

                let row_item = container(
                    row![
                        text(m.relative_path.to_string())
                            .size(12)
                            .color(tokens.text_primary),
                        text("→").size(12).color(tokens.text_muted),
                        text(m.target_destination.to_string())
                            .size(12)
                            .color(tokens.text_secondary),
                        Space::new().width(Length::Fill),
                        container(text(badge_text).size(11).color(badge_color))
                            .padding([2, 8])
                            .style(move |_theme| theme::badge(badge_bg, badge_color)),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .style(move |_theme| container::Style {
                    background: Some(tokens.bg_surface.into()),
                    border: Border {
                        color: tokens.border_subtle,
                        width: 1.0,
                        radius: theme::radius::SM,
                    },
                    ..Default::default()
                });

                mappings_col = mappings_col.push(row_item);
            }

            // Myers Diff Viewer Section
            let mut diffs_col = column![text("Configuration Diff Preview")
                .size(14)
                .color(tokens.text_primary),]
            .spacing(8);

            if !state.diff_reports.is_empty() {
                for report in &state.diff_reports {
                    diffs_col = diffs_col.push(view_diff_themed(report, theme));
                }
            } else if overwrite_files > 0 {
                diffs_col = diffs_col.push(
                    container(
                        text(
                            "Diff preview will be computed against active dotfiles during staging.",
                        )
                        .size(12)
                        .color(tokens.text_muted),
                    )
                    .padding(12),
                );
            } else {
                diffs_col = diffs_col.push(
                    container(
                        text("All selected configurations will be created as new files. No existing dotfiles will be modified.")
                            .size(12)
                            .color(tokens.text_muted),
                    )
                    .padding(12),
                );
            }

            // Primary Call-To-Action Button
            let on_install = on_action.clone();
            let mut install_btn =
                button(text(format!("Install Configuration ({} files)", sel_files_count)).size(14))
                    .padding([12, 24]);

            if state.is_install_allowed() {
                install_btn = install_btn
                    .on_press(on_install(InspectAction::OpenConfirmDialog))
                    .style(move |_theme, status| theme::button_primary(tokens, status));
            } else {
                install_btn = install_btn.style(move |_theme, _status| button::Style {
                    background: Some(tokens.bg_card.into()),
                    text_color: tokens.text_muted,
                    border: Border {
                        color: tokens.border_subtle,
                        width: 1.0,
                        radius: theme::radius::MD,
                    },
                    ..Default::default()
                });
            }

            let planning_card = container(
                column![
                    plan_title,
                    status_pills,
                    backup_notice,
                    text("Target Destination Mappings:")
                        .size(13)
                        .color(tokens.text_primary),
                    mappings_col,
                    Space::new().height(Length::Fixed(8.0)),
                    diffs_col,
                    Space::new().height(Length::Fixed(8.0)),
                    install_btn,
                ]
                .spacing(12),
            )
            .padding(16)
            .width(Length::Fill)
            .style(move |_theme| theme::container_card(tokens));

            scrollable(planning_card).width(Length::Fill).into()
        }
    };

    let main_view = container(
        column![header_card, tab_bar, tab_content]
            .spacing(14)
            .padding(16),
    )
    .width(Length::Fill)
    .height(Length::Fill);

    // =========================================================================
    // 4. Explicit Confirmation Modal Dialog
    // =========================================================================
    if state.show_confirm_dialog {
        let mappings = state.resolve_target_mappings();
        let new_cnt = mappings
            .iter()
            .filter(|m| m.conflict_status == ConflictStatus::NewFile)
            .count();
        let over_cnt = mappings
            .iter()
            .filter(|m| m.conflict_status == ConflictStatus::OverwritingExisting)
            .count();

        let modal_title = text("Confirm Configuration Installation")
            .size(18)
            .color(tokens.text_primary);

        let modal_repo = text(format!(
            "Repository: @{} / {} ({})",
            state.repository.owner, state.repository.repo, state.repository.default_branch
        ))
        .size(13)
        .color(tokens.text_secondary);

        let impact_summary = container(
            column![
                text(format!(
                    "• {} files to write ({})",
                    sel_files_count,
                    format_tree_size(Some(state.selected_total_bytes()))
                ))
                .size(12)
                .color(tokens.text_primary),
                text(format!("• {} fresh files will be created", new_cnt))
                    .size(12)
                    .color(tokens.success),
                text(format!(
                    "• {} existing dotfiles will be backed up & overwritten",
                    over_cnt
                ))
                .size(12)
                .color(if over_cnt > 0 {
                    tokens.warning
                } else {
                    tokens.text_muted
                }),
            ]
            .spacing(4),
        )
        .padding(12)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_surface.into()),
            border: Border {
                color: tokens.border_subtle,
                width: 1.0,
                radius: theme::radius::SM,
            },
            ..Default::default()
        });

        let snapshot_guarantee = container(
            column![
                text("✓ Pre-Install Backup Snapshot Guarantee:")
                    .size(12)
                    .color(tokens.accent_primary),
                text(
                    "All replaced dotfiles are archived before writing. 1-click rollback is guaranteed.",
                )
                .size(11)
                .color(tokens.text_secondary),
            ]
            .spacing(2),
        )
        .padding(10)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(tokens.accent_subtle.into()),
            border: Border {
                color: Color::from_rgba(tokens.accent_primary.r, tokens.accent_primary.g, tokens.accent_primary.b, 0.4),
                width: 1.0,
                radius: theme::radius::SM,
            },
            ..Default::default()
        });

        // Procedural Directives Approvals (if any)
        let mut directives_box = column![].spacing(6);
        if !state.procedural_directives.is_empty() {
            directives_box = directives_box.push(
                text("Procedural Command Approvals (Neutralized by default):")
                    .size(12)
                    .color(tokens.text_primary),
            );
            for (idx, directive) in state.procedural_directives.iter().enumerate() {
                let is_approved = state.approved_directives.contains(&idx);
                let on_dir = on_action.clone();
                let label = format!("{}: {}", directive.directive_type, directive.command);
                let cb = checkbox(is_approved)
                    .label(label)
                    .size(13)
                    .on_toggle(move |_| on_dir(InspectAction::ToggleDirective(idx)));
                directives_box = directives_box.push(cb);
            }
        }

        let on_cancel = on_action.clone();
        let cancel_btn = button(text("Cancel").size(13))
            .padding([8, 16])
            .on_press(on_cancel(InspectAction::CloseConfirmDialog))
            .style(move |_theme, status| theme::button_secondary(tokens, status));

        let on_confirm = on_action.clone();
        let confirm_btn = button(text("Confirm & Apply Installation").size(13))
            .padding([8, 16])
            .on_press(on_confirm(InspectAction::ConfirmInstall))
            .style(move |_theme, status| theme::button_primary(tokens, status));

        let actions_row = row![cancel_btn, Space::new().width(Length::Fill), confirm_btn]
            .align_y(Alignment::Center);

        let modal_box = container(
            column![
                modal_title,
                modal_repo,
                impact_summary,
                snapshot_guarantee,
                directives_box,
                Space::new().height(Length::Fixed(6.0)),
                actions_row,
            ]
            .spacing(12),
        )
        .padding(20)
        .max_width(620)
        .style(move |_theme| container::Style {
            background: Some(tokens.bg_card.into()),
            border: Border {
                color: tokens.border_focus,
                width: 1.5,
                radius: theme::radius::LG,
            },
            ..Default::default()
        });

        let modal_overlay = container(modal_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(Color::from_rgba(0.02, 0.04, 0.08, 0.82).into()),
                ..Default::default()
            });

        iced::widget::stack![main_view, modal_overlay].into()
    } else {
        main_view.into()
    }
}

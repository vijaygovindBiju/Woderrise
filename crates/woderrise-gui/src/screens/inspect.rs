//! Two-column Inspect and Preview screen.
//!
//! Provides granular component selection, hardware preservation protection,
//! procedural directive security review, and tabbed Markdown README & config diff previews.

use iced::widget::{button, checkbox, column, container, markdown, row, scrollable, text};
use iced::{Color, Element, Length};
use std::collections::HashSet;
use woderrise_core::models::{
    CatalogRepository, ConfigComponent, DetectedComponent, HardwareConflictNotice,
};
use woderrise_detect::{ComponentDetection, ProceduralDirective};

use crate::widgets::diff::{view_diff, FileDiffReport};

/// Tabs available in the right preview pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectTab {
    #[default]
    Readme,
    FileTree,
    DiffPreview,
}

/// State for the Inspect screen.
#[derive(Debug, Clone)]
pub struct InspectState {
    pub repository: CatalogRepository,
    pub detected_components: Vec<DetectedComponent>,
    pub selected_components: HashSet<ConfigComponent>,
    pub raw_detections: Vec<ComponentDetection>,
    pub procedural_directives: Vec<ProceduralDirective>,
    pub hardware_notices: Vec<HardwareConflictNotice>,
    pub active_tab: InspectTab,
    pub markdown_source: Option<String>,
    pub parsed_markdown: Vec<markdown::Item>,
    pub preview_file: Option<String>,
    pub preview_content: Option<String>,
    pub is_loading: bool,
    pub diff_reports: Vec<FileDiffReport>,
}

impl InspectState {
    pub fn new(
        repository: CatalogRepository,
        directives: Vec<ProceduralDirective>,
        hardware_notices: Vec<HardwareConflictNotice>,
    ) -> Self {
        let mut selected = HashSet::new();
        for comp in &repository.detected_components {
            if comp.component != ConfigComponent::Monitors {
                selected.insert(comp.component);
            }
        }

        Self {
            detected_components: repository.detected_components.clone(),
            repository,
            selected_components: selected,
            raw_detections: Vec::new(),
            procedural_directives: directives,
            hardware_notices,
            active_tab: InspectTab::Readme,
            markdown_source: None,
            parsed_markdown: Vec::new(),
            preview_file: None,
            preview_content: None,
            is_loading: false,
            diff_reports: Vec::new(),
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

    pub fn is_install_allowed(&self) -> bool {
        !self.selected_components.is_empty() && !self.is_loading
    }

    pub fn selected_file_count(&self) -> usize {
        self.detected_components
            .iter()
            .filter(|c| self.selected_components.contains(&c.component))
            .map(|c| c.file_count)
            .sum()
    }
}

/// Renders the inspect view screen.
pub fn view_inspect<'a, Message: Clone + 'a>(
    state: &'a InspectState,
    on_back: Message,
    on_toggle_component: impl Fn(ConfigComponent) -> Message + Clone + 'a,
    on_select_tab: impl Fn(InspectTab) -> Message + 'a,
    on_install: Message,
) -> Element<'a, Message> {
    // --- LEFT COLUMN: Metadata, Component Selection, Security Review ---
    let back_btn = button(text("← Back to Catalog").size(12))
        .padding([4, 10])
        .on_press(on_back.clone());

    let title = text(state.repository.resolved_full_name())
        .size(18)
        .color(Color::from_rgb(0.9, 0.94, 0.98));

    let stars = text(format!("★ {} Stars", state.repository.stargazers_count))
        .size(13)
        .color(Color::from_rgb(0.95, 0.85, 0.35));

    let author_text = format!("Author: @{}", state.repository.owner);
    let author = text(author_text)
        .size(12)
        .color(Color::from_rgb(0.7, 0.75, 0.85));

    let desc_text = state
        .repository
        .description
        .as_deref()
        .unwrap_or("No description provided.");
    let description = text(desc_text)
        .size(12)
        .color(Color::from_rgb(0.75, 0.78, 0.88));

    let meta_section = column![
        back_btn,
        title,
        row![stars, author].spacing(12),
        description
    ]
    .spacing(8);

    // Component selection checkboxes
    let mut comp_list = column![text("Detected Components:").size(14)].spacing(6);
    for comp in &state.detected_components {
        let is_checked = state.selected_components.contains(&comp.component);
        let label = format!("{:?} ({} files)", comp.component, comp.file_count);
        let toggle = on_toggle_component.clone();
        let c = comp.component;
        let cb = checkbox(is_checked)
            .label(label)
            .on_toggle(move |_| toggle(c))
            .size(14);
        comp_list = comp_list.push(cb);
    }

    // Install CTA button
    let install_label = format!("Install Selected ({})", state.selected_file_count());
    let mut install_btn = button(text(install_label).size(14)).padding([10, 20]);

    if state.is_install_allowed() {
        install_btn = install_btn.on_press(on_install);
    }

    let mut left_col = column![meta_section, comp_list].spacing(16);

    // Hardware preservation protection notice
    if !state.hardware_notices.is_empty() {
        let mut hw_col = column![
            text("🛡️ Hardware Display Preservation:")
                .size(13)
                .color(Color::from_rgb(0.98, 0.75, 0.35)),
            text("Display configurations (monitors.conf, workspaces.conf) are preserved by default to protect monitor setups.")
                .size(11)
                .color(Color::from_rgb(0.85, 0.85, 0.9)),
        ]
        .spacing(4);

        for notice in &state.hardware_notices {
            hw_col = hw_col.push(
                text(format!("• {}", notice.description))
                    .size(11)
                    .color(Color::from_rgb(0.75, 0.75, 0.8)),
            );
        }

        let hw_container = container(hw_col)
            .padding(10)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.25, 0.2, 0.08, 0.4).into()),
                border: iced::Border {
                    color: Color::from_rgba(0.9, 0.65, 0.2, 0.4),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        left_col = left_col.push(hw_container);
    }

    // Security audit directives review panel
    if !state.procedural_directives.is_empty() {
        let count = state.procedural_directives.len();
        let mut sec_col = column![
            text(format!(
                "⚠️ Security Review ({} inline commands detected):",
                count
            ))
            .size(13)
            .color(Color::from_rgb(0.96, 0.58, 0.66)),
            text(
                "Procedural commands (exec, exec-once, on-click) are neutralized by default for safety."
            )
            .size(11)
            .color(Color::from_rgb(0.8, 0.8, 0.85)),
        ]
        .spacing(4);

        for d in state.procedural_directives.iter().take(4) {
            sec_col = sec_col.push(
                text(format!("{}: {}", d.directive_type, d.command))
                    .size(11)
                    .color(Color::from_rgb(0.7, 0.75, 0.85)),
            );
        }

        let sec_container = container(sec_col)
            .padding(10)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.3, 0.1, 0.15, 0.4).into()),
                border: iced::Border {
                    color: Color::from_rgba(0.9, 0.35, 0.45, 0.4),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        left_col = left_col.push(sec_container);
    }

    left_col = left_col.push(install_btn);

    let left_pane = scrollable(left_col.padding(12)).width(Length::FillPortion(2));

    // --- RIGHT COLUMN: Tabbed Viewer (README / FileTree / Diff) ---
    let tab_readme = button(text("README.md").size(12))
        .padding([4, 12])
        .on_press(on_select_tab(InspectTab::Readme));
    let tab_tree = button(text("File Tree").size(12))
        .padding([4, 12])
        .on_press(on_select_tab(InspectTab::FileTree));
    let tab_diff = button(text("Diff Preview").size(12))
        .padding([4, 12])
        .on_press(on_select_tab(InspectTab::DiffPreview));

    let tab_bar = row![tab_readme, tab_tree, tab_diff].spacing(8);

    let right_content: Element<'a, Message> = match state.active_tab {
        InspectTab::Readme => {
            if !state.parsed_markdown.is_empty() {
                let settings = markdown::Settings::with_style(markdown::Style::from_palette(
                    iced::theme::Palette::DARK,
                ));
                let back_msg = on_back.clone();
                scrollable(
                    markdown::view(&state.parsed_markdown, settings).map(move |_| back_msg.clone()),
                )
                .into()
            } else if state.is_loading {
                container(text("Loading README from repository...").size(13))
                    .padding(20)
                    .into()
            } else {
                container(text("No README.md available for this repository.").size(13))
                    .padding(20)
                    .into()
            }
        }
        InspectTab::FileTree => {
            let mut tree_col = column![text("Target Configuration Files:").size(14)].spacing(6);
            for comp in &state.detected_components {
                tree_col = tree_col.push(
                    text(format!(
                        "📂 {:?} -> Entry: {}",
                        comp.component, comp.entrypoint
                    ))
                    .size(12)
                    .color(Color::from_rgb(0.55, 0.75, 0.98)),
                );
            }
            scrollable(tree_col).into()
        }
        InspectTab::DiffPreview => {
            if state.diff_reports.is_empty() {
                container(
                    text("No configuration changes detected or diffs not yet loaded.").size(13),
                )
                .padding(20)
                .into()
            } else {
                let mut diffs_col = column![].spacing(16);
                for report in &state.diff_reports {
                    diffs_col = diffs_col.push(view_diff(report));
                }
                scrollable(diffs_col).into()
            }
        }
    };

    let right_pane = column![tab_bar, right_content]
        .spacing(12)
        .padding(12)
        .width(Length::FillPortion(3));

    row![left_pane, right_pane].spacing(16).padding(12).into()
}

//! 4-Phase installation stepper and security approval modal screen.
//!
//! Visualizes transaction phases:
//! Phase 1: Staging (CDN download, SHA-256 validation)
//! Phase 2: Backup (Pre-install snapshot)
//! Phase 3: Journal WAL (Atomic state commitment)
//! Phase 4: Atomic Swap (EXDEV stream copy, rename, fsync)

use camino::Utf8PathBuf;
use iced::widget::{button, checkbox, column, container, progress_bar, row, scrollable, text};
use iced::{Alignment, Color, Element, Length};
use woderrise_core::models::{CatalogRepository, ConfigComponent, StagedFile};
use woderrise_detect::ProceduralDirective;

/// 4-phase transaction installation phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPhase {
    PendingApproval, // Security review approval
    Staging,         // Phase 1: Download & verify
    Backup,          // Phase 2: Snapshot pre-install
    JournalWal,      // Phase 3: WAL commit Applying
    AtomicSwap,      // Phase 4: EXDEV stream copy & rename
    Completed,       // Success state
    Failed,          // Error state
}

impl InstallPhase {
    pub fn step_number(&self) -> usize {
        match self {
            Self::PendingApproval => 0,
            Self::Staging => 1,
            Self::Backup => 2,
            Self::JournalWal => 3,
            Self::AtomicSwap => 4,
            Self::Completed => 5,
            Self::Failed => 0,
        }
    }
}

/// A procedural command directive approval record.
#[derive(Debug, Clone)]
pub struct DirectiveApproval {
    pub directive: ProceduralDirective,
    pub approved: bool,
}

/// State for the Install modal screen.
#[derive(Debug, Clone)]
pub struct InstallState {
    pub transaction_id: String,
    pub repository: CatalogRepository,
    pub selected_components: Vec<ConfigComponent>,
    pub current_phase: InstallPhase,
    pub progress_ratio: f32,
    pub status_message: String,
    pub directive_approvals: Vec<DirectiveApproval>,
    pub staged_files: Vec<StagedFile>,
    pub installed_files: Vec<Utf8PathBuf>,
    pub error_message: Option<String>,
}

impl InstallState {
    pub fn new(
        transaction_id: String,
        repository: CatalogRepository,
        selected_components: Vec<ConfigComponent>,
        directives: Vec<ProceduralDirective>,
    ) -> Self {
        let directive_approvals = directives
            .into_iter()
            .map(|d| DirectiveApproval {
                directive: d,
                approved: false, // Neutralized by default
            })
            .collect();

        Self {
            transaction_id,
            repository,
            selected_components,
            current_phase: InstallPhase::PendingApproval,
            progress_ratio: 0.0,
            status_message: "Awaiting security review approval".to_string(),
            directive_approvals,
            staged_files: Vec::new(),
            installed_files: Vec::new(),
            error_message: None,
        }
    }

    pub fn toggle_directive_approval(&mut self, index: usize) {
        if let Some(item) = self.directive_approvals.get_mut(index) {
            item.approved = !item.approved;
        }
    }

    pub fn advance_phase(&mut self, phase: InstallPhase, progress: f32, msg: impl Into<String>) {
        self.current_phase = phase;
        self.progress_ratio = progress.clamp(0.0, 1.0);
        self.status_message = msg.into();
    }

    pub fn fail(&mut self, error: impl Into<String>) {
        self.current_phase = InstallPhase::Failed;
        self.error_message = Some(error.into());
    }
}

/// Renders the install screen view.
pub fn view_install<'a, Message: Clone + 'a>(
    state: &'a InstallState,
    on_toggle_approval: impl Fn(usize) -> Message + Clone + 'a,
    on_confirm_install: Message,
    on_cancel: Message,
    on_rollback: Message,
    on_done: Message,
) -> Element<'a, Message> {
    let header = column![
        text("Installing Configuration").size(20),
        text(format!(
            "Repository: {} (Tx: {})",
            state.repository.resolved_full_name(),
            state.transaction_id
        ))
        .size(13)
        .color(Color::from_rgb(0.7, 0.75, 0.85)),
    ]
    .spacing(4);

    // 4-phase transaction stepper indicators
    let step_label =
        |num: usize, name: &'static str, current: usize| -> iced::widget::Row<'static, Message> {
            let (color, icon) = if current > num {
                (Color::from_rgb(0.3, 0.85, 0.4), "✓")
            } else if current == num {
                (Color::from_rgb(0.55, 0.75, 0.98), "●")
            } else {
                (Color::from_rgb(0.5, 0.52, 0.6), "○")
            };

            row![
                text(icon).size(13).color(color),
                text(name).size(12).color(color),
            ]
            .spacing(6)
        };

    let cur_step = state.current_phase.step_number();
    let stepper = row![
        step_label(1, "1. Staging", cur_step),
        text("→").size(12).color(Color::from_rgb(0.4, 0.45, 0.55)),
        step_label(2, "2. Backup", cur_step),
        text("→").size(12).color(Color::from_rgb(0.4, 0.45, 0.55)),
        step_label(3, "3. Journal WAL", cur_step),
        text("→").size(12).color(Color::from_rgb(0.4, 0.45, 0.55)),
        step_label(4, "4. Atomic Swap", cur_step),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let progress = column![
        stepper,
        progress_bar(0.0..=1.0, state.progress_ratio).girth(8),
        text(&state.status_message)
            .size(13)
            .color(Color::from_rgb(0.85, 0.88, 0.95)),
    ]
    .spacing(10);

    // Directive approvals list (if in PendingApproval phase)
    let directive_section: Element<'a, Message> = if state.current_phase
        == InstallPhase::PendingApproval
        && !state.directive_approvals.is_empty()
    {
        let mut approvals_col = column![
            text("Procedural Command Approvals:").size(14),
            text("Select any commands you want to allow. Unapproved commands will be safely neutralized.")
                .size(12)
                .color(Color::from_rgb(0.7, 0.72, 0.8)),
        ]
        .spacing(6);

        for (i, item) in state.directive_approvals.iter().enumerate() {
            let label = format!(
                "{}: {}",
                item.directive.directive_type, item.directive.command
            );
            let toggle = on_toggle_approval.clone();
            let cb = checkbox(item.approved)
                .label(label)
                .on_toggle(move |_| toggle(i))
                .size(13);
            approvals_col = approvals_col.push(cb);
        }

        container(approvals_col)
            .padding(12)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.18, 0.2, 0.28, 0.5).into()),
                border: iced::Border {
                    color: Color::from_rgba(0.4, 0.45, 0.6, 0.3),
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        container(text("").size(0)).into()
    };

    // Actions depending on current phase
    let actions: Element<'a, Message> = match state.current_phase {
        InstallPhase::PendingApproval => {
            let confirm_btn = button(text("Proceed to Install").size(13))
                .padding([8, 16])
                .on_press(on_confirm_install);
            let cancel_btn = button(text("Cancel").size(13))
                .padding([8, 16])
                .on_press(on_cancel);
            row![confirm_btn, cancel_btn].spacing(12).into()
        }
        InstallPhase::Completed => {
            let rollback_btn = button(text("1-Click Rollback").size(13))
                .padding([8, 16])
                .on_press(on_rollback);
            let done_btn = button(text("Done").size(13))
                .padding([8, 16])
                .on_press(on_done);
            row![rollback_btn, done_btn].spacing(12).into()
        }
        InstallPhase::Failed => {
            let cancel_btn = button(text("Dismiss").size(13))
                .padding([8, 16])
                .on_press(on_cancel);
            row![cancel_btn].into()
        }
        _ => text("Transaction in progress, please wait...")
            .size(12)
            .color(Color::from_rgb(0.65, 0.7, 0.8))
            .into(),
    };

    let content = column![header, progress, directive_section, actions,]
        .spacing(18)
        .padding(20)
        .max_width(650);

    container(scrollable(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .padding(24)
        .into()
}

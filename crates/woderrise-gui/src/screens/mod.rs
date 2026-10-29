//! Application screens: Search, Inspect, Install, History.

pub mod history;
pub mod inspect;
pub mod install;
pub mod search;
pub mod settings;

pub use history::{
    view_history, DirtyStateModal, HistoryAction, HistoryComponentFilter, HistoryItem,
    HistoryMetrics, HistoryState, HistoryStatusFilter, RollbackConfirmState,
};
pub use inspect::{view_inspect, InspectAction, InspectState, InspectTab};
pub use install::{view_install, DirectiveApproval, InstallPhase, InstallState};
pub use search::{
    view_search, CategoryFilter, RateLimitColorTier, RateLimitGaugeState, SearchState,
};
pub use settings::{mask_token, view_settings, SettingsState};

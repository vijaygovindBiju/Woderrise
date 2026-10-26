//! Library interface for the Woderrise desktop GUI application.
//! Exposes view models, data adapters, test helpers, and compiled Slint component modules.

pub mod test_helpers;
pub mod view_model;

// Include Slint-generated code from build.rs
slint::include_modules!();

pub use test_helpers::*;
pub use view_model::*;

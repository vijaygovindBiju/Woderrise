//! Woderrise GUI presentation layer.
//!
//! Native desktop GUI application built on Iced v0.14+ for discovering,
//! previewing, installing, and rolling back Wayland configurations.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

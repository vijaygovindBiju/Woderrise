//! Woderrise Native Wayland Desktop GUI Application.
//!
//! Entrypoint initializing logging, Tokio runtime, XDG directories,
//! domain engines, startup crash recovery, and the Iced v0.14+ presentation loop.

fn main() -> iced::Result {
    woderrise_gui::run()
}

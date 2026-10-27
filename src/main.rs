//! Woderrise root binary forwarder.
//!
//! Invokes `woderrise_gui::run()` for launching the native desktop GUI.

fn main() -> iced::Result {
    woderrise_gui::run()
}

//! Headless offscreen testing platform using MinimalSoftwareWindow.

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use std::rc::Rc;

struct HeadlessPlatform;

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer))
    }
}

/// Initializes the offscreen software Slint platform adapter.
/// Safe and idempotent across all test threads. Completely eliminates
/// dependencies on X11 ($DISPLAY), Wayland ($WAYLAND_DISPLAY), OpenGL, or GPU.
pub fn init_headless_slint() {
    let _ = slint::platform::set_platform(Box::new(HeadlessPlatform));
}

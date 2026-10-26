//! Build script compiling Slint UI declarations for woderrise-desktop.

fn main() {
    slint_build::compile("ui/appwindow.slint")
        .expect("Failed to compile Slint UI definitions (ui/appwindow.slint)");
}

//! Woderrise Native Wayland Desktop GUI Application and CLI entrypoint.
//!
//! Handles CLI subcommands (such as `auth` and `config`) and dispatches
//! to the Iced v0.14+ desktop application when launched without subcommands or with `gui`.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if woderrise_gui::cli::is_cli_invocation(&args) {
        woderrise_gui::cli::run_cli(&args)?;
        Ok(())
    } else {
        woderrise_gui::run()?;
        Ok(())
    }
}

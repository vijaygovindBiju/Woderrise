//! CLI subcommand handler for Woderrise.
//!
//! Provides the `auth` (and `config`) subcommand for configuring an optional
//! GitHub Personal Access Token (PAT), verifying quota via GitHub's API,
//! inspecting current rate limit status, and clearing saved credentials.

use std::error::Error;
use woderrise_core::models::{load_settings, save_settings_durably, Settings};
use woderrise_core::paths::XdgPaths;
use woderrise_github::GitHubClientBuilder;

use crate::screens::settings::mask_token;

/// Checks if the provided command line arguments represent a CLI command or flag.
pub fn is_cli_invocation(args: &[String]) -> bool {
    if args.len() <= 1 {
        return false;
    }
    for arg in &args[1..] {
        if matches!(
            arg.as_str(),
            "auth" | "config" | "--help" | "-h" | "help" | "--version" | "-V" | "version"
        ) {
            return true;
        }
    }
    false
}

/// Executes the CLI command and outputs formatted status or errors.
pub fn run_cli(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() <= 1 {
        print_usage();
        return Ok(());
    }

    // Filter out optional global flags like --data-dir <path>
    let mut filtered_args = Vec::new();
    let mut custom_data_dir: Option<camino::Utf8PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--data-dir" && i + 1 < args.len() {
            custom_data_dir = Some(camino::Utf8PathBuf::from(&args[i + 1]));
            i += 2;
        } else if let Some(stripped) = args[i].strip_prefix("--data-dir=") {
            custom_data_dir = Some(camino::Utf8PathBuf::from(stripped));
            i += 1;
        } else {
            filtered_args.push(args[i].clone());
            i += 1;
        }
    }

    if filtered_args.is_empty() {
        print_usage();
        return Ok(());
    }

    let xdg = if let Some(ref base) = custom_data_dir {
        XdgPaths::custom(base)
    } else {
        XdgPaths::from_env()
            .unwrap_or_else(|_| XdgPaths::custom(camino::Utf8Path::new("/tmp/woderrise")))
    };

    match filtered_args[0].as_str() {
        "--version" | "-V" | "version" => {
            println!("woderrise {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "--help" | "-h" | "help" => {
            print_usage();
            Ok(())
        }
        "auth" | "config" => handle_auth_command_with_paths(&filtered_args[1..], &xdg),
        unknown => {
            eprintln!("Unknown command or option: '{}'", unknown);
            eprintln!("Run 'woderrise --help' for usage instructions.");
            Err(format!("Unknown command: {}", unknown).into())
        }
    }
}

/// Prints top-level CLI usage information.
pub fn print_usage() {
    println!(
        r#"Woderrise — Native Wayland Desktop Configuration Store & Discovery Engine

USAGE:
    woderrise [COMMAND] [OPTIONS]

COMMANDS:
    gui                  Launch the native Wayland desktop GUI (default)
    auth, config         Manage GitHub Personal Access Token and view API rate limits

OPTIONS:
    -h, --help           Print help information
    -V, --version        Print version information

Run 'woderrise auth --help' for more details on authentication options."#
    );
}

/// Prints usage information specifically for the `auth` subcommand.
pub fn print_auth_usage() {
    println!(
        r#"Manage GitHub Personal Access Token (PAT) and view live API rate limit quotas.

USAGE:
    woderrise auth [OPTIONS]

OPTIONS:
    --token <TOKEN>      Save GitHub Personal Access Token to settings.json with POSIX 0600
                         permissions, verify quota with GitHub API, and activate elevated limits.
    --status             Display current authentication status, masked token, and rate limit quotas.
    --clear              Remove saved GitHub token and revert to unauthenticated limits.
    -h, --help           Print this help message"#
    );
}

/// Handles the `auth` / `config` subcommands using system XDG paths.
pub fn handle_auth_command(sub_args: &[String]) -> Result<(), Box<dyn Error>> {
    let xdg = XdgPaths::from_env()
        .unwrap_or_else(|_| XdgPaths::custom(camino::Utf8Path::new("/tmp/woderrise")));
    handle_auth_command_with_paths(sub_args, &xdg)
}

/// Handles the `auth` / `config` subcommands with explicit XDG paths.
pub fn handle_auth_command_with_paths(
    sub_args: &[String],
    xdg: &XdgPaths,
) -> Result<(), Box<dyn Error>> {
    if sub_args.is_empty() {
        print_auth_usage();
        return Ok(());
    }

    let mut token_arg: Option<String> = None;
    let mut show_status = false;
    let mut clear_token = false;

    let mut i = 0;
    while i < sub_args.len() {
        let arg = &sub_args[i];
        if arg == "--help" || arg == "-h" || arg == "help" {
            print_auth_usage();
            return Ok(());
        } else if arg == "--status" {
            show_status = true;
            i += 1;
        } else if arg == "--clear" {
            clear_token = true;
            i += 1;
        } else if arg == "--token" {
            if i + 1 < sub_args.len() {
                token_arg = Some(sub_args[i + 1].clone());
                i += 2;
            } else {
                eprintln!("Error: Missing argument value for '--token'.");
                eprintln!("Usage: woderrise auth --token <TOKEN>");
                return Err("Missing argument for --token".into());
            }
        } else if let Some(stripped) = arg.strip_prefix("--token=") {
            token_arg = Some(stripped.to_string());
            i += 1;
        } else {
            eprintln!("Unknown option for 'auth': '{}'", arg);
            eprintln!("Run 'woderrise auth --help' for available options.");
            return Err(format!("Unknown option: {}", arg).into());
        }
    }

    let settings_path = xdg.settings_file();

    if let Some(token) = token_arg {
        let token_trimmed = token.trim().to_string();
        if token_trimmed.is_empty() {
            eprintln!("Error: Provided token is empty.");
            return Err("Empty token provided".into());
        }

        let mut settings = load_settings(&settings_path).unwrap_or_else(|_| Settings::default());
        settings.github_token = Some(token_trimmed.clone());
        save_settings_durably(&settings_path, &settings)?;

        println!("✓ GitHub Personal Access Token saved successfully.");
        println!("  File: {} (POSIX 0600 permissions)", settings_path);

        // Verify quota via GitHub API
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let verify_res = rt.block_on(async {
            let client = GitHubClientBuilder::new()
                .auth_token(Some(token_trimmed.clone()))
                .timeout(std::time::Duration::from_secs(5))
                .build()?;
            client.get_rate_limit().await
        });

        match verify_res {
            Ok((core, search)) => {
                println!("  Live GitHub API Quota Verified:");
                println!(
                    "    • Search API: {}/{} requests/min (Elevated)",
                    search.remaining, search.limit
                );
                println!(
                    "    • Core API:   {}/{} requests/hr (Elevated)",
                    core.remaining, core.limit
                );
            }
            Err(e) => {
                println!(
                    "  Notice: Token saved with elevated rate limits (Search: 30/min, Core: 5,000/hr),"
                );
                println!("  but immediate API quota verification was skipped: {}", e);
            }
        }

        return Ok(());
    }

    if clear_token {
        let mut settings = load_settings(&settings_path).unwrap_or_else(|_| Settings::default());
        settings.github_token = None;
        save_settings_durably(&settings_path, &settings)?;

        println!("✓ GitHub Personal Access Token removed.");
        println!("  File: {}", settings_path);
        println!("  Reverted to unauthenticated limits: Search 10/min, Core 60/hr.");
        return Ok(());
    }

    if show_status {
        let settings = load_settings(&settings_path).unwrap_or_else(|_| Settings::default());

        if let Some(ref token) = settings.github_token {
            let masked = mask_token(token);
            println!("GitHub Authentication Status: Authenticated");
            println!("  Token:         {}", masked);
            println!("  Configuration: {} (POSIX 0600)", settings_path);

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            let rate_res = rt.block_on(async {
                let client = GitHubClientBuilder::new()
                    .auth_token(Some(token.clone()))
                    .timeout(std::time::Duration::from_secs(5))
                    .build()?;
                client.get_rate_limit().await
            });

            match rate_res {
                Ok((core, search)) => {
                    println!("  Rate Limit Quotas:");
                    println!(
                        "    • Search API: {}/{} requests/min remaining",
                        search.remaining, search.limit
                    );
                    println!(
                        "    • Core API:   {}/{} requests/hr remaining",
                        core.remaining, core.limit
                    );
                }
                Err(_) => {
                    println!("  Rate Limit Quotas (Elevated Defaults):");
                    println!("    • Search API: 30 requests/min");
                    println!("    • Core API:   5,000 requests/hr");
                }
            }
        } else {
            println!("GitHub Authentication Status: Unauthenticated (Public Anonymous Access)");
            println!("  Configuration: {}", settings_path);
            println!("  Rate Limit Quotas:");
            println!("    • Search API: 10 requests/min");
            println!("    • Core API:   60 requests/hr");
            println!(
                "  Tip: Configure a Personal Access Token via 'woderrise auth --token <TOKEN>'"
            );
            println!("       to elevate rate limits to Search 30/min and Core 5,000/hr.");
        }

        return Ok(());
    }

    print_auth_usage();
    Ok(())
}

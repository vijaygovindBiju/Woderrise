//! Adversarial stress tests for Milestone M7: Multi-Interface Integration.
//!
//! Stress tests edge cases in CLI:
//! - Malformed URLs (schemes, hosts, missing segments, extra segments, credentials, ports, traversal)
//! - Unreachable endpoints (non-existent local file fixtures, unreachable/offline hosts)
//! - Invalid flags in URLs and argument injection attempts
//! - Empty and whitespace inputs across fetch, install, diff, info
//! - Non-destructive isolation of system state

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct TempDirGuard(PathBuf);
impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Deref for TempDirGuard {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn create_test_env(prefix: &str) -> (TempDirGuard, PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("woderrise-cli-adv-{prefix}-{nonce}"));
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir_all(&home).expect("failed to create temp home");
    fs::create_dir_all(&data).expect("failed to create temp data");
    (TempDirGuard(root), home, data)
}

fn run_woderrise(home: &Path, data: &Path, args: &[&str]) -> std::process::Output {
    let bin_path = env!("CARGO_BIN_EXE_woderrise");
    let ws = workspace_root();
    let reg_dir = ws.join("registry/packages");
    Command::new(bin_path)
        .current_dir(&ws)
        .env("HOME", home)
        .env("WODERRISE_DATA_DIR", data)
        .env("WODERRISE_REGISTRY_DIR", &reg_dir)
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .expect("failed to execute woderrise binary")
}

#[test]
fn test_cli_fetch_malformed_urls_rejected_without_panic() {
    let (_guard, home, data) = create_test_env("malformed-fetch");

    let malformed_urls = [
        "",                                                  // Empty
        "   ",                                               // Whitespace
        "https://github.com/ owner / repo",                  // Internal whitespace
        "http://github.com/owner/repo",                      // Non-HTTPS remote
        "ftp://github.com/owner/repo",                       // Unsupported scheme
        "git://github.com/owner/repo",                       // Insecure git protocol
        "https://gitlab.com/owner/repo",                     // Non-GitHub host
        "https://evil-site.com/owner/repo",                  // Malicious host
        "https://github.com/owner",                          // Missing repository
        "https://github.com/owner/repo/extra/path",          // Excess segments
        "https://user:pass@github.com/owner/repo",           // Embedded credentials
        "https://github.com:8443/owner/repo",                // Custom port
        "https://github.com/owner/../repo",                  // Directory traversal
        "https://github.com/owner/repo\r\nevil=1",           // CRLF injection
    ];

    for url in &malformed_urls {
        let output = run_woderrise(&home, &data, &["fetch", url]);
        assert!(
            !output.status.success(),
            "Expected failure for malformed URL '{url}', but got success: {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid repository URL")
                || stderr.contains("URL must")
                || stderr.contains("error:")
                || stderr.contains("woderrise:"),
            "Expected structured validation error for URL '{url}', got stderr: {stderr}"
        );
    }
}

#[test]
fn test_cli_fetch_flag_and_argument_injections_rejected() {
    let (_guard, home, data) = create_test_env("flag-injection");

    let flag_injections = [
        "--upload-pack=touch /tmp/pwned",
        "-u",
        "--config=core.gitProxy=evil",
        "https://github.com/-owner/repo",
        "https://github.com/owner/-repo",
        "file://-o/evil",
        "file:///tmp/-flag/repo",
    ];

    for flag_input in &flag_injections {
        // Use "--" separator so clap passes flag_input as positional argument
        let output = run_woderrise(&home, &data, &["fetch", "--", flag_input]);
        assert!(
            !output.status.success(),
            "Expected failure for flag injection '{flag_input}', but got success"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("must not start with flags")
                || stderr.contains("invalid repository URL")
                || stderr.contains("error:")
                || stderr.contains("woderrise:"),
            "Expected flag rejection error for '{flag_input}', got stderr: {stderr}"
        );
    }
}

#[test]
fn test_cli_fetch_unreachable_endpoint_handling() {
    let (_guard, home, data) = create_test_env("unreachable-endpoint");

    // 1. Unreachable local file target
    let output = run_woderrise(
        &home,
        &data,
        &["fetch", "file:///tmp/woderrise-nonexistent-endpoint-999999999"],
    );
    assert!(
        !output.status.success(),
        "Expected failure for non-existent local file target"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("git CLI failed") || stderr.contains("woderrise:"),
        "Expected structured git error, got stderr: {stderr}"
    );

    // 2. Unreachable local path without file:// prefix
    let output2 = run_woderrise(
        &home,
        &data,
        &["fetch", "/tmp/woderrise-nonexistent-endpoint-999999999"],
    );
    assert!(!output2.status.success());
}

#[test]
fn test_cli_install_adversarial_and_empty_inputs() {
    let (_guard, home, data) = create_test_env("install-adversarial");

    let invalid_inputs = [
        "",                                                  // Empty
        "   ",                                               // Whitespace
        "https://gitlab.com/unsupported/repo",               // Unsupported remote
        "https://github.com/invalid url with spaces",        // Spaces in remote
        "file:///tmp/nonexistent-woderrise-path-777",        // Nonexistent file URL
        "nonexistent.package.id.that.does.not.exist",        // Nonexistent catalog ID
        "com.catppuccin.waybar@999.0.0",                     // Nonexistent catalog version
    ];

    for input in &invalid_inputs {
        let output = run_woderrise(&home, &data, &["install", input]);
        assert!(
            !output.status.success(),
            "Expected failure for install with input '{input}', but got success"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("package not found")
                || stderr.contains("invalid repository URL")
                || stderr.contains("git CLI failed")
                || stderr.contains("woderrise:"),
            "Expected structured error for install '{input}', got: {stderr}"
        );
    }
}

#[test]
fn test_cli_diff_adversarial_and_empty_inputs() {
    let (_guard, home, data) = create_test_env("diff-adversarial");

    let invalid_inputs = [
        "",
        "   ",
        "https://gitlab.com/unsupported/repo",
        "https://github.com/invalid url with spaces",
        "file:///tmp/nonexistent-woderrise-diff-path-777",
        "nonexistent.package.diff.id",
        "com.catppuccin.waybar@888.0.0",
    ];

    for input in &invalid_inputs {
        let output = run_woderrise(&home, &data, &["diff", input]);
        assert!(
            !output.status.success(),
            "Expected failure for diff with input '{input}', but got success"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("package not found")
                || stderr.contains("invalid repository URL")
                || stderr.contains("git CLI failed")
                || stderr.contains("woderrise:"),
            "Expected structured error for diff '{input}', got: {stderr}"
        );
    }
}

#[test]
fn test_cli_info_and_search_edge_cases() {
    let (_guard, home, data) = create_test_env("info-search-edge");

    // Empty package for info should fail gracefully
    let output = run_woderrise(&home, &data, &["info", ""]);
    assert!(
        !output.status.success(),
        "Expected failure for info with empty package ID"
    );

    // Nonexistent package info should fail gracefully
    let output = run_woderrise(&home, &data, &["info", "nonexistent.unknown.pkg"]);
    assert!(
        !output.status.success(),
        "Expected failure for info with nonexistent package"
    );

    // Empty search query should succeed and list catalog packages
    let output = run_woderrise(&home, &data, &["search", ""]);
    assert!(
        output.status.success(),
        "Search with empty query should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("com.catppuccin.waybar") || stdout.contains("Packages"),
        "Catalog listing missing in search output: {stdout}"
    );

    // Search with zero limit should succeed without crash
    let output = run_woderrise(&home, &data, &["search", "catppuccin", "--limit", "0"]);
    assert!(output.status.success());
}

#[test]
fn test_cli_fetch_missing_required_arguments_fails_cleanly() {
    let (_guard, home, data) = create_test_env("fetch-missing-args");

    // Running fetch without any argument must fail with clap usage/error
    let output = run_woderrise(&home, &data, &["fetch"]);
    assert!(
        !output.status.success(),
        "fetch without args must exit with error"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage") || stderr.contains("required") || stderr.contains("<URL>"),
        "Expected usage or required argument error, got: {stderr}"
    );
}

#[test]
fn test_in_process_control_char_and_null_byte_rejection() {
    let (_guard, _home, data) = create_test_env("control-chars");

    let adversarial = [
        "https://github.com/owner/repo\0evil",
        "https://github.com/owner/repo\x1b[2J",
        "https://github.com/owner/repo\t\n",
        "https://github.com/\x01\x02/repo",
    ];

    for raw in &adversarial {
        let res_fetch = woderrise_cli::handle_fetch(raw, None, false, &data);
        assert!(res_fetch.is_err(), "handle_fetch must reject control chars");

        let res_install = woderrise_cli::handle_install(raw, &data, PathBuf::from("/tmp"));
        assert!(res_install.is_err(), "handle_install must reject control chars");

        let res_diff = woderrise_cli::handle_diff(raw, false, true, &data, Path::new("/tmp"));
        assert!(res_diff.is_err(), "handle_diff must reject control chars");
    }
}


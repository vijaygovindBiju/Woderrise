//! Integration tests for dynamic GitHub fetching, URL installation, and Hyprland discovery in CLI.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use woderrise_core::{catalog, Database, Manifest};

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
    let root = std::env::temp_dir().join(format!("woderrise-cli-fetch-{prefix}-{nonce}"));
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

fn init_git_repo(root: &Path) {
    fs::create_dir_all(root).expect("failed to create repo root");
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command failed");
        assert!(status.success(), "git {:?} failed", args);
    };
    run(&["init"]);
    run(&["checkout", "-b", "main"]);
    run(&["config", "user.name", "CLI Integration Tester"]);
    run(&["config", "user.email", "tester@example.com"]);
    run(&["config", "commit.gpgsign", "false"]);
}

fn commit_all(root: &Path, msg: &str, tag: Option<&str>) {
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command failed");
        assert!(status.success(), "git {:?} failed", args);
    };
    run(&["add", "-A"]);
    run(&["commit", "-m", msg]);
    if let Some(t) = tag {
        run(&["tag", t]);
    }
}

fn setup_hyprland_git_fixture(dir: &Path) {
    init_git_repo(dir);
    let hypr_dir = dir.join("hypr");
    fs::create_dir_all(&hypr_dir).unwrap();
    fs::write(hypr_dir.join("hyprland.conf"), "general {\n  gaps_in = 5\n}\n$base = 0xff1e1e2e\n").unwrap();
    fs::write(hypr_dir.join("hyprlock.conf"), "background {\n  color = rgb(30, 30, 46)\n}\n").unwrap();
    fs::write(dir.join("LICENSE"), "MIT License\nCopyright (c) 2026 Test\n").unwrap();
    commit_all(dir, "feat: initial hyprland setup", Some("v1.0.0"));
}

// ============================================================================
// 1. FETCH COMMAND TESTS
// ============================================================================

#[test]
fn test_cli_fetch_git_repo_to_dest_dry_run() {
    let (guard, home, data) = create_test_env("fetch-dest");
    let repo_dir = guard.join("repo");
    let dest_dir = guard.join("fetched_dest");
    setup_hyprland_git_fixture(&repo_dir);

    let output = run_woderrise(&home, &data, &[
        "fetch",
        repo_dir.to_str().unwrap(),
        "--dest",
        dest_dir.to_str().unwrap(),
    ]);

    assert!(output.status.success(), "fetch failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fetched repository:"));
    assert!(stdout.contains("hypr/hyprland.conf"));
    assert!(dest_dir.join("woderrise.yaml").is_file());
    assert!(dest_dir.join("hypr/hyprland.conf").is_file());

    let manifest = Manifest::from_file(dest_dir.join("woderrise.yaml")).unwrap();
    assert_eq!(manifest.package.license, "MIT");
    assert_eq!(manifest.package.version, "1.0.0");
    assert_eq!(manifest.artifacts.len(), 2);
}

#[test]
fn test_cli_fetch_with_index_and_search() {
    let (guard, home, data) = create_test_env("fetch-index");
    let repo_dir = guard.join("custom-dots");
    let dest_dir = guard.join("cached_dots");
    setup_hyprland_git_fixture(&repo_dir);

    let output = run_woderrise(&home, &data, &[
        "fetch",
        repo_dir.to_str().unwrap(),
        "--dest",
        dest_dir.to_str().unwrap(),
        "--index",
    ]);
    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));

    let search_out = run_woderrise(&home, &data, &["search", "custom-dots"]);
    assert_eq!(search_out.status.code(), Some(0));
    let search_str = String::from_utf8_lossy(&search_out.stdout);
    assert!(search_str.contains("custom-dots") || search_str.contains("io.github.local"));
}

#[test]
fn test_cli_fetch_missing_hyprland_config_fails_gracefully() {
    let (guard, home, data) = create_test_env("fetch-missing");
    let empty_repo = guard.join("empty-repo");
    init_git_repo(&empty_repo);
    fs::write(empty_repo.join("README.md"), "# Not Hyprland").unwrap();
    commit_all(&empty_repo, "initial", None);

    let output = run_woderrise(&home, &data, &["fetch", empty_repo.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("woderrise:"));
    assert!(stderr.contains("missing Hyprland configuration") || stderr.contains("hyprland.conf"));
}

// ============================================================================
// 2. REMOTE URL INSTALL AND DIFF TESTS
// ============================================================================

#[test]
fn test_cli_install_github_url_creates_files_and_journal_record() {
    let (guard, home, data) = create_test_env("install-url");
    let repo_dir = guard.join("repo-install");
    setup_hyprland_git_fixture(&repo_dir);

    let target_hyprland = home.join(".config/hypr/hyprland.conf");
    assert!(!target_hyprland.exists());

    let file_url = format!("file://{}", repo_dir.display());
    let output = run_woderrise(&home, &data, &["install", &file_url]);
    assert!(output.status.success(), "install URL failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(0));

    assert!(target_hyprland.exists(), "Target hyprland.conf was not created in home directory");
    let content = fs::read_to_string(&target_hyprland).unwrap();
    assert!(content.contains("gaps_in = 5"));

    let db = Database::open(data.join("journal.db")).unwrap();
    let txn_status: String = db.conn.query_row(
        "SELECT status FROM transactions ORDER BY started_at DESC LIMIT 1",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(txn_status, "COMMITTED");
}

#[test]
fn test_cli_diff_github_url_read_only() {
    let (guard, home, data) = create_test_env("diff-url");
    let repo_dir = guard.join("repo-diff");
    setup_hyprland_git_fixture(&repo_dir);

    let target_hyprland = home.join(".config/hypr/hyprland.conf");
    assert!(!target_hyprland.exists());

    let file_url = format!("file://{}", repo_dir.display());
    let output = run_woderrise(&home, &data, &["diff", &file_url, "--summary"]);
    assert!(output.status.success(), "diff URL failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[CREATE]"));
    assert!(stdout.contains("hyprland.conf"));

    // Invariant: diff must not mutate filesystem
    assert!(!target_hyprland.exists(), "diff created file in home directory!");
}

// ============================================================================
// 3. REAL HYPRLAND REGISTRY SEARCH & METADATA TESTS
// ============================================================================

#[test]
fn test_cli_search_real_hyprland_package() {
    let (_guard, home, data) = create_test_env("search-hyprland");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["search", "hyprland"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.hyprland"));
    assert!(stdout.contains("1.0.0"));
    assert!(stdout.contains("desktop_setups"));
    assert!(stdout.contains("Catppuccin Wayland Hyprland Suite"));
}

#[test]
fn test_cli_info_real_hyprland_metadata() {
    let (_guard, home, data) = create_test_env("info-hyprland");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let output = run_woderrise(&home, &data, &["info", "com.catppuccin.hyprland"]);
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("com.catppuccin.hyprland v1.0.0"));
    assert!(stdout.contains("MIT"));
    assert!(stdout.contains("Display Server: wayland_only"));
    assert!(stdout.contains("Compositors:    hyprland >=0.40.0"));
    assert!(stdout.contains("mocha_base"));
    assert!(stdout.contains("#1e1e2e"));
    assert!(stdout.contains("latte_mauve"));
    assert!(stdout.contains("#8839ef"));
    assert!(stdout.contains("~/.config/hypr/hyprland.conf"));
    assert!(stdout.contains("~/.config/waybar/config.jsonc"));
}

#[test]
fn test_cli_invalid_github_url_exit_code_1() {
    let (_guard, home, data) = create_test_env("invalid-url");
    let output = run_woderrise(&home, &data, &["fetch", "https://invalid.github.com/bad/repo"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("woderrise:"));
}

#[test]
fn test_cli_install_and_rollback_catalog_hyprland_package() {
    let (_guard, home, data) = create_test_env("install-catalog-hyprland");
    let ws = workspace_root();
    let db = Database::open(data.join("journal.db")).unwrap();
    catalog::import_registry(&db, ws.join("registry/packages")).unwrap();

    let target_hyprland = home.join(".config/hypr/hyprland.conf");
    let target_waybar = home.join(".config/waybar/config.jsonc");
    assert!(!target_hyprland.exists());

    // 1. Install by catalog package ID
    let install_out = run_woderrise(&home, &data, &["install", "com.catppuccin.hyprland"]);
    assert!(install_out.status.success(), "install failed: {}", String::from_utf8_lossy(&install_out.stderr));
    assert_eq!(install_out.status.code(), Some(0));

    assert!(target_hyprland.exists());
    assert!(target_waybar.exists());

    let txn_id = String::from_utf8_lossy(&install_out.stdout).trim().to_string();

    // 2. Rollback
    let rollback_out = run_woderrise(&home, &data, &["rollback", &txn_id]);
    assert!(rollback_out.status.success(), "rollback failed: {}", String::from_utf8_lossy(&rollback_out.stderr));
    assert_eq!(rollback_out.status.code(), Some(0));

    // 3. Verify clean revert
    assert!(!target_hyprland.exists(), "rollback failed to remove installed hyprland.conf");
    assert!(!target_waybar.exists(), "rollback failed to remove installed waybar config");
}

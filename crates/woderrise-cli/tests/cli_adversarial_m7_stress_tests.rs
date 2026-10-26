//! Adversarial stress test suite for Milestone M7: Multi-Interface Dynamic Import.
//! Tests:
//! 1. Concurrent URL fetches & race condition resistance (multi-threaded fetches & catalog upserts)
//! 2. Imported package index consistency (FTS5 search, manifest digest verification, idempotence)
//! 3. Diff preview fidelity on imported dotfiles (Myers line diffs, planned backup SHA256 prediction, binary dotfiles)
//! 4. CLI error resilience on adversarial or invalid remote repository inputs

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Barrier},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use woderrise_core::{
    catalog::{self, CatalogEntry},
    fetcher::{self, FetchOptions},
    Database,
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
    let root = std::env::temp_dir().join(format!("woderrise-m7-stress-{prefix}-{nonce}"));
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
    run(&["config", "user.name", "Adversarial Tester"]);
    run(&["config", "user.email", "challenger@example.com"]);
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

/// Sets up a rich Hyprland repository with configs and a binary wallpaper fixture.
fn setup_rich_hyprland_git_fixture(dir: &Path, pkg_title: &str) {
    init_git_repo(dir);
    let hypr_dir = dir.join("hypr");
    let waybar_dir = dir.join("waybar");
    fs::create_dir_all(&hypr_dir).unwrap();
    fs::create_dir_all(&waybar_dir).unwrap();

    let hyprland_conf = format!(
        "# Hyprland Main Config - {pkg_title}\n\
        monitor=,preferred,auto,1\n\
        general {{\n\
          gaps_in = 6\n\
          gaps_out = 12\n\
          border_size = 2\n\
          col.active_border = rgba(cba6f7ee)\n\
        }}\n\
        $color_base = 0xff1e1e2e\n\
        $color_mauve = 0xffcba6f7\n"
    );
    fs::write(hypr_dir.join("hyprland.conf"), hyprland_conf).unwrap();

    let hyprlock_conf = "\
        background {\n\
          monitor =\n\
          path = ~/.config/hypr/wallpaper.png\n\
          color = rgba(30, 30, 46, 1.0)\n\
        }\n";
    fs::write(hypr_dir.join("hyprlock.conf"), hyprlock_conf).unwrap();

    let hypridle_conf = "\
        general {\n\
          lock_cmd = pidof hyprlock || hyprlock\n\
        }\n";
    fs::write(hypr_dir.join("hypridle.conf"), hypridle_conf).unwrap();

    let waybar_cfg = "{\n  \"layer\": \"top\",\n  \"position\": \"top\",\n  \"modules-left\": [\"hyprland/workspaces\"]\n}\n";
    fs::write(waybar_dir.join("config.jsonc"), waybar_cfg).unwrap();

    let waybar_style = "* { font-family: 'JetBrainsMono Nerd Font'; }\nwindow#waybar { background: #1e1e2e; }\n";
    fs::write(waybar_dir.join("style.css"), waybar_style).unwrap();

    // Binary wallpaper artifact
    let fake_png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG magic
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00,
        0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54,
        0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A,
        0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    fs::write(hypr_dir.join("wallpaper.png"), &fake_png_bytes).unwrap();

    fs::write(
        dir.join("LICENSE"),
        "MIT License\nCopyright (c) 2026 Test Team\n",
    ).unwrap();

    commit_all(dir, "feat: initial complete hyprland suite", Some("v1.0.0"));
}

// ============================================================================
// 1. CONCURRENT URL FETCHES & RACE CONDITION STRESS
// ============================================================================

#[test]
fn test_adversarial_concurrent_url_fetches_multi_threaded() {
    let (guard, _home, _data) = create_test_env("concurrent-fetches");
    let repo_count = 6;
    let mut repo_paths = Vec::new();

    for i in 0..repo_count {
        let repo_dir = guard.join(format!("repo_{i}"));
        setup_rich_hyprland_git_fixture(&repo_dir, &format!("Test Rice {i}"));
        repo_paths.push(repo_dir);
    }

    let barrier = Arc::new(Barrier::new(repo_count));
    let mut handles = Vec::new();

    for (idx, repo_path) in repo_paths.into_iter().enumerate() {
        let b = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            b.wait(); // Synchronize all threads to start fetch simultaneously
            let raw_url = format!("file://{}", repo_path.display());
            let fetched = fetcher::fetch_github_repository(
                &raw_url,
                FetchOptions {
                    timeout: std::time::Duration::from_secs(30),
                    branch: None,
                },
            ).unwrap_or_else(|e| panic!("Thread {idx} failed to fetch: {e}"));

            assert_eq!(fetched.license, "MIT");
            assert!(fetched.detected_files.len() >= 4);
            assert!(fetched.manifest.package.id.starts_with("io.github.local.repo_") || fetched.manifest.package.id.contains("repo_"));
            fetched
        });
        handles.push(handle);
    }

    let mut fetched_ids = Vec::new();
    for h in handles {
        let fetched = h.join().expect("Worker thread panicked");
        fetched_ids.push(fetched.manifest.package.id);
    }

    assert_eq!(fetched_ids.len(), repo_count);
    let unique_ids: std::collections::HashSet<_> = fetched_ids.into_iter().collect();
    assert_eq!(unique_ids.len(), repo_count, "All concurrent fetches must produce distinct package ids");
}

#[test]
fn test_adversarial_concurrent_catalog_upsert_wal_resilience() {
    let (_guard, _home, data) = create_test_env("concurrent-upsert");
    let db_path = data.join("journal.db");
    let db = Database::open(&db_path).expect("Failed to initialize database");
    catalog::migrate(&db).expect("Failed to migrate catalog");

    let thread_count = 8;
    let barrier = Arc::new(Barrier::new(thread_count));
    let mut handles = Vec::new();

    for t in 0..thread_count {
        let b = Arc::clone(&barrier);
        let p = db_path.clone();
        let handle = thread::spawn(move || {
            b.wait();
            // SQLite without preset busy_timeout requires retry backoff under concurrent BEGIN transactions
            let mut thread_db = None;
            for _ in 0..50 {
                match Database::open(&p) {
                    Ok(db) => {
                        thread_db = Some(db);
                        break;
                    }
                    Err(_) => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
            }
            let thread_db = thread_db.expect("Thread failed to open DB after retries");

            for i in 0..5 {
                let entry = CatalogEntry {
                    id: format!("org.concurrent.pkg_{t}_{i}"),
                    name: format!("Concurrent Package {t}-{i}"),
                    description: format!("Description for package {t}_{i} under thread contention"),
                    version: "1.0.0".into(),
                    package_type: "desktop_setups".into(),
                    category: "window_manager_hyprland".into(),
                    tags: vec!["hyprland".into(), "test".into(), format!("thread_{t}")],
                    license: "MIT".into(),
                    repository: Some(format!("https://github.com/test/pkg_{t}_{i}")),
                    manifest_path: PathBuf::from(format!("/tmp/manifest_{t}_{i}.yaml")),
                    manifest_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                    validated: true,
                    yanked: false,
                };
                
                // Retry on SQLite busy during concurrent upserts
                let mut upsert_ok = false;
                for _ in 0..50 {
                    if catalog::upsert(&thread_db, &entry).is_ok() {
                        upsert_ok = true;
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(15));
                }
                assert!(upsert_ok, "Upsert failed under concurrency after retries");
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().expect("Upsert thread panicked");
    }

    let final_db = Database::open(&db_path).unwrap();
    let total_rows: i64 = final_db.conn.query_row("SELECT COUNT(*) FROM catalog", [], |r| r.get(0)).unwrap();
    assert_eq!(total_rows, (thread_count * 5) as i64, "Catalog should contain all concurrent entries");

    // FTS5 query must locate entries written by all threads
    for t in 0..thread_count {
        let search_results = catalog::search(&final_db, &format!("thread_{t}"), 20).unwrap();
        assert_eq!(search_results.len(), 5, "FTS5 search must retrieve all entries for thread {t}");
    }
}

// ============================================================================
// 2. IMPORTED PACKAGE INDEX CONSISTENCY
// ============================================================================

#[test]
fn test_adversarial_imported_package_index_and_fts_consistency() {
    let (guard, home, data) = create_test_env("index-consistency");
    let repo_dir = guard.join("hypr-rice-consistent");
    setup_rich_hyprland_git_fixture(&repo_dir, "Consistent Nordic Hyprland");

    let dest_dir = data.join("imported_packages").join("nordic");

    // Run fetch with --index
    let out = run_woderrise(&home, &data, &[
        "fetch",
        repo_dir.to_str().unwrap(),
        "--dest",
        dest_dir.to_str().unwrap(),
        "--index",
    ]);
    assert!(out.status.success(), "Fetch --index failed: {}", String::from_utf8_lossy(&out.stderr));

    // 1. Verify manifest on disk and SHA-256 integrity
    let manifest_file = dest_dir.join("woderrise.yaml");
    assert!(manifest_file.is_file(), "Manifest file missing at {:?}", manifest_file);
    let disk_sha256 = woderrise_core::sha256_file(&manifest_file).unwrap();

    // 2. Query SQLite catalog directly and verify index consistency
    let db = Database::open(data.join("journal.db")).unwrap();
    let (indexed_sha256, indexed_path, indexed_license, indexed_category): (String, String, String, String) = db.conn.query_row(
        "SELECT manifest_sha256, manifest_path, license, category FROM catalog ORDER BY rowid DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).unwrap();

    assert_eq!(indexed_sha256, disk_sha256, "Indexed SHA256 must match real file on disk");
    assert_eq!(PathBuf::from(indexed_path), manifest_file, "Indexed manifest_path must match disk location");
    assert_eq!(indexed_license, "MIT");
    assert_eq!(indexed_category, "window_manager_hyprland");

    // 3. Test FTS search queries
    let q_exact = run_woderrise(&home, &data, &["search", "consistent"]);
    assert_eq!(q_exact.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&q_exact.stdout);
    assert!(stdout.contains("window_manager_hyprland"), "Expected category in search output: {}", stdout);
    assert!(stdout.contains("1.0.0"), "Expected version in search output: {}", stdout);
    assert!(stdout.contains("hypr-rice-consistent"), "Expected package id in search output: {}", stdout);

    // 4. Test idempotence: running fetch --index a second time should update cleanly without duplicate errors
    let out2 = run_woderrise(&home, &data, &[
        "fetch",
        repo_dir.to_str().unwrap(),
        "--dest",
        dest_dir.to_str().unwrap(),
        "--index",
    ]);
    assert!(out2.status.success(), "Second fetch --index failed: {}", String::from_utf8_lossy(&out2.stderr));

    let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM catalog", [], |r| r.get(0)).unwrap();
    // In addition to registry packages auto-seeded if any, there must only be 1 instance of the imported package
    let pkg_count: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM catalog WHERE manifest_path = ?1",
        rusqlite::params![manifest_file.to_string_lossy().to_string()],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(pkg_count, 1, "Duplicate catalog rows created for the same manifest path");
    assert!(count >= 1);
}

// ============================================================================
// 3. DIFF PREVIEW FIDELITY ON IMPORTED DOTFILES
// ============================================================================

#[test]
fn test_adversarial_diff_preview_fidelity_on_imported_dotfiles() {
    let (guard, home, data) = create_test_env("diff-fidelity");
    let repo_dir = guard.join("hypr-diff-repo");
    setup_rich_hyprland_git_fixture(&repo_dir, "Fidelity Test Suite");

    // Scenario Setup in user's HOME directory:
    // 1. ~/.config/hypr/hyprland.conf exists and differs -> MUST be [MODIFY] with Myers diff lines
    let home_hypr = home.join(".config/hypr");
    let home_waybar = home.join(".config/waybar");
    fs::create_dir_all(&home_hypr).unwrap();
    fs::create_dir_all(&home_waybar).unwrap();

    let user_custom_hyprland = "\
        # User Custom Configuration\n\
        monitor=DP-1,1920x1080@144,0x0,1\n\
        general {\n\
          gaps_in = 3\n\
          gaps_out = 6\n\
        }\n";
    fs::write(home_hypr.join("hyprland.conf"), user_custom_hyprland).unwrap();
    let pre_hypr_sha = woderrise_core::sha256_file(home_hypr.join("hyprland.conf")).unwrap();

    // 2. ~/.config/waybar/config.jsonc exists and is IDENTICAL to repo -> MUST be [UNCHANGED]
    let repo_waybar_cfg = fs::read(repo_dir.join("waybar/config.jsonc")).unwrap();
    fs::write(home_waybar.join("config.jsonc"), &repo_waybar_cfg).unwrap();

    // 3. ~/.config/hypr/hyprlock.conf does NOT exist -> MUST be [CREATE] (no backup)
    assert!(!home_hypr.join("hyprlock.conf").exists());

    // 4. ~/.config/hypr/wallpaper.png exists with different binary bytes -> MUST be [MODIFY] binary
    let old_png_bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0x00, 0x00, 0x00];
    fs::write(home_hypr.join("wallpaper.png"), &old_png_bytes).unwrap();
    let pre_wall_sha = woderrise_core::sha256_file(home_hypr.join("wallpaper.png")).unwrap();

    // Execute woderrise diff <URL>
    let file_url = format!("file://{}", repo_dir.display());
    let diff_out = run_woderrise(&home, &data, &["diff", &file_url]);
    assert!(diff_out.status.success(), "woderrise diff failed: {}", String::from_utf8_lossy(&diff_out.stderr));

    let stdout = String::from_utf8_lossy(&diff_out.stdout);

    // Verify Output Fidelity
    // 1. Actions identified
    assert!(stdout.contains("[MODIFY]"), "Expected [MODIFY] action in diff output");
    assert!(stdout.contains("[CREATE]"), "Expected [CREATE] action in diff output");
    assert!(stdout.contains("[UNCHANGED]"), "Expected [UNCHANGED] action in diff output");

    // 2. Planned backup predictions
    let expected_hypr_backup = format!("backups/{}", pre_hypr_sha);
    let expected_wall_backup = format!("backups/{}", pre_wall_sha);
    assert!(stdout.contains(&expected_hypr_backup), "Expected backup prediction for hyprland.conf: {}", expected_hypr_backup);
    assert!(stdout.contains(&expected_wall_backup), "Expected backup prediction for wallpaper.png: {}", expected_wall_backup);
    assert!(stdout.contains("None (new file or backup disabled)"), "Expected 'None' backup for created files");

    // 3. Myers line diff verification
    assert!(stdout.contains("- # User Custom Configuration") || stdout.contains("-monitor=DP-1") || stdout.contains("-  gaps_in = 3"));
    assert!(stdout.contains("+ # Hyprland Main Config") || stdout.contains("+monitor=,preferred") || stdout.contains("+  gaps_in = 6"));

    // 4. Binary file diff output fidelity
    assert!(stdout.contains("[binary artifacts modified]"), "Expected binary modification warning for wallpaper.png");

    // 5. Invariant: diff must NEVER alter target files on disk
    assert_eq!(fs::read_to_string(home_hypr.join("hyprland.conf")).unwrap(), user_custom_hyprland, "Diff command mutated target hyprland.conf");
    assert!(!home_hypr.join("hyprlock.conf").exists(), "Diff command created hyprlock.conf on disk");
}

#[test]
fn test_adversarial_diff_summary_mode_fidelity() {
    let (guard, home, data) = create_test_env("diff-summary");
    let repo_dir = guard.join("hypr-summary-repo");
    setup_rich_hyprland_git_fixture(&repo_dir, "Summary Test Suite");

    let file_url = format!("file://{}", repo_dir.display());
    let diff_out = run_woderrise(&home, &data, &["diff", &file_url, "--summary"]);
    assert!(diff_out.status.success());

    let stdout = String::from_utf8_lossy(&diff_out.stdout);
    assert!(stdout.contains("Install Plan for"));
    assert!(stdout.contains("[CREATE]"));
    // In summary mode, individual line diff additions ('+') should NOT be printed
    assert!(!stdout.contains("+ # Hyprland Main Config"));
}

// ============================================================================
// 4. ADVERSARIAL ERROR HANDLING & REJECTION
// ============================================================================

#[test]
fn test_adversarial_cli_fetch_path_traversal_url_rejected() {
    let (_guard, home, data) = create_test_env("traversal-url");
    let out = run_woderrise(&home, &data, &["fetch", "https://github.com/../../etc/passwd"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("woderrise:"));
    assert!(stderr.contains("path traversal") || stderr.contains("invalid repository URL"));
}

#[test]
fn test_adversarial_cli_fetch_non_hyprland_git_repo_fails_with_exit_code_1() {
    let (guard, home, data) = create_test_env("non-hyprland");
    let fake_repo = guard.join("generic-repo");
    init_git_repo(&fake_repo);
    fs::write(fake_repo.join("README.md"), "# Just a python repo\n").unwrap();
    fs::write(fake_repo.join("main.py"), "print('hello')\n").unwrap();
    commit_all(&fake_repo, "init", None);

    let out = run_woderrise(&home, &data, &["fetch", fake_repo.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("woderrise:"));
    assert!(stderr.contains("missing Hyprland configuration") || stderr.contains("hyprland.conf"));
}

//! Empirical Stress Tests by Challenger 2 for Milestone M6.
//!
//! Thoroughly challenges:
//! 1. Multi-package catalog behavior (com.catppuccin.waybar & com.catppuccin.hyprland).
//! 2. Edge-case query parsing & FTS5 sanitization (punctuation, quotes, operators, wildcards, injection, unicode).
//! 3. Limit boundary behaviors (0, 1, 2, 3, large numbers, i64::MAX).
//! 4. Multi-package isolation & yanking behavior.
//! 5. Concurrent installation simulation across waybar and hyprland.
//! 6. Interleaved and concurrent rollback simulations.
//! 7. Reverse-order installation and rollback with pre-existing custom user configurations.
//! 8. Cross-package file conflict detection and forced rollback resolution.
//! 9. Empirical vulnerability verification: re-import failure on contentless FTS5 table.
//! 10. Empirical vulnerability verification: upsert failure on contentless FTS5 table.

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier, Mutex},
    thread,
};
use woderrise_core::{
    catalog, expand_target, sha256_file, BackupManager, Database, InstallationEngine, Manifest,
    WoderriseError,
};

fn registry_packages_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages")
}

fn waybar_pkg_dir() -> PathBuf {
    registry_packages_dir().join("com.catppuccin.waybar/1.0.0")
}

fn hyprland_pkg_dir() -> PathBuf {
    registry_packages_dir().join("com.catppuccin.hyprland/1.0.0")
}

fn create_temp_env(name: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let home = tmp.path().join(format!("home_{name}"));
    let backups = tmp.path().join(format!("backups_{name}"));
    let db_path = tmp.path().join(format!("journal_{name}.db"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&backups).unwrap();
    (tmp, home, backups, db_path)
}

// ---------------------------------------------------------------------------
// 1. Multi-package catalog behavior (both packages in registry)
// ---------------------------------------------------------------------------
#[test]
fn test_multi_package_catalog_import_and_queries() {
    let reg_dir = registry_packages_dir();
    assert!(
        waybar_pkg_dir().join("woderrise.yaml").exists(),
        "Waybar package must exist"
    );
    assert!(
        hyprland_pkg_dir().join("woderrise.yaml").exists(),
        "Hyprland package must exist"
    );

    let db = Database::in_memory().expect("in-memory db");
    catalog::migrate(&db).expect("catalog migration");

    // 1. Verify import_registry indexes both packages
    let count = catalog::import_registry(&db, &reg_dir).expect("import registry");
    assert_eq!(
        count, 2,
        "import_registry must find and index both waybar and hyprland"
    );

    // 2. Exact record retrieval
    let waybar_entry = catalog::get(&db, "com.catppuccin.waybar", "1.0.0").unwrap();
    assert_eq!(waybar_entry.id, "com.catppuccin.waybar");
    assert_eq!(waybar_entry.version, "1.0.0");
    assert_eq!(waybar_entry.category, "bar_themes_modules");
    assert_eq!(waybar_entry.license, "MIT");
    assert!(waybar_entry.validated);
    assert!(!waybar_entry.yanked);

    let hypr_entry = catalog::get(&db, "com.catppuccin.hyprland", "1.0.0").unwrap();
    assert_eq!(hypr_entry.id, "com.catppuccin.hyprland");
    assert_eq!(hypr_entry.version, "1.0.0");
    assert_eq!(hypr_entry.category, "desktop_setups");
    assert_eq!(hypr_entry.license, "MIT");
    assert!(hypr_entry.validated);
    assert!(!hypr_entry.yanked);

    // 3. Manifest loading and verification via catalog
    let waybar_m = catalog::manifest(&db, "com.catppuccin.waybar", "1.0.0").unwrap();
    assert_eq!(waybar_m.package.id, "com.catppuccin.waybar");
    let hypr_m = catalog::manifest(&db, "com.catppuccin.hyprland", "1.0.0").unwrap();
    assert_eq!(hypr_m.package.id, "com.catppuccin.hyprland");

    // 4. Queries that match BOTH packages
    let res = catalog::search(&db, "catppuccin", 10).unwrap();
    assert_eq!(res.len(), 2, "Both packages share catppuccin");
    let res = catalog::search(&db, "mocha", 10).unwrap();
    assert_eq!(res.len(), 2, "Both packages reference mocha");
    let res = catalog::search(&db, "catppuccin mocha", 10).unwrap();
    assert_eq!(res.len(), 2, "Multi-term query matches both");

    // 5. Queries that match ONLY waybar
    let res = catalog::search(&db, "waybar", 10).unwrap();
    assert_eq!(res.len(), 1, "Query 'waybar' must match ONLY waybar package");
    assert_eq!(res[0].id, "com.catppuccin.waybar");

    let res = catalog::search(&db, "bar_themes_modules", 10).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id, "com.catppuccin.waybar");

    // 6. Queries that match ONLY hyprland
    let hypr_exclusive_terms = [
        "hyprland",
        "desktop_setups",
        "hyprlock",
        "hypridle",
        "latte",
        "frappe",
        "macchiato",
    ];
    for term in hypr_exclusive_terms {
        let res = catalog::search(&db, term, 10).unwrap();
        assert_eq!(
            res.len(),
            1,
            "Term '{term}' must match exclusively hyprland"
        );
        assert_eq!(res[0].id, "com.catppuccin.hyprland");
    }

    // 7. Non-matching query
    let res = catalog::search(&db, "xfce_openbox_sway_absent", 10).unwrap();
    assert!(res.is_empty());
}

// ---------------------------------------------------------------------------
// 2. Edge-case query parsing & FTS5 sanitization
// ---------------------------------------------------------------------------
#[test]
fn test_catalog_search_edge_case_query_parsing_and_sanitization() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().unwrap();
    catalog::import_registry(&db, &reg_dir).unwrap();

    // A. Whitespace variations (must fall back to listing validated non-yanked)
    let ws_queries = ["", " ", "   ", "\t", "\n", "\r\n", "  \t \n  "];
    for q in ws_queries {
        let res = catalog::search(&db, q, 10).expect("Whitespace query must succeed");
        assert_eq!(
            res.len(),
            2,
            "Query '{q:?}' should return all 2 validated packages"
        );
    }

    // B. Punctuation and standalone special characters
    let punctuation_queries = [
        "-", "_", ".", ":", "/", "\\", "!", "?", ";", "@", "#", "$", "%", "^", "&", "(", ")",
        "[", "]", "{", "}", "+", "=", "~", "`", "<", ">", "|", ",",
    ];
    for q in punctuation_queries {
        // Must not crash or panic; should return Ok (either empty or matched)
        let res = catalog::search(&db, q, 10);
        assert!(
            res.is_ok(),
            "Punctuation query '{q}' must not cause unhandled error or panic"
        );
    }

    // C. Quotation mark stress-testing (double quotes, mismatched quotes)
    let quote_queries = [
        "\"",
        "\"\"",
        "\"\"\"",
        "\"catppuccin\"",
        "\"hyprland\"",
        "\"waybar\"",
        "hyprland\"",
        "\"hyprland",
        "\"catppuccin\" \"hyprland\"",
        "\"\"catppuccin\"\"",
    ];
    for q in quote_queries {
        let res = catalog::search(&db, q, 10);
        assert!(
            res.is_ok(),
            "Quote query {q:?} must be safely handled without SQL syntax error"
        );
    }

    // D. FTS5 reserved keywords
    let keyword_queries = [
        "AND",
        "OR",
        "NOT",
        "NEAR",
        "MATCH",
        "RANK",
        "column:name",
        "hyprland AND waybar",
        "hyprland OR waybar",
        "hyprland NOT mocha",
        "NEAR(hyprland, 1)",
    ];
    for q in keyword_queries {
        let res = catalog::search(&db, q, 10);
        assert!(
            res.is_ok(),
            "Keyword query '{q}' must be handled cleanly"
        );
    }

    // E. Wildcards and asterisks
    let wildcard_queries = [
        "*", "**", "***", "cat*", "*cat*", "hypr*", "*land", "*waybar*",
    ];
    for q in wildcard_queries {
        let res = catalog::search(&db, q, 10);
        assert!(
            res.is_ok(),
            "Wildcard query '{q}' must be safely handled"
        );
    }

    // F. SQL Injection attempts
    let sqli_queries = [
        "' OR '1'='1",
        "'; DROP TABLE catalog; --",
        "'; DROP TABLE catalog_fts; --",
        "\" OR \"\"=\"\"",
        "1; SELECT * FROM transactions;",
        "admin' --",
        "' UNION SELECT id, version FROM catalog --",
    ];
    for q in sqli_queries {
        let res = catalog::search(&db, q, 10);
        assert!(res.is_ok(), "SQL injection payload '{q}' must be safely handled");
        // Verify catalog table was not dropped
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM catalog", [], |r| r.get(0))
            .expect("Catalog table must still exist and be intact");
        assert_eq!(count, 2);
    }

    // G. Unicode, emojis, and non-ASCII
    let unicode_queries = [
        "Catppuccin™",
        "hyprland 🦀",
        "Café",
        "こんにちは",
        "مرحبا",
        "русский",
        "🚀🌟🔥",
        "Cätppüccïn",
    ];
    for q in unicode_queries {
        let res = catalog::search(&db, q, 10);
        assert!(res.is_ok(), "Unicode query '{q}' must succeed without panic");
    }

    // H. Long queries (stressing tokenization buffer limits)
    let long_word = "a".repeat(5000);
    let res = catalog::search(&db, &long_word, 10);
    assert!(res.is_ok(), "Long single token must not crash");

    let many_words = (0..500)
        .map(|i| format!("term{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let res = catalog::search(&db, &many_words, 10);
    assert!(res.is_ok(), "500-term query must not crash");
}

// ---------------------------------------------------------------------------
// 3. Limit boundary behaviors
// ---------------------------------------------------------------------------
#[test]
fn test_catalog_search_limit_boundaries() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().unwrap();
    catalog::import_registry(&db, &reg_dir).unwrap();

    // 1. Limit 0 -> returns empty Vec
    let res_empty_0 = catalog::search(&db, "", 0).unwrap();
    assert_eq!(res_empty_0.len(), 0);
    let res_match_0 = catalog::search(&db, "catppuccin", 0).unwrap();
    assert_eq!(res_match_0.len(), 0);

    // 2. Limit 1 -> returns exactly 1 item even when 2 match
    let res_empty_1 = catalog::search(&db, "", 1).unwrap();
    assert_eq!(res_empty_1.len(), 1);
    let res_match_1 = catalog::search(&db, "catppuccin", 1).unwrap();
    assert_eq!(res_match_1.len(), 1);

    // 3. Limit 2 -> returns both items
    let res_empty_2 = catalog::search(&db, "", 2).unwrap();
    assert_eq!(res_empty_2.len(), 2);
    let res_match_2 = catalog::search(&db, "catppuccin", 2).unwrap();
    assert_eq!(res_match_2.len(), 2);

    // 4. Limit > number of available items
    let res_empty_100 = catalog::search(&db, "", 100).unwrap();
    assert_eq!(res_empty_100.len(), 2);
    let res_match_100 = catalog::search(&db, "catppuccin", 100).unwrap();
    assert_eq!(res_match_100.len(), 2);

    // 5. Alphabetical sorting check on empty query (ORDER BY name)
    let res_all = catalog::search(&db, "", 10).unwrap();
    assert_eq!(res_all.len(), 2);
    assert!(
        res_all[0].name <= res_all[1].name,
        "Empty search must order by name ascending"
    );

    // 6. Very large limit boundary (i64::MAX as usize)
    let large_limit = i64::MAX as usize;
    let res_large = catalog::search(&db, "catppuccin", large_limit).unwrap();
    assert_eq!(res_large.len(), 2);
}

// ---------------------------------------------------------------------------
// 4. Multi-package isolation & yanking behavior
// ---------------------------------------------------------------------------
#[test]
fn test_multi_package_yank_and_isolation() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().unwrap();
    catalog::import_registry(&db, &reg_dir).unwrap();

    // Mark com.catppuccin.hyprland as yanked
    db.conn
        .execute(
            "UPDATE catalog SET yanked=1 WHERE id='com.catppuccin.hyprland'",
            [],
        )
        .unwrap();

    // Search should now ONLY return com.catppuccin.waybar
    let res_empty = catalog::search(&db, "", 10).unwrap();
    assert_eq!(res_empty.len(), 1);
    assert_eq!(res_empty[0].id, "com.catppuccin.waybar");

    let res_cat = catalog::search(&db, "catppuccin", 10).unwrap();
    assert_eq!(res_cat.len(), 1);
    assert_eq!(res_cat[0].id, "com.catppuccin.waybar");

    // Hyprland search should now return empty
    let res_hypr = catalog::search(&db, "hyprland", 10).unwrap();
    assert!(res_hypr.is_empty(), "Yanked package must not appear in search");

    // Loading manifest of yanked package must fail
    let manifest_err = catalog::manifest(&db, "com.catppuccin.hyprland", "1.0.0");
    assert!(manifest_err.is_err(), "Yanked package must not be installable");
    match manifest_err.err().unwrap() {
        WoderriseError::Registry(msg) => assert!(msg.contains("not installable")),
        other => panic!("Expected WoderriseError::Registry, got: {other:?}"),
    }

    // Waybar manifest must still load cleanly
    let waybar_m = catalog::manifest(&db, "com.catppuccin.waybar", "1.0.0");
    assert!(waybar_m.is_ok(), "Non-yanked package must remain installable");

    // Un-yank hyprland and verify full recovery
    db.conn
        .execute(
            "UPDATE catalog SET yanked=0 WHERE id='com.catppuccin.hyprland'",
            [],
        )
        .unwrap();
    let res_recovered = catalog::search(&db, "catppuccin", 10).unwrap();
    assert_eq!(res_recovered.len(), 2);
}

// ---------------------------------------------------------------------------
// 5. Concurrent installation simulation across waybar and hyprland
// ---------------------------------------------------------------------------
#[test]
fn test_concurrent_installation_simulation_waybar_and_hyprland() {
    let (_tmp, home, backups, db_path) = create_temp_env("concurrent-installs");
    let waybar_dir = waybar_pkg_dir();
    let hyprland_dir = hyprland_pkg_dir();

    // We initialize SQLite WAL database file
    let db = Database::open(&db_path).unwrap();
    drop(db);

    let engine = Arc::new(Mutex::new(InstallationEngine {
        db: Database::open(&db_path).unwrap(),
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    }));

    let barrier = Arc::new(Barrier::new(2));

    // Spawn Thread 1 for Waybar
    let e1 = Arc::clone(&engine);
    let b1 = Arc::clone(&barrier);
    let w_dir = waybar_dir.clone();
    let h1 = thread::spawn(move || {
        b1.wait();
        let guard = e1.lock().unwrap();
        guard
            .install(&w_dir, "thread_waybar")
            .expect("Waybar install must succeed")
    });

    // Spawn Thread 2 for Hyprland
    let e2 = Arc::clone(&engine);
    let b2 = Arc::clone(&barrier);
    let hy_dir = hyprland_dir.clone();
    let h2 = thread::spawn(move || {
        b2.wait();
        let guard = e2.lock().unwrap();
        guard
            .install(&hy_dir, "thread_hyprland")
            .expect("Hyprland install must succeed")
    });

    let waybar_txn = h1.join().expect("Waybar install thread panicked");
    let hyprland_txn = h2.join().expect("Hyprland install thread panicked");

    // Verification on disk and database
    let guard = engine.lock().unwrap();

    // Verify installed_packages has both
    let installed_count: i64 = guard
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(installed_count, 2);

    let active_waybar_txn: String = guard
        .db
        .conn
        .query_row(
            "SELECT install_txn_id FROM installed_packages WHERE package_id='com.catppuccin.waybar'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_waybar_txn, waybar_txn);

    let active_hypr_txn: String = guard
        .db
        .conn
        .query_row(
            "SELECT install_txn_id FROM installed_packages WHERE package_id='com.catppuccin.hyprland'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active_hypr_txn, hyprland_txn);

    // Verify operations table has 1 op for waybar and 9 ops for hyprland
    let waybar_ops: i64 = guard
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM operations WHERE txn_id=?1 AND status='COMPLETED'",
            [&waybar_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(waybar_ops, 1);

    let hypr_ops: i64 = guard
        .db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM operations WHERE txn_id=?1 AND status='COMPLETED'",
            [&hyprland_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hypr_ops, 9);

    // Verify files on disk:
    // 1. Waybar file
    let waybar_target = home.join(".config/waybar/catppuccin-mocha.css");
    assert!(waybar_target.is_file());
    assert_eq!(
        sha256_file(&waybar_target).unwrap(),
        sha256_file(waybar_dir.join("artifacts/mocha.css")).unwrap()
    );

    // 2. Hyprland 9 files
    let hypr_manifest = Manifest::from_file(hyprland_dir.join("woderrise.yaml")).unwrap();
    for f in &hypr_manifest.install.files {
        let target = expand_target(&f.target, &home).unwrap();
        assert!(target.is_file(), "Hyprland target must exist: {}", target.display());
        assert_eq!(
            sha256_file(&target).unwrap(),
            sha256_file(hyprland_dir.join(&f.source)).unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// 6. Interleaved and concurrent rollback simulations
// ---------------------------------------------------------------------------
#[test]
fn test_concurrent_and_interleaved_rollback_simulation() {
    let (_tmp, home, backups, db_path) = create_temp_env("rollback-sim");
    let waybar_dir = waybar_pkg_dir();
    let hyprland_dir = hyprland_pkg_dir();

    let engine = InstallationEngine {
        db: Database::open(&db_path).unwrap(),
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    // Install both packages sequentially first
    let waybar_txn = engine.install(&waybar_dir, "seq_waybar").unwrap();
    let hyprland_txn = engine.install(&hyprland_dir, "seq_hyprland").unwrap();

    let waybar_target = home.join(".config/waybar/catppuccin-mocha.css");
    let hypr_bar_cfg = home.join(".config/waybar/config.jsonc");
    let hypr_conf = home.join(".config/hypr/hyprland.conf");

    assert!(waybar_target.is_file());
    assert!(hypr_bar_cfg.is_file());
    assert!(hypr_conf.is_file());

    // Step 1: Rollback waybar
    let rb_waybar_txn = engine.rollback(&waybar_txn, false).expect("Rollback waybar");
    assert!(!rb_waybar_txn.is_empty());

    // Waybar file must be removed
    assert!(!waybar_target.exists(), "Waybar file must be deleted on clean rollback");

    // All Hyprland files MUST REMAIN INTACT!
    assert!(
        hypr_bar_cfg.is_file(),
        "Hyprland waybar config must still exist after waybar package rollback"
    );
    assert!(
        hypr_conf.is_file(),
        "Hyprland main config must still exist after waybar package rollback"
    );

    // Verify installed_packages has ONLY hyprland
    let installed_ids: Vec<String> = engine
        .db
        .conn
        .prepare("SELECT package_id FROM installed_packages")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(installed_ids, vec!["com.catppuccin.hyprland".to_string()]);

    // Step 2: Rollback hyprland
    let rb_hypr_txn = engine.rollback(&hyprland_txn, false).expect("Rollback hyprland");
    assert!(!rb_hypr_txn.is_empty());

    // Now hyprland files must also be removed
    assert!(!hypr_bar_cfg.exists(), "Hyprland waybar config must be removed");
    assert!(!hypr_conf.exists(), "Hyprland conf must be removed");

    // installed_packages must now be completely empty
    let remaining_installed: i64 = engine
        .db
        .conn
        .query_row("SELECT COUNT(*) FROM installed_packages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining_installed, 0);

    // Transactions table should record both as ROLLED_BACK
    let waybar_status: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id=?1",
            [&waybar_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(waybar_status, "ROLLED_BACK");

    let hypr_status: String = engine
        .db
        .conn
        .query_row(
            "SELECT status FROM transactions WHERE txn_id=?1",
            [&hyprland_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hypr_status, "ROLLED_BACK");
}

// ---------------------------------------------------------------------------
// 7. Reverse-order installation & rollback with pre-existing custom configs
// ---------------------------------------------------------------------------
#[test]
fn test_reverse_order_installation_and_rollback_with_preexisting_files() {
    let (_tmp, home, backups, db_path) = create_temp_env("reverse-order-preexisting");
    let waybar_dir = waybar_pkg_dir();
    let hyprland_dir = hyprland_pkg_dir();

    // Create pre-existing user configurations
    let custom_waybar_css = home.join(".config/waybar/catppuccin-mocha.css");
    fs::create_dir_all(custom_waybar_css.parent().unwrap()).unwrap();
    let user_css_bytes = b"/* Custom user waybar theme pre-existing */\n#waybar { font-size: 14px; }\n";
    fs::write(&custom_waybar_css, user_css_bytes).unwrap();

    let custom_hypr_conf = home.join(".config/hypr/hyprland.conf");
    fs::create_dir_all(custom_hypr_conf.parent().unwrap()).unwrap();
    let user_hypr_bytes = b"# Custom user hyprland configuration\n$mod = SUPER\n";
    fs::write(&custom_hypr_conf, user_hypr_bytes).unwrap();

    let custom_bar_json = home.join(".config/waybar/config.jsonc");
    let user_bar_bytes = b"{\n  \"layer\": \"top\",\n  \"custom\": true\n}\n";
    fs::write(&custom_bar_json, user_bar_bytes).unwrap();

    let engine = InstallationEngine {
        db: Database::open(&db_path).unwrap(),
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    // Install HYPRLAND first
    let hyprland_txn = engine.install(&hyprland_dir, "inst_hypr").unwrap();

    // Install WAYBAR second
    let waybar_txn = engine.install(&waybar_dir, "inst_waybar").unwrap();

    // Verify new contents were applied
    assert_ne!(fs::read(&custom_waybar_css).unwrap(), user_css_bytes);
    assert_ne!(fs::read(&custom_hypr_conf).unwrap(), user_hypr_bytes);
    assert_ne!(fs::read(&custom_bar_json).unwrap(), user_bar_bytes);

    // Rollback HYPRLAND first (reverse order of install)
    engine.rollback(&hyprland_txn, false).expect("Rollback hyprland");

    // Hyprland files must be restored to original pre-existing user bytes
    assert_eq!(
        fs::read(&custom_hypr_conf).unwrap(),
        user_hypr_bytes,
        "Custom user hyprland.conf must be restored"
    );
    assert_eq!(
        fs::read(&custom_bar_json).unwrap(),
        user_bar_bytes,
        "Custom user config.jsonc must be restored"
    );

    // Waybar file must STILL have the waybar package content!
    assert_eq!(
        sha256_file(&custom_waybar_css).unwrap(),
        sha256_file(waybar_dir.join("artifacts/mocha.css")).unwrap(),
        "Waybar package file must NOT be affected by hyprland rollback"
    );

    // Rollback WAYBAR second
    engine.rollback(&waybar_txn, false).expect("Rollback waybar");

    // Waybar file must be restored to original pre-existing user bytes!
    assert_eq!(
        fs::read(&custom_waybar_css).unwrap(),
        user_css_bytes,
        "Custom user catppuccin-mocha.css must be restored"
    );
}

// ---------------------------------------------------------------------------
// 8. Cross-package file conflict detection and forced rollback resolution
// ---------------------------------------------------------------------------
#[test]
fn test_cross_package_file_conflict_detection_on_rollback() {
    let (_tmp, home, backups, db_path) = create_temp_env("conflict-detection");
    let waybar_dir = waybar_pkg_dir();

    let engine = InstallationEngine {
        db: Database::open(&db_path).unwrap(),
        backups: BackupManager::new(&backups).unwrap(),
        home: home.clone(),
    };

    let txn = engine.install(&waybar_dir, "test_conflict").unwrap();
    let target_file = home.join(".config/waybar/catppuccin-mocha.css");
    assert!(target_file.is_file());

    // External change: user or another tool modifies the file after installation
    fs::write(&target_file, b"/* Tampered content modified post-install */\n").unwrap();

    // Attempting unforced rollback must detect Conflict!
    let rb_err = engine.rollback(&txn, false);
    assert!(
        rb_err.is_err(),
        "Unforced rollback must reject when target file was tampered"
    );
    match rb_err.err().unwrap() {
        WoderriseError::Conflict(path) => {
            assert_eq!(path, target_file.to_string_lossy());
        }
        other => panic!("Expected WoderriseError::Conflict, got: {other:?}"),
    }

    // Target file must remain untouched by failed rollback
    assert_eq!(
        fs::read(&target_file).unwrap(),
        b"/* Tampered content modified post-install */\n"
    );

    // Forced rollback must override conflict
    let forced_rb = engine.rollback(&txn, true).expect("Forced rollback must succeed");
    assert!(!forced_rb.is_empty());

    // File was clean install, so forced rollback deletes it
    assert!(!target_file.exists());
}

// ---------------------------------------------------------------------------
// 9. Regression Test: Re-import succeeds on populated contentless FTS5 table
// ---------------------------------------------------------------------------
#[test]
fn test_contentless_fts5_reimport_success() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().unwrap();

    // First import succeeds because catalog_fts is empty
    let count = catalog::import_registry(&db, &reg_dir).expect("Initial import must succeed");
    assert_eq!(count, 2);

    // Second import succeeds cleanly using FTS5 delete-all
    let reimport_res = catalog::import_registry(&db, &reg_dir);
    assert!(
        reimport_res.is_ok(),
        "Re-import must succeed on populated contentless FTS5 table"
    );
    assert_eq!(reimport_res.unwrap(), 2);

    // Verify search works after re-import
    let search_res = catalog::search(&db, "hyprland", 10).expect("search after re-import");
    assert_eq!(search_res.len(), 1);
    assert_eq!(search_res[0].id, "com.catppuccin.hyprland");
}

// ---------------------------------------------------------------------------
// 10. Regression Test: Upsert succeeds on populated contentless FTS5 table
// ---------------------------------------------------------------------------
#[test]
fn test_contentless_fts5_upsert_success() {
    let reg_dir = registry_packages_dir();
    let db = Database::in_memory().unwrap();

    // Import initial registry
    catalog::import_registry(&db, &reg_dir).unwrap();

    // Fetch an existing entry and attempt to upsert it
    let mut entry = catalog::get(&db, "com.catppuccin.hyprland", "1.0.0").unwrap();
    entry.description = "Updated description for FTS5 upsert test".to_string();

    // Calling upsert invokes rebuild_fts, which succeeds using FTS5 delete-all
    let upsert_res = catalog::upsert(&db, &entry);
    assert!(
        upsert_res.is_ok(),
        "Upsert must succeed on populated contentless FTS5 table"
    );

    // Verify search works after upsert
    let search_res = catalog::search(&db, "Updated description", 10).expect("search after upsert");
    assert_eq!(search_res.len(), 1);
    assert_eq!(search_res[0].id, "com.catppuccin.hyprland");
}

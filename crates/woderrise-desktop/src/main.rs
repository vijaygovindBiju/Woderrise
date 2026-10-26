//! Desktop GUI application entry point for Woderrise.
//! Connects Slint AppWindow callbacks to woderrise-core engines and background worker threads.

use slint::ComponentHandle;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
};
use woderrise_core::{
    catalog::{self, CatalogEntry},
    fetcher::{self, FetchOptions},
    BackupManager, Database, InstallationEngine, ProgressCallback,
};
use woderrise_desktop::{
    catalog_entries_to_cards_model, diff_line_to_slint, install_plan_to_slint_files,
    AppWindow, NavTab, PackageDetailViewModel, SettingsState,
};

// ============================================================================
// Directory & Path Resolvers
// ============================================================================

/// Returns user HOME directory with fallback to current directory.
fn resolve_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolves application data directory (respecting `WODERRISE_DATA_DIR`).
fn resolve_data_dir(home: &Path) -> PathBuf {
    if let Some(val) = std::env::var_os("WODERRISE_DATA_DIR") {
        PathBuf::from(val)
    } else {
        home.join(".local/share/woderrise")
    }
}

/// Locates packages registry directory from environment or candidate relative paths.
fn resolve_registry_dir() -> PathBuf {
    if let Some(val) = std::env::var_os("WODERRISE_REGISTRY_DIR") {
        return PathBuf::from(val);
    }
    for candidate in &["registry/packages", "../registry/packages", "../../registry/packages"] {
        let p = PathBuf::from(candidate);
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from("registry/packages")
}

/// Recursively copies directory contents ignoring .git directories.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if file_name == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&file_name);
        if src_path.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ============================================================================
// Database & Catalog Helpers
// ============================================================================

/// Fetches set of all installed package IDs from SQLite `installed_packages` table.
fn query_installed_ids(db: &Database) -> HashSet<String> {
    let mut installed = HashSet::new();
    if let Ok(mut stmt) = db.conn.prepare("SELECT package_id FROM installed_packages") {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for r in rows.flatten() {
                installed.insert(r);
            }
        }
    }
    installed
}

/// Counts total transactions recorded in SQLite `transactions` table.
fn query_total_transactions(db: &Database) -> i32 {
    db.conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as i32
}

// ============================================================================
// Main UI Controller State
// ============================================================================

struct AppState {
    home_dir: PathBuf,
    data_dir: PathBuf,
    registry_dir: PathBuf,
    all_entries: Vec<CatalogEntry>,
    filtered_entries: Vec<CatalogEntry>,
    selected_index: usize,
    selected_category: String,
    settings: SettingsState,
}

impl AppState {
    fn new(home_dir: PathBuf, data_dir: PathBuf, registry_dir: PathBuf) -> Self {
        let settings = SettingsState::load_from_dir(&data_dir);
        Self {
            home_dir,
            data_dir,
            registry_dir,
            all_entries: Vec::new(),
            filtered_entries: Vec::new(),
            selected_index: 0,
            selected_category: "All".to_string(),
            settings,
        }
    }

    fn selected_entry(&self) -> Option<&CatalogEntry> {
        self.filtered_entries.get(self.selected_index)
    }

    fn apply_filter(&mut self, query: &str) {
        let clean_q = query.trim().to_lowercase();
        let clean_cat = self.selected_category.trim().to_lowercase();

        self.filtered_entries = self
            .all_entries
            .iter()
            .filter(|e| {
                if clean_cat != "all" && !clean_cat.is_empty() && e.category.to_lowercase() != clean_cat {
                    return false;
                }
                if clean_q.is_empty() {
                    return true;
                }
                e.id.to_lowercase().contains(&clean_q)
                    || e.name.to_lowercase().contains(&clean_q)
                    || e.description.to_lowercase().contains(&clean_q)
            })
            .cloned()
            .collect();
        self.selected_index = 0;
    }
}

// ============================================================================
// Application Entry Point
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = resolve_home_dir();
    let data = resolve_data_dir(&home);
    let reg_dir = resolve_registry_dir();

    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(data.join("backups"))?;

    // 1. Initialize SQLite Database & Import Registry
    let db = Database::open(data.join("journal.db"))?;
    catalog::migrate(&db)?;
    if reg_dir.is_dir() {
        let _ = catalog::import_registry(&db, &reg_dir);
    }

    // 2. Instantiate Slint Application Window
    let app = AppWindow::new()?;
    let app_state = Arc::new(Mutex::new(AppState::new(
        home.clone(),
        data.clone(),
        reg_dir.clone(),
    )));

    // 3. Load initial catalog entries
    let all_entries = catalog::search(&db, "", 200).unwrap_or_default();
    let installed_ids = query_installed_ids(&db);

    {
        let mut state = app_state.lock().unwrap();
        state.all_entries = all_entries.clone();
        state.filtered_entries = all_entries.clone();
        state.selected_index = 0;
    }

    // Bind cards model
    let cards = catalog_entries_to_cards_model(&all_entries, &installed_ids);
    app.set_packages(cards);
    app.set_total_packages_count(all_entries.len() as i32);

    // Bind initial selected package details if present
    if let Some(first) = all_entries.first() {
        if let Ok(manifest) = catalog::manifest(&db, &first.id, &first.version) {
            let is_inst = installed_ids.contains(&first.id);
            let vm = PackageDetailViewModel::from_manifest(&manifest, is_inst);
            vm.apply_to_window(&app);
        }
    }

    // Bind initial settings properties
    let total_txns = query_total_transactions(&db);
    app_state.lock().unwrap().settings.apply_to_window(&app, &data, total_txns);
    app.set_status_message(format!("Registry loaded ({} packages)", all_entries.len()).into());

    // ========================================================================
    // Callback 1: Navigation Tab Changed
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_clone = data.clone();

        app.on_nav_changed(move |tab: NavTab| {
            let Some(app) = app_weak.upgrade() else { return; };
            let Ok(db) = Database::open(data_clone.join("journal.db")) else { return; };
            let installed_ids = query_installed_ids(&db);
            let mut state = state_rc.lock().unwrap();

            match tab {
                NavTab::Installed => {
                    // Filter to only installed packages
                    state.filtered_entries = state
                        .all_entries
                        .iter()
                        .filter(|e| installed_ids.contains(&e.id))
                        .cloned()
                        .collect();
                    state.selected_index = 0;
                }
                NavTab::Settings => {
                    let txns = query_total_transactions(&db);
                    state.settings.apply_to_window(&app, &data_clone, txns);
                    return;
                }
                _ => {
                    let q = app.get_search_query().to_string();
                    state.apply_filter(&q);
                }
            }

            let cards = catalog_entries_to_cards_model(&state.filtered_entries, &installed_ids);
            app.set_packages(cards);

            if let Some(entry) = state.selected_entry() {
                if let Ok(m) = catalog::manifest(&db, &entry.id, &entry.version) {
                    let is_inst = installed_ids.contains(&entry.id);
                    let vm = PackageDetailViewModel::from_manifest(&m, is_inst);
                    vm.apply_to_window(&app);
                }
            }
        });
    }

    // ========================================================================
    // Callback 2: Search Query Changed
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_clone = data.clone();

        app.on_search_changed(move |query| {
            let Some(app) = app_weak.upgrade() else { return; };
            let Ok(db) = Database::open(data_clone.join("journal.db")) else { return; };
            let installed_ids = query_installed_ids(&db);

            let mut state = state_rc.lock().unwrap();
            state.apply_filter(query.as_str());

            let cards = catalog_entries_to_cards_model(&state.filtered_entries, &installed_ids);
            app.set_packages(cards);

            if let Some(first) = state.selected_entry() {
                if let Ok(m) = catalog::manifest(&db, &first.id, &first.version) {
                    let is_inst = installed_ids.contains(&first.id);
                    let vm = PackageDetailViewModel::from_manifest(&m, is_inst);
                    vm.apply_to_window(&app);
                }
            }
        });
    }

    // ========================================================================
    // Callback 3: Category Selected
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_clone = data.clone();

        app.on_category_selected(move |category| {
            let Some(app) = app_weak.upgrade() else { return; };
            let Ok(db) = Database::open(data_clone.join("journal.db")) else { return; };
            let installed_ids = query_installed_ids(&db);

            let mut state = state_rc.lock().unwrap();
            state.selected_category = category.to_string();
            let q = app.get_search_query().to_string();
            state.apply_filter(&q);

            let cards = catalog_entries_to_cards_model(&state.filtered_entries, &installed_ids);
            app.set_packages(cards);

            if let Some(first) = state.selected_entry() {
                if let Ok(m) = catalog::manifest(&db, &first.id, &first.version) {
                    let is_inst = installed_ids.contains(&first.id);
                    let vm = PackageDetailViewModel::from_manifest(&m, is_inst);
                    vm.apply_to_window(&app);
                }
            }
        });
    }

    // ========================================================================
    // Callback 4: Package Selected from List
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_clone = data.clone();

        app.on_package_selected(move |idx: i32| {
            let Some(app) = app_weak.upgrade() else { return; };
            let mut state = state_rc.lock().unwrap();
            if idx < 0 || (idx as usize) >= state.filtered_entries.len() {
                return;
            }
            state.selected_index = idx as usize;
            let entry = state.filtered_entries[state.selected_index].clone();
            drop(state);

            let Ok(db) = Database::open(data_clone.join("journal.db")) else { return; };
            let installed_ids = query_installed_ids(&db);

            if let Ok(manifest) = catalog::manifest(&db, &entry.id, &entry.version) {
                let is_inst = installed_ids.contains(&entry.id);
                let vm = PackageDetailViewModel::from_manifest(&manifest, is_inst);
                vm.apply_to_window(&app);
            }
        });
    }

    // Helper closure for Diff Computation
    let compute_diff_fn = {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        move || {
            let Some(app) = app_weak.upgrade() else { return; };
            let state = state_rc.lock().unwrap();
            let Some(entry) = state.selected_entry() else { return; };

            let manifest_path = entry.manifest_path.clone();
            let data_dir = state.data_dir.clone();
            let home_dir = state.home_dir.clone();
            drop(state);

            let result = (|| -> woderrise_core::Result<woderrise_core::diff::InstallPlan> {
                let db = Database::open(data_dir.join("journal.db"))?;
                let engine = InstallationEngine {
                    db,
                    backups: BackupManager::new(data_dir.join("backups"))?,
                    home: home_dir,
                };
                engine.compute_plan(&manifest_path)
            })();

            match result {
                Ok(plan) => {
                    let file_diff_models = install_plan_to_slint_files(&plan);
                    app.set_diff_files(file_diff_models);
                    if let Some(first_file) = plan.file_diffs.first() {
                        app.set_diff_target_path(first_file.target_path.display().to_string().into());
                        let backup_str = match &first_file.planned_backup_path {
                            Some(p) => p.display().to_string(),
                            None => "None (clean install)".to_string(),
                        };
                        app.set_diff_backup_path(backup_str.into());
                        let action_str = match first_file.action {
                            woderrise_core::diff::FileDiffAction::Create => "Create",
                            woderrise_core::diff::FileDiffAction::Modify => "Modify",
                            woderrise_core::diff::FileDiffAction::Unchanged => "Unchanged",
                        };
                        app.set_diff_action_type(action_str.into());

                        let mut adds = 0;
                        let mut dels = 0;
                        let mut lines = Vec::new();
                        for l in &first_file.lines {
                            if l.kind == woderrise_core::diff::DiffLineKind::Addition {
                                adds += 1;
                            } else if l.kind == woderrise_core::diff::DiffLineKind::Deletion {
                                dels += 1;
                            }
                            lines.push(diff_line_to_slint(l));
                        }
                        app.set_diff_additions(adds);
                        app.set_diff_deletions(dels);
                        let lines_model: slint::ModelRc<woderrise_desktop::DiffLineData> =
                            Rc::new(slint::VecModel::from(lines)).into();
                        app.set_diff_lines(lines_model);
                    }
                    app.set_diff_modal_open(true);
                    app.set_show_diff_modal(true);
                }
                Err(err) => {
                    app.set_status_message(format!("Diff calculation failed: {}", err).into());
                }
            }
        }
    };

    // Callback 5: Trigger Diff Preview
    app.on_trigger_diff(compute_diff_fn);

    // Callback 6: Close Diff Modal
    {
        let app_weak = app.as_weak();
        app.on_close_diff_modal(move || {
            if let Some(app) = app_weak.upgrade() {
                app.set_diff_modal_open(false);
                app.set_show_diff_modal(false);
            }
        });
    }

    // Helper closure for Transactional Install
    let run_install_fn = {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        move || {
            let Some(app) = app_weak.upgrade() else { return; };
            let state = state_rc.lock().unwrap();
            let Some(entry) = state.selected_entry() else { return; };

            let manifest_path = entry.manifest_path.clone();
            let pkg_name = entry.name.clone();
            let data_dir = state.data_dir.clone();
            let home_dir = state.home_dir.clone();
            drop(state);

            // Activate progress modal immediately
            app.set_progress_active(true);
            app.set_show_progress_modal(true);
            app.set_progress_title(format!("Installing {}", pkg_name).into());
            app.set_progress_stage("Resolve".into());
            app.set_progress_message("Resolving package manifest...".into());
            app.set_progress_fraction(0.05);
            app.set_progress_percentage(5);

            let weak_thread = app_weak.clone();
            let state_thread = Arc::clone(&state_rc);

            std::thread::spawn(move || {
                let weak_cb = weak_thread.clone();
                let progress_cb: ProgressCallback = Box::new(move |stage, msg, frac| {
                    let stage_str = stage.as_str().to_string();
                    let msg_str = msg.to_string();
                    let _ = weak_cb.upgrade_in_event_loop(move |app| {
                        app.set_progress_stage(stage_str.into());
                        app.set_progress_message(msg_str.into());
                        app.set_progress_fraction(frac);
                        let pct = if frac.is_nan() || frac < 0.0 {
                            0
                        } else if frac > 1.0 {
                            100
                        } else {
                            (frac * 100.0).round() as i32
                        };
                        app.set_progress_percentage(pct);
                    });
                });

                let result = (|| -> woderrise_core::Result<String> {
                    let db = Database::open(data_dir.join("journal.db"))?;
                    let engine = InstallationEngine {
                        db,
                        backups: BackupManager::new(data_dir.join("backups"))?,
                        home: home_dir,
                    };
                    engine.install_with_progress(&manifest_path, "desktop_gui", Some(progress_cb))
                })();

                let _ = weak_thread.upgrade_in_event_loop(move |app| {
                    app.set_progress_active(false);
                    app.set_show_progress_modal(false);
                    match result {
                        Ok(txn_id) => {
                            app.set_status_message(
                                format!("Installed {} successfully ({})", pkg_name, txn_id).into(),
                            );
                            app.set_sel_installed(true);

                            // Refresh cards model
                            if let Ok(db) = Database::open(data_dir.join("journal.db")) {
                                let installed_ids = query_installed_ids(&db);
                                let state = state_thread.lock().unwrap();
                                let cards = catalog_entries_to_cards_model(
                                    &state.filtered_entries,
                                    &installed_ids,
                                );
                                app.set_packages(cards);
                                let txns = query_total_transactions(&db);
                                state.settings.apply_to_window(&app, &data_dir, txns);
                            }
                        }
                        Err(err) => {
                            app.set_status_message(format!("Install failed: {}", err).into());
                        }
                    }
                });
            });
        }
    };

    // Callback 7 & 8: Trigger Install / Apply Diff Install
    app.on_trigger_install(run_install_fn.clone());
    app.on_apply_diff_install(run_install_fn);

    // ========================================================================
    // Callback 9: Trigger Asynchronous Transactional Rollback
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);

        app.on_trigger_rollback(move || {
            let Some(app) = app_weak.upgrade() else { return; };
            let state = state_rc.lock().unwrap();
            let Some(entry) = state.selected_entry() else { return; };

            let pkg_id = entry.id.clone();
            let pkg_name = entry.name.clone();
            let data_dir = state.data_dir.clone();
            let home_dir = state.home_dir.clone();
            drop(state);

            // Activate progress modal
            app.set_progress_active(true);
            app.set_show_progress_modal(true);
            app.set_progress_title(format!("Rolling back {}", pkg_name).into());
            app.set_progress_stage("Plan".into());
            app.set_progress_message("Locating transaction in journal...".into());
            app.set_progress_fraction(0.1);
            app.set_progress_percentage(10);

            let weak_thread = app_weak.clone();
            let state_thread = Arc::clone(&state_rc);

            std::thread::spawn(move || {
                let result = (|| -> woderrise_core::Result<String> {
                    let db = Database::open(data_dir.join("journal.db"))?;
                    let txn_id: String = db.conn.query_row(
                        "SELECT txn_id FROM transactions WHERE package_id = ?1 AND txn_type = 'INSTALL' AND status = 'COMMITTED' ORDER BY started_at DESC LIMIT 1",
                        rusqlite::params![pkg_id],
                        |row| row.get(0),
                    )?;

                    let engine = InstallationEngine {
                        db,
                        backups: BackupManager::new(data_dir.join("backups"))?,
                        home: home_dir,
                    };
                    engine.rollback(&txn_id, false)
                })();

                let _ = weak_thread.upgrade_in_event_loop(move |app| {
                    app.set_progress_active(false);
                    app.set_show_progress_modal(false);
                    match result {
                        Ok(rb_id) => {
                            app.set_status_message(
                                format!("Rolled back {} successfully ({})", pkg_name, rb_id).into(),
                            );
                            app.set_sel_installed(false);

                            if let Ok(db) = Database::open(data_dir.join("journal.db")) {
                                let installed_ids = query_installed_ids(&db);
                                let state = state_thread.lock().unwrap();
                                let cards = catalog_entries_to_cards_model(
                                    &state.filtered_entries,
                                    &installed_ids,
                                );
                                app.set_packages(cards);
                                let txns = query_total_transactions(&db);
                                state.settings.apply_to_window(&app, &data_dir, txns);
                            }
                        }
                        Err(err) => {
                            app.set_status_message(format!("Rollback failed: {}", err).into());
                        }
                    }
                });
            });
        });
    }

    // ========================================================================
    // Callback 10: Toggle Settings
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);

        app.on_toggle_setting(move |key, val| {
            let Some(app) = app_weak.upgrade() else { return; };
            let mut state = state_rc.lock().unwrap();
            let data_dir = state.data_dir.clone();

            match key.as_str() {
                "auto_backup" => {
                    state.settings.auto_backup = val;
                    app.set_setting_auto_backup(val);
                }
                "strict_hash" => {
                    state.settings.strict_hash = val;
                    app.set_setting_strict_hash(val);
                }
                "offline_first" => {
                    state.settings.offline_first = val;
                    app.set_setting_offline_first(val);
                    app.set_setting_offline_cache(val);
                }
                _ => {}
            }

            let _ = state.settings.save_to_dir(&data_dir);
            app.set_status_message(format!("Setting '{}' updated to {}", key, val).into());
        });
    }

    // ========================================================================
    // Callback 11: Reindex Registry
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_clone = data.clone();

        app.on_reindex_registry(move || {
            let Some(app) = app_weak.upgrade() else { return; };
            let Ok(db) = Database::open(data_clone.join("journal.db")) else { return; };
            let reg_clone = state_rc.lock().unwrap().registry_dir.clone();
            let _ = catalog::import_registry(&db, &reg_clone);

            let all = catalog::search(&db, "", 200).unwrap_or_default();
            let installed_ids = query_installed_ids(&db);

            let mut state = state_rc.lock().unwrap();
            state.all_entries = all.clone();
            let q = app.get_search_query().to_string();
            state.apply_filter(&q);

            let cards = catalog_entries_to_cards_model(&state.filtered_entries, &installed_ids);
            app.set_packages(cards);
            app.set_total_packages_count(all.len() as i32);
            app.set_status_message(format!("Reindexed {} packages", all.len()).into());
        });
    }

    // ========================================================================
    // Callback 12: Restore Settings to Default
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_clone = data.clone();

        app.on_restore_settings(move || {
            let Some(app) = app_weak.upgrade() else { return; };
            let mut state = state_rc.lock().unwrap();
            state.settings = SettingsState::default();
            let _ = state.settings.save_to_dir(&data_clone);

            let total_txns = Database::open(data_clone.join("journal.db"))
                .map(|db| query_total_transactions(&db))
                .unwrap_or(0);
            state.settings.apply_to_window(&app, &data_clone, total_txns);
            app.set_status_message("Preferences restored to default".into());
        });
    }

    // ========================================================================
    // Callback 13: Open Import Dialog
    // ========================================================================
    {
        let app_weak = app.as_weak();
        app.on_open_import_dialog(move || {
            if let Some(app) = app_weak.upgrade() {
                app.set_import_error("".into());
                app.set_import_is_loading(false);
                app.set_show_import_dialog(true);
            }
        });
    }

    // ========================================================================
    // Callback 14: Close Import Dialog
    // ========================================================================
    {
        let app_weak = app.as_weak();
        app.on_close_import_dialog(move || {
            if let Some(app) = app_weak.upgrade() {
                app.set_show_import_dialog(false);
                app.set_import_error("".into());
                app.set_import_is_loading(false);
            }
        });
    }

    // ========================================================================
    // Callback 15: Submit GitHub Import
    // ========================================================================
    {
        let app_weak = app.as_weak();
        let state_rc = Arc::clone(&app_state);
        let data_dir = data.clone();

        app.on_submit_github_import(move |url_str| {
            let Some(app) = app_weak.upgrade() else { return; };
            let raw_url = url_str.trim().to_string();

            if raw_url.is_empty() {
                app.set_import_error("Please enter a GitHub repository URL".into());
                return;
            }

            if let Err(err) = fetcher::parse_and_validate_github_url(&raw_url) {
                app.set_import_error(format!("{err}").into());
                return;
            }

            app.set_import_is_loading(true);
            app.set_import_error("".into());
            app.set_progress_active(true);
            app.set_show_progress_modal(true);
            app.set_progress_title("Importing from GitHub...".into());
            app.set_progress_stage("Clone".into());
            app.set_progress_message(format!("Cloning {raw_url}...").into());
            app.set_progress_fraction(0.2);
            app.set_progress_percentage(20);

            let weak_thread = app_weak.clone();
            let state_thread = Arc::clone(&state_rc);
            let data_dir = data_dir.clone();

            std::thread::spawn(move || {
                let options = FetchOptions {
                    timeout: std::time::Duration::from_secs(30),
                    branch: None,
                };

                let result = (|| -> woderrise_core::Result<(String, woderrise_core::Manifest)> {
                    let fetched = fetcher::fetch_github_repository(&raw_url, options)?;
                    let pkg_id = fetched.manifest.package.id.clone();
                    let pkg_version = fetched.manifest.package.version.clone();
                    let manifest = fetched.manifest.clone();

                    let import_dest = data_dir
                        .join("imported")
                        .join(&pkg_id)
                        .join(&pkg_version);
                    copy_dir_all(fetched.temp_dir.path(), &import_dest)?;

                    let manifest_path = import_dest.join("woderrise.yaml");
                    let manifest_sha256 = woderrise_core::sha256_file(&manifest_path)?;

                    let db = Database::open(data_dir.join("journal.db"))?;
                    let entry = catalog::CatalogEntry {
                        id: manifest.package.id.clone(),
                        name: manifest.package.name.clone(),
                        description: manifest.package.description.clone(),
                        version: manifest.package.version.clone(),
                        package_type: manifest.package.package_type.clone(),
                        category: manifest.package.category.clone(),
                        tags: manifest.package.tags.clone(),
                        license: manifest.package.license.clone(),
                        repository: manifest.package.repository.clone(),
                        manifest_path,
                        manifest_sha256,
                        validated: true,
                        yanked: manifest.yanked,
                    };
                    catalog::upsert(&db, &entry)?;

                    Ok((pkg_id, manifest))
                })();

                let _ = weak_thread.upgrade_in_event_loop(move |app| {
                    app.set_progress_active(false);
                    app.set_show_progress_modal(false);
                    app.set_import_is_loading(false);

                    match result {
                        Ok((pkg_id, manifest)) => {
                            app.set_show_import_dialog(false);
                            app.set_import_error("".into());
                            app.set_import_url_input("".into());

                            app.set_current_tab(NavTab::Browse);
                            app.set_active_view(1);

                            if let Ok(db) = Database::open(data_dir.join("journal.db")) {
                                let all_entries = catalog::search(&db, "", 200).unwrap_or_default();
                                let installed_ids = query_installed_ids(&db);

                                let new_index = all_entries
                                    .iter()
                                    .position(|e| e.id == pkg_id)
                                    .unwrap_or(0);

                                {
                                    let mut state = state_thread.lock().unwrap();
                                    state.all_entries = all_entries.clone();
                                    state.filtered_entries = all_entries.clone();
                                    state.selected_index = new_index;
                                    state.selected_category = "All".to_string();
                                }

                                let cards = catalog_entries_to_cards_model(&all_entries, &installed_ids);
                                app.set_packages(cards);
                                app.set_total_packages_count(all_entries.len() as i32);
                                app.set_selected_package_index(new_index as i32);
                                app.set_selected_category("All".into());

                                let is_inst = installed_ids.contains(&pkg_id);
                                let vm = PackageDetailViewModel::from_manifest(&manifest, is_inst);
                                vm.apply_to_window(&app);

                                app.set_status_message(
                                    format!("Successfully imported {} from GitHub", pkg_id).into(),
                                );
                            }
                        }
                        Err(err) => {
                            app.set_import_error(format!("Import failed: {err}").into());
                            app.set_status_message(format!("GitHub import failed: {err}").into());
                        }
                    }
                });
            });
        });
    }

    // 4. Run main Slint event loop
    app.run()?;
    Ok(())
}

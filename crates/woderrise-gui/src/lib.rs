//! Woderrise GUI presentation layer.
//!
//! Native desktop GUI application built on Iced v0.14+ for discovering,
//! previewing, installing, and rolling back Wayland desktop configurations.

pub mod app;
pub mod screens;
pub mod theme;
pub mod widgets;

pub use app::{App, EngineEvent, GuiCommand, Message, Screen};
pub use screens::{
    CategoryFilter, HistoryState, InspectState, InstallPhase, InstallState, SearchState,
};
pub use theme::{fetch_portal_theme, map_color_scheme_to_theme, map_raw_scheme_value};
pub use widgets::{FileDiffReport, RateLimitColorTier, RateLimitGaugeState};

use tracing::{debug, error, info, warn};
use woderrise_core::models::{Catalog, StagedFile};
use woderrise_core::paths::XdgPaths;
use woderrise_core::schema::{
    read_json_file, write_json_file_durably, CATALOG_SCHEMA_V1, CURRENT_CATALOG_VERSION,
};
use woderrise_detect::{DetectionEngine, TreeEntry};
use woderrise_github::{GitHubClient, GitHubClientBuilder};
use woderrise_transaction::TransactionEngine;

/// Returns the crate version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Asynchronous background worker processing GUI commands and emitting engine events.
pub async fn run_worker(
    mut cmd_rx: tokio::sync::mpsc::Receiver<GuiCommand>,
    event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
    github_client: GitHubClient,
    detection_engine: DetectionEngine,
    tx_engine: TransactionEngine,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            GuiCommand::Search { query, token } => {
                let _ = event_tx
                    .send(EngineEvent::SearchStarted {
                        query: query.clone(),
                    })
                    .await;

                tokio::select! {
                    _ = token.cancelled() => {
                        debug!("Search cancelled for query: {}", query);
                    }
                    res = github_client.search_repositories(&query) => {
                        match res {
                            Ok(resp) => {
                                let search_quota = github_client.rate_limiter().search_status();
                                let _ = event_tx.send(EngineEvent::SearchCompleted {
                                    query,
                                    results: resp.items,
                                    search_quota,
                                }).await;
                            }
                            Err(e) => {
                                let _ = event_tx.send(EngineEvent::SearchError {
                                    query,
                                    error: e.to_string(),
                                }).await;
                            }
                        }
                    }
                }
            }

            GuiCommand::RefreshCatalog => {
                let search_quota = github_client.rate_limiter().search_status();
                let catalog_path_res = XdgPaths::from_env().map(|paths| paths.catalog_file());

                let mut loaded_catalog: Option<Catalog> = None;
                if let Ok(ref catalog_path) = catalog_path_res {
                    if catalog_path.exists() {
                        if let Ok(cat) = read_json_file::<Catalog>(catalog_path) {
                            if !cat.repositories.is_empty() {
                                loaded_catalog = Some(cat);
                            }
                        }
                    }
                }

                if let Some(cat) = loaded_catalog {
                    let _ = event_tx
                        .send(EngineEvent::SearchCompleted {
                            query: String::new(),
                            results: cat.repositories,
                            search_quota,
                        })
                        .await;
                } else {
                    let mut popular_repos = Vec::new();
                    if github_client.rate_limiter().check_search_quota().is_ok() {
                        if let Ok(resp) =
                            github_client.search_repositories("hyprland dotfiles").await
                        {
                            if !resp.items.is_empty() {
                                popular_repos = resp.items;
                            }
                        }
                    }

                    if popular_repos.is_empty() {
                        popular_repos = Catalog::default_popular().repositories;
                    }

                    let new_catalog = Catalog {
                        schema: Some(CATALOG_SCHEMA_V1.to_string()),
                        version: CURRENT_CATALOG_VERSION,
                        last_synced: chrono::Utc::now(),
                        repositories: popular_repos.clone(),
                    };

                    if let Ok(ref catalog_path) = catalog_path_res {
                        if let Err(e) = write_json_file_durably(catalog_path, &new_catalog) {
                            warn!("Failed to persist initial catalog: {}", e);
                        }
                    }

                    let updated_quota = github_client.rate_limiter().search_status();
                    let _ = event_tx
                        .send(EngineEvent::SearchCompleted {
                            query: String::new(),
                            results: popular_repos,
                            search_quota: updated_quota,
                        })
                        .await;
                }
            }

            GuiCommand::InspectPackage {
                repo,
                commit_sha,
                token,
            } => {
                let _ = event_tx
                    .send(EngineEvent::InspectStarted { repo: repo.clone() })
                    .await;

                let parts: Vec<&str> = repo.split('/').collect();
                if parts.len() != 2 {
                    let _ = event_tx
                        .send(EngineEvent::InspectError {
                            repo: repo.clone(),
                            error: "Invalid repository format; expected 'owner/repo'".into(),
                        })
                        .await;
                    continue;
                }
                let (owner, name) = (parts[0], parts[1]);

                tokio::select! {
                    _ = token.cancelled() => {
                        debug!("Inspection cancelled for repo: {}", repo);
                    }
                    res = github_client.get_tree(owner, name, &commit_sha) => {
                        match res {
                            Ok(tree) => {
                                let entries: Vec<TreeEntry> = tree.tree.iter().map(|e| {
                                    TreeEntry::new(e.path.clone(), e.mode.clone(), e.entry_type.clone(), e.sha.clone(), e.size)
                                }).collect();

                                let detections = detection_engine.detect_all(&entries);

                                let _ = event_tx.send(EngineEvent::TreeLoaded {
                                    repo: repo.clone(),
                                    tree: tree.clone(),
                                    detections,
                                }).await;

                                // Fetch README if available
                                if let Some(readme_entry) = tree.tree.iter().find(|e| e.path.to_lowercase() == "readme.md") {
                                    if let Ok((content, _)) = github_client.stream_blob(owner, name, &commit_sha, &readme_entry.path, Some(&readme_entry.sha)).await {
                                        let _ = event_tx.send(EngineEvent::FileContentFetched {
                                            repo: repo.clone(),
                                            path: readme_entry.path.clone(),
                                            content,
                                        }).await;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = event_tx.send(EngineEvent::InspectError {
                                    repo: repo.clone(),
                                    error: e.to_string(),
                                }).await;
                            }
                        }
                    }
                }
            }

            GuiCommand::FetchFileContent {
                repo,
                commit_sha,
                path,
                blob_sha,
            } => {
                let parts: Vec<&str> = repo.split('/').collect();
                if parts.len() == 2 {
                    if let Ok((content, _)) = github_client
                        .stream_blob(parts[0], parts[1], &commit_sha, &path, blob_sha.as_deref())
                        .await
                    {
                        let _ = event_tx
                            .send(EngineEvent::FileContentFetched {
                                repo,
                                path,
                                content,
                            })
                            .await;
                    }
                }
            }

            GuiCommand::ComputeDiff {
                repo,
                commit_sha,
                mappings,
            } => {
                let parts: Vec<&str> = repo.split('/').collect();
                if parts.len() == 2 {
                    let mut diffs = Vec::new();
                    for m in &mappings {
                        let old_content =
                            std::fs::read_to_string(&m.target_destination).unwrap_or_default();
                        if let Ok((blob_bytes, _)) = github_client
                            .stream_blob(
                                parts[0],
                                parts[1],
                                &commit_sha,
                                m.relative_path.as_str(),
                                Some(&m.blob_sha),
                            )
                            .await
                        {
                            let new_content = String::from_utf8_lossy(&blob_bytes);
                            diffs.push(FileDiffReport::compute(
                                m.relative_path.as_str(),
                                &old_content,
                                &new_content,
                            ));
                        }
                    }
                    let _ = event_tx
                        .send(EngineEvent::DiffComputed { repo, diffs })
                        .await;
                }
            }

            GuiCommand::StagePackage {
                tx_id,
                component,
                repo,
                commit_sha,
                entries,
            } => {
                let parts: Vec<&str> = repo.split('/').collect();
                if parts.len() != 2 {
                    let _ = event_tx
                        .send(EngineEvent::StagingError {
                            tx_id,
                            error: "Invalid repository format".into(),
                        })
                        .await;
                    continue;
                }
                let (owner, name) = (parts[0], parts[1]);
                let total = entries.len();
                let mut staged_entries = Vec::with_capacity(total);
                let mut failed = false;

                for (idx, entry) in entries.iter().enumerate() {
                    let _ = event_tx
                        .send(EngineEvent::StagingProgress {
                            tx_id: tx_id.clone(),
                            file_path: entry.path.clone(),
                            index: idx + 1,
                            total,
                            bytes_downloaded: entry.size.unwrap_or(0),
                        })
                        .await;

                    match github_client
                        .stream_blob(owner, name, &commit_sha, &entry.path, Some(&entry.sha))
                        .await
                    {
                        Ok((bytes, _)) => {
                            staged_entries.push((entry.clone(), bytes));
                        }
                        Err(e) => {
                            let _ = event_tx
                                .send(EngineEvent::StagingError {
                                    tx_id: tx_id.clone(),
                                    error: format!("Failed to fetch '{}': {}", entry.path, e),
                                })
                                .await;
                            failed = true;
                            break;
                        }
                    }
                }

                if !failed {
                    match tx_engine.stage_transaction(
                        &tx_id,
                        component,
                        &repo,
                        &commit_sha,
                        &staged_entries,
                    ) {
                        Ok(report) => {
                            let staged_files: Vec<StagedFile> = report
                                .target_mappings
                                .iter()
                                .map(|m| {
                                    let file_size = staged_entries
                                        .iter()
                                        .find(|(e, _)| {
                                            e.path == m.relative_path || e.sha == m.blob_sha
                                        })
                                        .map(|(_, b)| b.len() as u64)
                                        .unwrap_or(0);
                                    let is_executable = m.git_file_mode == "100755"
                                        || m.git_file_mode.ends_with("755");
                                    let is_symlink = m.git_file_mode == "120000"
                                        || m.git_file_mode.starts_with("120");
                                    StagedFile {
                                        relative_path: m.relative_path.clone(),
                                        target_destination: Some(m.target_destination.clone()),
                                        blob_sha: Some(m.blob_sha.clone()),
                                        file_size,
                                        sha256: m.expected_sha256.clone(),
                                        file_mode: m.git_file_mode.clone(),
                                        permissions: if is_executable { 0o755 } else { 0o644 },
                                        is_executable,
                                        is_symlink,
                                        syntax_valid: true,
                                        path_safe: true,
                                    }
                                })
                                .collect();

                            let _ = event_tx
                                .send(EngineEvent::StagingCompleted {
                                    tx_id,
                                    report,
                                    staged_files,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = event_tx
                                .send(EngineEvent::StagingError {
                                    tx_id,
                                    error: e.to_string(),
                                })
                                .await;
                        }
                    }
                }
            }

            GuiCommand::ApplyInstallation {
                tx_id,
                component,
                repo,
                commit_sha,
                staged_files,
            } => {
                let _ = event_tx
                    .send(EngineEvent::InstallProgress {
                        tx_id: tx_id.clone(),
                        phase: InstallPhase::Backup,
                        step_description: "Creating pre-install backup snapshot...".into(),
                        percent: 30,
                    })
                    .await;

                let _ = event_tx
                    .send(EngineEvent::InstallProgress {
                        tx_id: tx_id.clone(),
                        phase: InstallPhase::JournalWal,
                        step_description: "Writing WAL journal Applying status...".into(),
                        percent: 50,
                    })
                    .await;

                let _ = event_tx
                    .send(EngineEvent::InstallProgress {
                        tx_id: tx_id.clone(),
                        phase: InstallPhase::AtomicSwap,
                        step_description:
                            "Atomically swapping dotfiles with parent fsync durability...".into(),
                        percent: 75,
                    })
                    .await;

                match tx_engine.apply_transaction(
                    &tx_id,
                    component,
                    &repo,
                    &commit_sha,
                    &staged_files,
                ) {
                    Ok(entry) => {
                        let _ = event_tx
                            .send(EngineEvent::InstallCompleted { tx_id, entry })
                            .await;
                    }
                    Err(e) => {
                        let _ = event_tx
                            .send(EngineEvent::InstallError {
                                tx_id,
                                phase: InstallPhase::AtomicSwap,
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            GuiCommand::CancelStaging { tx_id } => {
                let staging = woderrise_transaction::StagingWorkspace::new(&tx_engine.xdg, &tx_id);
                if let Ok(workspace) = staging {
                    let _ = workspace.cleanup();
                }
            }

            GuiCommand::LoadHistory => {
                let transactions = tx_engine.get_history().unwrap_or_default();
                let active_tx = tx_engine.get_active_transaction().unwrap_or_default();
                let _ = event_tx
                    .send(EngineEvent::HistoryLoaded {
                        transactions,
                        active_tx,
                    })
                    .await;
            }

            GuiCommand::RollbackLatest { force_dirty } => {
                match tx_engine.rollback_latest(force_dirty) {
                    Ok(report) => {
                        let _ = event_tx
                            .send(EngineEvent::RollbackCompleted { report })
                            .await;
                    }
                    Err(e) => {
                        let _ = event_tx
                            .send(EngineEvent::RollbackError {
                                tx_id: "latest".into(),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            GuiCommand::RollbackTransaction { tx_id, force_dirty } => {
                match tx_engine.rollback_transaction(&tx_id, force_dirty) {
                    Ok(report) => {
                        let _ = event_tx
                            .send(EngineEvent::RollbackCompleted { report })
                            .await;
                    }
                    Err(e) => {
                        let _ = event_tx
                            .send(EngineEvent::RollbackError {
                                tx_id,
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            GuiCommand::FetchRateLimits => {
                let core = github_client.rate_limiter().core_status();
                let search = github_client.rate_limiter().search_status();
                let _ = event_tx
                    .send(EngineEvent::RateLimitsUpdated { core, search })
                    .await;
            }

            GuiCommand::SetGitHubToken { token: _ } => {
                // Token update handled on client builder
            }
        }
    }
}

/// Primary entrypoint launching the native Iced desktop application.
pub fn run() -> iced::Result {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("info,woderrise=debug,iced=warn,wgpu=error")
    });
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    info!("Initializing Woderrise Desktop GUI Runtime");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");

    let _guard = rt.enter();

    let xdg = XdgPaths::from_env().unwrap_or_else(|e| {
        warn!(
            "Failed to resolve system XDG paths ({}). Initializing isolated fallback.",
            e
        );
        XdgPaths::custom(camino::Utf8Path::new("/tmp/woderrise"))
    });

    let tx_engine =
        TransactionEngine::new(xdg.clone()).expect("Failed to initialize TransactionEngine");
    let detection_engine = DetectionEngine::new();
    let github_client = GitHubClientBuilder::new()
        .build()
        .expect("Failed to initialize GitHubClient");

    // Mandatory startup crash recovery
    let recovery_outcome = match tx_engine.recover_on_startup() {
        Ok(outcome) => {
            if let Some(ref o) = outcome {
                info!(
                    tx_id = %o.tx_id,
                    restored_count = o.restored_files.len(),
                    "Startup crash recovery executed successfully."
                );
            }
            outcome
        }
        Err(e) => {
            error!("Startup crash recovery error: {:?}", e);
            None
        }
    };

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<GuiCommand>(100);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<EngineEvent>(100);

    app::set_engine_event_receiver(event_rx);

    // Spawn background engine worker task
    let event_tx_worker = event_tx.clone();
    rt.spawn(async move {
        run_worker(
            cmd_rx,
            event_tx_worker,
            github_client,
            detection_engine,
            tx_engine,
        )
        .await;
    });

    // Spawn desktop portal theme listener
    let event_tx_theme = event_tx.clone();
    rt.spawn(async move {
        use ashpd::desktop::settings::Settings;
        use futures_util::StreamExt;
        if let Ok(settings) = Settings::new().await {
            if let Ok(mut stream) = settings.receive_color_scheme_changed().await {
                while let Some(scheme) = stream.next().await {
                    let dark_mode = matches!(
                        scheme,
                        ashpd::desktop::settings::ColorScheme::NoPreference
                            | ashpd::desktop::settings::ColorScheme::PreferDark
                    );
                    let _ = event_tx_theme
                        .send(EngineEvent::PortalThemeChanged { dark_mode })
                        .await;
                }
            }
        }
    });

    let cmd_tx_boot = cmd_tx.clone();
    let recovery_boot = recovery_outcome.clone();

    iced::application(
        move || {
            (
                App::new(cmd_tx_boot.clone(), recovery_boot.clone()),
                iced::Task::none(),
            )
        },
        App::update,
        App::view,
    )
    .title(App::title)
    .theme(App::theme)
    .subscription(App::subscription)
    .window(iced::window::Settings {
        size: iced::Size::new(1120.0, 750.0),
        min_size: Some(iced::Size::new(900.0, 600.0)),
        ..Default::default()
    })
    .run()
}

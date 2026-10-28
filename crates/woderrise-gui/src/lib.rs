//! Woderrise GUI presentation layer.
//!
//! Native desktop GUI application built on Iced v0.14+ for discovering,
//! previewing, installing, and rolling back Wayland desktop configurations.

pub mod app;
pub mod cli;
pub mod screens;
pub mod theme;
pub mod widgets;

pub use app::{App, EngineEvent, GuiCommand, Message, Screen};
pub use screens::{
    CategoryFilter, HistoryState, InspectState, InstallPhase, InstallState, SearchState,
};
pub use theme::{fetch_portal_theme, map_color_scheme_to_theme, map_raw_scheme_value};
pub use widgets::{view_card, FileDiffReport, RateLimitColorTier, RateLimitGaugeState};

use tracing::{debug, error, info, warn};
use woderrise_core::models::{Catalog, StagedFile};
use woderrise_core::paths::XdgPaths;
use woderrise_core::schema::{read_json_file, write_json_file_durably};
use woderrise_detect::{DetectionEngine, TreeEntry};
use woderrise_github::{GitHubClient, GitHubClientBuilder};
use woderrise_transaction::TransactionEngine;

/// Returns the crate version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Deterministic 4-tier catalog fallback:
/// - Tier 1 (Remote Static Feed): HTTP GET WODERRISE_CATALOG_URL (or default raw GitHub CDN URL) with a 4-second timeout.
///   On success, parse Catalog and durably write to local XDG cache. Consumes 0 REST/Search API quota.
/// - Tier 2 (Local XDG Cache): If CDN fails or offline, read $XDG_DATA_HOME/woderrise/catalog.json.
/// - Tier 3 (Bundled Static Catalog): If cache missing/empty, fall back to bundled catalog
///   (embedded via include_str!("../../../registry/catalog.v1.json") or Catalog::default_popular()).
/// - Tier 4 (Live Search API): Reserved strictly for explicit user query submissions where local results are insufficient.
pub async fn resolve_catalog_4tier(
    custom_url: Option<&str>,
    catalog_cache_path: Option<&camino::Utf8Path>,
) -> Catalog {
    let mut resolved_catalog: Option<Catalog> = None;

    // Stage 1: Attempt to load from static CDN feed (0 REST/Search API quota)
    let cdn_url = custom_url
        .map(String::from)
        .or_else(|| std::env::var("WODERRISE_CATALOG_URL").ok())
        .unwrap_or_else(|| woderrise_github::DEFAULT_CATALOG_FEED_URL.to_string());

    if let Ok(resp) = reqwest::Client::new()
        .get(&cdn_url)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(cat) = resp.json::<Catalog>().await {
                if !cat.repositories.is_empty() {
                    debug!(
                        "Tier 1: Successfully loaded static catalog from CDN ({} repositories)",
                        cat.repositories.len()
                    );
                    if let Some(path) = catalog_cache_path {
                        if let Err(e) = write_json_file_durably(path, &cat) {
                            warn!("Failed to persist refreshed catalog to XDG cache: {}", e);
                        }
                    }
                    resolved_catalog = Some(cat);
                }
            }
        }
    }

    // Stage 2: Fallback to local cached file if CDN unavailable / offline
    if resolved_catalog.is_none() {
        if let Some(path) = catalog_cache_path {
            if path.exists() {
                if let Ok(cat) = read_json_file::<Catalog>(path) {
                    if !cat.repositories.is_empty() {
                        debug!(
                            "Tier 2: Fallback to local XDG cache at {} ({} repositories)",
                            path,
                            cat.repositories.len()
                        );
                        resolved_catalog = Some(cat);
                    }
                }
            }
        }
    }

    // Stage 3: Fallback to bundled pre-seeded catalog (ensures 100% offline day-1 operation)
    resolved_catalog.unwrap_or_else(|| {
        debug!("Tier 3: Fallback to bundled static catalog");
        const BUNDLED_JSON: &str = include_str!("../../../registry/catalog.v1.json");
        serde_json::from_str::<Catalog>(BUNDLED_JSON).unwrap_or_else(|e| {
            warn!(
                "Failed to parse bundled catalog JSON ({}): falling back to default popular",
                e
            );
            Catalog::default_popular()
        })
    })
}

/// Asynchronously pre-inspects top repositories in background without blocking the UI.
/// Checks rate limit quota safety (remaining >= 5 or authenticated) and cancellation tokens.
pub fn spawn_tree_pre_inspection(
    repos: Vec<woderrise_core::models::CatalogRepository>,
    github_client: GitHubClient,
    detection_engine: std::sync::Arc<DetectionEngine>,
    event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(async move {
        let is_auth = github_client.config().auth_token.is_some();
        let core_rem = github_client.rate_limiter().core_status().remaining;
        if !is_auth && core_rem < 5 {
            debug!(
                core_rem,
                "Core rate limit quota low; skipping background tree pre-inspection"
            );
            return;
        }

        let max_candidates = if is_auth || core_rem >= 20 { 10 } else { 5 };

        let candidates: Vec<_> = repos
            .into_iter()
            .filter(|r| r.detected_components.is_empty())
            .take(max_candidates)
            .collect();

        for repo in candidates {
            if cancel_token.is_cancelled() {
                debug!("Background tree pre-inspection cancelled");
                break;
            }

            if !github_client.config().auth_token.is_some()
                && github_client.rate_limiter().core_status().remaining < 5
            {
                debug!("Core rate limit quota low during pre-inspection; aborting remaining");
                break;
            }

            let full_name = repo.resolved_full_name();
            let (owner, name) = match full_name.split_once('/') {
                Some((o, n)) => (o.to_string(), n.to_string()),
                None => {
                    if !repo.owner.is_empty() && !repo.repo.is_empty() {
                        (repo.owner.clone(), repo.repo.clone())
                    } else {
                        continue;
                    }
                }
            };
            let branch = if repo.default_branch.is_empty() {
                "main".to_string()
            } else {
                repo.default_branch.clone()
            };

            tokio::select! {
                _ = cancel_token.cancelled() => {
                    debug!("Background tree pre-inspection cancelled during tree fetch");
                    break;
                }
                res = github_client.get_tree(&owner, &name, &branch) => {
                    match res {
                        Ok(tree) => {
                            let entries: Vec<TreeEntry> = tree
                                .tree
                                .into_iter()
                                .map(|e| TreeEntry::new(e.path, e.mode, e.entry_type, e.sha, e.size))
                                .collect();

                            let components = detection_engine.scan_catalog_components(&entries);
                            if !components.is_empty() {
                                let _ = event_tx
                                    .send(EngineEvent::RepoPreInspected {
                                        repo: full_name,
                                        components,
                                    })
                                    .await;
                            }
                        }
                        Err(e) => {
                            debug!(
                                repo = %full_name,
                                error = %e,
                                "Background tree pre-inspection skipped or failed"
                            );
                        }
                    }
                }
            }
        }
    });
}

/// Asynchronous background worker processing GUI commands and emitting engine events.
pub async fn run_worker(
    mut cmd_rx: tokio::sync::mpsc::Receiver<GuiCommand>,
    event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
    mut github_client: GitHubClient,
    detection_engine: DetectionEngine,
    tx_engine: TransactionEngine,
) {
    let detection_engine = std::sync::Arc::new(detection_engine);
    let mut preinspect_cancel_token: Option<tokio_util::sync::CancellationToken> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            GuiCommand::Search { query, token } => {
                if let Some(old_cancel) = preinspect_cancel_token.take() {
                    old_cancel.cancel();
                }

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
                                    results: resp.items.clone(),
                                    search_quota,
                                }).await;

                                let preinspect_token = token.child_token();
                                preinspect_cancel_token = Some(preinspect_token.clone());

                                spawn_tree_pre_inspection(
                                    resp.items,
                                    github_client.clone(),
                                    detection_engine.clone(),
                                    event_tx.clone(),
                                    preinspect_token,
                                );
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
                if let Some(old_cancel) = preinspect_cancel_token.take() {
                    old_cancel.cancel();
                }

                let catalog_path_res = XdgPaths::from_env().map(|paths| paths.catalog_file());
                let final_catalog =
                    resolve_catalog_4tier(None, catalog_path_res.as_deref().ok()).await;

                let search_quota = github_client.rate_limiter().search_status();
                let _ = event_tx
                    .send(EngineEvent::SearchCompleted {
                        query: String::new(),
                        results: final_catalog.repositories.clone(),
                        search_quota,
                    })
                    .await;

                let preinspect_token = tokio_util::sync::CancellationToken::new();
                preinspect_cancel_token = Some(preinspect_token.clone());

                spawn_tree_pre_inspection(
                    final_catalog.repositories,
                    github_client.clone(),
                    detection_engine.clone(),
                    event_tx.clone(),
                    preinspect_token,
                );
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
                let (core, search) = match github_client.get_rate_limit().await {
                    Ok(limits) => limits,
                    Err(_) => (
                        github_client.rate_limiter().core_status(),
                        github_client.rate_limiter().search_status(),
                    ),
                };
                let _ = event_tx
                    .send(EngineEvent::RateLimitsUpdated { core, search })
                    .await;
            }

            GuiCommand::SetGitHubToken { token } => {
                let settings_path = tx_engine.xdg.settings_file();
                let previous_settings =
                    woderrise_core::models::load_settings(&settings_path).unwrap_or_default();
                let previous_token = previous_settings.github_token.clone();

                if let Some(ref new_token) = token {
                    github_client.set_auth_token(Some(new_token.clone()));
                    match github_client.get_rate_limit().await {
                        Ok((core, search)) => {
                            let mut settings = previous_settings;
                            settings.github_token = Some(new_token.clone());
                            if let Err(e) = woderrise_core::models::save_settings_durably(
                                &settings_path,
                                &settings,
                            ) {
                                warn!("Failed to persist settings.json: {:?}", e);
                            }
                            let _ = event_tx
                                .send(EngineEvent::RateLimitsUpdated { core, search })
                                .await;
                        }
                        Err(e) => {
                            warn!("GitHub PAT verification failed: {:?}", e);
                            github_client.set_auth_token(previous_token);
                            let _ = event_tx
                                .send(EngineEvent::TokenVerificationFailed {
                                    error: e.to_string(),
                                })
                                .await;
                        }
                    }
                } else {
                    github_client.set_auth_token(None);
                    let mut settings = previous_settings;
                    settings.github_token = None;
                    if let Err(e) =
                        woderrise_core::models::save_settings_durably(&settings_path, &settings)
                    {
                        warn!("Failed to persist settings.json: {:?}", e);
                    }
                    let (core, search) = match github_client.get_rate_limit().await {
                        Ok(limits) => limits,
                        Err(_) => (
                            github_client.rate_limiter().core_status(),
                            github_client.rate_limiter().search_status(),
                        ),
                    };
                    let _ = event_tx
                        .send(EngineEvent::RateLimitsUpdated { core, search })
                        .await;
                }
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

    let mut client_builder = GitHubClientBuilder::new();
    let settings_file = xdg.settings_file();
    if let Ok(settings) = woderrise_core::models::load_settings(&settings_file) {
        if let Some(token) = settings.github_token {
            info!("Configuring GitHubClient with saved Personal Access Token.");
            client_builder = client_builder.auth_token(Some(token));
        }
    }
    let github_client = client_builder
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

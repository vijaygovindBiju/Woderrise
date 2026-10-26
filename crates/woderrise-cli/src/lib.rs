use clap::{Parser, Subcommand};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use woderrise_core::{catalog, BackupManager, Database, InstallationEngine, Manifest};

#[derive(Parser, Debug, Clone)]
#[command(name = "woderrise", about = "Safe declarative desktop package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[arg(long, default_value = "~/.local/share/woderrise", global = true)]
    pub data_dir: PathBuf,
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Install {
        /// Local package directory, manifest path, package ID in catalog, or remote GitHub URL
        package: String,
    },
    Rollback {
        transaction: String,
        #[arg(long)]
        force: bool,
    },
    List,
    Lint {
        manifest: PathBuf,
    },
    /// Preview filesystem differences and planned backup destinations for a package
    Diff {
        /// Package path (directory or manifest), package ID, or remote GitHub URL
        package: String,
        /// Show only file summary without detailed line diffs
        #[arg(long, short)]
        summary: bool,
        /// Disable ANSI color output
        #[arg(long)]
        no_color: bool,
    },
    /// Search catalog packages using SQLite FTS5 full-text search
    Search {
        /// Search query (empty matches all available packages)
        #[arg(default_value = "")]
        query: String,
        /// Maximum number of results to display
        #[arg(long, short, default_value = "50")]
        limit: usize,
    },
    /// Inspect package manifest, authors, upstream repo, compatibility, and palette
    Info {
        /// Package ID, ID@version, or path to manifest
        package: String,
        /// Disable ANSI color styling on palette swatches
        #[arg(long)]
        no_color: bool,
    },
    /// Scan and recover interrupted transactions (PLANNED, PREPARED, EXECUTING, FAILED)
    Recover,
    /// Fetch and synthesize a package from a remote GitHub repository URL
    Fetch {
        /// GitHub repository URL (e.g. https://github.com/owner/repo)
        url: String,
        /// Destination directory to save the package (defaults to data cache)
        #[arg(long)]
        dest: Option<PathBuf>,
        /// Ingest synthesized package manifest into local SQLite catalog database
        #[arg(long)]
        index: bool,
    },
}

/// Determines if an input string is a remote Git repository URL.
pub fn is_remote_url(raw: &str) -> bool {
    let s = raw.trim();
    s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("git://")
        || s.starts_with("ssh://")
        || s.starts_with("file://")
        || (s.starts_with("github.com/") && !Path::new(s).exists())
}

/// Recursively copies directory contents ignoring .git directories.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
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
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Helper for safe hex RGB parsing in CLI without depending on GUI.
pub fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if !clean.is_ascii() {
        return None;
    }
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
        let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
        let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
        Some((r, g, b))
    } else if clean.len() == 3 {
        let r = u8::from_str_radix(&clean[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&clean[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&clean[2..3], 16).ok()? * 17;
        Some((r, g, b))
    } else {
        None
    }
}

/// Locates package registry directory from environment or relative candidates.
pub fn registry_dir() -> PathBuf {
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

/// Auto-seeds catalog database from registry if catalog is empty.
pub fn ensure_catalog_seeded(db: &Database) {
    let _ = catalog::migrate(db);
    let count: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM catalog", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        let reg = registry_dir();
        if reg.is_dir() {
            let _ = catalog::import_registry(db, &reg);
        }
    }
}

/// Resolves data directory expanding ~ or using WODERRISE_DATA_DIR.
pub fn resolve_data_dir(cli_data_dir: &Path, home: &Path) -> PathBuf {
    let raw = if cli_data_dir != Path::new("~/.local/share/woderrise") {
        cli_data_dir.to_path_buf()
    } else if let Some(env_dir) = std::env::var_os("WODERRISE_DATA_DIR") {
        PathBuf::from(env_dir)
    } else {
        cli_data_dir.to_path_buf()
    };
    if raw.to_string_lossy().starts_with("~/") {
        home.join(raw.to_string_lossy().trim_start_matches("~/"))
    } else {
        raw
    }
}

pub fn handle_lint(manifest: &Path) -> woderrise_core::Result<()> {
    Manifest::from_file(manifest)?;
    println!("manifest is valid");
    Ok(())
}

pub fn handle_list(data: &Path) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    let mut s = db.conn.prepare(
        "SELECT txn_id,package_id,version,status FROM transactions ORDER BY started_at DESC",
    )?;
    for r in s.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (id, p, v, status) = r?;
        println!("{id}\t{p}\t{v}\t{status}");
    }
    Ok(())
}

pub fn handle_fetch(
    url: &str,
    dest: Option<&Path>,
    index: bool,
    data: &Path,
) -> woderrise_core::Result<()> {
    let fetched = woderrise_core::fetch_github_repository(
        url,
        woderrise_core::FetchOptions::default(),
    )?;

    let final_dir = match dest {
        Some(d) => d.to_path_buf(),
        None => data
            .join("packages")
            .join(&fetched.manifest.package.id)
            .join(&fetched.manifest.package.version),
    };

    copy_dir_all(fetched.temp_dir.path(), &final_dir)?;

    if index {
        let db = Database::open(data.join("journal.db"))?;
        ensure_catalog_seeded(&db);
        let manifest_path = final_dir.join("woderrise.yaml");
        let manifest_sha256 = woderrise_core::sha256_file(&manifest_path)?;
        let entry = catalog::CatalogEntry {
            id: fetched.manifest.package.id.clone(),
            name: fetched.manifest.package.name.clone(),
            description: fetched.manifest.package.description.clone(),
            version: fetched.manifest.package.version.clone(),
            package_type: fetched.manifest.package.package_type.clone(),
            category: fetched.manifest.package.category.clone(),
            tags: fetched.manifest.package.tags.clone(),
            license: fetched.manifest.package.license.clone(),
            repository: fetched.manifest.package.repository.clone(),
            manifest_path,
            manifest_sha256,
            validated: true,
            yanked: fetched.manifest.yanked,
        };
        catalog::upsert(&db, &entry)?;
    }

    println!(
        "Fetched repository: {} (commit {})",
        url, fetched.commit_sha
    );
    println!("Package:            {} v{}", fetched.manifest.package.id, fetched.manifest.package.version);
    println!("Name:               {}", fetched.manifest.package.name);
    println!("License:            {}", fetched.manifest.package.license);
    println!("Destination:        {}", final_dir.display());
    println!("Detected Hyprland files ({}):", fetched.detected_files.len());
    for file in &fetched.detected_files {
        println!(
            "  - {} -> {} (sha256: {})",
            file.relative_path.display(),
            file.target_path,
            file.sha256
        );
    }
    if index {
        println!("Catalog:            Indexed into catalog database.");
    }

    Ok(())
}

pub fn handle_install(package: &str, data: &Path, home: PathBuf) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups"))?,
        home,
    };
    if is_remote_url(package) {
        let fetched = woderrise_core::fetch_github_repository(
            package,
            woderrise_core::FetchOptions::default(),
        )?;
        println!("{}", engine.install(fetched.temp_dir.path(), "user_cli")?);
    } else if Path::new(package).exists() {
        let p = Path::new(package);
        if p.join("woderrise.yaml").is_file() || p.is_file() {
            println!("{}", engine.install(p, "user_cli")?);
        } else if p.join(".git").exists() || p.join("hyprland.conf").exists() {
            let fetched = woderrise_core::fetch_github_repository(
                package,
                woderrise_core::FetchOptions::default(),
            )?;
            println!("{}", engine.install(fetched.temp_dir.path(), "user_cli")?);
        } else {
            println!("{}", engine.install(p, "user_cli")?);
        }
    } else {
        ensure_catalog_seeded(&engine.db);
        let (id, version_opt) = if let Some((id, ver)) = package.split_once('@') {
            (id, Some(ver))
        } else if let Some((id, ver)) = package.split_once('/') {
            (id, Some(ver))
        } else {
            (package, None)
        };
        let manifest_path: String = if let Some(ver) = version_opt {
            engine.db.conn
                .query_row(
                    "SELECT manifest_path FROM catalog WHERE id=?1 AND version=?2 AND validated=1 AND yanked=0",
                    rusqlite::params![id, ver],
                    |r| r.get(0),
                )
                .map_err(|_| {
                    woderrise_core::WoderriseError::Other(format!("package not found: {package}"))
                })?
        } else {
            engine.db.conn
                .query_row(
                    "SELECT manifest_path FROM catalog WHERE id=?1 AND validated=1 AND yanked=0 ORDER BY version DESC LIMIT 1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .map_err(|_| {
                    woderrise_core::WoderriseError::Other(format!("package not found: {package}"))
                })?
        };
        let manifest_dir = Path::new(&manifest_path).parent().unwrap_or(Path::new(&manifest_path));
        println!("{}", engine.install(manifest_dir, "user_cli")?);
    }
    Ok(())
}

pub fn handle_rollback(
    transaction: &str,
    force: bool,
    data: &Path,
    home: PathBuf,
) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups"))?,
        home,
    };
    println!("{}", engine.rollback(transaction, force)?);
    Ok(())
}

pub fn handle_diff(
    package: &str,
    summary: bool,
    no_color: bool,
    data: &Path,
    home: &Path,
) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    ensure_catalog_seeded(&db);

    let (pkg_path, _guard) = if is_remote_url(package) {
        let fetched = woderrise_core::fetch_github_repository(
            package,
            woderrise_core::FetchOptions::default(),
        )?;
        (fetched.temp_dir.path().to_path_buf(), Some(fetched))
    } else if Path::new(package).exists() {
        let p = Path::new(package);
        if p.join("woderrise.yaml").is_file() || p.is_file() {
            (p.to_path_buf(), None)
        } else if p.join(".git").exists() || p.join("hyprland.conf").exists() {
            let fetched = woderrise_core::fetch_github_repository(
                package,
                woderrise_core::FetchOptions::default(),
            )?;
            (fetched.temp_dir.path().to_path_buf(), Some(fetched))
        } else {
            (p.to_path_buf(), None)
        }
    } else {
        let (id, version_opt) = if let Some((id, ver)) = package.split_once('@') {
            (id, Some(ver))
        } else if let Some((id, ver)) = package.split_once('/') {
            (id, Some(ver))
        } else {
            (package, None)
        };
        let manifest_path: String = if let Some(ver) = version_opt {
            db.conn
                .query_row(
                    "SELECT manifest_path FROM catalog WHERE id=?1 AND version=?2 AND validated=1 AND yanked=0",
                    rusqlite::params![id, ver],
                    |r| r.get(0),
                )
                .map_err(|_| {
                    woderrise_core::WoderriseError::Other(format!("package not found: {package}"))
                })?
        } else {
            db.conn
                .query_row(
                    "SELECT manifest_path FROM catalog WHERE id=?1 AND validated=1 AND yanked=0 ORDER BY version DESC LIMIT 1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .map_err(|_| {
                    woderrise_core::WoderriseError::Other(format!("package not found: {package}"))
                })?
        };
        (PathBuf::from(manifest_path), None)
    };

    let engine = InstallationEngine {
        db,
        backups: BackupManager::new(data.join("backups"))?,
        home: home.to_path_buf(),
    };
    let plan = engine.compute_plan(&pkg_path)?;

    println!(
        "Install Plan for {} v{} ({} file(s), {} planned backup(s)):",
        plan.package_id,
        plan.version,
        plan.file_diffs.len(),
        plan.will_backup_count
    );
    println!("--------------------------------------------------");

    let mut create_count = 0;
    let mut modify_count = 0;
    let mut unchanged_count = 0;

    for diff in &plan.file_diffs {
        let action_str = match diff.action {
            woderrise_core::FileDiffAction::Create => {
                create_count += 1;
                "[CREATE]"
            }
            woderrise_core::FileDiffAction::Modify => {
                modify_count += 1;
                "[MODIFY]"
            }
            woderrise_core::FileDiffAction::Unchanged => {
                unchanged_count += 1;
                "[UNCHANGED]"
            }
        };
        let backup_str = match &diff.planned_backup_path {
            Some(p) => p.display().to_string(),
            None => "None (new file or backup disabled)".to_string(),
        };
        println!("Target:         {} {}", diff.target_path.display(), action_str);
        println!("Planned Backup: {}", backup_str);
        println!("Source:         {}", diff.source_path.display());

        if !summary {
            if diff.lines.is_empty() {
                if diff.action == woderrise_core::FileDiffAction::Unchanged {
                    println!("  Files are identical. No differences found.");
                } else {
                    println!("  No line differences to display.");
                }
            } else {
                for line in &diff.lines {
                    match line.kind {
                        woderrise_core::DiffLineKind::Addition => {
                            let r_no = line
                                .right_line_no
                                .map(|n| format!("{n:4}"))
                                .unwrap_or_else(|| "    ".into());
                            if !no_color {
                                println!("\x1b[32m       {r_no} │ +{}\x1b[0m", line.text);
                            } else {
                                println!("       {r_no} │ +{}", line.text);
                            }
                        }
                        woderrise_core::DiffLineKind::Deletion => {
                            let l_no = line
                                .left_line_no
                                .map(|n| format!("{n:4}"))
                                .unwrap_or_else(|| "    ".into());
                            if !no_color {
                                println!("\x1b[31m{l_no}        │ -{}\x1b[0m", line.text);
                            } else {
                                println!("{l_no}        │ -{}", line.text);
                            }
                        }
                        woderrise_core::DiffLineKind::Context => {
                            let l_no = line
                                .left_line_no
                                .map(|n| format!("{n:4}"))
                                .unwrap_or_else(|| "    ".into());
                            let r_no = line
                                .right_line_no
                                .map(|n| format!("{n:4}"))
                                .unwrap_or_else(|| "    ".into());
                            println!("{l_no}   {r_no} │  {}", line.text);
                        }
                    }
                }
            }
            println!();
        }
    }

    if summary {
        println!(
            "Summary: {} to create, {} to modify, {} unchanged, {} to backup.",
            create_count, modify_count, unchanged_count, plan.will_backup_count
        );
    }
    Ok(())
}

pub fn handle_search(query: &str, limit: usize, data: &Path) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    ensure_catalog_seeded(&db);

    let results = catalog::search(&db, query, limit)?;
    if results.is_empty() {
        if query.trim().is_empty() {
            println!("No packages found in catalog.");
        } else {
            println!("No packages found matching '{query}'.");
        }
    } else {
        println!("{:<28} {:<10} {:<20} NAME", "PACKAGE ID", "VERSION", "CATEGORY");
        println!("{}", "-".repeat(78));
        for pkg in results {
            let is_installed: bool = db
                .conn
                .query_row(
                    "SELECT 1 FROM installed_packages WHERE package_id=?1 AND active_version=?2",
                    rusqlite::params![pkg.id, pkg.version],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            let status = if is_installed { " [installed]" } else { "" };
            println!("{:<28} {:<10} {:<20} {}{}", pkg.id, pkg.version, pkg.category, pkg.name, status);
            if !pkg.description.is_empty() {
                println!("    {}", pkg.description);
            }
        }
    }
    Ok(())
}

pub fn handle_info(package: &str, no_color: bool, data: &Path) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    let pkg_path = PathBuf::from(package);
    let manifest = if pkg_path.exists() {
        let manifest_file = if pkg_path.is_file() {
            pkg_path
        } else {
            pkg_path.join("woderrise.yaml")
        };
        let m = Manifest::from_file(&manifest_file)?;
        catalog::verify_artifacts(&m, manifest_file.parent().unwrap_or(Path::new(".")))?;
        m
    } else {
        ensure_catalog_seeded(&db);
        let (id, version_opt) = if let Some((id, ver)) = package.split_once('@') {
            (id, Some(ver))
        } else if let Some((id, ver)) = package.split_once('/') {
            (id, Some(ver))
        } else {
            (package, None)
        };
        let version = if let Some(ver) = version_opt {
            ver.to_string()
        } else {
            db.conn
                .query_row(
                    "SELECT version FROM catalog WHERE id=?1 AND validated=1 AND yanked=0 ORDER BY version DESC LIMIT 1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .map_err(|_| {
                    woderrise_core::WoderriseError::Other(format!("package not found: {package}"))
                })?
        };
        catalog::manifest(&db, id, &version)?
    };

    println!("Package:      {} v{}", manifest.package.id, manifest.package.version);
    println!("Name:         {}", manifest.package.name);
    println!("Type:         {}", manifest.package.package_type);
    println!("Category:     {}", manifest.package.category);
    println!("License:      {}", manifest.package.license);
    println!("Description:  {}", manifest.package.description);
    if let Some(repo) = &manifest.package.repository {
        println!("Repository:   {repo}");
    }
    if !manifest.package.authors.is_empty() {
        println!("Authors:");
        for a in &manifest.package.authors {
            let mut s = format!("  - {}", a.name);
            if let Some(email) = &a.email {
                s.push_str(&format!(" <{email}>"));
            }
            if let Some(url) = &a.url {
                s.push_str(&format!(" ({url})"));
            }
            println!("{s}");
        }
    }
    if !manifest.package.tags.is_empty() {
        println!("Tags:         {}", manifest.package.tags.join(", "));
    }
    if let Some(compat) = &manifest.compatibility {
        println!("Compatibility:");
        if let Some(ds) = &compat.display_server {
            println!("  Display Server: {ds}");
        }
        if !compat.architectures.is_empty() {
            println!("  Architectures:  {}", compat.architectures.join(", "));
        }
        if !compat.compositors.is_empty() {
            let comps: Vec<String> = compat
                .compositors
                .iter()
                .map(|c| format!("{} {}", c.name, c.version))
                .collect();
            println!("  Compositors:    {}", comps.join(", "));
        }
    }
    if let Some(previews) = &manifest.previews {
        if !previews.palette.is_empty() {
            println!("Palette Swatches:");
            for color in &previews.palette {
                let rgb = if !no_color { parse_hex_rgb(&color.hex) } else { None };
                if let Some((r, g, b)) = rgb {
                    println!("  \x1b[48;2;{r};{g};{b}m  \x1b[0m  {:<16} {}", color.name, color.hex);
                } else {
                    println!("  [#] {:<16} {}", color.name, color.hex);
                }
            }
        }
    }
    if !manifest.install.files.is_empty() {
        println!("Install Actions:");
        for file in &manifest.install.files {
            println!(
                "  - Source: {} -> Target: {} (backup: {}, strategy: {})",
                file.source,
                file.target,
                file.backup_before_write,
                file.strategy.as_deref().unwrap_or("replace")
            );
        }
    }
    if !manifest.artifacts.is_empty() {
        println!("Artifacts:");
        for art in &manifest.artifacts {
            println!("  - {} (sha256: {})", art.path, art.sha256);
        }
    }
    Ok(())
}

pub fn handle_recover(data: &Path, home: PathBuf) -> woderrise_core::Result<()> {
    let db = Database::open(data.join("journal.db"))?;
    let interrupted: Vec<(String, String, String, String)> = {
        let mut st = db.conn.prepare(
            "SELECT txn_id, package_id, version, status FROM transactions WHERE status IN ('PLANNED','PREPARED','EXECUTING','FAILED') ORDER BY started_at ASC",
        )?;
        let rows = st.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        let items: std::result::Result<Vec<_>, _> = rows.collect();
        items?
    };

    if interrupted.is_empty() {
        println!("No interrupted transactions found. Journal and filesystem are clean.");
    } else {
        println!("Found {} interrupted transaction(s):", interrupted.len());
        for (id, pkg, ver, status) in &interrupted {
            println!("  - {id}: {pkg}@{ver} [{status}]");
        }
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups"))?,
            home,
        };
        let count = engine.recover()?;
        println!(
            "Successfully recovered {} transaction(s). Uncommitted files reverted; status marked as ABORTED_RECOVERED.",
            count
        );
    }
    Ok(())
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let data = resolve_data_dir(&cli.data_dir, &home);

    let result = match cli.command {
        Command::Lint { manifest } => handle_lint(&manifest),
        Command::List => handle_list(&data),
        Command::Install { package } => handle_install(&package, &data, home),
        Command::Rollback { transaction, force } => handle_rollback(&transaction, force, &data, home),
        Command::Diff { package, summary, no_color } => {
            handle_diff(&package, summary, no_color, &data, &home)
        }
        Command::Search { query, limit } => handle_search(&query, limit, &data),
        Command::Info { package, no_color } => handle_info(&package, no_color, &data),
        Command::Recover => handle_recover(&data, home),
        Command::Fetch { url, dest, index } => handle_fetch(&url, dest.as_deref(), index, &data),
    };

    if let Err(e) = result {
        eprintln!("woderrise: {e}");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_rgb() {
        assert_eq!(parse_hex_rgb("#1e1e2e"), Some((30, 30, 46)));
        assert_eq!(parse_hex_rgb("#89b4fa"), Some((137, 180, 250)));
        assert_eq!(parse_hex_rgb("fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_rgb("#000"), Some((0, 0, 0)));
        assert_eq!(parse_hex_rgb("invalid"), None);
        assert_eq!(parse_hex_rgb("#12"), None);
        assert_eq!(parse_hex_rgb(""), None);
    }

    #[test]
    fn test_cli_parse_diff() {
        let cli = Cli::try_parse_from(["woderrise", "diff", "com.catppuccin.waybar"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Diff {
                package: "com.catppuccin.waybar".into(),
                summary: false,
                no_color: false,
            }
        );

        let cli_summary = Cli::try_parse_from(["woderrise", "diff", "pkg", "--summary", "--no-color"]).unwrap();
        assert_eq!(
            cli_summary.command,
            Command::Diff {
                package: "pkg".into(),
                summary: true,
                no_color: true,
            }
        );
    }

    #[test]
    fn test_cli_parse_search() {
        let cli = Cli::try_parse_from(["woderrise", "search"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Search {
                query: "".into(),
                limit: 50,
            }
        );

        let cli_query = Cli::try_parse_from(["woderrise", "search", "waybar", "--limit", "10"]).unwrap();
        assert_eq!(
            cli_query.command,
            Command::Search {
                query: "waybar".into(),
                limit: 10,
            }
        );
    }

    #[test]
    fn test_cli_parse_info() {
        let cli = Cli::try_parse_from(["woderrise", "info", "com.catppuccin.waybar/1.0.0"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Info {
                package: "com.catppuccin.waybar/1.0.0".into(),
                no_color: false,
            }
        );

        let cli_no_color = Cli::try_parse_from(["woderrise", "info", "pkg", "--no-color"]).unwrap();
        assert_eq!(
            cli_no_color.command,
            Command::Info {
                package: "pkg".into(),
                no_color: true,
            }
        );
    }

    #[test]
    fn test_cli_parse_recover() {
        let cli = Cli::try_parse_from(["woderrise", "recover"]).unwrap();
        assert_eq!(cli.command, Command::Recover);
    }

    #[test]
    fn test_cli_parse_fetch() {
        let cli = Cli::try_parse_from(["woderrise", "fetch", "https://github.com/user/dots"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Fetch {
                url: "https://github.com/user/dots".into(),
                dest: None,
                index: false,
            }
        );

        let cli_full = Cli::try_parse_from([
            "woderrise",
            "fetch",
            "https://github.com/user/dots",
            "--dest",
            "/tmp/dest",
            "--index",
        ])
        .unwrap();
        assert_eq!(
            cli_full.command,
            Command::Fetch {
                url: "https://github.com/user/dots".into(),
                dest: Some(PathBuf::from("/tmp/dest")),
                index: true,
            }
        );
    }

    #[test]
    fn test_is_remote_url() {
        assert!(is_remote_url("https://github.com/foo/bar"));
        assert!(is_remote_url("http://example.com/repo.git"));
        assert!(is_remote_url("git@github.com:foo/bar.git"));
        assert!(is_remote_url("file:///tmp/repo"));
        assert!(!is_remote_url("/tmp/local/path"));
        assert!(!is_remote_url("com.catppuccin.waybar"));
    }
}

//! Dynamic GitHub repository fetcher, Hyprland dotfile detector, and manifest synthesizer.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use walkdir::WalkDir;

use crate::{
    Artifact, Author, Compatibility, Compositor, FileInstall, InstallSpec, Manifest, PackageSpec,
    PaletteColor, Previews, RemovalSpec, WoderriseError,
};

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("invalid repository URL '{url}': {reason}")]
    InvalidUrl { url: String, reason: String },

    #[error("network unreachable or connection failed: {0}")]
    NetworkUnreachable(String),

    #[error("repository not found (404) or private: {0}")]
    NotFound(String),

    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    #[error("missing Hyprland configuration: repository does not contain hyprland.conf")]
    MissingHyprlandConfig,

    #[error("git CLI failed with exit code {code}: {detail}")]
    GitExecution { code: i32, detail: String },

    #[error("git binary not found in PATH")]
    GitNotFound,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub timeout: Duration,
    pub branch: Option<String>,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            branch: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedDotfile {
    pub relative_path: PathBuf,
    pub target_path: String,
    pub sha256: String,
    pub is_primary: bool,
}

#[derive(Debug)]
pub struct FetchedRepository {
    pub temp_dir: tempfile::TempDir,
    pub owner: String,
    pub repo: String,
    pub commit_sha: String,
    pub author_name: String,
    pub author_email: Option<String>,
    pub license: String,
    pub version: String,
    pub description: String,
    pub detected_files: Vec<DetectedDotfile>,
    pub palette: Vec<PaletteColor>,
    pub manifest: Manifest,
}

pub fn parse_and_validate_github_url(raw: &str) -> std::result::Result<(String, String, String), FetchError> {
    if raw.is_empty() {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "URL must not be empty".to_string(),
        });
    }

    // Reject whitespace or control characters immediately (prevents CRLF and WHATWG stripping bypasses)
    if raw.chars().any(|c| c.is_ascii_whitespace() || c.is_ascii_control()) {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "URL must not contain whitespace or control characters".to_string(),
        });
    }

    // Reject arguments starting with flags
    if raw.starts_with('-') {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "URL must not start with flags".to_string(),
        });
    }

    // Reject path traversal sequences
    if raw.contains("..") {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "URL must not contain path traversal sequences ('..')".to_string(),
        });
    }

    // Handle local paths and file:// scheme
    let local_path = if let Some(path) = raw.strip_prefix("file://") {
        Some(path)
    } else if raw.starts_with('/') || raw.starts_with('.') {
        Some(raw)
    } else {
        None
    };

    if let Some(path) = local_path {
        if path.is_empty() || path.starts_with('-') {
            return Err(FetchError::InvalidUrl {
                url: raw.to_string(),
                reason: "local path must not start with flags".to_string(),
            });
        }
        let p = Path::new(path);
        for component in p.components() {
            if let std::path::Component::Normal(os_str) = component {
                if let Some(s) = os_str.to_str() {
                    if s.starts_with('-') {
                        return Err(FetchError::InvalidUrl {
                            url: raw.to_string(),
                            reason: "local path components must not start with flags".to_string(),
                        });
                    }
                }
            }
        }
        let repo_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("fixture");
        let repo_name = if repo_name.is_empty() { "fixture" } else { repo_name };
        return Ok(("local".to_string(), repo_name.to_string(), raw.to_string()));
    }

    // Parse remote URL
    let parsed = url::Url::parse(raw).map_err(|e| FetchError::InvalidUrl {
        url: raw.to_string(),
        reason: e.to_string(),
    })?;

    if parsed.scheme() != "https" {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "only HTTPS URLs are permitted".to_string(),
        });
    }

    if parsed.host_str() != Some("github.com") {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "repository host must be github.com".to_string(),
        });
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "credentials in repository URL are not permitted".to_string(),
        });
    }

    if parsed.port().is_some() {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "custom ports in repository URL are not permitted".to_string(),
        });
    }

    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();

    if segments.len() != 2 {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "URL must include owner and repository name (e.g. https://github.com/owner/repo)".to_string(),
        });
    }

    let owner = segments[0];
    let repo = segments[1].trim_end_matches(".git");

    let is_valid_name = |name: &str| {
        !name.is_empty()
            && !name.starts_with('-')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };

    if !is_valid_name(owner) || !is_valid_name(repo) {
        return Err(FetchError::InvalidUrl {
            url: raw.to_string(),
            reason: "owner and repository names must be alphanumeric with '-', '_', or '.'".to_string(),
        });
    }

    let canonical_url = format!("https://github.com/{owner}/{repo}");
    Ok((owner.to_string(), repo.to_string(), canonical_url))
}

pub fn run_git_bounded(
    command: &mut Command,
    timeout: Duration,
) -> std::result::Result<std::process::Output, FetchError> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0");
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FetchError::GitNotFound
        } else {
            FetchError::Io(e)
        }
    })?;

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let output = child.wait_with_output()?;
            if status.success() {
                return Ok(output);
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let code = status.code().unwrap_or(-1);

            if stderr.contains("Could not resolve host")
                || stderr.contains("Failed to connect")
                || stderr.contains("Connection refused")
                || stderr.contains("Network is unreachable")
            {
                return Err(FetchError::NetworkUnreachable(stderr));
            }
            if stderr.contains("Repository not found")
                || stderr.contains("remote: Not Found")
                || stderr.contains("Authentication failed")
            {
                return Err(FetchError::NotFound(stderr));
            }
            return Err(FetchError::GitExecution { code, detail: stderr });
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FetchError::Timeout(timeout));
        }

        thread::sleep(Duration::from_millis(50));
    }
}

pub fn detect_hyprland_files(root: &Path) -> std::result::Result<Vec<DetectedDotfile>, FetchError> {
    let mut candidate_anchors: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(root)
        .max_depth(6)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() > 0 && e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                if name == ".git" || name == ".github" || name == "target" || name == "node_modules" {
                    return false;
                }
                if name.starts_with('.') && name != ".config" && name != ".dotfiles" && name != ".dots" {
                    return false;
                }
            }
            true
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_file() && entry.file_name() == "hyprland.conf" {
            candidate_anchors.push(entry.path().to_path_buf());
        }
    }

    if candidate_anchors.is_empty() {
        return Err(FetchError::MissingHyprlandConfig);
    }

    // Sort anchors by precedence:
    // .config/hypr/hyprland.conf > hypr/hyprland.conf > hyprland.conf > shortest path
    candidate_anchors.sort_by_key(|p| {
        let rel = p.strip_prefix(root).unwrap_or(p);
        let s = rel.to_string_lossy();
        if s == ".config/hypr/hyprland.conf" {
            0
        } else if s == "hypr/hyprland.conf" {
            1
        } else if s == "hyprland.conf" {
            2
        } else {
            10 + rel.components().count()
        }
    });

    let primary = candidate_anchors[0].clone();
    let hypr_dir = primary.parent().unwrap_or(root).to_path_buf();

    let mut detected: Vec<DetectedDotfile> = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();

    let is_ignored_file = |name: &str| -> bool {
        name.starts_with('.')
            || name.starts_with("README")
            || name.starts_with("LICENSE")
            || name.starts_with("LICENCE")
            || name.starts_with("COPYING")
            || name == "Makefile"
            || name == "install.sh"
            || name == "setup.sh"
            || name == "bootstrap.sh"
            || name == "Cargo.toml"
            || name == "Cargo.lock"
    };

    if hypr_dir == root {
        // Flat repo layout
        for entry in WalkDir::new(root)
            .max_depth(2)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let file_name = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));
            if is_ignored_file(file_name) {
                continue;
            }
            if entry.metadata().map_or(0, |m| m.len()) > 5 * 1024 * 1024 {
                continue;
            }
            if file_name.ends_with(".conf") || file_name == "hyprshade.toml" {
                let rel = path.strip_prefix(root).unwrap_or(path);
                let target = format!("~/.config/hypr/{}", rel.to_string_lossy());
                if seen_targets.insert(target.clone()) {
                    let sha256 = crate::sha256_file(path).map_err(|e| match e {
                        WoderriseError::Io(io_err) => FetchError::Io(io_err),
                        other => FetchError::Io(std::io::Error::other(other.to_string())),
                    })?;
                    detected.push(DetectedDotfile {
                        relative_path: rel.to_path_buf(),
                        target_path: target,
                        sha256,
                        is_primary: path == primary,
                    });
                }
            }
        }
    } else {
        // Structured directory layout
        for entry in WalkDir::new(&hypr_dir)
            .max_depth(5)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let file_name = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));
            if is_ignored_file(file_name) {
                continue;
            }
            if entry.metadata().map_or(0, |m| m.len()) > 5 * 1024 * 1024 {
                continue;
            }
            let rel_from_hypr = match path.strip_prefix(&hypr_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rel_from_root = match path.strip_prefix(root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let target = format!("~/.config/hypr/{}", rel_from_hypr.to_string_lossy());
            if seen_targets.insert(target.clone()) {
                let sha256 = crate::sha256_file(path).map_err(|e| match e {
                    WoderriseError::Io(io_err) => FetchError::Io(io_err),
                    other => FetchError::Io(std::io::Error::other(other.to_string())),
                })?;
                detected.push(DetectedDotfile {
                    relative_path: rel_from_root.to_path_buf(),
                    target_path: target,
                    sha256,
                    is_primary: path == primary,
                });
            }
        }
    }

    // Locate Waybar directories
    let mut waybar_dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = hypr_dir.parent() {
        let sibling_waybar = parent.join("waybar");
        if sibling_waybar.is_dir() {
            waybar_dirs.push(sibling_waybar);
        }
    }
    let dotconfig_waybar = root.join(".config").join("waybar");
    if dotconfig_waybar.is_dir() && !waybar_dirs.contains(&dotconfig_waybar) {
        waybar_dirs.push(dotconfig_waybar);
    }
    let root_waybar = root.join("waybar");
    if root_waybar.is_dir() && !waybar_dirs.contains(&root_waybar) {
        waybar_dirs.push(root_waybar);
    }

    for waybar_dir in waybar_dirs {
        for entry in WalkDir::new(&waybar_dir)
            .max_depth(4)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let file_name = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));
            if is_ignored_file(file_name) {
                continue;
            }
            if entry.metadata().map_or(0, |m| m.len()) > 5 * 1024 * 1024 {
                continue;
            }
            let rel_from_waybar = match path.strip_prefix(&waybar_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rel_from_root = match path.strip_prefix(root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let target = format!("~/.config/waybar/{}", rel_from_waybar.to_string_lossy());
            if seen_targets.insert(target.clone()) {
                let sha256 = crate::sha256_file(path).map_err(|e| match e {
                    WoderriseError::Io(io_err) => FetchError::Io(io_err),
                    other => FetchError::Io(std::io::Error::other(other.to_string())),
                })?;
                detected.push(DetectedDotfile {
                    relative_path: rel_from_root.to_path_buf(),
                    target_path: target,
                    sha256,
                    is_primary: false,
                });
            }
        }
    }

    detected.sort_by(|a, b| {
        b.is_primary.cmp(&a.is_primary).then_with(|| a.relative_path.cmp(&b.relative_path))
    });

    Ok(detected)
}

pub fn detect_license(root: &Path) -> String {
    let license_filenames = [
        "LICENSE", "LICENSE.md", "LICENSE.txt",
        "LICENCE", "LICENCE.md", "LICENCE.txt",
        "COPYING", "COPYING.md", "COPYING.txt",
    ];

    for name in &license_filenames {
        let path = root.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            let sample = if content.len() > 8192 {
                &content[..8192]
            } else {
                &content
            };
            let lower = sample.to_ascii_lowercase();

            for line in sample.lines() {
                if let Some((_, rest)) = line.split_once("SPDX-License-Identifier:") {
                    let id = rest.trim().trim_end_matches("*/").trim();
                    if !id.is_empty() {
                        return id.to_string();
                    }
                }
            }

            if lower.contains("apache license") && lower.contains("version 2.0") {
                return "Apache-2.0".to_string();
            }
            if lower.contains("mit license")
                || (lower.contains("permission is hereby granted") && lower.contains("without restriction"))
            {
                return "MIT".to_string();
            }
            if lower.contains("gnu general public license") {
                if lower.contains("version 3") || lower.contains("gplv3") {
                    return "GPL-3.0-only".to_string();
                }
                if lower.contains("version 2") || lower.contains("gplv2") {
                    return "GPL-2.0-only".to_string();
                }
                return "GPL-3.0-or-later".to_string();
            }
            if lower.contains("gnu affero general public license") {
                return "AGPL-3.0-only".to_string();
            }
            if lower.contains("mozilla public license") && lower.contains("2.0") {
                return "MPL-2.0".to_string();
            }
            if lower.contains("bsd 3-clause")
                || (lower.contains("redistribution and use in source and binary") && lower.contains("neither the name"))
            {
                return "BSD-3-Clause".to_string();
            }
            if lower.contains("bsd 2-clause") {
                return "BSD-2-Clause".to_string();
            }
            if lower.contains("unlicense") || lower.contains("free and unencumbered software") {
                return "Unlicense".to_string();
            }
            if lower.contains("creative commons") || lower.contains("cc0 1.0") {
                return "CC0-1.0".to_string();
            }
            return "LicenseRef-Custom".to_string();
        }
    }

    "NOASSERTION".to_string()
}

pub fn detect_version(dir: &Path, timeout: Duration) -> String {
    let mut cmd = Command::new("git");
    cmd.args(["-C", dir.to_str().unwrap_or("."), "describe", "--tags", "--abbrev=0"]);
    if let Ok(out) = run_git_bounded(&mut cmd, timeout) {
        let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !tag.is_empty() {
            let stripped = tag.strip_prefix('v').or_else(|| tag.strip_prefix('V')).unwrap_or(&tag);
            let core = stripped.split(['-', '+']).next().unwrap_or("");
            let parts: Vec<&str> = core.split('.').collect();
            if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
                return stripped.to_string();
            } else if parts.len() == 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
                return format!("{stripped}.0");
            } else if parts.len() == 1 && !parts[0].is_empty() && parts[0].chars().all(|c| c.is_ascii_digit()) {
                return format!("{stripped}.0.0");
            }
        }
    }
    "0.1.0".to_string()
}

pub fn extract_palette(root: &Path, dotfiles: &[DetectedDotfile]) -> Vec<PaletteColor> {
    let mut colors = Vec::new();
    let mut seen_names = HashSet::new();
    let mut seen_hex = HashSet::new();

    for dotfile in dotfiles {
        let path = root.join(&dotfile.relative_path);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "conf" && ext != "css" {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') && !line.starts_with("#define") {
                continue;
            }

            if let Some(rest) = line.strip_prefix("@define-color") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() >= 2 {
                    let name = parts[0].trim();
                    let hex_raw = parts[1].trim().trim_end_matches(';');
                    if hex_raw.starts_with('#') && hex_raw.len() == 7 && hex_raw[1..].chars().all(|c| c.is_ascii_hexdigit()) {
                        let hex = hex_raw.to_ascii_lowercase();
                        if seen_names.insert(name.to_string()) && seen_hex.insert(hex.clone()) {
                            colors.push(PaletteColor {
                                name: name.to_string(),
                                hex,
                            });
                        }
                    }
                }
            }

            if let Some(rest) = line.strip_prefix('$') {
                if let Some((name, val)) = rest.split_once('=') {
                    let name = name.trim();
                    let val = val.trim();
                    let hex_opt = if val.starts_with('#') && val.len() == 7 {
                        Some(val[1..].to_ascii_lowercase())
                    } else if val.starts_with("0x") && val.len() == 10 {
                        Some(val[4..10].to_ascii_lowercase())
                    } else if val.starts_with("0x") && val.len() == 8 {
                        Some(val[2..8].to_ascii_lowercase())
                    } else if val.starts_with("rgb(") && val.ends_with(')') {
                        let inner = val[4..val.len() - 1].trim();
                        if inner.len() == 6 {
                            Some(inner.to_ascii_lowercase())
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(hex_part) = hex_opt {
                        if hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
                            let hex = format!("#{hex_part}");
                            if seen_names.insert(name.to_string()) && seen_hex.insert(hex.clone()) {
                                colors.push(PaletteColor {
                                    name: name.to_string(),
                                    hex,
                                });
                            }
                        }
                    }
                }
            }

            if colors.len() >= 10 {
                break;
            }
        }
        if colors.len() >= 10 {
            break;
        }
    }

    colors
}

#[allow(clippy::too_many_arguments)]
pub fn synthesize_manifest(
    owner: &str,
    repo: &str,
    commit_sha: &str,
    canonical_url: &str,
    author_name: &str,
    author_email: Option<&str>,
    license: &str,
    version: &str,
    description: &str,
    dotfiles: &[DetectedDotfile],
    palette: Vec<PaletteColor>,
) -> std::result::Result<Manifest, WoderriseError> {
    if dotfiles.is_empty() {
        return Err(WoderriseError::Validation("cannot synthesize manifest with zero detected dotfiles".into()));
    }

    let clean_owner: String = owner
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let clean_owner = if clean_owner.is_empty() { "unknown".to_string() } else { clean_owner };

    let clean_repo: String = repo
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let clean_repo = if clean_repo.is_empty() { "config".to_string() } else { clean_repo };

    let package_id = format!("io.github.{clean_owner}.{clean_repo}");

    let artifacts: Vec<Artifact> = dotfiles
        .iter()
        .map(|d| {
            let path_str = d.relative_path.to_string_lossy().to_string();
            let url = if canonical_url.starts_with("https://") {
                Some(format!("https://raw.githubusercontent.com/{owner}/{repo}/{commit_sha}/{path_str}"))
            } else {
                None
            };
            Artifact {
                path: path_str,
                sha256: d.sha256.clone(),
                url,
            }
        })
        .collect();

    let install_files: Vec<FileInstall> = dotfiles
        .iter()
        .map(|d| FileInstall {
            source: d.relative_path.to_string_lossy().to_string(),
            target: d.target_path.clone(),
            mode: Some("0644".to_string()),
            strategy: Some("replace".to_string()),
            backup_before_write: true,
        })
        .collect();

    let primary_file = dotfiles
        .iter()
        .find(|d| d.is_primary)
        .map(|d| d.relative_path.to_string_lossy().to_string())
        .unwrap_or_else(|| dotfiles[0].relative_path.to_string_lossy().to_string());

    let previews = Some(Previews {
        thumbnail: primary_file,
        screenshots: Vec::new(),
        palette,
    });

    let manifest = Manifest {
        schema_version: "1.0.0".to_string(),
        package: PackageSpec {
            id: package_id,
            name: format!("{repo} Hyprland Config"),
            version: version.to_string(),
            package_type: "config".to_string(),
            category: "window_manager_hyprland".to_string(),
            license: license.to_string(),
            authors: vec![Author {
                name: if author_name.is_empty() { owner.to_string() } else { author_name.to_string() },
                email: author_email.map(str::to_string),
                url: if canonical_url.starts_with("https://") {
                    Some(format!("https://github.com/{owner}"))
                } else {
                    None
                },
            }],
            description: if description.is_empty() {
                format!("Hyprland desktop configuration dynamically imported from {canonical_url}")
            } else {
                description.to_string()
            },
            tags: vec![
                "hyprland".to_string(),
                "wayland".to_string(),
                "desktop-config".to_string(),
                "dynamic-import".to_string(),
            ],
            repository: if canonical_url.starts_with("https://") {
                Some(canonical_url.to_string())
            } else {
                None
            },
        },
        yanked: false,
        previews,
        compatibility: Some(Compatibility {
            compositors: vec![Compositor {
                name: "hyprland".to_string(),
                version: ">=0.40.0".to_string(),
            }],
            display_server: Some("wayland_only".to_string()),
            architectures: vec!["x86_64".to_string(), "aarch64".to_string()],
        }),
        dependencies: None,
        conflicts: Vec::new(),
        artifacts,
        install: InstallSpec {
            files: install_files,
            config_injections: Vec::new(),
            service_triggers: Vec::new(),
        },
        removal: Some(RemovalSpec {
            revert_files: true,
            remove_injections: false,
            service_triggers: Vec::new(),
        }),
    };

    manifest.validate()?;
    Ok(manifest)
}

pub fn fetch_github_repository(
    url: &str,
    options: FetchOptions,
) -> std::result::Result<FetchedRepository, WoderriseError> {
    let (owner, repo, canonical_url) = parse_and_validate_github_url(url)?;

    let temp_dir = tempfile::Builder::new()
        .prefix("woderrise-fetch-")
        .tempdir()
        .map_err(WoderriseError::Io)?;

    let dest_str = temp_dir.path().to_string_lossy().to_string();

    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.stdin(Stdio::null());

    if canonical_url.starts_with("https://") {
        let mut args = vec![
            "clone",
            "--depth", "1",
            "--filter=blob:none",
            "--single-branch",
        ];
        if let Some(branch) = &options.branch {
            args.push("--branch");
            args.push(branch);
        }
        args.push("--");
        args.push(&canonical_url);
        args.push(&dest_str);
        cmd.args(&args);
    } else {
        let source_path = canonical_url.strip_prefix("file://").unwrap_or(&canonical_url);
        cmd.args(["clone", "--", source_path, &dest_str]);
    }

    run_git_bounded(&mut cmd, options.timeout)?;

    let commit_sha = get_git_commit_sha(temp_dir.path(), options.timeout)?;
    let (author_name, author_email) = get_git_author_info(temp_dir.path(), options.timeout)?;
    let description = get_git_description(temp_dir.path(), options.timeout)
        .unwrap_or_else(|| format!("{repo} desktop configuration"));
    let license = detect_license(temp_dir.path());
    let version = detect_version(temp_dir.path(), options.timeout);

    let detected_files = detect_hyprland_files(temp_dir.path())?;
    let palette = extract_palette(temp_dir.path(), &detected_files);

    let manifest = synthesize_manifest(
        &owner,
        &repo,
        &commit_sha,
        &canonical_url,
        &author_name,
        author_email.as_deref(),
        &license,
        &version,
        &description,
        &detected_files,
        palette.clone(),
    )?;

    let manifest_yaml = serde_yaml::to_string(&manifest)
        .map_err(|e| WoderriseError::Other(format!("failed to serialize manifest: {e}")))?;
    fs::write(temp_dir.path().join("woderrise.yaml"), manifest_yaml)?;

    Ok(FetchedRepository {
        temp_dir,
        owner,
        repo,
        commit_sha,
        author_name,
        author_email,
        license,
        version,
        description,
        detected_files,
        palette,
        manifest,
    })
}

fn get_git_commit_sha(dir: &Path, timeout: Duration) -> std::result::Result<String, FetchError> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", dir.to_str().unwrap_or("."), "rev-parse", "HEAD"]);
    let out = run_git_bounded(&mut cmd, timeout)?;
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() {
        return Err(FetchError::GitExecution {
            code: 1,
            detail: "empty commit SHA from rev-parse HEAD".to_string(),
        });
    }
    Ok(sha)
}

fn get_git_author_info(dir: &Path, timeout: Duration) -> std::result::Result<(String, Option<String>), FetchError> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", dir.to_str().unwrap_or("."), "log", "-1", "--format=%an%x00%ae"]);
    if let Ok(out) = run_git_bounded(&mut cmd, timeout) {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut parts = s.split('\0');
        let name = parts.next().map(|n| n.trim().to_string()).unwrap_or_default();
        let email_raw = parts.next().map(|e| e.trim().to_string()).unwrap_or_default();
        let email = if email_raw.is_empty() || email_raw.contains("noreply") {
            None
        } else {
            Some(email_raw)
        };
        Ok((name, email))
    } else {
        Ok((String::new(), None))
    }
}

fn get_git_description(dir: &Path, timeout: Duration) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["-C", dir.to_str().unwrap_or("."), "log", "-1", "--format=%s"]);
    if let Ok(out) = run_git_bounded(&mut cmd, timeout) {
        let desc = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !desc.is_empty() {
            return Some(desc);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BackupManager, Database, InstallationEngine};

    #[test]
    fn test_parse_and_validate_github_url_valid() {
        let (owner, repo, canon) = parse_and_validate_github_url("https://github.com/hyprwm/hyprland").unwrap();
        assert_eq!(owner, "hyprwm");
        assert_eq!(repo, "hyprland");
        assert_eq!(canon, "https://github.com/hyprwm/hyprland");

        let (owner, repo, _) = parse_and_validate_github_url("https://github.com/catppuccin/hyprland.git").unwrap();
        assert_eq!(owner, "catppuccin");
        assert_eq!(repo, "hyprland");

        let (owner, repo, _) = parse_and_validate_github_url("file:///tmp/test-repo").unwrap();
        assert_eq!(owner, "local");
        assert_eq!(repo, "test-repo");

        let (owner, repo, _) = parse_and_validate_github_url("/tmp/local-path").unwrap();
        assert_eq!(owner, "local");
        assert_eq!(repo, "local-path");

        let (owner, repo, _) = parse_and_validate_github_url("./relative-path").unwrap();
        assert_eq!(owner, "local");
        assert_eq!(repo, "relative-path");
    }

    #[test]
    fn test_parse_and_validate_github_url_rejections() {
        assert!(parse_and_validate_github_url("http://github.com/owner/repo").is_err());
        assert!(parse_and_validate_github_url("https://gitlab.com/owner/repo").is_err());
        assert!(parse_and_validate_github_url("https://github.com/owner").is_err());
        assert!(parse_and_validate_github_url("https://github.com/owner/repo/pull/1").is_err());
        assert!(parse_and_validate_github_url("--upload-pack=evil").is_err());
        assert!(parse_and_validate_github_url("").is_err());
        assert!(parse_and_validate_github_url("https://github.com/-invalid/repo").is_err());
        assert!(parse_and_validate_github_url("https://github.com/owner/-invalid").is_err());
    }

    #[test]
    fn test_detect_hyprland_files_layout_a_dotconfig() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hypr = root.join(".config/hypr");
        let waybar = root.join(".config/waybar");
        fs::create_dir_all(&hypr).unwrap();
        fs::create_dir_all(&waybar).unwrap();

        fs::write(hypr.join("hyprland.conf"), "general { gaps_in = 5 }\n$accent = #cba6f7\n").unwrap();
        fs::write(hypr.join("hyprlock.conf"), "background { color = #1e1e2e }\n").unwrap();
        fs::write(waybar.join("config"), "{\"layer\": \"top\"}\n").unwrap();
        fs::write(waybar.join("style.css"), "@define-color base #1e1e2e;\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 4);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf" && f.is_primary));
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprlock.conf" && !f.is_primary));
        assert!(files.iter().any(|f| f.target_path == "~/.config/waybar/config"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/waybar/style.css"));
    }

    #[test]
    fn test_detect_hyprland_files_layout_b_top_level_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let hypr = root.join("hypr");
        let waybar = root.join("waybar");
        fs::create_dir_all(&hypr).unwrap();
        fs::create_dir_all(&waybar).unwrap();

        fs::write(hypr.join("hyprland.conf"), "monitor=,preferred,auto,1\n").unwrap();
        fs::write(hypr.join("hypridle.conf"), "general { lock_cmd = hyprlock }\n").unwrap();
        fs::write(waybar.join("config.jsonc"), "{\"layer\": \"top\"}\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hypridle.conf"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/waybar/config.jsonc"));
    }

    #[test]
    fn test_detect_hyprland_files_layout_c_flat() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        fs::write(root.join("hyprland.conf"), "monitor=,preferred,auto,1\n").unwrap();
        fs::write(root.join("hyprpaper.conf"), "preload = ~/bg.png\n").unwrap();
        fs::write(root.join("README.md"), "# Dotfiles\n").unwrap();
        fs::write(root.join("LICENSE"), "MIT License\n").unwrap();
        fs::write(root.join("install.sh"), "#!/bin/bash\n").unwrap();

        let files = detect_hyprland_files(root).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprland.conf"));
        assert!(files.iter().any(|f| f.target_path == "~/.config/hypr/hyprpaper.conf"));
        assert!(!files.iter().any(|f| f.target_path.contains("README")));
        assert!(!files.iter().any(|f| f.target_path.contains("LICENSE")));
    }

    #[test]
    fn test_detect_hyprland_files_missing_hyprland_conf() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("alacritty.toml"), "[window]\n").unwrap();

        let err = detect_hyprland_files(root).unwrap_err();
        assert!(matches!(err, FetchError::MissingHyprlandConfig));
    }

    #[test]
    fn test_detect_license_variations() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        assert_eq!(detect_license(root), "NOASSERTION");

        fs::write(root.join("LICENSE"), "The MIT License (MIT)\nPermission is hereby granted, free of charge\nwithout restriction").unwrap();
        assert_eq!(detect_license(root), "MIT");

        fs::write(root.join("LICENSE"), "Apache License\nVersion 2.0, January 2004\nhttp://www.apache.org/licenses/").unwrap();
        assert_eq!(detect_license(root), "Apache-2.0");

        fs::write(root.join("LICENSE"), "GNU GENERAL PUBLIC LICENSE\nVersion 3, 29 June 2007").unwrap();
        assert_eq!(detect_license(root), "GPL-3.0-only");

        fs::write(root.join("LICENSE"), "// SPDX-License-Identifier: BSD-3-Clause\n").unwrap();
        assert_eq!(detect_license(root), "BSD-3-Clause");
    }

    #[test]
    fn test_extract_palette_colors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let hypr_conf = "general {\n    $accent = #cba6f7\n    $bg = 0xff1e1e2e\n    $fg = rgb(cdd6f4)\n}\n";
        let style_css = "@define-color surface #313244;\n@define-color overlay #6c7086;\n";

        fs::write(root.join("hyprland.conf"), hypr_conf).unwrap();
        fs::write(root.join("style.css"), style_css).unwrap();

        let dotfiles = vec![
            DetectedDotfile {
                relative_path: PathBuf::from("hyprland.conf"),
                target_path: "~/.config/hypr/hyprland.conf".to_string(),
                sha256: "0".repeat(64),
                is_primary: true,
            },
            DetectedDotfile {
                relative_path: PathBuf::from("style.css"),
                target_path: "~/.config/waybar/style.css".to_string(),
                sha256: "0".repeat(64),
                is_primary: false,
            },
        ];

        let palette = extract_palette(root, &dotfiles);
        assert_eq!(palette.len(), 5);
        assert!(palette.iter().any(|c| c.name == "accent" && c.hex == "#cba6f7"));
        assert!(palette.iter().any(|c| c.name == "bg" && c.hex == "#1e1e2e"));
        assert!(palette.iter().any(|c| c.name == "fg" && c.hex == "#cdd6f4"));
        assert!(palette.iter().any(|c| c.name == "surface" && c.hex == "#313244"));
        assert!(palette.iter().any(|c| c.name == "overlay" && c.hex == "#6c7086"));
    }

    #[test]
    fn test_dynamic_fetcher_hermetic_local_git_e2e() {
        let git_dir = tempfile::tempdir().unwrap();
        let p = git_dir.path();

        Command::new("git").args(["init"]).current_dir(p).status().unwrap();
        Command::new("git").args(["checkout", "-b", "main"]).current_dir(p).status().unwrap();
        Command::new("git").args(["config", "user.name", "Catppuccin Tester"]).current_dir(p).status().unwrap();
        Command::new("git").args(["config", "user.email", "tester@catppuccin.invalid"]).current_dir(p).status().unwrap();
        Command::new("git").args(["config", "commit.gpgsign", "false"]).current_dir(p).status().unwrap();

        let hypr_dir = p.join("hypr");
        fs::create_dir_all(&hypr_dir).unwrap();
        fs::write(hypr_dir.join("hyprland.conf"), "general { gaps_in = 5 }\n$accent = #cba6f7\n").unwrap();
        fs::write(hypr_dir.join("hyprlock.conf"), "background { color = #1e1e2e }\n").unwrap();
        fs::write(p.join("LICENSE"), "MIT License\nCopyright (c) 2026 Catppuccin\nPermission is hereby granted, free of charge\nwithout restriction\n").unwrap();

        Command::new("git").args(["add", "."]).current_dir(p).status().unwrap();
        Command::new("git").args(["commit", "-m", "feat: initial authentic hyprland setup"]).current_dir(p).status().unwrap();
        Command::new("git").args(["tag", "v1.0.0"]).current_dir(p).status().unwrap();

        let fetched = fetch_github_repository(p.to_str().unwrap(), FetchOptions::default()).unwrap();
        assert_eq!(fetched.author_name, "Catppuccin Tester");
        assert_eq!(fetched.author_email, Some("tester@catppuccin.invalid".to_string()));
        assert_eq!(fetched.license, "MIT");
        assert_eq!(fetched.version, "1.0.0");
        assert_eq!(fetched.detected_files.len(), 2);
        assert!(fetched.manifest.validate().is_ok());

        // Verify woderrise.yaml exists in temp_dir
        assert!(fetched.temp_dir.path().join("woderrise.yaml").exists());

        // Verify InstallationEngine can compute diff plan, install, and rollback
        let sim_home = tempfile::tempdir().unwrap();
        let db = Database::in_memory().unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(sim_home.path().join("backups")).unwrap(),
            home: sim_home.path().to_path_buf(),
        };

        let plan = engine.compute_plan(fetched.temp_dir.path()).unwrap();
        assert_eq!(plan.file_diffs.len(), 2);

        let txn = engine.install(fetched.temp_dir.path(), "test_runner").unwrap();
        assert!(sim_home.path().join(".config/hypr/hyprland.conf").exists());
        assert!(sim_home.path().join(".config/hypr/hyprlock.conf").exists());

        let rollback_res = engine.rollback(&txn, false);
        assert!(rollback_res.is_ok());
        assert!(!sim_home.path().join(".config/hypr/hyprland.conf").exists());
    }
}

//! Wallpaper asset detection, hyprpaper.conf parsing, and path remapping heuristics.

use crate::detector::ComponentDetector;
use crate::types::{ComponentDetection, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use regex::Regex;
use std::sync::LazyLock;
use woderrise_core::models::{ConfigComponent, SyntaxFormat};

static HYPRPAPER_DIRECTIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(preload|wallpaper)\s*=\s*(?:([^,]+),)?\s*(.+)$")
        .expect("Invalid hyprpaper directive regex")
});

/// List of recognized wallpaper image extensions.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Check if a path corresponds to a wallpaper or graphic asset.
pub fn is_wallpaper_asset(path: &str) -> bool {
    let p = Utf8Path::new(path);
    if let Some(ext) = p.extension() {
        IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

/// Component detector for Wallpapers and Hyprpaper configuration.
pub struct WallpaperDetector;

impl WallpaperDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WallpaperDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for WallpaperDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Wallpapers
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        let mut hyprpaper_entry: Option<&TreeEntry> = None;
        let mut image_entries = Vec::new();
        let mut total_size: u64 = 0;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "hyprpaper.conf" {
                    hyprpaper_entry = Some(entry);
                } else if is_wallpaper_asset(&entry.path) {
                    image_entries.push(entry.clone());
                    total_size += entry.size.unwrap_or(0);
                }
            }
        }

        // If neither hyprpaper.conf nor images exist, not detected
        if hyprpaper_entry.is_none() && image_entries.is_empty() {
            return None;
        }

        let mut matched = Vec::new();
        let entrypoint = if let Some(hp) = hyprpaper_entry {
            matched.push(hp.clone());
            total_size += hp.size.unwrap_or(0);
            hp.path.clone()
        } else {
            image_entries[0].path.clone()
        };

        matched.extend(image_entries);

        let root_dir = if let Some(hp) = hyprpaper_entry {
            hp.utf8_path()
                .parent()
                .map(|p| p.to_string())
                .unwrap_or_default()
        } else {
            "wallpapers".to_string()
        };

        Some(ComponentDetection {
            component: ConfigComponent::Wallpapers,
            syntax_format: if hyprpaper_entry.is_some() {
                SyntaxFormat::LegacyConf
            } else {
                SyntaxFormat::BinaryAsset
            },
            root_path: root_dir,
            entrypoint,
            style_file: None,
            file_count: matched.len(),
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices: Vec::new(),
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        if file_path.file_name() == Some("hyprpaper.conf") {
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if (trimmed.starts_with("preload") || trimmed.starts_with("wallpaper"))
                    && !trimmed.contains('=')
                {
                    return SyntaxValidationResult::error(
                        file_path.as_str(),
                        format!(
                            "Malformed hyprpaper directive at line {}: missing '='",
                            idx + 1
                        ),
                        Some(idx + 1),
                        None,
                    );
                }
            }
        }
        SyntaxValidationResult::ok(file_path.as_str())
    }
}

/// Remap author hardcoded paths in `hyprpaper.conf` to user `$XDG_DATA_HOME/wallpapers/`.
///
/// Converts lines like:
/// `preload = /home/bob/wallpapers/neon_city.png`
/// into:
/// `preload = /home/user/.local/share/wallpapers/neon_city.png`
pub fn remap_hyprpaper_paths(
    content: &str,
    target_wallpaper_dir: &str,
    repo_images: &[TreeEntry],
) -> String {
    let mut out_lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            out_lines.push(line.to_string());
            continue;
        }

        if let Some(captures) = HYPRPAPER_DIRECTIVE_RE.captures(trimmed) {
            let directive = captures.get(1).map_or("", |m| m.as_str());
            let display_opt = captures.get(2).map(|m| m.as_str().trim());
            let raw_path = captures.get(3).map_or("", |m| m.as_str().trim());

            // Extract file name from raw path
            let file_name = Utf8Path::new(raw_path).file_name().unwrap_or(raw_path);

            // Check if this image file actually exists in the repository
            let matched_in_repo = repo_images
                .iter()
                .any(|entry| entry.file_name() == Some(file_name));

            if matched_in_repo || is_wallpaper_asset(file_name) {
                let normalized_dir = target_wallpaper_dir.trim_end_matches('/');
                let remapped_path = format!("{}/{}", normalized_dir, file_name);

                let new_line = if let Some(disp) = display_opt {
                    format!("{} = {},{}", directive, disp, remapped_path)
                } else {
                    format!("{} = {}", directive, remapped_path)
                };
                out_lines.push(new_line);
                continue;
            }
        }

        out_lines.push(line.to_string());
    }

    out_lines.join("\n")
}

//! GTK 3/4 settings.ini, index.theme, and Hyprcursor manifest.hl detection.

use crate::detector::ComponentDetector;
use crate::types::{ComponentDetection, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use woderrise_core::models::{ConfigComponent, SyntaxFormat};

/// Component detector specifically for Hyprcursor cursor themes (`manifest.hl`).
pub struct HyprcursorDetector;

impl HyprcursorDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HyprcursorDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for HyprcursorDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Hyprcursor
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        let mut manifest_entry: Option<&TreeEntry> = None;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "manifest.hl"
                    && (manifest_entry.is_none()
                        || entry.path.len() < manifest_entry.unwrap().path.len())
                {
                    manifest_entry = Some(entry);
                }
            }
        }

        let manifest = manifest_entry?;
        let root_dir = manifest
            .utf8_path()
            .parent()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let prefix = if root_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", root_dir)
        };

        let mut matched = Vec::new();
        let mut total_size: u64 = 0;

        for entry in tree {
            if entry.is_blob() && (prefix.is_empty() || entry.path.starts_with(&prefix)) {
                matched.push(entry.clone());
                total_size += entry.size.unwrap_or(0);
            }
        }

        Some(ComponentDetection {
            component: ConfigComponent::Hyprcursor,
            syntax_format: SyntaxFormat::Hl,
            root_path: root_dir,
            entrypoint: manifest.path.clone(),
            style_file: None,
            file_count: matched.len(),
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices: Vec::new(),
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        validate_hyprcursor_manifest(file_path.as_str(), content)
    }
}

/// Component detector for GTK 3/4 settings and icon themes.
pub struct GtkDetector;

impl GtkDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GtkDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for GtkDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::GtkTheme
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        let mut settings_entry: Option<&TreeEntry> = None;
        let mut index_theme_entry: Option<&TreeEntry> = None;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "settings.ini" {
                    if settings_entry.is_none() || entry.path.contains("gtk-3.0") {
                        settings_entry = Some(entry);
                    }
                } else if name == "index.theme" {
                    index_theme_entry = Some(entry);
                }
            }
        }

        let primary_entry = settings_entry.or(index_theme_entry)?;
        let root_dir = primary_entry
            .utf8_path()
            .parent()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let prefix = if root_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", root_dir)
        };

        let mut matched = Vec::new();
        let mut total_size: u64 = 0;

        for entry in tree {
            if entry.is_blob() && (prefix.is_empty() || entry.path.starts_with(&prefix)) {
                matched.push(entry.clone());
                total_size += entry.size.unwrap_or(0);
            }
        }

        Some(ComponentDetection {
            component: ConfigComponent::GtkTheme,
            syntax_format: SyntaxFormat::Ini,
            root_path: root_dir,
            entrypoint: primary_entry.path.clone(),
            style_file: None,
            file_count: matched.len(),
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices: Vec::new(),
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        if file_path.file_name() == Some("manifest.hl") {
            validate_hyprcursor_manifest(file_path.as_str(), content)
        } else if file_path.extension() == Some("ini") {
            validate_gtk_settings_ini(file_path.as_str(), content)
        } else {
            SyntaxValidationResult::ok(file_path.as_str())
        }
    }
}

/// Validate Hyprcursor `manifest.hl` format.
pub fn validate_hyprcursor_manifest(file_name: &str, content: &str) -> SyntaxValidationResult {
    let mut has_name = false;
    let mut has_cursors_dir = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with("name") && trimmed.contains('=') {
            has_name = true;
        }
        if trimmed.starts_with("cursors_directory") && trimmed.contains('=') {
            has_cursors_dir = true;
        }
    }

    if !has_name && !has_cursors_dir {
        return SyntaxValidationResult::error(
            file_name,
            "manifest.hl must define 'name' or 'cursors_directory'",
            None,
            None,
        );
    }

    SyntaxValidationResult::ok(file_name)
}

/// Validate GTK `settings.ini` format.
pub fn validate_gtk_settings_ini(file_name: &str, content: &str) -> SyntaxValidationResult {
    let mut has_settings_header = false;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed == "[Settings]" {
            has_settings_header = true;
        } else if !trimmed.starts_with('[') && !trimmed.contains('=') {
            return SyntaxValidationResult::error(
                file_name,
                format!("Invalid INI line without '=' at line {}", idx + 1),
                Some(idx + 1),
                None,
            );
        }
    }

    if !has_settings_header {
        return SyntaxValidationResult::error(
            file_name,
            "GTK settings.ini missing required '[Settings]' section header",
            None,
            None,
        );
    }

    SyntaxValidationResult::ok(file_name)
}

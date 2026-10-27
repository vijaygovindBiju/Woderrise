//! Hyprlock screen locker configuration detector and validator.

use crate::detector::ComponentDetector;
use crate::types::{ComponentDetection, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use woderrise_core::models::{ConfigComponent, SyntaxFormat};

/// Component detector for Hyprlock (`hyprlock.conf`).
pub struct HyprlockDetector;

impl HyprlockDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HyprlockDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for HyprlockDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Hyprlock
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        let mut entrypoint_entry: Option<&TreeEntry> = None;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "hyprlock.conf"
                    && (entrypoint_entry.is_none()
                        || entry.path.len() < entrypoint_entry.unwrap().path.len())
                {
                    entrypoint_entry = Some(entry);
                }
            }
        }

        let entry = entrypoint_entry?;
        let entrypoint = entry.path.clone();
        let root_dir = entry
            .utf8_path()
            .parent()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let matched = vec![entry.clone()];
        let total_size = entry.size.unwrap_or(0);

        Some(ComponentDetection {
            component: ConfigComponent::Hyprlock,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: root_dir,
            entrypoint,
            style_file: None,
            file_count: 1,
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices: Vec::new(),
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        validate_hyprlock_conf(file_path.as_str(), content)
    }
}

/// Validate `hyprlock.conf` block syntax.
pub fn validate_hyprlock_conf(file_name: &str, content: &str) -> SyntaxValidationResult {
    let mut brace_depth: i32 = 0;
    let mut has_background = false;
    let mut has_input_field = false;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("background") && trimmed.contains('{') {
            has_background = true;
        }
        if trimmed.starts_with("input-field") && trimmed.contains('{') {
            has_input_field = true;
        }

        for ch in trimmed.chars() {
            if ch == '{' {
                brace_depth += 1;
            } else if ch == '}' {
                brace_depth -= 1;
                if brace_depth < 0 {
                    return SyntaxValidationResult::error(
                        file_name,
                        format!("Unexpected closing brace '}}' at line {}", idx + 1),
                        Some(idx + 1),
                        None,
                    );
                }
            }
        }
    }

    if brace_depth != 0 {
        return SyntaxValidationResult::error(
            file_name,
            format!("Unclosed block in hyprlock.conf (depth: {})", brace_depth),
            None,
            None,
        );
    }

    if !has_background && !has_input_field {
        // Warning or light validation: hyprlock config usually defines at least one
    }

    SyntaxValidationResult::ok(file_name)
}

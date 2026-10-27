//! Hardware monitor isolation, display config preservation, and notice generation.

use crate::types::{ComponentDetection, SyntaxValidationResult, TreeEntry};
use camino::{Utf8Path, Utf8PathBuf};
use regex::Regex;
use std::sync::LazyLock;
use woderrise_core::models::{ConfigComponent, HardwareConflictNotice, SyntaxFormat};

static INLINE_MONITOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*monitor\s*=\s*(.+)$").expect("Invalid inline monitor regex"));

/// List of filenames that are unconditionally classified as Hardware-Bound.
pub const HARDWARE_BOUND_FILENAMES: &[&str] = &[
    "monitors.conf",
    "monitor.conf",
    "workspaces.conf",
    "hardware.conf",
    "display.conf",
    "displays.conf",
    "screens.conf",
];

/// Check if a filename corresponds to a hardware-bound display configuration.
pub fn is_hardware_bound_filename(file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    HARDWARE_BOUND_FILENAMES.contains(&lower.as_str())
}

/// Check if a file path corresponds to a hardware-bound display configuration.
pub fn is_hardware_bound_path(path: &str) -> bool {
    let p = Utf8Path::new(path);
    if let Some(name) = p.file_name() {
        is_hardware_bound_filename(name)
    } else {
        false
    }
}

/// Create a standardized `HardwareConflictNotice` for a hardware-bound target file.
pub fn create_hardware_conflict_notice(file_path: &Utf8Path) -> HardwareConflictNotice {
    HardwareConflictNotice {
        file_path: file_path.to_path_buf(),
        description: "Hardware display and monitor configuration. Preserved by default to protect host resolution, refresh rate, and multi-monitor layout.".to_string(),
        preserved_by_default: true,
    }
}

/// Scan a file's content (such as `hyprland.conf`) for inline `monitor =` directives.
pub fn detect_inline_monitor_directives(
    file_path: &str,
    content: &str,
) -> Vec<HardwareConflictNotice> {
    let mut notices = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some(captures) = INLINE_MONITOR_RE.captures(trimmed) {
            let spec = captures.get(1).map_or("", |m| m.as_str());
            notices.push(HardwareConflictNotice {
                file_path: Utf8PathBuf::from(file_path),
                description: format!(
                    "Inline monitor configuration detected at line {} ('monitor = {}'). Overwriting will alter display resolution and multi-monitor setup.",
                    idx + 1,
                    spec
                ),
                preserved_by_default: true,
            });
        }
    }

    notices
}

/// Independent detector for dedicated monitor configuration files.
pub struct MonitorDetector;

impl MonitorDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MonitorDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::detector::ComponentDetector for MonitorDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Monitors
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        let mut matched = Vec::new();
        let mut total_size: u64 = 0;
        let mut notices = Vec::new();

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if is_hardware_bound_filename(name) {
                    matched.push(entry.clone());
                    total_size += entry.size.unwrap_or(0);
                    notices.push(create_hardware_conflict_notice(entry.utf8_path()));
                }
            }
        }

        if matched.is_empty() {
            return None;
        }

        let entrypoint = matched[0].path.clone();
        let root_path = matched[0]
            .utf8_path()
            .parent()
            .map(|p| p.to_string())
            .unwrap_or_default();

        Some(ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path,
            entrypoint,
            style_file: None,
            file_count: matched.len(),
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices: notices,
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.starts_with("monitor") && !trimmed.contains('=') {
                return SyntaxValidationResult::error(
                    file_path.as_str(),
                    format!(
                        "Malformed monitor directive at line {}: missing '='",
                        idx + 1
                    ),
                    Some(idx + 1),
                    None,
                );
            }
        }
        SyntaxValidationResult::ok(file_path.as_str())
    }
}

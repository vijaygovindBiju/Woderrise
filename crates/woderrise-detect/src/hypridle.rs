//! Hypridle idle daemon configuration detector and validator.

use crate::detector::ComponentDetector;
use crate::types::{ComponentDetection, ProceduralDirective, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use regex::Regex;
use std::sync::LazyLock;
use woderrise_core::models::{ConfigComponent, SyntaxFormat};

static TIMEOUT_DIRECTIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*(on-timeout|on-resume)\s*=\s*(.+)$"#)
        .expect("Invalid timeout directive regex")
});

/// Component detector for Hypridle (`hypridle.conf`).
pub struct HypridleDetector;

impl HypridleDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HypridleDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for HypridleDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Hypridle
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        let mut entrypoint_entry: Option<&TreeEntry> = None;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "hypridle.conf"
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
            component: ConfigComponent::Hypridle,
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
        validate_hypridle_conf(file_path.as_str(), content)
    }
}

/// Validate `hypridle.conf` syntax and listener timeout blocks.
pub fn validate_hypridle_conf(file_name: &str, content: &str) -> SyntaxValidationResult {
    let mut brace_depth: i32 = 0;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("timeout") {
            if let Some(eq_pos) = trimmed.find('=') {
                let val_str = trimmed[eq_pos + 1..].trim();
                if val_str.parse::<u64>().is_err() {
                    return SyntaxValidationResult::error(
                        file_name,
                        format!(
                            "Invalid non-numeric timeout value '{}' at line {}",
                            val_str,
                            idx + 1
                        ),
                        Some(idx + 1),
                        None,
                    );
                }
            }
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
            format!("Unclosed block in hypridle.conf (depth: {})", brace_depth),
            None,
            None,
        );
    }

    SyntaxValidationResult::ok(file_name)
}

/// Extract procedural commands (`on-timeout`, `on-resume`) from `hypridle.conf`.
pub fn extract_hypridle_directives(file_name: &str, content: &str) -> Vec<ProceduralDirective> {
    let mut directives = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some(captures) = TIMEOUT_DIRECTIVE_RE.captures(trimmed) {
            let dir_type = captures.get(1).map_or("", |m| m.as_str()).to_string();
            let mut cmd = captures
                .get(2)
                .map_or("", |m| m.as_str())
                .trim()
                .to_string();
            if (cmd.starts_with('"') && cmd.ends_with('"'))
                || (cmd.starts_with('\'') && cmd.ends_with('\''))
            {
                cmd = cmd[1..cmd.len() - 1].to_string();
            }

            directives.push(ProceduralDirective {
                directive_type: dir_type,
                command: cmd,
                file: file_name.to_string(),
                line: idx + 1,
            });
        }
    }

    directives
}

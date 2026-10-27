//! Hyprland configuration detection (legacy conf and modern Lua) and syntax validation.

use crate::detector::ComponentDetector;
use crate::monitors::{create_hardware_conflict_notice, is_hardware_bound_filename};
use crate::types::{ComponentDetection, ProceduralDirective, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use regex::Regex;
use std::sync::LazyLock;
use tracing::debug;
use woderrise_core::models::{ConfigComponent, SyntaxFormat};

static EXEC_DIRECTIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(exec-once|exec)\s*=\s*(.+)$").expect("Invalid exec directive regex")
});

static SOURCE_DIRECTIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*source\s*=\s*(.+)$").expect("Invalid source directive regex")
});

/// Component detector for Hyprland compositor configurations.
pub struct HyprlandDetector;

impl HyprlandDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HyprlandDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for HyprlandDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Hyprland
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        // 1. Locate candidates for legacy conf and lua entrypoints
        let mut conf_candidate: Option<&TreeEntry> = None;
        let mut lua_candidate: Option<&TreeEntry> = None;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "hyprland.conf"
                    && (conf_candidate.is_none()
                        || entry.path.len() < conf_candidate.unwrap().path.len())
                {
                    conf_candidate = Some(entry);
                } else if name == "hyprland.lua"
                    && (lua_candidate.is_none()
                        || entry.path.len() < lua_candidate.unwrap().path.len())
                {
                    lua_candidate = Some(entry);
                }
            }
        }

        // 2. Precedence handling: default to legacy conf if both exist
        let (entrypoint_entry, syntax_format) = match (conf_candidate, lua_candidate) {
            (Some(c), Some(_)) => {
                debug!("Both hyprland.conf and hyprland.lua detected. Prioritizing legacy conf by default.");
                (c, SyntaxFormat::LegacyConf)
            }
            (Some(c), None) => (c, SyntaxFormat::LegacyConf),
            (None, Some(l)) => (l, SyntaxFormat::Lua),
            (None, None) => return None,
        };

        let entrypoint_path = entrypoint_entry.path.clone();
        let root_dir = entrypoint_entry
            .utf8_path()
            .parent()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let prefix = if root_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", root_dir)
        };

        // 3. Collect associated files sharing this root directory
        let mut matched = Vec::new();
        let mut total_size: u64 = 0;
        let mut hardware_notices = Vec::new();

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }

            let matches_root = if prefix.is_empty() {
                // If entrypoint was at root, match hyprland files
                entry.path.ends_with(".conf") || entry.path.ends_with(".lua")
            } else {
                entry.path.starts_with(&prefix)
            };

            if matches_root {
                matched.push(entry.clone());
                total_size += entry.size.unwrap_or(0);

                if let Some(fname) = entry.file_name() {
                    if is_hardware_bound_filename(fname) {
                        hardware_notices.push(create_hardware_conflict_notice(entry.utf8_path()));
                    }
                }
            }
        }

        Some(ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format,
            root_path: root_dir,
            entrypoint: entrypoint_path,
            style_file: None,
            file_count: matched.len(),
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices,
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        validate_hyprland_conf(file_path.as_str(), content)
    }
}

/// Lightweight syntax validation for `hyprland.conf`.
///
/// Verifies:
/// - Balanced opening and closing curly braces (`{` and `}`)
/// - Ignores `#` comments and quoted strings
pub fn validate_hyprland_conf(file_name: &str, content: &str) -> SyntaxValidationResult {
    let mut brace_depth: i32 = 0;
    let mut unclosed_line: Option<usize> = None;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        let mut in_quote: Option<char> = None;
        let mut prev = '\0';

        for ch in trimmed.chars() {
            if let Some(q) = in_quote {
                if ch == q && prev != '\\' {
                    in_quote = None;
                }
            } else if ch == '#' {
                // Inline comment starts here; ignore rest of line
                break;
            } else if ch == '"' || ch == '\'' {
                in_quote = Some(ch);
            } else if ch == '{' {
                if brace_depth == 0 {
                    unclosed_line = Some(idx + 1);
                }
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
            prev = ch;
        }
    }

    if brace_depth != 0 {
        return SyntaxValidationResult::error(
            file_name,
            format!(
                "Unclosed section block '{{' (unclosed depth: {})",
                brace_depth
            ),
            unclosed_line,
            None,
        );
    }

    SyntaxValidationResult::ok(file_name)
}

/// Extract procedural directives (`exec-once`, `exec`) from Hyprland configuration text.
pub fn extract_hyprland_directives(file_name: &str, content: &str) -> Vec<ProceduralDirective> {
    let mut directives = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some(captures) = EXEC_DIRECTIVE_RE.captures(trimmed) {
            let dir_type = captures.get(1).map_or("", |m| m.as_str()).to_string();
            let cmd = captures.get(2).map_or("", |m| m.as_str()).to_string();
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

/// Extract `source = <path>` directives from Hyprland configuration text.
pub fn extract_hyprland_sources(content: &str) -> Vec<String> {
    let mut sources = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(caps) = SOURCE_DIRECTIVE_RE.captures(trimmed) {
            if let Some(path_match) = caps.get(1) {
                sources.push(path_match.as_str().trim().to_string());
            }
        }
    }

    sources
}

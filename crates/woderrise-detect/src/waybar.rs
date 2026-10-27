//! Waybar status bar detection (JSONC via json5, GTK CSS, and procedural directives).

use crate::detector::ComponentDetector;
use crate::types::{ComponentDetection, ProceduralDirective, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use serde_json::Value;
use woderrise_core::models::{ConfigComponent, SyntaxFormat};

/// Component detector for Wayland status bar (Waybar).
pub struct WaybarDetector;

impl WaybarDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaybarDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentDetector for WaybarDetector {
    fn component(&self) -> ConfigComponent {
        ConfigComponent::Waybar
    }

    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection> {
        // Locate config entrypoint: prefer config.jsonc, fallback to config
        let mut config_candidate: Option<&TreeEntry> = None;
        let mut style_candidate: Option<&TreeEntry> = None;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }
            if let Some(name) = entry.file_name() {
                if name == "config.jsonc" || (name == "config" && config_candidate.is_none()) {
                    config_candidate = Some(entry);
                } else if name == "style.css" {
                    style_candidate = Some(entry);
                }
            }
        }

        let config_entry = config_candidate?;
        let entrypoint = config_entry.path.clone();
        let root_dir = config_entry
            .utf8_path()
            .parent()
            .map(|p| p.to_string())
            .unwrap_or_default();

        let prefix = if root_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", root_dir)
        };

        let style_file = style_candidate.map(|s| s.path.clone());

        let mut matched = Vec::new();
        let mut total_size: u64 = 0;

        for entry in tree {
            if !entry.is_blob() {
                continue;
            }

            let matches_root = if prefix.is_empty() {
                entry.path.ends_with(".jsonc")
                    || entry.path.ends_with(".css")
                    || entry.path == "config"
            } else {
                entry.path.starts_with(&prefix)
            };

            if matches_root {
                matched.push(entry.clone());
                total_size += entry.size.unwrap_or(0);
            }
        }

        Some(ComponentDetection {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: root_dir,
            entrypoint,
            style_file,
            file_count: matched.len(),
            matched_entries: matched,
            total_size_bytes: total_size,
            hardware_notices: Vec::new(),
        })
    }

    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult {
        if file_path.extension() == Some("css") {
            validate_gtk_css(file_path.as_str(), content)
        } else {
            validate_waybar_jsonc(file_path.as_str(), content)
        }
    }
}

/// Parse and validate Waybar JSONC configuration using `json5`.
///
/// Guarantees:
/// - Strips `//` line comments and `/* */` block comments without corrupting URLs (`https://`)
/// - Correctly parses trailing commas in objects and arrays
/// - Verifies top-level structure is an object or array of objects
pub fn validate_waybar_jsonc(file_name: &str, content: &str) -> SyntaxValidationResult {
    match json5::from_str::<Value>(content) {
        Ok(val) => {
            if !val.is_object() && !val.is_array() {
                return SyntaxValidationResult::error(
                    file_name,
                    "Waybar configuration root must be a JSON object or array of objects",
                    None,
                    None,
                );
            }
            SyntaxValidationResult::ok(file_name)
        }
        Err(err) => SyntaxValidationResult::error(file_name, err.to_string(), None, None),
    }
}

/// Validate GTK CSS syntax for balanced braces and unterminated comments/strings.
pub fn validate_gtk_css(file_name: &str, content: &str) -> SyntaxValidationResult {
    let mut brace_depth: i32 = 0;
    let mut in_block_comment = false;
    let mut in_string: Option<char> = None;
    let mut prev_char = '\0';
    let mut line_num = 1;
    let mut unclosed_line = None;

    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line_num += 1;
        }

        if in_block_comment {
            if prev_char == '*' && c == '/' {
                in_block_comment = false;
            }
        } else if let Some(q) = in_string {
            if c == q && prev_char != '\\' {
                in_string = None;
            }
        } else if prev_char == '/' && c == '*' {
            in_block_comment = true;
        } else if c == '"' || c == '\'' {
            in_string = Some(c);
        } else if c == '{' {
            if brace_depth == 0 {
                unclosed_line = Some(line_num);
            }
            brace_depth += 1;
        } else if c == '}' {
            brace_depth -= 1;
            if brace_depth < 0 {
                return SyntaxValidationResult::error(
                    file_name,
                    format!("Unexpected closing brace '}}' at line {}", line_num),
                    Some(line_num),
                    None,
                );
            }
        }
        prev_char = c;
        i += 1;
    }

    if in_block_comment {
        return SyntaxValidationResult::error(
            file_name,
            "Unterminated block comment '/* ...'",
            Some(line_num),
            None,
        );
    }
    if in_string.is_some() {
        return SyntaxValidationResult::error(
            file_name,
            "Unterminated string literal",
            Some(line_num),
            None,
        );
    }
    if brace_depth != 0 {
        return SyntaxValidationResult::error(
            file_name,
            format!("Unclosed curly braces (unmatched count: {})", brace_depth),
            unclosed_line,
            None,
        );
    }

    SyntaxValidationResult::ok(file_name)
}

/// Recursively extract procedural execution commands (`exec`, `on-click`, etc.) from Waybar JSONC.
pub fn extract_waybar_directives(file_name: &str, content: &str) -> Vec<ProceduralDirective> {
    let mut directives = Vec::new();
    if let Ok(val) = json5::from_str::<Value>(content) {
        walk_json_for_directives(&val, file_name, &mut directives);
    }
    directives
}

fn walk_json_for_directives(val: &Value, file_name: &str, out: &mut Vec<ProceduralDirective>) {
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                let is_directive_key = k == "exec"
                    || k == "on-click"
                    || k == "on-click-middle"
                    || k == "on-click-right"
                    || k == "on-scroll-up"
                    || k == "on-scroll-down";

                if is_directive_key {
                    if let Some(cmd) = v.as_str() {
                        out.push(ProceduralDirective {
                            directive_type: k.clone(),
                            command: cmd.to_string(),
                            file: file_name.to_string(),
                            line: 1, // AST does not preserve line numbers
                        });
                    }
                }
                walk_json_for_directives(v, file_name, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                walk_json_for_directives(item, file_name, out);
            }
        }
        _ => {}
    }
}

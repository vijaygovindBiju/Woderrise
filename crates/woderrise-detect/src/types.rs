//! Core types for tree entries, component detections, procedural directives, and syntax validation.

use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use woderrise_core::models::{
    ConfigComponent, DetectedComponent, HardwareConflictNotice, SyntaxFormat,
};

/// An entry in a Git tree returned by GitHub API or constructed from test fixtures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub mode: String,
    #[serde(rename = "type")]
    pub entry_type: String, // "blob" | "tree" | "commit"
    pub sha: String,
    #[serde(default)]
    pub size: Option<u64>,
}

impl TreeEntry {
    pub fn new(
        path: impl Into<String>,
        mode: impl Into<String>,
        entry_type: impl Into<String>,
        sha: impl Into<String>,
        size: Option<u64>,
    ) -> Self {
        Self {
            path: path.into(),
            mode: mode.into(),
            entry_type: entry_type.into(),
            sha: sha.into(),
            size,
        }
    }

    /// Whether this entry is a blob (file).
    pub fn is_blob(&self) -> bool {
        self.entry_type == "blob"
    }

    /// Whether this entry is a directory/tree.
    pub fn is_tree(&self) -> bool {
        self.entry_type == "tree"
    }

    /// Whether this entry is a Git symlink (mode 120000).
    pub fn is_symlink(&self) -> bool {
        self.mode == "120000" || self.mode.starts_with("120")
    }

    /// Whether this entry is marked executable (mode 100755).
    pub fn is_executable(&self) -> bool {
        self.mode == "100755" || self.mode.ends_with("755")
    }

    /// Access the path as a Utf8Path.
    pub fn utf8_path(&self) -> &Utf8Path {
        Utf8Path::new(&self.path)
    }

    /// File name of this entry if available.
    pub fn file_name(&self) -> Option<&str> {
        self.utf8_path().file_name()
    }

    /// Extension of this entry if available.
    pub fn extension(&self) -> Option<&str> {
        self.utf8_path().extension()
    }
}

/// Detailed result of component detection on a Git tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentDetection {
    pub component: ConfigComponent,
    pub syntax_format: SyntaxFormat,
    pub root_path: String,
    pub entrypoint: String,
    pub style_file: Option<String>,
    pub matched_entries: Vec<TreeEntry>,
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub hardware_notices: Vec<HardwareConflictNotice>,
}

impl From<&ComponentDetection> for DetectedComponent {
    fn from(d: &ComponentDetection) -> Self {
        Self {
            component: d.component,
            syntax_format: d.syntax_format,
            root_path: d.root_path.clone(),
            entrypoint: d.entrypoint.clone(),
            style_file: d.style_file.clone(),
            file_count: d.file_count,
            total_size_bytes: d.total_size_bytes,
        }
    }
}

/// A procedural execution directive extracted from a configuration file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProceduralDirective {
    pub directive_type: String, // "exec-once", "exec", "on-click", "on-timeout"
    pub command: String,
    pub file: String,
    pub line: usize,
}

/// Outcome of lightweight syntax validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyntaxValidationResult {
    pub valid: bool,
    pub file: String,
    pub error_message: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl SyntaxValidationResult {
    pub fn ok(file: impl Into<String>) -> Self {
        Self {
            valid: true,
            file: file.into(),
            error_message: None,
            line: None,
            column: None,
        }
    }

    pub fn error(
        file: impl Into<String>,
        msg: impl Into<String>,
        line: Option<usize>,
        col: Option<usize>,
    ) -> Self {
        Self {
            valid: false,
            file: file.into(),
            error_message: Some(msg.into()),
            line,
            column: col,
        }
    }
}

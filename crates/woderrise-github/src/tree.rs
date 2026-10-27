//! Recursive Git Trees API models and tree operations.

use serde::{Deserialize, Serialize};

/// Git file mode constants.
pub const MODE_REGULAR_FILE: &str = "100644";
pub const MODE_EXECUTABLE: &str = "100755";
pub const MODE_SYMLINK: &str = "120000";
pub const MODE_DIRECTORY: &str = "040000";

/// Maximum number of tree entries allowed to prevent memory exhaustion DoS.
pub const MAX_TREE_ENTRIES: usize = 50_000;

/// An individual entry within a Git Tree returned by the GitHub API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitTreeEntry {
    pub path: String,
    pub mode: String,
    #[serde(rename = "type")]
    pub entry_type: String, // "blob" | "tree" | "commit"
    pub sha: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}

impl GitTreeEntry {
    /// Whether this entry is a blob (file).
    pub fn is_blob(&self) -> bool {
        self.entry_type == "blob"
    }

    /// Whether this entry is a subtree (directory).
    pub fn is_tree(&self) -> bool {
        self.entry_type == "tree"
    }

    /// Whether this entry is a regular file or script.
    pub fn is_file(&self) -> bool {
        self.mode == MODE_REGULAR_FILE || self.mode == MODE_EXECUTABLE
    }

    /// Whether this entry is marked executable (100755).
    pub fn is_executable(&self) -> bool {
        self.mode == MODE_EXECUTABLE || self.mode.ends_with("755")
    }

    /// Whether this entry is a Git symlink (120000).
    pub fn is_symlink(&self) -> bool {
        self.mode == MODE_SYMLINK || self.mode.starts_with("120")
    }

    /// Whether this entry is a directory (040000 or tree).
    pub fn is_directory(&self) -> bool {
        self.mode == MODE_DIRECTORY || self.entry_type == "tree"
    }
}

/// A Git Tree object representing the filesystem hierarchy of a repository commit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitTree {
    pub sha: String,
    #[serde(default)]
    pub url: Option<String>,
    pub tree: Vec<GitTreeEntry>,
    #[serde(default)]
    pub truncated: bool,
}

impl GitTree {
    pub fn new(sha: impl Into<String>, tree: Vec<GitTreeEntry>, truncated: bool) -> Self {
        Self {
            sha: sha.into(),
            url: None,
            tree,
            truncated,
        }
    }

    /// Find an entry by exact path.
    pub fn find_entry(&self, path: &str) -> Option<&GitTreeEntry> {
        self.tree.iter().find(|e| e.path == path)
    }

    /// Filter entries by prefix.
    pub fn entries_under_prefix(&self, prefix: &str) -> Vec<&GitTreeEntry> {
        let normalized = if prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{}/", prefix)
        };
        self.tree
            .iter()
            .filter(|e| e.path.starts_with(&normalized))
            .collect()
    }

    /// Collect all unexpanded subtrees (where `type == "tree"`).
    pub fn unexpanded_subtrees(&self) -> Vec<&GitTreeEntry> {
        self.tree.iter().filter(|e| e.is_tree()).collect()
    }
}

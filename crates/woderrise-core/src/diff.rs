//! Source-versus-target diff and install planning engine for Woderrise.

use crate::{expand_target, sha256_file, InstallationEngine, Manifest, Result, WoderriseError};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileDiffAction {
    Create,
    Modify,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub left_line_no: Option<usize>,
    pub right_line_no: Option<usize>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub planned_backup_path: Option<PathBuf>,
    pub action: FileDiffAction,
    pub lines: Vec<DiffLine>,
}

impl FileDiff {
    pub fn has_changes(&self) -> bool {
        self.action != FileDiffAction::Unchanged
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub package_id: String,
    pub version: String,
    pub file_diffs: Vec<FileDiff>,
    pub will_backup_count: usize,
}

impl InstallPlan {
    pub fn has_changes(&self) -> bool {
        self.file_diffs.iter().any(|f| f.has_changes())
    }
}

/// Computes a unified line-by-line diff between target (old/left) and source (new/right) text.
pub fn compute_line_diff(target_text: &str, source_text: &str) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = target_text.lines().collect();
    let new_lines: Vec<&str> = source_text.lines().collect();

    if old_lines.is_empty() && new_lines.is_empty() {
        return Vec::new();
    }

    if old_lines.is_empty() {
        return new_lines
            .iter()
            .enumerate()
            .map(|(idx, line)| DiffLine {
                kind: DiffLineKind::Addition,
                left_line_no: None,
                right_line_no: Some(idx + 1),
                text: line.to_string(),
            })
            .collect();
    }

    if new_lines.is_empty() {
        return old_lines
            .iter()
            .enumerate()
            .map(|(idx, line)| DiffLine {
                kind: DiffLineKind::Deletion,
                left_line_no: Some(idx + 1),
                right_line_no: None,
                text: line.to_string(),
            })
            .collect();
    }

    // 1. Common Prefix Trimming
    let mut prefix_len = 0;
    while prefix_len < old_lines.len()
        && prefix_len < new_lines.len()
        && old_lines[prefix_len] == new_lines[prefix_len]
    {
        prefix_len += 1;
    }

    // 2. Common Suffix Trimming
    let mut suffix_len = 0;
    while suffix_len < (old_lines.len() - prefix_len)
        && suffix_len < (new_lines.len() - prefix_len)
        && old_lines[old_lines.len() - 1 - suffix_len] == new_lines[new_lines.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let mut result = Vec::new();

    // Emit prefix context lines
    for (i, line) in old_lines.iter().enumerate().take(prefix_len) {
        result.push(DiffLine {
            kind: DiffLineKind::Context,
            left_line_no: Some(i + 1),
            right_line_no: Some(i + 1),
            text: (*line).to_string(),
        });
    }

    // Process middle slice
    let old_mid = &old_lines[prefix_len..old_lines.len() - suffix_len];
    let new_mid = &new_lines[prefix_len..new_lines.len() - suffix_len];

    let mid_edits = myers_diff(old_mid, new_mid);
    let mut left_cursor = prefix_len + 1;
    let mut right_cursor = prefix_len + 1;

    for edit in mid_edits {
        match edit {
            Edit::Keep(line) => {
                result.push(DiffLine {
                    kind: DiffLineKind::Context,
                    left_line_no: Some(left_cursor),
                    right_line_no: Some(right_cursor),
                    text: line.to_string(),
                });
                left_cursor += 1;
                right_cursor += 1;
            }
            Edit::Delete(line) => {
                result.push(DiffLine {
                    kind: DiffLineKind::Deletion,
                    left_line_no: Some(left_cursor),
                    right_line_no: None,
                    text: line.to_string(),
                });
                left_cursor += 1;
            }
            Edit::Insert(line) => {
                result.push(DiffLine {
                    kind: DiffLineKind::Addition,
                    left_line_no: None,
                    right_line_no: Some(right_cursor),
                    text: line.to_string(),
                });
                right_cursor += 1;
            }
        }
    }

    // Emit suffix context lines
    let old_suffix_start = old_lines.len() - suffix_len;
    let new_suffix_start = new_lines.len() - suffix_len;
    for i in 0..suffix_len {
        result.push(DiffLine {
            kind: DiffLineKind::Context,
            left_line_no: Some(old_suffix_start + i + 1),
            right_line_no: Some(new_suffix_start + i + 1),
            text: old_lines[old_suffix_start + i].to_string(),
        });
    }

    result
}

/// Standalone unified diff alias.
pub fn unified_diff(old_text: &str, new_text: &str) -> Vec<DiffLine> {
    compute_line_diff(old_text, new_text)
}

#[derive(Debug, PartialEq, Eq)]
enum Edit<'a> {
    Keep(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

/// Myers diff algorithm on middle slices with guaranteed convergence and boundary safety.
fn myers_diff<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<Edit<'a>> {
    let n = a.len();
    let m = b.len();

    if n == 0 {
        return b.iter().map(|&s| Edit::Insert(s)).collect();
    }
    if m == 0 {
        return a.iter().map(|&s| Edit::Delete(s)).collect();
    }

    let max = n + m;
    let v_size = 2 * max + 1;
    let mut v = vec![0usize; v_size];
    let mut trace: Vec<Vec<usize>> = Vec::new();

    let mut found_d = None;
    for d in 0..=max {
        trace.push(v.clone());
        for k in (-(d as isize)..=(d as isize)).step_by(2) {
            let k_idx = (k + max as isize) as usize;
            let mut x = if d == 0 {
                0
            } else if k == -(d as isize) || (k != (d as isize) && v[k_idx - 1] < v[k_idx + 1]) {
                v[k_idx + 1]
            } else {
                v[k_idx - 1] + 1
            };
            let mut y = (x as isize - k) as usize;

            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            v[k_idx] = x;
            if x >= n && y >= m {
                found_d = Some(d);
                break;
            }
        }
        if found_d.is_some() {
            break;
        }
    }

    let found_d = match found_d {
        Some(d) => d,
        None => {
            let mut fallback = Vec::new();
            for item in a {
                fallback.push(Edit::Delete(item));
            }
            for item in b {
                fallback.push(Edit::Insert(item));
            }
            return fallback;
        }
    };

    let mut edits = Vec::new();
    let mut x = n;
    let mut y = m;

    for d in (1..=found_d).rev() {
        let v_prev = &trace[d];
        let k = x as isize - y as isize;
        let k_idx = (k + max as isize) as usize;

        let prev_k = if k == -(d as isize)
            || (k != (d as isize) && v_prev[k_idx - 1] < v_prev[k_idx + 1])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_k_idx = (prev_k + max as isize) as usize;
        let prev_x = v_prev[prev_k_idx];
        let prev_y = (prev_x as isize - prev_k) as usize;

        while x > prev_x && y > prev_y {
            edits.push(Edit::Keep(a[x - 1]));
            x -= 1;
            y -= 1;
        }

        if x == prev_x {
            edits.push(Edit::Insert(b[y - 1]));
            y -= 1;
        } else {
            edits.push(Edit::Delete(a[x - 1]));
            x -= 1;
        }
    }

    while x > 0 && y > 0 {
        edits.push(Edit::Keep(a[x - 1]));
        x -= 1;
        y -= 1;
    }

    edits.reverse();
    edits
}

impl InstallationEngine {
    /// Computes the complete install plan and source-vs-target diff for a package.
    pub fn compute_plan(&self, package_dir: impl AsRef<Path>) -> Result<InstallPlan> {
        let dir = package_dir.as_ref();
        let manifest_path = if dir.is_file() {
            dir.to_path_buf()
        } else {
            dir.join("woderrise.yaml")
        };
        let base_dir = manifest_path
            .parent()
            .ok_or_else(|| WoderriseError::Other("manifest has no parent directory".into()))?;

        let manifest = Manifest::from_file(&manifest_path)?;
        let mut file_diffs = Vec::new();

        for file in &manifest.install.files {
            let source_path = base_dir.join(&file.source);
            let target_path = expand_target(&file.target, &self.home)?;

            let target_exists = target_path.exists();
            let planned_backup_path = if target_exists && file.backup_before_write {
                let target_hash = sha256_file(&target_path)?;
                Some(self.backups.root.join(&target_hash))
            } else {
                None
            };

            let source_bytes = fs::read(&source_path)?;
            let is_source_utf8 = std::str::from_utf8(&source_bytes).is_ok();

            if !target_exists {
                let lines = if is_source_utf8 {
                    let source_text = String::from_utf8_lossy(&source_bytes);
                    compute_line_diff("", &source_text)
                } else {
                    vec![DiffLine {
                        kind: DiffLineKind::Addition,
                        left_line_no: None,
                        right_line_no: None,
                        text: "[binary artifact added]".into(),
                    }]
                };
                file_diffs.push(FileDiff {
                    source_path,
                    target_path,
                    planned_backup_path,
                    action: FileDiffAction::Create,
                    lines,
                });
            } else {
                let target_bytes = fs::read(&target_path)?;
                if source_bytes == target_bytes {
                    let lines = if is_source_utf8 {
                        let text = String::from_utf8_lossy(&source_bytes);
                        compute_line_diff(&text, &text)
                    } else {
                        vec![DiffLine {
                            kind: DiffLineKind::Context,
                            left_line_no: None,
                            right_line_no: None,
                            text: "[binary artifacts identical]".into(),
                        }]
                    };
                    file_diffs.push(FileDiff {
                        source_path,
                        target_path,
                        planned_backup_path,
                        action: FileDiffAction::Unchanged,
                        lines,
                    });
                } else {
                    let is_target_utf8 = std::str::from_utf8(&target_bytes).is_ok();
                    let lines = if is_source_utf8 && is_target_utf8 {
                        let target_text = String::from_utf8_lossy(&target_bytes);
                        let source_text = String::from_utf8_lossy(&source_bytes);
                        compute_line_diff(&target_text, &source_text)
                    } else {
                        vec![DiffLine {
                            kind: DiffLineKind::Context,
                            left_line_no: None,
                            right_line_no: None,
                            text: "[binary artifacts modified]".into(),
                        }]
                    };
                    file_diffs.push(FileDiff {
                        source_path,
                        target_path,
                        planned_backup_path,
                        action: FileDiffAction::Modify,
                        lines,
                    });
                }
            }
        }

        let will_backup_count = file_diffs
            .iter()
            .filter(|f| f.planned_backup_path.is_some())
            .count();

        Ok(InstallPlan {
            package_id: manifest.package.id,
            version: manifest.package.version,
            file_diffs,
            will_backup_count,
        })
    }
}

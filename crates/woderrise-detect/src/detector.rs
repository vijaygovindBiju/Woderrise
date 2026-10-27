//! Unified DetectionEngine and ComponentDetector trait.

use crate::monitors::{
    create_hardware_conflict_notice, is_hardware_bound_filename, is_hardware_bound_path,
};
use crate::types::{ComponentDetection, SyntaxValidationResult, TreeEntry};
use camino::Utf8Path;
use woderrise_core::error::{PathError, Result, WoderriseError};
use woderrise_core::models::{
    ConfigComponent, ConflictStatus, DetectedComponent, HardwareConflictNotice, TargetMapping,
};
use woderrise_core::paths::{sanitize_relative_path, strip_dotfile_repo_prefix, XdgPaths};

/// Trait implemented by each independent desktop component detector.
pub trait ComponentDetector: Send + Sync {
    /// Returns the component variant handled by this detector.
    fn component(&self) -> ConfigComponent;

    /// Analyzes the Git tree entries and detects presence of the component.
    fn detect(&self, tree: &[TreeEntry]) -> Option<ComponentDetection>;

    /// Validates the syntax of a file belonging to this component.
    fn validate_syntax(&self, file_path: &Utf8Path, content: &str) -> SyntaxValidationResult;
}

/// Unified Detection Engine coordinating independent component detectors.
pub struct DetectionEngine {
    detectors: Vec<Box<dyn ComponentDetector>>,
}

impl DetectionEngine {
    /// Create a new DetectionEngine with all 7 component detectors registered.
    pub fn new() -> Self {
        Self {
            detectors: vec![
                Box::new(crate::hyprland::HyprlandDetector::new()),
                Box::new(crate::waybar::WaybarDetector::new()),
                Box::new(crate::hyprlock::HyprlockDetector::new()),
                Box::new(crate::hypridle::HypridleDetector::new()),
                Box::new(crate::wallpaper::WallpaperDetector::new()),
                Box::new(crate::gtk::HyprcursorDetector::new()),
                Box::new(crate::gtk::GtkDetector::new()),
                Box::new(crate::monitors::MonitorDetector::new()),
            ],
        }
    }

    /// Fast scan returning summary `DetectedComponent` entries for catalog metadata.
    pub fn scan_catalog_components(&self, tree: &[TreeEntry]) -> Vec<DetectedComponent> {
        let mut results = Vec::new();
        for detector in &self.detectors {
            if let Some(detection) = detector.detect(tree) {
                results.push(DetectedComponent::from(&detection));
            }
        }
        results
    }

    /// Detailed detection returning all matched file entries and hardware notices.
    pub fn detect_all(&self, tree: &[TreeEntry]) -> Vec<ComponentDetection> {
        self.detectors
            .iter()
            .filter_map(|d| d.detect(tree))
            .collect()
    }

    /// Detect a specific target component from the tree.
    pub fn detect_component(
        &self,
        tree: &[TreeEntry],
        component: ConfigComponent,
    ) -> Option<ComponentDetection> {
        for detector in &self.detectors {
            let det_comp = detector.component();
            let matches = match (component, det_comp) {
                (ConfigComponent::Hyprland, ConfigComponent::Hyprland) => true,
                (ConfigComponent::HyprlandLegacy, ConfigComponent::Hyprland) => true,
                (ConfigComponent::HyprlandLua, ConfigComponent::Hyprland) => true,
                (c1, c2) if c1 == c2 => true,
                _ => false,
            };

            if matches {
                if let Some(detection) = detector.detect(tree) {
                    return Some(detection);
                }
            }
        }
        None
    }

    /// Generate target mappings and hardware conflict notices for a selected component.
    ///
    /// Enforces:
    /// 1. Rejection of Git tree symlinks (`mode 120000`) for CVE-2021-21300 security.
    /// 2. Strict component root containment via `XdgPaths::resolve_destination`.
    /// 3. Hardware monitor preservation (`monitors.conf`, `workspaces.conf`, etc.).
    pub fn generate_target_mappings(
        &self,
        tree: &[TreeEntry],
        component: ConfigComponent,
        xdg: &XdgPaths,
    ) -> Result<(Vec<TargetMapping>, Vec<HardwareConflictNotice>)> {
        // CVE-2021-21300: Check for prohibited Git tree symlinks
        for entry in tree {
            if entry.is_symlink() {
                return Err(WoderriseError::Path(PathError::ForbiddenSymlink {
                    path: entry.utf8_path().to_path_buf(),
                    target: "<git-symlink-traversal>".into(),
                }));
            }
        }

        let detection = self.detect_component(tree, component).ok_or_else(|| {
            WoderriseError::Validation(format!(
                "Component '{}' not detected in repository tree",
                component.as_str()
            ))
        })?;

        let mut mappings = Vec::new();
        let mut hardware_notices = Vec::new();

        // Preserve non-file notices (e.g. inline monitor notices)
        for notice in &detection.hardware_notices {
            if notice.description.contains("Inline monitor") {
                hardware_notices.push(notice.clone());
            }
        }

        for entry in &detection.matched_entries {
            if !entry.is_blob() {
                continue;
            }

            // Strip repo dotfile prefixes
            let subpath = component.default_config_subpath();
            let stripped = strip_dotfile_repo_prefix(entry.utf8_path(), subpath);
            let clean_rel = sanitize_relative_path(stripped)?;

            // Resolve and enforce destination containment
            let target_dest = xdg.resolve_destination(component, &clean_rel)?;

            // Check hardware display preservation
            let is_hardware = is_hardware_bound_path(entry.path.as_str())
                || is_hardware_bound_filename(clean_rel.file_name().unwrap_or(""));

            let (is_protected, conflict_status) = if is_hardware {
                let notice = create_hardware_conflict_notice(&target_dest);
                if !hardware_notices
                    .iter()
                    .any(|n| n.file_path == notice.file_path)
                {
                    hardware_notices.push(notice);
                }
                (true, ConflictStatus::HardwareProtected)
            } else {
                let status = if target_dest.is_symlink() {
                    ConflictStatus::ExistingSymlink
                } else if target_dest.exists() {
                    ConflictStatus::OverwritingExisting
                } else {
                    ConflictStatus::NewFile
                };
                (false, status)
            };

            mappings.push(TargetMapping {
                component,
                relative_path: clean_rel,
                blob_sha: entry.sha.clone(),
                target_destination: target_dest,
                git_file_mode: entry.mode.clone(),
                expected_sha256: String::new(),
                is_hardware_protected: is_protected,
                conflict_status,
            });
        }

        Ok((mappings, hardware_notices))
    }
}

impl Default for DetectionEngine {
    fn default() -> Self {
        Self::new()
    }
}

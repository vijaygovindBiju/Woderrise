//! View model definitions and conversions between woderrise-core and Slint UI types.

use crate::{
    AppWindow, DiffLineData, FileDiffData, PackageCardData, PaletteSwatchData,
};
use slint::{Color, ModelRc, SharedString, VecModel};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
};
use woderrise_core::{
    catalog::CatalogEntry,
    diff::{DiffLine, DiffLineKind, FileDiff, FileDiffAction, InstallPlan},
    Author, Compatibility, Manifest, PaletteColor,
};

// ============================================================================
// 1. Color Palette Conversion
// ============================================================================

/// RGB tuple representation of a palette swatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteSwatchViewModel {
    pub name: String,
    pub hex: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Parses a CSS hex color string (`#RRGGBB`, `#RGB`, or `#RRGGBBAA`) into a `slint::Color`.
/// Returns `None` if the input is malformed or not standard hex.
pub fn parse_hex_color(hex: &str) -> Option<Color> {
    let clean = hex.trim().trim_start_matches('#');
    if !clean.is_ascii() {
        return None;
    }
    match clean.len() {
        6 => {
            let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
            let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
            let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
            Some(Color::from_argb_u8(255, r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
            let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
            let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
            let a = u8::from_str_radix(&clean[6..8], 16).ok()?;
            Some(Color::from_argb_u8(a, r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&clean[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&clean[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&clean[2..3], 16).ok()? * 17;
            Some(Color::from_argb_u8(255, r, g, b))
        }
        _ => None,
    }
}

/// Parses a hex color string into raw RGB u8 values.
pub fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if !clean.is_ascii() {
        return None;
    }
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
        let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
        let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
        Some((r, g, b))
    } else if clean.len() == 3 {
        let r = u8::from_str_radix(&clean[0..1], 16).ok()?;
        let g = u8::from_str_radix(&clean[1..2], 16).ok()?;
        let b = u8::from_str_radix(&clean[2..3], 16).ok()?;
        Some((r * 17, g * 17, b * 17))
    } else {
        None
    }
}

/// Converts a `woderrise_core::PaletteColor` into a Slint `PaletteSwatchData`.
pub fn palette_color_to_slint(pc: &PaletteColor) -> PaletteSwatchData {
    let color = parse_hex_color(&pc.hex)
        .unwrap_or_else(|| Color::from_argb_u8(255, 128, 128, 128));
    PaletteSwatchData {
        name: SharedString::from(pc.name.as_str()),
        hex: SharedString::from(pc.hex.as_str()),
        color,
    }
}

/// Converts a slice of `PaletteColor` into a Slint `ModelRc<PaletteSwatchData>`.
pub fn palette_colors_to_model(palette: &[PaletteColor]) -> ModelRc<PaletteSwatchData> {
    let items: Vec<PaletteSwatchData> = palette.iter().map(palette_color_to_slint).collect();
    Rc::new(VecModel::from(items)).into()
}

/// Extracts palette swatches with raw RGB values from a Manifest.
pub fn extract_palette(manifest: &Manifest) -> Vec<PaletteSwatchViewModel> {
    manifest
        .previews
        .as_ref()
        .map(|p| {
            p.palette
                .iter()
                .filter_map(|c| {
                    let (r, g, b) = parse_hex_rgb(&c.hex)?;
                    Some(PaletteSwatchViewModel {
                        name: c.name.clone(),
                        hex: c.hex.clone(),
                        r,
                        g,
                        b,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// 2. Metadata Provenance & Formatting
// ============================================================================

/// Formats author provenance list into a readable string.
pub fn format_authors(authors: &[Author]) -> String {
    if authors.is_empty() {
        return "Not declared".to_string();
    }
    let parts: Vec<String> = authors
        .iter()
        .map(|a| match (&a.email, &a.url) {
            (Some(email), Some(url)) => format!("{} <{}> ({})", a.name, email, url),
            (Some(email), None) => format!("{} <{}>", a.name, email),
            (None, Some(url)) => format!("{} ({})", a.name, url),
            (None, None) => a.name.clone(),
        })
        .collect();
    parts.join(", ")
}

/// Formats repository URL or returns fallback string.
pub fn format_repository(repo: &Option<String>) -> String {
    repo.clone().unwrap_or_else(|| "Not declared".to_string())
}

/// Formats display server compatibility badge.
pub fn format_display_server(compat: &Option<Compatibility>) -> String {
    match compat.as_ref().and_then(|c| c.display_server.as_deref()) {
        Some("wayland_only") => "Wayland Only".to_string(),
        Some("wayland_and_x11") => "Wayland & X11".to_string(),
        Some(other) => other.to_string(),
        None => "Universal".to_string(),
    }
}

/// Extracts supported architectures.
pub fn extract_architectures(compat: &Option<Compatibility>) -> Vec<String> {
    compat
        .as_ref()
        .map(|c| c.architectures.clone())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["all".to_string()])
}

// ============================================================================
// 3. Package View Models
// ============================================================================

/// Pure Rust decoupled package view model for testing and business logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageViewModel {
    pub id: String,
    pub name: String,
    pub version: String,
    pub package_type: String,
    pub category: String,
    pub description: String,
    pub license: String,
    pub repository: String,
    pub authors_formatted: String,
    pub display_server: String,
    pub architectures: Vec<String>,
    pub palette: Vec<PaletteSwatchViewModel>,
    pub installed: bool,
    pub manifest_path: PathBuf,
}

/// Converts a Manifest into a pure Rust `PackageViewModel`.
pub fn manifest_to_view_model(
    manifest: &Manifest,
    manifest_path: impl AsRef<Path>,
    installed: bool,
) -> PackageViewModel {
    let pkg = &manifest.package;
    PackageViewModel {
        id: pkg.id.clone(),
        name: pkg.name.clone(),
        version: pkg.version.clone(),
        package_type: pkg.package_type.clone(),
        category: pkg.category.clone(),
        description: pkg.description.clone(),
        license: pkg.license.clone(),
        repository: format_repository(&pkg.repository),
        authors_formatted: format_authors(&pkg.authors),
        display_server: format_display_server(&manifest.compatibility),
        architectures: extract_architectures(&manifest.compatibility),
        palette: extract_palette(manifest),
        installed,
        manifest_path: manifest_path.as_ref().to_path_buf(),
    }
}

/// Filters package view models by query text and category tag.
pub fn filter_packages<'a>(
    packages: &'a [PackageViewModel],
    query: &str,
    category: &str,
) -> Vec<&'a PackageViewModel> {
    let clean_query = query.trim().to_lowercase();
    let clean_category = category.trim().to_lowercase();

    packages
        .iter()
        .filter(|p| {
            if clean_category != "all"
                && !clean_category.is_empty()
                && p.category.to_lowercase() != clean_category
            {
                return false;
            }
            if clean_query.is_empty() {
                return true;
            }
            p.id.to_lowercase().contains(&clean_query)
                || p.name.to_lowercase().contains(&clean_query)
                || p.description.to_lowercase().contains(&clean_query)
        })
        .collect()
}

/// Converts a `CatalogEntry` and an installed state flag into a Slint `PackageCardData`.
pub fn catalog_entry_to_card(entry: &CatalogEntry, installed: bool) -> PackageCardData {
    PackageCardData {
        id: SharedString::from(entry.id.as_str()),
        name: SharedString::from(entry.name.as_str()),
        version: SharedString::from(entry.version.as_str()),
        category: SharedString::from(entry.category.as_str()),
        description: SharedString::from(entry.description.as_str()),
        license: SharedString::from(entry.license.as_str()),
        installed,
    }
}

/// Converts a collection of `CatalogEntry` items into a Slint `ModelRc<PackageCardData>`.
pub fn catalog_entries_to_cards_model(
    entries: &[CatalogEntry],
    installed_ids: &HashSet<String>,
) -> ModelRc<PackageCardData> {
    let cards: Vec<PackageCardData> = entries
        .iter()
        .map(|e| catalog_entry_to_card(e, installed_ids.contains(&e.id)))
        .collect();
    Rc::new(VecModel::from(cards)).into()
}

/// Detailed package view model bound into Slint `AppWindow`.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageDetailViewModel {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: String,
    pub license: String,
    pub maintainer: String,
    pub repo_url: String,
    pub description: String,
    pub display_server: String,
    pub architectures: String,
    pub target_path: String,
    pub installed: bool,
    pub palette: Vec<PaletteSwatchData>,
}

impl PackageDetailViewModel {
    /// Extracts genuine metadata from a verified `woderrise_core::Manifest`.
    pub fn from_manifest(manifest: &Manifest, installed: bool) -> Self {
        let pkg = &manifest.package;
        let maintainer = format_authors(&pkg.authors);
        let repo_url = format_repository(&pkg.repository);
        let display_server = format_display_server(&manifest.compatibility);
        let architectures = extract_architectures(&manifest.compatibility).join(", ");

        let target_path = manifest
            .install
            .files
            .first()
            .map(|f| f.target.clone())
            .unwrap_or_else(|| "None".to_string());

        let palette = manifest
            .previews
            .as_ref()
            .map(|p| p.palette.iter().map(palette_color_to_slint).collect())
            .unwrap_or_default();

        Self {
            id: pkg.id.clone(),
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            category: pkg.category.clone(),
            license: pkg.license.clone(),
            maintainer,
            repo_url,
            description: pkg.description.clone(),
            display_server,
            architectures,
            target_path,
            installed,
            palette,
        }
    }

    /// Binds this view model's properties into an active Slint `AppWindow`.
    pub fn apply_to_window(&self, app: &AppWindow) {
        app.set_sel_id(SharedString::from(self.id.as_str()));
        app.set_sel_name(SharedString::from(self.name.as_str()));
        app.set_sel_version(SharedString::from(self.version.as_str()));
        app.set_sel_category(SharedString::from(self.category.as_str()));
        app.set_sel_license(SharedString::from(self.license.as_str()));
        app.set_sel_maintainer(SharedString::from(self.maintainer.as_str()));
        app.set_sel_repo_url(SharedString::from(self.repo_url.as_str()));
        app.set_sel_description(SharedString::from(self.description.as_str()));
        app.set_sel_display_server(SharedString::from(self.display_server.as_str()));
        app.set_sel_architectures(SharedString::from(self.architectures.as_str()));
        app.set_sel_target_path(SharedString::from(self.target_path.as_str()));
        app.set_sel_installed(self.installed);

        let swatches_model: ModelRc<PaletteSwatchData> =
            Rc::new(VecModel::from(self.palette.clone())).into();
        app.set_sel_palette(swatches_model);
    }
}

// ============================================================================
// 4. Diff & Impact Inspection View Models
// ============================================================================

/// Line in unified diff view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLineViewModel {
    pub kind: String, // "context", "addition", "deletion"
    pub text: String,
    pub prefix: String, // " ", "+", "-"
    pub left_no: String,
    pub right_no: String,
}

/// File diff view model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiffViewModel {
    pub source_path: String,
    pub target_path: String,
    pub planned_backup_path: String,
    pub action: String, // "CREATE", "MODIFY", "UNCHANGED"
    pub has_changes: bool,
    pub additions_count: usize,
    pub deletions_count: usize,
    pub lines: Vec<DiffLineViewModel>,
}

/// Diff plan view model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlanViewModel {
    pub package_id: String,
    pub version: String,
    pub file_diffs: Vec<FileDiffViewModel>,
    pub will_backup_count: usize,
    pub total_additions: usize,
    pub total_deletions: usize,
    pub summary_text: String,
}

pub fn map_diff_line(line: &DiffLine) -> DiffLineViewModel {
    let (kind, prefix) = match line.kind {
        DiffLineKind::Context => ("context", " "),
        DiffLineKind::Addition => ("addition", "+"),
        DiffLineKind::Deletion => ("deletion", "-"),
    };
    DiffLineViewModel {
        kind: kind.to_string(),
        text: line.text.clone(),
        prefix: prefix.to_string(),
        left_no: line.left_line_no.map(|n| n.to_string()).unwrap_or_default(),
        right_no: line.right_line_no.map(|n| n.to_string()).unwrap_or_default(),
    }
}

pub fn map_file_diff(diff: &FileDiff) -> FileDiffViewModel {
    let action_str = match diff.action {
        FileDiffAction::Create => "CREATE",
        FileDiffAction::Modify => "MODIFY",
        FileDiffAction::Unchanged => "UNCHANGED",
    };
    let planned_backup_str = diff
        .planned_backup_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "None (new file)".to_string());

    let mut additions = 0;
    let mut deletions = 0;
    let lines: Vec<DiffLineViewModel> = diff
        .lines
        .iter()
        .map(|l| {
            if l.kind == DiffLineKind::Addition {
                additions += 1;
            } else if l.kind == DiffLineKind::Deletion {
                deletions += 1;
            }
            map_diff_line(l)
        })
        .collect();

    FileDiffViewModel {
        source_path: diff.source_path.to_string_lossy().to_string(),
        target_path: diff.target_path.to_string_lossy().to_string(),
        planned_backup_path: planned_backup_str,
        action: action_str.to_string(),
        has_changes: diff.has_changes(),
        additions_count: additions,
        deletions_count: deletions,
        lines,
    }
}

pub fn map_install_plan(plan: &InstallPlan) -> InstallPlanViewModel {
    let mut total_adds = 0;
    let mut total_dels = 0;
    let file_diffs: Vec<FileDiffViewModel> = plan
        .file_diffs
        .iter()
        .map(|d| {
            let fd = map_file_diff(d);
            total_adds += fd.additions_count;
            total_dels += fd.deletions_count;
            fd
        })
        .collect();

    let files_affected = file_diffs.iter().filter(|d| d.has_changes).count();
    let summary_text = format!(
        "{} affected • {} planned • +{} / -{}",
        if files_affected == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", files_affected)
        },
        if plan.will_backup_count == 1 {
            "1 pre-install backup".to_string()
        } else {
            format!("{} pre-install backups", plan.will_backup_count)
        },
        total_adds,
        total_dels
    );

    InstallPlanViewModel {
        package_id: plan.package_id.clone(),
        version: plan.version.clone(),
        file_diffs,
        will_backup_count: plan.will_backup_count,
        total_additions: total_adds,
        total_deletions: total_dels,
        summary_text,
    }
}

/// Converts a `woderrise_core::diff::DiffLine` into a Slint `DiffLineData`.
pub fn diff_line_to_slint(dl: &DiffLine) -> DiffLineData {
    let kind = match dl.kind {
        DiffLineKind::Context => "context",
        DiffLineKind::Addition => "addition",
        DiffLineKind::Deletion => "deletion",
    };
    DiffLineData {
        kind: SharedString::from(kind),
        left_num: SharedString::from(
            dl.left_line_no.map(|n| n.to_string()).unwrap_or_default().as_str(),
        ),
        right_num: SharedString::from(
            dl.right_line_no.map(|n| n.to_string()).unwrap_or_default().as_str(),
        ),
        text: SharedString::from(dl.text.as_str()),
    }
}

/// Converts a `woderrise_core::diff::FileDiff` into a Slint `FileDiffData`.
pub fn file_diff_to_slint(fd: &FileDiff) -> FileDiffData {
    let action = match fd.action {
        FileDiffAction::Create => "Create",
        FileDiffAction::Modify => "Modify",
        FileDiffAction::Unchanged => "Unchanged",
    };

    let backup_path = match &fd.planned_backup_path {
        Some(path) => path.display().to_string(),
        None => "None (clean install)".to_string(),
    };

    let mut additions = 0;
    let mut deletions = 0;
    let lines: Vec<DiffLineData> = fd
        .lines
        .iter()
        .map(|l| {
            if l.kind == DiffLineKind::Addition {
                additions += 1;
            } else if l.kind == DiffLineKind::Deletion {
                deletions += 1;
            }
            diff_line_to_slint(l)
        })
        .collect();
    let lines_model: ModelRc<DiffLineData> = Rc::new(VecModel::from(lines)).into();

    FileDiffData {
        source_path: SharedString::from(fd.source_path.display().to_string().as_str()),
        target_path: SharedString::from(fd.target_path.display().to_string().as_str()),
        backup_path: SharedString::from(backup_path.as_str()),
        action: SharedString::from(action),
        additions,
        deletions,
        lines: lines_model,
    }
}

/// Converts an `InstallPlan` into a Slint `ModelRc<FileDiffData>` for the Diff modal.
pub fn install_plan_to_slint_files(plan: &InstallPlan) -> ModelRc<FileDiffData> {
    let files: Vec<FileDiffData> = plan.file_diffs.iter().map(file_diff_to_slint).collect();
    Rc::new(VecModel::from(files)).into()
}

// ============================================================================
// 5. Settings State & Serialization
// ============================================================================

/// Persistent desktop preferences stored in `settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SettingsState {
    pub auto_backup: bool,
    pub strict_hash: bool,
    pub offline_first: bool,
    pub compositor: String,
    pub shell: String,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            auto_backup: true,
            strict_hash: true,
            offline_first: true,
            compositor: "Hyprland (Wayland)".to_string(),
            shell: "Waybar (~/.config/waybar)".to_string(),
        }
    }
}

impl SettingsState {
    /// Loads settings from `data_dir/settings.json`, falling back to default if absent or corrupted.
    pub fn load_from_dir(data_dir: &Path) -> Self {
        let path = data_dir.join("settings.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<Self>(&data) {
                return settings;
            }
        }
        Self::default()
    }

    /// Saves settings to `data_dir/settings.json`.
    pub fn save_to_dir(&self, data_dir: &Path) -> Result<(), std::io::Error> {
        let path = data_dir.join("settings.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Applies settings to an active Slint `AppWindow`.
    pub fn apply_to_window(&self, app: &AppWindow, data_dir: &Path, total_txns: i32) {
        app.set_setting_auto_backup(self.auto_backup);
        app.set_setting_strict_hash(self.strict_hash);
        app.set_setting_offline_first(self.offline_first);
        app.set_setting_offline_cache(self.offline_first);
        app.set_setting_compositor(SharedString::from(self.compositor.as_str()));
        app.set_setting_shell(SharedString::from(self.shell.as_str()));
        app.set_setting_data_dir(SharedString::from(data_dir.display().to_string().as_str()));
        app.set_setting_backup_dir(SharedString::from(
            data_dir.join("backups").display().to_string().as_str(),
        ));
        app.set_setting_journal_path(SharedString::from(
            data_dir.join("journal.db").display().to_string().as_str(),
        ));
        app.set_setting_total_transactions(total_txns);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_rgb_valid_and_shorthand() {
        assert_eq!(parse_hex_rgb("#1e1e2e"), Some((30, 30, 46)));
        assert_eq!(parse_hex_rgb("#89b4fa"), Some((137, 180, 250)));
        assert_eq!(parse_hex_rgb("#cba6f7"), Some((203, 166, 247)));
        assert_eq!(parse_hex_rgb("#cdd6f4"), Some((205, 214, 244)));
        // Shorthand
        assert_eq!(parse_hex_rgb("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_rgb("#000"), Some((0, 0, 0)));
        assert_eq!(parse_hex_rgb("#123"), Some((17, 34, 51)));
        // Missing leading hash
        assert_eq!(parse_hex_rgb("1e1e2e"), Some((30, 30, 46)));
    }

    #[test]
    fn test_parse_hex_rgb_adversarial_rejections() {
        assert_eq!(parse_hex_rgb(""), None);
        assert_eq!(parse_hex_rgb("#"), None);
        assert_eq!(parse_hex_rgb("#12"), None);
        assert_eq!(parse_hex_rgb("#1234"), None);
        assert_eq!(parse_hex_rgb("#12345"), None);
        assert_eq!(parse_hex_rgb("#1234567"), None);
        assert_eq!(parse_hex_rgb("#zzzzzz"), None);
        assert_eq!(parse_hex_rgb("#12g456"), None);
        assert_eq!(parse_hex_rgb("#🔥"), None);
    }

    #[test]
    fn test_parse_hex_color_slint() {
        let c = parse_hex_color("#1e1e2e").expect("valid hex");
        assert_eq!(c.red(), 30);
        assert_eq!(c.green(), 30);
        assert_eq!(c.blue(), 46);
        assert_eq!(c.alpha(), 255);

        assert!(parse_hex_color("invalid").is_none());
    }

    #[test]
    fn test_author_provenance_formatting_all_variants() {
        assert_eq!(format_authors(&[]), "Not declared");

        let a1 = vec![Author {
            name: "Alice".into(),
            email: None,
            url: None,
        }];
        assert_eq!(format_authors(&a1), "Alice");

        let a2 = vec![Author {
            name: "Catppuccin contributors".into(),
            email: None,
            url: Some("https://github.com/catppuccin/waybar".into()),
        }];
        assert_eq!(
            format_authors(&a2),
            "Catppuccin contributors (https://github.com/catppuccin/waybar)"
        );

        let a3 = vec![
            Author {
                name: "Alice".into(),
                email: Some("alice@test.org".into()),
                url: None,
            },
            Author {
                name: "Bob".into(),
                email: None,
                url: Some("https://bob.dev".into()),
            },
        ];
        assert_eq!(
            format_authors(&a3),
            "Alice <alice@test.org>, Bob (https://bob.dev)"
        );
    }

    #[test]
    fn test_repository_formatting() {
        assert_eq!(format_repository(&None), "Not declared");
        assert_eq!(
            format_repository(&Some("https://github.com/test".into())),
            "https://github.com/test"
        );
    }

    #[test]
    fn test_search_and_category_filtering() {
        let p1 = PackageViewModel {
            id: "com.catppuccin.waybar".into(),
            name: "Catppuccin Waybar Mocha Theme".into(),
            version: "1.0.0".into(),
            package_type: "theme".into(),
            category: "bar_themes_modules".into(),
            description: "Catppuccin Mocha color definitions for Waybar.".into(),
            license: "MIT".into(),
            repository: "https://github.com/catppuccin/waybar".into(),
            authors_formatted: "Catppuccin contributors".into(),
            display_server: "Wayland Only".into(),
            architectures: vec!["x86_64".into()],
            palette: vec![],
            installed: false,
            manifest_path: PathBuf::from("/test/path"),
        };
        let p2 = PackageViewModel {
            id: "org.hyprland.rice".into(),
            name: "Nordic Frost Setup".into(),
            version: "2.1.0".into(),
            package_type: "rice".into(),
            category: "rices_setups".into(),
            description: "Complete dark minimalist rice for Hyprland.".into(),
            license: "GPL-3.0".into(),
            repository: "Not declared".into(),
            authors_formatted: "Nordic Team".into(),
            display_server: "Wayland Only".into(),
            architectures: vec!["all".into()],
            palette: vec![],
            installed: true,
            manifest_path: PathBuf::from("/test/path2"),
        };
        let list = vec![p1, p2];

        assert_eq!(filter_packages(&list, "", "all").len(), 2);

        let res = filter_packages(&list, "mOcHa", "all");
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "com.catppuccin.waybar");

        let res = filter_packages(&list, "waybar", "all");
        assert_eq!(res.len(), 1);

        let res = filter_packages(&list, "minimalist", "all");
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "org.hyprland.rice");

        let res = filter_packages(&list, "", "bar_themes_modules");
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "com.catppuccin.waybar");

        assert_eq!(filter_packages(&list, "nonexistent", "all").len(), 0);
    }

    #[test]
    fn test_settings_state_serialization_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!(
            "woderrise-settings-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let s1 = SettingsState {
            auto_backup: false,
            strict_hash: true,
            offline_first: false,
            compositor: "Sway".into(),
            shell: "Waybar".into(),
        };

        s1.save_to_dir(&temp_dir).unwrap();
        let s2 = SettingsState::load_from_dir(&temp_dir);
        assert_eq!(s1, s2);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

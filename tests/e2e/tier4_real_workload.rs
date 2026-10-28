//! Tier 4: Real-World Application Workload Scenarios
//! Validates complete end-to-end desktop customization workflows on real package data.

use super::common::{
    TestEnvironment, REAL_CATPPUCCIN_DIR, REAL_CATPPUCCIN_ID, REAL_CATPPUCCIN_VER,
};
use std::fs;
use woderrise_core::models::{load_settings, Catalog, ConfigComponent};
use woderrise_detect::{DetectionEngine, TreeEntry};
use woderrise_github::search::filter_local_catalog;

#[test]
fn test_tier4_end_to_end_user_discovery_and_configuration_lifecycle() {
    let env = TestEnvironment::new("t4-e2e-lifecycle");
    let settings_file = env.settings_file();

    // 1. Initial State: Unauthenticated & 0 Rate Limit Startup
    let out_init = env.run_auth(&["--status"]);
    assert_eq!(out_init.status.code(), Some(0));
    let stdout_init = String::from_utf8_lossy(&out_init.stdout);
    assert!(stdout_init.contains("Unauthenticated"));
    assert!(stdout_init.contains("10 requests/min"));
    assert!(stdout_init.contains("60 requests/hr"));

    // 2. User sets GitHub Personal Access Token via CLI
    let token = "ghp_realworld_workload_lifecycle_pat_123456";
    let out_auth = env.run_auth(&["--token", token]);
    assert_eq!(out_auth.status.code(), Some(0));

    // 3. Verify file permissions and masked status
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&settings_file).expect("Read metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }
    let out_status = env.run_auth(&["--status"]);
    assert_eq!(out_status.status.code(), Some(0));
    let stdout_status = String::from_utf8_lossy(&out_status.stdout);
    assert!(stdout_status.contains("Authenticated"));
    assert!(stdout_status.contains("ghp_••••••••3456"));
    assert!(stdout_status.contains("30 requests/min"));
    assert!(stdout_status.contains("5,000 requests/hr"));

    // 4. Discover packages using typo-tolerant fuzzy search
    let catalog = Catalog::default_popular();

    // Search by abbreviation
    let ml4_matches = filter_local_catalog(&catalog, "ml4");
    assert!(!ml4_matches.is_empty());
    assert_eq!(ml4_matches[0].owner, "mylinuxforwork");

    // Search by typo
    let hypr_matches = filter_local_catalog(&catalog, "hyprlnd");
    assert!(!hypr_matches.is_empty());

    // 5. Build card view with badges and file counts
    let top_repo = &ml4_matches[0];
    let card = woderrise_gui::view_card(top_repo, woderrise_gui::app::Message::Noop);
    let _ = card;

    // 6. User clears credentials
    let out_clear = env.run_auth(&["--clear"]);
    assert_eq!(out_clear.status.code(), Some(0));

    let settings =
        load_settings(&camino::Utf8PathBuf::from_path_buf(settings_file).unwrap()).unwrap();
    assert_eq!(settings.github_token, None);
}

#[tokio::test]
async fn test_tier4_catppuccin_waybar_complete_customization_lifecycle() {
    let ws = TestEnvironment::workspace_root();
    let catppuccin_path = ws.join(REAL_CATPPUCCIN_DIR);

    // 1. Verify verified Catppuccin Waybar package directory exists
    assert!(
        catppuccin_path.exists(),
        "Catppuccin Waybar package directory must exist at: {}",
        catppuccin_path.display()
    );

    // 2. Validate package manifest woderrise.yaml
    let manifest_file = catppuccin_path.join("woderrise.yaml");
    assert!(
        manifest_file.exists(),
        "Manifest file woderrise.yaml must exist"
    );

    let manifest_content = fs::read_to_string(&manifest_file).expect("Read manifest");
    assert!(manifest_content.contains(REAL_CATPPUCCIN_ID));
    assert!(manifest_content.contains(REAL_CATPPUCCIN_VER));
    assert!(manifest_content.contains("MIT"));
    assert!(manifest_content.contains("Catppuccin Waybar Mocha Theme"));

    // 3. Validate package artifact payload
    let artifact_file = catppuccin_path.join("artifacts/mocha.css");
    assert!(artifact_file.exists(), "Artifact mocha.css must exist");
    let artifact_content = fs::read_to_string(&artifact_file).expect("Read artifact");
    assert!(
        artifact_content.contains("@define-color base #1e1e2e;"),
        "Artifact must contain genuine Catppuccin Mocha palette"
    );

    // 4. Run detection engine on package files
    let engine = DetectionEngine::new();
    let entries = vec![
        TreeEntry {
            path: ".config/waybar/config.jsonc".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(1024),
            sha: "sha_cfg".into(),
        },
        TreeEntry {
            path: ".config/waybar/catppuccin-mocha.css".into(),
            mode: "100644".into(),
            entry_type: "blob".into(),
            size: Some(artifact_content.len() as u64),
            sha: "sha_mocha".into(),
        },
    ];

    let detected = engine.detect_components(&entries);
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].component, ConfigComponent::Waybar);
    assert_eq!(detected[0].file_count, 2);
}

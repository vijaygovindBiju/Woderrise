//! Tests for untrusted path sanitization, null byte rejection, and traversal prevention.

use camino::{Utf8Path, Utf8PathBuf};
use woderrise_core::error::{PathError, WoderriseError};
use woderrise_core::paths::{sanitize_relative_path, strip_dotfile_repo_prefix};

#[test]
fn test_sanitize_valid_relative_paths() {
    let valid_cases = vec![
        ("hyprland.conf", "hyprland.conf"),
        ("themes/mocha.conf", "themes/mocha.conf"),
        ("modules/clock/config.jsonc", "modules/clock/config.jsonc"),
        ("./hyprland.conf", "hyprland.conf"),
        ("configs/./sub/./file.conf", "configs/sub/file.conf"),
    ];

    for (input, expected) in valid_cases {
        let res = sanitize_relative_path(Utf8Path::new(input)).expect("Expected valid path");
        assert_eq!(res, Utf8PathBuf::from(expected));
    }
}

#[test]
fn test_reject_parent_directory_traversal() {
    let traversal_cases = vec![
        "../hyprland.conf",
        "../../etc/passwd",
        "foo/../../bar.conf",
        "hypr/../../../root/.ssh/id_rsa",
        "a/b/../..",
        "..",
        "subdir/..",
    ];

    for input in traversal_cases {
        let err = sanitize_relative_path(Utf8Path::new(input)).unwrap_err();
        match err {
            WoderriseError::Path(PathError::PathTraversalAttempted { token, .. }) => {
                assert!(
                    token.contains("..") || token == "empty_path",
                    "Expected token to contain '..', got: {}",
                    token
                );
            }
            other => panic!("Expected PathTraversalAttempted for '{}', got: {:?}", input, other),
        }
    }
}

#[test]
fn test_reject_null_bytes() {
    let null_cases = vec![
        "hyprland.conf\0.hack",
        "\0initial_null",
        "path/with/\0/inside",
    ];

    for input in null_cases {
        let err = sanitize_relative_path(Utf8Path::new(input)).unwrap_err();
        match err {
            WoderriseError::Path(PathError::ContainsNullByte { path }) => {
                assert_eq!(path, input);
            }
            other => panic!("Expected ContainsNullByte for '{}', got: {:?}", input, other),
        }
    }
}

#[test]
fn test_reject_absolute_paths_and_windows_drives() {
    let abs_cases = vec![
        "/etc/passwd",
        "/home/user/.config/hypr",
        "\\Windows\\System32",
        "C:\\Windows\\System32",
        "D:/dotfiles/hypr",
    ];

    for input in abs_cases {
        let err = sanitize_relative_path(Utf8Path::new(input)).unwrap_err();
        match err {
            WoderriseError::Path(PathError::AbsolutePathNotAllowed { path }) => {
                assert_eq!(path, input);
            }
            other => panic!("Expected AbsolutePathNotAllowed for '{}', got: {:?}", input, other),
        }
    }
}

#[test]
fn test_reject_empty_paths() {
    let empty_cases = vec!["", "   ", "\t", "\n"];

    for input in empty_cases {
        let err = sanitize_relative_path(Utf8Path::new(input)).unwrap_err();
        match err {
            WoderriseError::Path(PathError::PathTraversalAttempted { token, .. }) => {
                assert_eq!(token, "empty_path");
            }
            other => panic!("Expected PathTraversalAttempted for '{}', got: {:?}", input, other),
        }
    }
}

#[test]
fn test_strip_dotfile_repo_prefix() {
    let p1 = Utf8Path::new("Configs/.config/hypr/hyprland.conf");
    assert_eq!(
        strip_dotfile_repo_prefix(p1, "hypr"),
        Utf8Path::new("hyprland.conf")
    );

    let p2 = Utf8Path::new("dotfiles/.config/waybar/modules/clock.jsonc");
    assert_eq!(
        strip_dotfile_repo_prefix(p2, "waybar"),
        Utf8Path::new("modules/clock.jsonc")
    );

    let p3 = Utf8Path::new("hyprland.conf");
    assert_eq!(
        strip_dotfile_repo_prefix(p3, "hypr"),
        Utf8Path::new("hyprland.conf")
    );
}

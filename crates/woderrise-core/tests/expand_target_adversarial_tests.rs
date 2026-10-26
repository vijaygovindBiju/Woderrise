use std::path::Path;
use woderrise_core::{expand_target, WoderriseError};

#[test]
fn test_parent_dir_traversal_attacks() {
    let home = Path::new("/home/testuser");

    let malicious_paths = vec![
        "~/../../etc/shadow",
        "~/../..",
        "~//..",
        "~///..",
        "~////..",
        "~/.config/../../etc/passwd",
        "../../etc/shadow",
        "foo/../../bar",
        "foo/../../../etc",
        "..",
        "../",
        "~/..",
        "~/../",
        "foo/../bar",
        "foo/bar/..",
        "./../../etc/shadow",
        "~/.//../../etc/shadow",
        "~/foo/../../etc/shadow",
        "a/b/../../../etc/shadow",
    ];

    for path in malicious_paths {
        let res = expand_target(path, home);
        assert!(
            matches!(res, Err(WoderriseError::UnsafePath(_))),
            "Expected UnsafePath for '{path}', but got: {res:?}"
        );
    }
}

#[test]
fn test_absolute_path_escapes() {
    let home = Path::new("/home/testuser");

    let absolute_paths = vec![
        "/etc/shadow",
        "/",
        "///",
        "////etc/shadow",
        "~//etc/shadow",
        "~///etc/shadow",
        "~////etc/shadow",
    ];

    for path in absolute_paths {
        let res = expand_target(path, home);
        assert!(
            matches!(res, Err(WoderriseError::UnsafePath(_))),
            "Expected UnsafePath for absolute escape '{path}', but got: {res:?}"
        );
    }
}

#[test]
fn test_valid_double_dot_and_multi_dot_filenames_not_falsely_rejected() {
    let home = Path::new("/home/testuser");

    let valid_multi_dot = vec![
        ("foo..bar.conf", home.join("foo..bar.conf")),
        ("..hidden", home.join("..hidden")),
        ("file...tar.gz", home.join("file...tar.gz")),
        ("~/.config/foo..bar.conf", home.join(".config/foo..bar.conf")),
        ("~/..hidden", home.join("..hidden")),
        ("~/file...tar.gz", home.join("file...tar.gz")),
        ("a..b/c..d", home.join("a..b/c..d")),
        ("~/a..b/c..d", home.join("a..b/c..d")),
        ("...tar.gz", home.join("...tar.gz")),
        ("~/...", home.join("...")),
        ("...", home.join("...")),
    ];

    for (target, expected) in valid_multi_dot {
        let res = expand_target(target, home);
        assert!(
            res.is_ok(),
            "Expected Ok for valid target '{target}', but got: {res:?}"
        );
        assert_eq!(res.unwrap(), expected, "Mismatch for '{target}'");
    }
}

#[test]
fn test_valid_paths_and_trailing_slashes() {
    let home = Path::new("/home/testuser");

    let valid_paths = vec![
        ("~", home.to_path_buf()),
        ("~/", home.to_path_buf()),
        ("~/.config", home.join(".config")),
        ("~/.config/waybar/config", home.join(".config/waybar/config")),
        (".config/waybar/config", home.join(".config/waybar/config")),
        ("foo/bar", home.join("foo/bar")),
        (".", home.join(".")),
        ("./foo", home.join("./foo")),
        ("~/foo/", home.join("foo/")),
        ("~/.config/", home.join(".config/")),
    ];

    for (target, expected) in valid_paths {
        let res = expand_target(target, home);
        assert!(
            res.is_ok(),
            "Expected Ok for valid target '{target}', but got: {res:?}"
        );
        assert_eq!(res.unwrap(), expected, "Mismatch for '{target}'");
    }
}

#[test]
fn test_windows_style_backslashes() {
    let home = Path::new("/home/testuser");

    let backslash_paths = vec![
        "~/..\\..\\etc\\shadow",
        "..\\..\\etc\\shadow",
        "~\\..\\..\\etc\\shadow",
        "foo\\..\\..\\bar",
        "foo/..\\../bar",
    ];

    for path in backslash_paths {
        let res = expand_target(path, home);
        println!("Backslash test for '{path}': {res:?}");
    }
}

#[test]
fn test_whitespace_and_unicode_edge_cases() {
    let home = Path::new("/home/testuser");

    let cases = vec![
        "",
        " ",
        "   ",
        "~ /foo",
        "~/ foo",
        "~/foo ",
        " ~/foo",
        "~/\u{200B}../etc/shadow",
        "~/..\u{200B}/etc/shadow",
        "~/foo\u{200B}/bar",
        "~root",
        "~root/foo",
        "~other/bar",
    ];

    for path in cases {
        let res = expand_target(path, home);
        println!("Edge case for '{path}': {res:?}");
    }
}

//! Tests for unified error hierarchy, display formatting, and source chaining.

use camino::Utf8PathBuf;
use std::error::Error;
use std::io;
use woderrise_core::error::{
    DomainError, IoError, PathError, SecurityError, SerializationError, TransactionError,
    WoderriseError,
};

#[test]
fn test_woderrise_error_display_formatting() {
    let domain_err: WoderriseError = DomainError::PackageNotFound {
        id: "prasanthrangan/hyprdots".to_string(),
    }
    .into();
    let display = format!("{}", domain_err);
    assert!(
        display.contains("Package 'prasanthrangan/hyprdots' was not found in catalog"),
        "Unexpected display: {}",
        display
    );

    let path_err: WoderriseError = PathError::PathTraversalAttempted {
        path: "../etc/passwd".to_string(),
        token: "..".to_string(),
    }
    .into();
    let display = format!("{}", path_err);
    assert!(
        display.contains("Path traversal attempted in '../etc/passwd': forbidden token '..'"),
        "Unexpected display: {}",
        display
    );

    let tx_err: WoderriseError = TransactionError::DirtyStateDetected {
        file: Utf8PathBuf::from("/home/user/.config/hypr/hyprland.conf"),
        expected_sha256: "aaaabbbb".to_string(),
        actual_sha256: "ccccdddd".to_string(),
    }
    .into();
    let display = format!("{}", tx_err);
    assert!(
        display.contains("Dirty state detected in '/home/user/.config/hypr/hyprland.conf'"),
        "Unexpected display: {}",
        display
    );
    assert!(display.contains("expected SHA-256 'aaaabbbb'"));
    assert!(display.contains("found 'ccccdddd'"));
}

#[test]
fn test_io_error_source_chaining_and_downcasting() {
    let raw_io = io::Error::new(io::ErrorKind::NotFound, "disk read failure");
    let io_err = IoError::Read {
        path: Utf8PathBuf::from("/tmp/missing.conf"),
        source: raw_io,
    };
    let top_err: WoderriseError = io_err.into();

    let display = format!("{}", top_err);
    assert!(display.contains("Failed to read file at '/tmp/missing.conf': disk read failure"));

    let source1 = top_err.source().expect("Expected primary source");
    let downcasted_io = source1
        .downcast_ref::<IoError>()
        .expect("Expected source to be IoError");

    match downcasted_io {
        IoError::Read { path, source } => {
            assert_eq!(path, "/tmp/missing.conf");
            assert_eq!(source.kind(), io::ErrorKind::NotFound);
        }
        other => panic!("Unexpected IoError variant: {:?}", other),
    }

    let source2 = source1.source().expect("Expected nested std::io::Error");
    let downcasted_std = source2
        .downcast_ref::<io::Error>()
        .expect("Expected source to be std::io::Error");
    assert_eq!(downcasted_std.kind(), io::ErrorKind::NotFound);
}

#[test]
fn test_transaction_error_lifo_and_variants() {
    let lifo_err = TransactionError::LifoViolation {
        tx_id: "tx_123".to_string(),
        top_tx_id: "tx_456".to_string(),
    };
    let display = format!("{}", lifo_err);
    assert!(display.contains("LIFO order violation"));
    assert!(display.contains(
        "cannot rollback transaction 'tx_123' because newer transaction 'tx_456' exists"
    ));

    let progress_err = TransactionError::TransactionInProgress {
        active_tx_id: "active_999".to_string(),
    };
    assert_eq!(
        format!("{}", progress_err),
        "Active transaction 'active_999' is already in progress"
    );
}

#[test]
fn test_security_error_display() {
    let script_err = SecurityError::ScriptExecutionBlocked {
        path: Utf8PathBuf::from("setup.sh"),
        script_type: "bash".to_string(),
    };
    assert_eq!(
        format!("{}", script_err),
        "Untrusted executable script blocked at 'setup.sh' (type: bash)"
    );

    let bypass_err = SecurityError::InterpreterBypassDetected {
        command: "sh install.sh".to_string(),
    };
    assert_eq!(
        format!("{}", bypass_err),
        "Interpreter bypass detected in command 'sh install.sh'"
    );

    let procedural_err = SecurityError::ProceduralDirectiveNeutralized {
        directive: "exec-once = waybar".to_string(),
        file: "hyprland.conf".to_string(),
        line: 42,
    };
    assert_eq!(
        format!("{}", procedural_err),
        "Procedural execution directive 'exec-once = waybar' neutralized in 'hyprland.conf:42'"
    );

    let limit_err = SecurityError::ResourceLimitExceeded {
        resource: "wallpaper.png".to_string(),
        limit_bytes: 50_000_000,
        actual_bytes: 65_000_000,
    };
    assert_eq!(
        format!("{}", limit_err),
        "Resource limit exceeded for 'wallpaper.png': limit 50000000 bytes, got 65000000 bytes"
    );
}

#[test]
fn test_serialization_error_display() {
    let bad_json_err = serde_json::from_str::<serde_json::Value>("{ invalid json }").unwrap_err();
    let ser_err = SerializationError::JsonDeserialize {
        path: Some(Utf8PathBuf::from("/tmp/catalog.json")),
        source: bad_json_err,
    };
    let display = format!("{}", ser_err);
    assert!(display.contains("JSON deserialization failed from '/tmp/catalog.json'"));
}

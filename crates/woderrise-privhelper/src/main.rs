use std::process::Command;
use woderrise_core::{Result, WoderriseError};

const ACTION: &str = "org.woderrise.install-system-packages";
pub fn validate_request(action: &str, packages: &[String]) -> Result<()> {
    if action != ACTION
        || packages.is_empty()
        || packages.iter().any(|p| {
            p.is_empty()
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._+".contains(c))
        })
    {
        return Err(WoderriseError::Validation(
            "only native package names are accepted".into(),
        ));
    }
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (action, packages) = args
        .split_first()
        .map(|(a, p)| (a.as_str(), p.to_vec()))
        .unwrap_or(("", vec![]));
    if let Err(e) = validate_request(action, &packages) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    let status = Command::new("pacman")
        .arg("-S")
        .arg("--needed")
        .arg("--noconfirm")
        .args(packages)
        .status();
    match status {
        Ok(s) if s.success() => {}
        _ => std::process::exit(1),
    }
}

use crate::{Result, WoderriseError};
use std::process::Command;

pub trait SystemPackageManager {
    fn install(&self, packages: &[String]) -> Result<()>;
    fn remove(&self, packages: &[String]) -> Result<()>;
    fn is_available(&self) -> bool;
}
fn validate_names(packages: &[String]) -> Result<()> {
    if packages.is_empty()
        || packages.iter().any(|p| {
            p.is_empty()
                || p.starts_with('-')
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._+".contains(c))
        })
    {
        return Err(WoderriseError::Validation(
            "invalid native package name".into(),
        ));
    }
    Ok(())
}
pub struct DnfPackageManager;
impl SystemPackageManager for DnfPackageManager {
    fn install(&self, packages: &[String]) -> Result<()> {
        validate_names(packages)?;
        run("dnf", "install", packages)
    }
    fn remove(&self, packages: &[String]) -> Result<()> {
        validate_names(packages)?;
        run("dnf", "remove", packages)
    }
    fn is_available(&self) -> bool {
        Command::new("dnf")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}
fn run(program: &str, action: &str, packages: &[String]) -> Result<()> {
    let status = Command::new(program)
        .args(["-y", action])
        .args(packages)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(WoderriseError::Other(format!("{program} {action} failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_options() {
        assert!(validate_names(&["--no-confirm".into()]).is_err());
    }
}

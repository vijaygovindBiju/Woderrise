use crate::{Result, WoderriseError};
use std::{path::PathBuf, process::Command};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyprlandVersion {
    pub version: String,
    pub commit: Option<String>,
}
pub trait HyprlandTransport: Send + Sync {
    fn request(&self, command: &str) -> Result<String>;
}
pub struct HyprctlTransport;
impl HyprlandTransport for HyprctlTransport {
    fn request(&self, command: &str) -> Result<String> {
        let out = Command::new("hyprctl")
            .args(command.split_whitespace())
            .output()
            .map_err(|e| WoderriseError::Other(format!("hyprctl unavailable: {e}")))?;
        if !out.status.success() {
            return Err(WoderriseError::Other(
                String::from_utf8_lossy(&out.stderr).trim().into(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into())
    }
}
pub struct HyprlandClient<T = HyprctlTransport> {
    transport: T,
}
impl<T: HyprlandTransport> HyprlandClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
    pub fn version(&self) -> Result<HyprlandVersion> {
        let raw = self.transport.request("version")?;
        let version = raw
            .lines()
            .find_map(|l| {
                l.split_once("v")
                    .map(|(_, v)| v.split_whitespace().next().unwrap_or(v).to_string())
            })
            .unwrap_or_else(|| raw.trim().to_string());
        let commit = raw
            .lines()
            .find(|l| l.to_ascii_lowercase().contains("commit"))
            .and_then(|l| l.split_whitespace().last())
            .map(str::to_string);
        Ok(HyprlandVersion { version, commit })
    }
    pub fn reload(&self) -> Result<()> {
        self.transport.request("reload").map(|_| ())
    }
    pub fn plugin_load(&self, path: &str, expected_abi: Option<&str>) -> Result<()> {
        if PathBuf::from(path).is_absolute() == false || path.contains("..") {
            return Err(WoderriseError::Validation(
                "plugin path must be absolute and confined".into(),
            ));
        }
        if let Some(expected) = expected_abi {
            let current = self
                .version()?
                .commit
                .ok_or_else(|| WoderriseError::Other("Hyprland commit unavailable".into()))?;
            if current != expected {
                return Err(WoderriseError::Validation(format!(
                    "plugin ABI mismatch: expected {expected}, running {current}"
                )));
            }
        }
        self.transport
            .request(&format!("plugin load {path}"))
            .map(|_| ())
    }
}

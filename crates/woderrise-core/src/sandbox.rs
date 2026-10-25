use crate::{Result, WoderriseError};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub home: PathBuf,
    pub timeout: Duration,
    pub network: bool,
}
impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            args: vec![],
            home: PathBuf::new(),
            timeout: Duration::from_secs(30),
            network: false,
        }
    }
}
pub fn run(config: &SandboxConfig) -> Result<i32> {
    if config.timeout > Duration::from_secs(30) {
        return Err(WoderriseError::Validation(
            "sandbox timeout cannot exceed 30 seconds".into(),
        ));
    }
    if config.executable.as_os_str().is_empty() {
        return Err(WoderriseError::Validation("missing executable".into()));
    }
    let mut c = Command::new("bwrap");
    c.args([
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/",
        "/",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--bind",
    ]);
    c.arg(&config.home).arg("/home");
    if !config.network {
        c.arg("--unshare-net");
    }
    c.arg(&config.executable)
        .args(&config.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c
        .spawn()
        .map_err(|e| WoderriseError::Other(format!("bwrap unavailable: {e}")))?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if start.elapsed() >= config.timeout {
            child.kill()?;
            return Err(WoderriseError::Other("sandbox timed out".into()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub fn validate_home(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Err(WoderriseError::Validation(
            "sandbox home must be a directory".into(),
        ));
    }
    Ok(())
}

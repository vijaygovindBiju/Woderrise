//! Settings model and secure durable persistence for user configuration.

use crate::error::{IoError, Result, SerializationError, WoderriseError};
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;

pub const CURRENT_SETTINGS_VERSION: u32 = 1;

/// Application user configuration settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub github_token: Option<String>,
    pub auto_backup: bool,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_SETTINGS_VERSION,
            github_token: None,
            auto_backup: true,
            theme: "system".to_string(),
        }
    }
}

/// Load settings from disk. Returns `Settings::default()` if the file does not exist.
pub fn load_settings(path: &Utf8Path) -> Result<Settings> {
    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = fs::read_to_string(path).map_err(|source| {
        WoderriseError::Io(IoError::Read {
            path: path.to_path_buf(),
            source,
        })
    })?;

    serde_json::from_str(&content).map_err(|source| {
        WoderriseError::Serialization(SerializationError::JsonDeserialize {
            path: Some(path.to_path_buf()),
            source,
        })
    })
}

/// Durably persist settings to disk with strict POSIX 0600 permissions
/// (read/write by owner only), following the 4-step atomic durability protocol.
pub fn save_settings_durably(path: &Utf8Path, settings: &Settings) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|source| {
                WoderriseError::Io(IoError::CreateDir {
                    path: parent.to_path_buf(),
                    source,
                })
            })?;
        }
    }

    let json_bytes =
        serde_json::to_vec_pretty(settings).map_err(|e| WoderriseError::Serialization(e.into()))?;

    let uuid_str = uuid::Uuid::new_v4().to_string();
    let tmp_path = path.with_extension(format!("tmp.{}", uuid_str));

    let write_res = (|| -> Result<()> {
        let mut file = File::create(&tmp_path).map_err(|source| {
            WoderriseError::Io(IoError::Write {
                path: tmp_path.clone(),
                source,
            })
        })?;

        // Enforce POSIX 0600 (owner read/write only) on temporary staging file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            file.set_permissions(perms).map_err(|source| {
                WoderriseError::Io(IoError::Write {
                    path: tmp_path.clone(),
                    source,
                })
            })?;
        }

        file.write_all(&json_bytes).map_err(|source| {
            WoderriseError::Io(IoError::Write {
                path: tmp_path.clone(),
                source,
            })
        })?;

        file.sync_all().map_err(|source| {
            WoderriseError::Io(IoError::Fsync {
                path: tmp_path.clone(),
                source,
            })
        })?;

        drop(file);

        fs::rename(&tmp_path, path).map_err(|source| {
            WoderriseError::Io(IoError::Rename {
                from: tmp_path.clone(),
                to: path.to_path_buf(),
                source,
            })
        })?;

        // Commit directory entry
        let parent = path
            .parent()
            .filter(|p| !p.as_str().is_empty())
            .unwrap_or(Utf8Path::new("."));

        match File::open(parent) {
            Ok(dir) => {
                if let Err(source) = dir.sync_all() {
                    let is_unsupported = source.kind() == std::io::ErrorKind::Unsupported
                        || source.raw_os_error() == Some(22);
                    if !is_unsupported {
                        return Err(WoderriseError::Io(IoError::Fsync {
                            path: parent.to_path_buf(),
                            source,
                        }));
                    }
                }
            }
            Err(source) => {
                #[cfg(unix)]
                {
                    return Err(WoderriseError::Io(IoError::Fsync {
                        path: parent.to_path_buf(),
                        source,
                    }));
                }
                #[cfg(not(unix))]
                {
                    let _ = source;
                }
            }
        }

        Ok(())
    })();

    if write_res.is_err() && tmp_path.exists() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_res
}

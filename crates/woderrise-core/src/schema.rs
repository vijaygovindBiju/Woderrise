//! Schema URIs, version constants, serialization helpers, and 4-step durability protocol.

use crate::error::{IoError, Result, SerializationError, WoderriseError};
use camino::Utf8Path;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, File};
use std::io::Write;

pub const CATALOG_SCHEMA_V1: &str = "https://woderrise.dev/schemas/catalog.v1.json";
pub const MANIFEST_SCHEMA_V1: &str = "https://woderrise.dev/schemas/manifest.v1.json";
pub const JOURNAL_SCHEMA_V1: &str = "https://woderrise.dev/schemas/journal.v1.json";

pub const CURRENT_CATALOG_VERSION: u32 = 1;
pub const CURRENT_MANIFEST_VERSION: u32 = 1;
pub const CURRENT_JOURNAL_VERSION: u32 = 1;

/// Serialize value to a compact JSON string.
pub fn serialize_to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| WoderriseError::Serialization(e.into()))
}

/// Serialize value to a pretty-printed JSON string.
pub fn serialize_to_json_pretty<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|e| WoderriseError::Serialization(e.into()))
}

/// Deserialize value from a JSON string.
pub fn deserialize_from_json<T: DeserializeOwned>(json_str: &str) -> Result<T> {
    serde_json::from_str(json_str).map_err(|e| {
        WoderriseError::Serialization(SerializationError::JsonDeserialize {
            path: None,
            source: e,
        })
    })
}

/// Read and deserialize a JSON file from disk with contextual error reporting.
pub fn read_json_file<T: DeserializeOwned>(path: &Utf8Path) -> Result<T> {
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

/// Durably write bytes to disk following the 4-step durability protocol:
/// 1. Create parent directory if missing.
/// 2. Write to sibling temporary file (`<target>.tmp.<uuid>`).
/// 3. Call `file.sync_all()` to physically flush data blocks.
/// 4. Atomic POSIX replacement via `fs::rename()`.
/// 5. Commit directory entry by opening parent directory and calling `dir.sync_all()`.
pub fn write_bytes_durably(target_path: &Utf8Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|source| {
                WoderriseError::Io(IoError::CreateDir {
                    path: parent.to_path_buf(),
                    source,
                })
            })?;
        }
    }

    let uuid_str = uuid::Uuid::new_v4().to_string();
    let tmp_path = target_path.with_extension(format!("tmp.{}", uuid_str));

    let write_res = (|| -> Result<()> {
        let mut file = File::create(&tmp_path).map_err(|source| {
            WoderriseError::Io(IoError::Write {
                path: tmp_path.clone(),
                source,
            })
        })?;

        file.write_all(data).map_err(|source| {
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

        fs::rename(&tmp_path, target_path).map_err(|source| {
            WoderriseError::Io(IoError::Rename {
                from: tmp_path.clone(),
                to: target_path.to_path_buf(),
                source,
            })
        })?;

        let parent = target_path
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

/// Durably write a serializable data structure to disk as formatted JSON.
pub fn write_json_file_durably<T: Serialize>(target_path: &Utf8Path, value: &T) -> Result<()> {
    let json_bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| WoderriseError::Serialization(e.into()))?;
    write_bytes_durably(target_path, &json_bytes)
}

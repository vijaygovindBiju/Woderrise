//! 4-Phase atomic file replacement and EXDEV-safe swapper.
//!
//! Guarantees:
//! 1. Cross-Device EXDEV Mitigation: Stream copies staged bytes into temporary sibling
//!    `<target_file>.woderrise_tmp.<uuid>` within the exact destination directory.
//! 2. Data Block Durability: Calls `sync_all()` on the temporary sibling descriptor.
//! 3. Atomic Inode Replacement: Replaces destination atomically via `std::fs::rename()`.
//! 4. Directory Durability: Calls `sync_all()` on the parent directory descriptor to commit
//!    the inode entry to disk.

use camino::{Utf8Path, Utf8PathBuf};
use std::fs::{self, File};
use std::io;
use uuid::Uuid;
use walkdir::WalkDir;
use woderrise_core::error::{IoError, Result, WoderriseError};

/// Engine for executing atomic, durable, cross-filesystem file replacements.
pub struct AtomicSwapper;

impl AtomicSwapper {
    /// Atomically replaces `target_path` with the content from `source_path`.
    ///
    /// Follows the 4-phase protocol:
    /// - Creates sibling temporary `<target_path>.woderrise_tmp.<uuid>`
    /// - Streams source bytes into sibling via `std::io::copy` (EXDEV-safe)
    /// - Commits file payload via `sync_all()`
    /// - Atomically renames temporary sibling over target path
    /// - Flushes parent directory descriptor via `sync_all()`
    pub fn swap_file(source_path: &Utf8Path, target_path: &Utf8Path) -> Result<()> {
        let parent = target_path.parent().unwrap_or(Utf8Path::new("."));
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|source| {
                WoderriseError::Io(IoError::CreateDir {
                    path: parent.to_path_buf(),
                    source,
                })
            })?;
        }

        let file_name = target_path.file_name().unwrap_or("target");
        let tmp_name = format!("{}.woderrise_tmp.{}", file_name, Uuid::new_v4());
        let tmp_path = parent.join(&tmp_name);

        let swap_result = (|| -> Result<()> {
            // 1. Stream copy from source to sibling tmp file (safe across different mountpoints/EXDEV)
            let mut src_file = File::open(source_path).map_err(|source| {
                WoderriseError::Io(IoError::Read {
                    path: source_path.to_path_buf(),
                    source,
                })
            })?;
            let mut tmp_file = File::create(&tmp_path).map_err(|source| {
                WoderriseError::Io(IoError::Write {
                    path: tmp_path.clone(),
                    source,
                })
            })?;

            io::copy(&mut src_file, &mut tmp_file).map_err(|source| {
                WoderriseError::Io(IoError::Write {
                    path: tmp_path.clone(),
                    source,
                })
            })?;

            // 2. Commit file payload data blocks to storage
            tmp_file.sync_all().map_err(|source| {
                WoderriseError::Io(IoError::Fsync {
                    path: tmp_path.clone(),
                    source,
                })
            })?;
            drop(tmp_file);
            drop(src_file);

            // 3. Atomic POSIX rename over target file
            fs::rename(&tmp_path, target_path).map_err(|source| {
                WoderriseError::Io(IoError::Rename {
                    from: tmp_path.clone(),
                    to: target_path.to_path_buf(),
                    source,
                })
            })?;

            // 4. Durability of parent directory inode
            Self::sync_parent_directory(parent)?;

            Ok(())
        })();

        // Clean up temporary sibling on any failure
        if swap_result.is_err() && tmp_path.exists() {
            let _ = fs::remove_file(&tmp_path);
        }

        swap_result
    }

    /// Safely deactivates a legacy configuration file by renaming it to `<path>.disabled_by_woderrise`.
    pub fn deactivate_legacy_file(legacy_path: &Utf8Path) -> Result<Utf8PathBuf> {
        let parent = legacy_path.parent().unwrap_or(Utf8Path::new("."));
        let disabled_name = format!(
            "{}.disabled_by_woderrise",
            legacy_path.file_name().unwrap_or("legacy.conf")
        );
        let disabled_path = parent.join(&disabled_name);

        if legacy_path.exists() {
            fs::rename(legacy_path, &disabled_path).map_err(|source| {
                WoderriseError::Io(IoError::Rename {
                    from: legacy_path.to_path_buf(),
                    to: disabled_path.clone(),
                    source,
                })
            })?;
            Self::sync_parent_directory(parent)?;
        }

        Ok(disabled_path)
    }

    /// Restores a previously disabled configuration file `<path>.disabled_by_woderrise` back to `<path>`.
    pub fn restore_legacy_file(legacy_path: &Utf8Path) -> Result<()> {
        let parent = legacy_path.parent().unwrap_or(Utf8Path::new("."));
        let disabled_name = format!(
            "{}.disabled_by_woderrise",
            legacy_path.file_name().unwrap_or("legacy.conf")
        );
        let disabled_path = parent.join(&disabled_name);

        if disabled_path.exists() {
            fs::rename(&disabled_path, legacy_path).map_err(|source| {
                WoderriseError::Io(IoError::Rename {
                    from: disabled_path.clone(),
                    to: legacy_path.to_path_buf(),
                    source,
                })
            })?;
            Self::sync_parent_directory(parent)?;
        }

        Ok(())
    }

    /// Scans a directory and its subdirectories, deleting any stray `.woderrise_tmp.*` files.
    pub fn prune_stray_tmp_files(dir: &Utf8Path) -> Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut pruned = 0;
        for entry in WalkDir::new(dir)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if entry.file_type().is_file() {
                let name = entry.file_name().to_string_lossy();
                if name.contains(".woderrise_tmp.") {
                    let path = entry.path();
                    if fs::remove_file(path).is_ok() {
                        pruned += 1;
                    }
                }
            }
        }

        Ok(pruned)
    }

    /// Internal helper to sync parent directory file descriptor to disk.
    fn sync_parent_directory(parent: &Utf8Path) -> Result<()> {
        match File::open(parent) {
            Ok(dir_file) => {
                if let Err(source) = dir_file.sync_all() {
                    let is_unsupported = source.kind() == io::ErrorKind::Unsupported
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
    }
}

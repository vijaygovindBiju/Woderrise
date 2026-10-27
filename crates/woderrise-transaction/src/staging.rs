//! Sandboxed ephemeral staging workspace manager.
//!
//! Creates an isolated workspace in `$XDG_CACHE_HOME/woderrise/staging/<tx_id>/`
//! to stage and inspect repository dotfiles prior to any modifications to the
//! host environment. Computes SHA-256 hashes and generates `staging_manifest.json`.

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use woderrise_core::error::{IoError, Result, WoderriseError};
use woderrise_core::models::{StagedFile, StagingManifest};
use woderrise_core::paths::{validate_identifier, XdgPaths};
use woderrise_core::schema::{read_json_file, write_json_file_durably};

/// Staging workspace managing files under `$XDG_CACHE_HOME/woderrise/staging/<tx_id>/`.
#[derive(Debug, Clone)]
pub struct StagingWorkspace {
    pub tx_id: String,
    pub root_dir: Utf8PathBuf,
    pub files_dir: Utf8PathBuf,
    pub manifest_file: Utf8PathBuf,
}

impl StagingWorkspace {
    /// Creates or opens a staging workspace for the given transaction ID.
    pub fn new(xdg: &XdgPaths, tx_id: &str) -> Result<Self> {
        validate_identifier(tx_id)?;
        let root_dir = xdg.staging_dir(tx_id)?;
        let files_dir = root_dir.join("files");
        let manifest_file = root_dir.join("staging_manifest.json");

        if !files_dir.exists() {
            fs::create_dir_all(&files_dir).map_err(|source| {
                WoderriseError::Io(IoError::CreateDir {
                    path: files_dir.clone(),
                    source,
                })
            })?;
        }

        Ok(Self {
            tx_id: tx_id.to_string(),
            root_dir,
            files_dir,
            manifest_file,
        })
    }

    /// Writes content for a file into the staging sandbox and computes its content hash.
    pub fn stage_file(
        &self,
        rel_path: &Utf8Path,
        content: &[u8],
        target_destination: Option<Utf8PathBuf>,
        git_mode: &str,
        blob_sha: Option<String>,
    ) -> Result<StagedFile> {
        let dest = self.files_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|source| {
                    WoderriseError::Io(IoError::CreateDir {
                        path: parent.to_path_buf(),
                        source,
                    })
                })?;
            }
        }

        let mut file = File::create(&dest).map_err(|source| {
            WoderriseError::Io(IoError::Write {
                path: dest.clone(),
                source,
            })
        })?;
        file.write_all(content).map_err(|source| {
            WoderriseError::Io(IoError::Write {
                path: dest.clone(),
                source,
            })
        })?;
        file.sync_all().map_err(|source| {
            WoderriseError::Io(IoError::Fsync {
                path: dest.clone(),
                source,
            })
        })?;

        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha256_hex = format!("{:x}", hasher.finalize());

        let is_symlink = git_mode == "120000" || git_mode.starts_with("120");
        let is_executable = git_mode == "100755" || git_mode.ends_with("755");

        Ok(StagedFile {
            relative_path: rel_path.to_path_buf(),
            target_destination,
            blob_sha,
            file_size: content.len() as u64,
            sha256: sha256_hex,
            file_mode: git_mode.to_string(),
            permissions: if is_executable { 0o755 } else { 0o644 },
            is_executable,
            is_symlink,
            syntax_valid: true,
            path_safe: true,
        })
    }

    /// Durably writes the staging manifest to disk (`staging_manifest.json` and `manifest.json`).
    pub fn commit_manifest(&self, manifest: &StagingManifest) -> Result<()> {
        manifest
            .validate_security_invariants()
            .map_err(WoderriseError::Validation)?;

        // Write both staging_manifest.json and standard manifest.json for compatibility
        write_json_file_durably(&self.manifest_file, manifest)?;
        let alt_manifest = self.root_dir.join("manifest.json");
        if alt_manifest != self.manifest_file {
            write_json_file_durably(&alt_manifest, manifest)?;
        }
        Ok(())
    }

    /// Reads and validates the staging manifest from the sandbox.
    pub fn load_manifest(&self) -> Result<StagingManifest> {
        if self.manifest_file.exists() {
            let manifest: StagingManifest = read_json_file(&self.manifest_file)?;
            manifest
                .validate_security_invariants()
                .map_err(WoderriseError::Validation)?;
            Ok(manifest)
        } else {
            let alt_manifest = self.root_dir.join("manifest.json");
            if alt_manifest.exists() {
                let manifest: StagingManifest = read_json_file(&alt_manifest)?;
                manifest
                    .validate_security_invariants()
                    .map_err(WoderriseError::Validation)?;
                Ok(manifest)
            } else {
                Err(WoderriseError::Io(IoError::Read {
                    path: self.manifest_file.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Manifest file not found in staging workspace",
                    ),
                }))
            }
        }
    }

    /// Deletes the staging sandbox directory and all staged files.
    pub fn cleanup(&self) -> Result<()> {
        if self.root_dir.exists() {
            fs::remove_dir_all(&self.root_dir).map_err(|source| {
                WoderriseError::Io(IoError::Remove {
                    path: self.root_dir.clone(),
                    source,
                })
            })?;
        }
        Ok(())
    }
}

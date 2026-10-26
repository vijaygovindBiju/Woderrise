//! The portable Woderrise package, journal, backup, and installation engine.
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WoderriseError {
    #[error("manifest parse failed: {0}")]
    ManifestParse(String),
    #[error("manifest validation failed: {0}")]
    Validation(String),
    #[error("path is outside the permitted home directory: {0}")]
    UnsafePath(String),
    #[error("checksum mismatch for {path}: expected {expected}, got {actual}")]
    Checksum {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("rollback conflict: {0}")]
    Conflict(String),
    #[error("missing backup snapshot: {0}")]
    MissingBackup(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("registry refresh failed: {0}")]
    Registry(String),
    #[error("fetch failed: {0}")]
    Fetch(#[from] fetcher::FetchError),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Other(String),
}
pub type Result<T> = std::result::Result<T, WoderriseError>;

pub mod catalog;
pub mod diff;
pub mod fetcher;
pub mod hyprland;
pub mod package_manager;
pub mod resolver;
pub mod sandbox;

pub use diff::{
    compute_line_diff, unified_diff, DiffLine, DiffLineKind, FileDiff, FileDiffAction, InstallPlan,
};
pub use fetcher::{
    detect_hyprland_files, fetch_github_repository, parse_and_validate_github_url,
    synthesize_manifest, DetectedDotfile, FetchError, FetchOptions, FetchedRepository,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema_version: String,
    pub package: PackageSpec,
    #[serde(default)]
    pub yanked: bool,
    #[serde(default)]
    pub previews: Option<Previews>,
    #[serde(default)]
    pub compatibility: Option<Compatibility>,
    #[serde(default)]
    pub dependencies: Option<Dependencies>,
    #[serde(default)]
    pub conflicts: Vec<ConflictSpec>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    pub install: InstallSpec,
    #[serde(default)]
    pub removal: Option<RemovalSpec>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageSpec {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub package_type: String,
    pub category: String,
    pub license: String,
    #[serde(default)]
    pub authors: Vec<Author>,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub repository: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Author {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Previews {
    pub thumbnail: String,
    #[serde(default)]
    pub screenshots: Vec<Screenshot>,
    #[serde(default)]
    pub palette: Vec<PaletteColor>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Screenshot {
    pub path: String,
    #[serde(default)]
    pub caption: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaletteColor {
    pub name: String,
    pub hex: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Compatibility {
    #[serde(default)]
    pub compositors: Vec<Compositor>,
    #[serde(default)]
    pub display_server: Option<String>,
    #[serde(default)]
    pub architectures: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Compositor {
    pub name: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dependencies {
    #[serde(default)]
    pub system: Vec<SystemDependency>,
    #[serde(default)]
    pub packages: Vec<PackageDependency>,
    #[serde(default)]
    pub optional: Vec<OptionalDependency>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemDependency {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageDependency {
    pub id: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptionalDependency {
    pub id: String,
    pub description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConflictSpec {
    pub id: String,
    #[serde(default)]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Artifact {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstallSpec {
    #[serde(default)]
    pub files: Vec<FileInstall>,
    #[serde(default)]
    pub config_injections: Vec<ConfigInjection>,
    #[serde(default)]
    pub service_triggers: Vec<ServiceTrigger>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileInstall {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub backup_before_write: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigInjection {
    pub target: String,
    pub source_block: String,
    pub unique_marker: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceTrigger {
    pub action: String,
    #[serde(default)]
    pub process_name: Option<String>,
    #[serde(default)]
    pub signal: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemovalSpec {
    #[serde(default)]
    pub revert_files: bool,
    #[serde(default)]
    pub remove_injections: bool,
    #[serde(default)]
    pub service_triggers: Vec<ServiceTrigger>,
}

impl Manifest {
    pub fn parse(input: &str) -> Result<Self> {
        serde_yaml::from_str(input)
            .map_err(|e| WoderriseError::ManifestParse(e.to_string()))
            .and_then(|m: Self| {
                m.validate()?;
                Ok(m)
            })
    }
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::parse(&fs::read_to_string(path)?)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != "1.0.0" {
            return Err(WoderriseError::Validation(format!(
                "unsupported schema_version {}",
                self.schema_version
            )));
        }
        if self.package.id.trim().is_empty() || self.package.version.trim().is_empty() {
            return Err(WoderriseError::Validation(
                "package id and version are required".into(),
            ));
        }
        if !self.package.id.contains('.')
            || !self.package.id.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            })
        {
            return Err(WoderriseError::Validation(format!(
                "invalid reverse-DNS package id {}",
                self.package.id
            )));
        }
        if !is_semver(&self.package.version) {
            return Err(WoderriseError::Validation(format!(
                "invalid semantic version {}",
                self.package.version
            )));
        }
        if self.package.category.trim().is_empty() || self.package.license.trim().is_empty() {
            return Err(WoderriseError::Validation(
                "package category and license are required".into(),
            ));
        }
        for p in self
            .artifacts
            .iter()
            .map(|a| &a.path)
            .chain(self.install.files.iter().map(|f| &f.source))
        {
            validate_relative(p)?;
        }
        if let Some(previews) = &self.previews {
            validate_relative(&previews.thumbnail)?;
            for screenshot in &previews.screenshots {
                validate_relative(&screenshot.path)?;
            }
            for color in &previews.palette {
                let valid_hex = color.hex.len() == 7
                    && color.hex.starts_with('#')
                    && color.hex[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
                if !valid_hex {
                    return Err(WoderriseError::Validation(format!(
                        "invalid preview palette color {}",
                        color.hex
                    )));
                }
            }
        }
        fn is_semver(version: &str) -> bool {
            let core = version.split(['-', '+']).next().unwrap_or_default();
            let parts: Vec<_> = core.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
        }
        for f in &self.install.files {
            validate_target(&f.target)?;
            if let Some(m) = &f.mode {
                parse_mode(m)?;
            }
        }
        for i in &self.install.config_injections {
            validate_target(&i.target)?;
        }
        for a in &self.artifacts {
            if a.sha256.len() != 64 || !a.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(WoderriseError::Validation(format!(
                    "invalid SHA-256 for {}",
                    a.path
                )));
            }
            if let Some(url) = &a.url {
                if !url.starts_with("https://") {
                    return Err(WoderriseError::Validation(format!(
                        "artifact URL must use HTTPS for {}",
                        a.path
                    )));
                }
            }
        }
        Ok(())
    }
}
fn validate_relative(p: &str) -> Result<()> {
    let x = Path::new(p);
    if x.is_absolute()
        || x.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(WoderriseError::Validation(format!("unsafe path {p}")));
    }
    Ok(())
}
fn validate_target(p: &str) -> Result<()> {
    if p.contains("..") {
        return Err(WoderriseError::Validation(format!("unsafe target {p}")));
    }
    Ok(())
}
fn parse_mode(m: &str) -> Result<u32> {
    let s = m.trim_start_matches('0');
    let n = u32::from_str_radix(if s.is_empty() { "0" } else { s }, 8)
        .map_err(|_| WoderriseError::Validation(format!("invalid mode {m}")))?;
    if n & 0o6000 != 0 {
        return Err(WoderriseError::Validation(
            "setuid/setgid modes are forbidden".into(),
        ));
    }
    Ok(n)
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
pub fn sha256_file(path: impl AsRef<Path>) -> Result<String> {
    Ok(sha256_bytes(&fs::read(path)?))
}
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub struct Database {
    pub conn: Connection,
}
impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(p) = path.as_ref().parent() {
            fs::create_dir_all(p)?;
        }
        let c = Connection::open(path)?;
        c.pragma_update(None, "journal_mode", "WAL")?;
        c.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn: c };
        db.migrate()?;
        Ok(db)
    }
    pub fn in_memory() -> Result<Self> {
        let c = Connection::open_in_memory()?;
        c.pragma_update(None, "journal_mode", "WAL")?;
        let db = Self { conn: c };
        db.migrate()?;
        Ok(db)
    }
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN; CREATE TABLE IF NOT EXISTS transactions(txn_id TEXT PRIMARY KEY,parent_txn_id TEXT,package_id TEXT NOT NULL,version TEXT NOT NULL,txn_type TEXT NOT NULL,status TEXT NOT NULL,started_at INTEGER NOT NULL,completed_at INTEGER,initiator TEXT NOT NULL,error_message TEXT,metadata_json TEXT); CREATE TABLE IF NOT EXISTS backup_snapshots(backup_id TEXT PRIMARY KEY,txn_id TEXT NOT NULL,original_path TEXT NOT NULL,storage_path TEXT NOT NULL,sha256 TEXT NOT NULL,file_mode INTEGER NOT NULL,created_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS operations(op_id TEXT PRIMARY KEY,txn_id TEXT NOT NULL,sequence_num INTEGER NOT NULL,op_type TEXT NOT NULL,target_path TEXT NOT NULL,pre_sha256 TEXT,post_sha256 TEXT,backup_id TEXT,status TEXT NOT NULL,details_json TEXT); CREATE TABLE IF NOT EXISTS installed_packages(package_id TEXT PRIMARY KEY,active_version TEXT NOT NULL,package_type TEXT NOT NULL,installed_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,is_explicit INTEGER NOT NULL DEFAULT 1,ref_count INTEGER NOT NULL DEFAULT 1,pin_version TEXT,install_txn_id TEXT NOT NULL); CREATE TABLE IF NOT EXISTS file_ownership(path TEXT PRIMARY KEY,package_id TEXT NOT NULL,installed_version TEXT NOT NULL,installed_sha256 TEXT NOT NULL,file_mode INTEGER NOT NULL DEFAULT 420,is_symlink INTEGER NOT NULL DEFAULT 0,backup_id TEXT,claimed_at INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS dependency_graph(parent_id TEXT NOT NULL,child_id TEXT NOT NULL,dep_type TEXT NOT NULL,version_constraint TEXT NOT NULL,PRIMARY KEY(parent_id,child_id)); CREATE INDEX IF NOT EXISTS idx_op_txn ON operations(txn_id,sequence_num); CREATE INDEX IF NOT EXISTS idx_owner_package ON file_ownership(package_id); PRAGMA user_version=1; COMMIT;")?;
        Ok(())
    }
    pub fn begin_transaction(
        &self,
        package_id: &str,
        version: &str,
        kind: &str,
        initiator: &str,
    ) -> Result<String> {
        let id = format!("TXN-{}-{}", now_ms(), uuid::Uuid::new_v4());
        self.conn.execute(
            "INSERT INTO transactions VALUES(?1,NULL,?2,?3,?4,'PLANNED',?5,NULL,?6,NULL,NULL)",
            params![id, package_id, version, kind, now_ms(), initiator],
        )?;
        Ok(id)
    }
    pub fn set_status(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE transactions SET status=?1,completed_at=CASE WHEN ?1 IN ('COMMITTED','FAILED','ABORTED_RECOVERED','ROLLED_BACK') THEN ?2 ELSE completed_at END,error_message=?3 WHERE txn_id=?4",params![status,now_ms(),error,id])?;
        Ok(())
    }
}

pub struct BackupManager {
    pub root: PathBuf,
}
impl BackupManager {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let b = Self { root: root.into() };
        fs::create_dir_all(&b.root)?;
        Ok(b)
    }
    pub fn snapshot(&self, path: impl AsRef<Path>, txn_id: &str, db: &Database) -> Result<String> {
        let p = path.as_ref();
        let data = fs::read(p)?;
        let hash = sha256_bytes(&data);
        let id = format!("BKP-{}-{}", &hash[..12], uuid::Uuid::new_v4());
        let dest = self.root.join(&hash);
        if !dest.exists() {
            fs::write(&dest, &data)?;
        }
        let mode = unix_mode(p);
        db.conn.execute(
            "INSERT INTO backup_snapshots VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                txn_id,
                p.to_string_lossy(),
                dest.to_string_lossy(),
                hash,
                mode,
                now_ms()
            ],
        )?;
        Ok(id)
    }
    pub fn restore(
        &self,
        storage: impl AsRef<Path>,
        target: impl AsRef<Path>,
        mode: u32,
    ) -> Result<()> {
        atomic_replace(storage.as_ref(), target.as_ref(), mode)
    }
}
#[cfg(unix)]
fn unix_mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o7777)
        .unwrap_or(0o644)
}
#[cfg(not(unix))]
fn unix_mode(_: &Path) -> u32 {
    0o644
}

pub fn atomic_replace(source: &Path, target: &Path, mode: u32) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = target.with_file_name(format!(
        ".{}.woderrise.tmp.{}",
        target.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    fs::copy(source, &tmp)?;
    set_mode(&tmp, mode)?;
    if let Err(error) = fs::rename(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}
/// Portable atomic file replacement. Linux uses the same-filesystem rename
/// fallback when `renameat2(RENAME_EXCHANGE)` is unavailable.
pub struct AtomicFileSwapper;
impl AtomicFileSwapper {
    pub fn replace(source: impl AsRef<Path>, target: impl AsRef<Path>, mode: u32) -> Result<()> {
        atomic_replace(source.as_ref(), target.as_ref(), mode)
    }
}
pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    let _ = (path, mode);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallStage {
    Resolve,
    Compatibility,
    Fetch,
    Validate,
    Inspect,
    Plan,
    Conflicts,
    Backup,
    Prepared,
    Execute,
    Outcomes,
    Activate,
    Verify,
    Commit,
    Present,
}

impl InstallStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolve => "Resolve",
            Self::Compatibility => "Compatibility",
            Self::Fetch => "Fetch",
            Self::Validate => "Validate",
            Self::Inspect => "Inspect",
            Self::Plan => "Plan",
            Self::Conflicts => "Conflicts",
            Self::Backup => "Backup",
            Self::Prepared => "Prepared",
            Self::Execute => "Execute",
            Self::Outcomes => "Outcomes",
            Self::Activate => "Activate",
            Self::Verify => "Verify",
            Self::Commit => "Commit",
            Self::Present => "Present",
        }
    }
}

impl std::fmt::Display for InstallStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub type ProgressCallback = Box<dyn Fn(InstallStage, &str, f32) + Send + Sync>;

pub struct InstallationEngine {
    pub db: Database,
    pub backups: BackupManager,
    pub home: PathBuf,
}
impl InstallationEngine {
    pub fn install(&self, package_dir: impl AsRef<Path>, initiator: &str) -> Result<String> {
        self.install_with_progress(package_dir, initiator, None)
    }

    pub fn install_with_progress(
        &self,
        package_dir: impl AsRef<Path>,
        initiator: &str,
        progress: Option<ProgressCallback>,
    ) -> Result<String> {
        let report = |stage: InstallStage, message: &str, fraction: f32| {
            if let Some(cb) = &progress {
                cb(stage, message, fraction);
            }
        };

        // 1. Resolve (0.05 - 0.15)
        report(InstallStage::Resolve, "Resolving package manifest", 0.05);
        let dir = package_dir.as_ref();
        let manifest_path = if dir.is_file() {
            dir.to_path_buf()
        } else {
            dir.join("woderrise.yaml")
        };
        let base_dir = manifest_path
            .parent()
            .ok_or_else(|| WoderriseError::Other("manifest has no parent directory".into()))?;
        let manifest = Manifest::from_file(&manifest_path)?;
        report(InstallStage::Resolve, "Package manifest resolved", 0.15);

        // 2. Validate (0.18 - 0.25)
        report(InstallStage::Validate, "Validating install sources and checksums", 0.18);
        validate_install_sources(base_dir, &manifest)?;
        report(InstallStage::Validate, "Sources validated successfully", 0.25);

        // 3. Plan (0.28 - 0.38)
        report(InstallStage::Plan, "Preparing transaction", 0.28);
        let txn = self.db.begin_transaction(
            &manifest.package.id,
            &manifest.package.version,
            "INSTALL",
            initiator,
        )?;
        self.db.set_status(&txn, "PREPARED", None)?;
        report(InstallStage::Plan, "Transaction prepared", 0.38);

        let result = self.install_inner_with_progress(base_dir, &manifest, &txn, &report);
        match result {
            Ok(()) => {
                // 7. Commit (0.96 - 1.00)
                report(InstallStage::Commit, "Recording installed package state", 0.96);
                self.db.conn.execute(
                    "INSERT OR REPLACE INTO installed_packages(package_id,active_version,package_type,installed_at,updated_at,is_explicit,ref_count,install_txn_id) VALUES(?1,?2,?3,?4,?4,1,1,?5)",
                    params![
                        manifest.package.id,
                        manifest.package.version,
                        manifest.package.package_type,
                        now_ms(),
                        txn
                    ],
                )?;
                self.db.set_status(&txn, "COMMITTED", None)?;
                report(InstallStage::Commit, "Installation committed successfully", 1.0);
                Ok(txn)
            }
            Err(e) => {
                self.db.set_status(&txn, "FAILED", Some(&e.to_string()))?;
                self.compensate(&txn).ok();
                self.db
                    .set_status(&txn, "ABORTED_RECOVERED", Some(&e.to_string()))?;
                Err(e)
            }
        }
    }

    fn install_inner_with_progress<F>(
        &self,
        dir: &Path,
        m: &Manifest,
        txn: &str,
        report: &F,
    ) -> Result<()>
    where
        F: Fn(InstallStage, &str, f32),
    {
        let total_files = m.install.files.len().max(1);

        // 4. Backup (0.40 - 0.60)
        report(InstallStage::Backup, "Starting backup of modified targets", 0.40);
        struct PreparedFile {
            target: PathBuf,
            src: PathBuf,
            pre: Option<String>,
            post: String,
            backup: Option<String>,
            mode: u32,
        }
        let mut prepared = Vec::with_capacity(m.install.files.len());

        for (n, f) in m.install.files.iter().enumerate() {
            let target = expand_target(&f.target, &self.home)?;
            let src = dir.join(&f.source);
            let post = sha256_file(&src)?;
            let expected = m
                .artifacts
                .iter()
                .find(|a| a.path == f.source)
                .map(|a| a.sha256.as_str());
            if let Some(e) = expected {
                if e != post {
                    return Err(WoderriseError::Checksum {
                        path: f.source.clone(),
                        expected: e.into(),
                        actual: post,
                    });
                }
            }
            let pre = if target.exists() {
                Some(sha256_file(&target)?)
            } else {
                None
            };
            let backup = if target.exists() && f.backup_before_write {
                Some(self.backups.snapshot(&target, txn, &self.db)?)
            } else {
                None
            };

            let backup_fraction = 0.40 + 0.20 * ((n + 1) as f32 / total_files as f32);
            report(InstallStage::Backup, "Target files backed up", backup_fraction);

            prepared.push(PreparedFile {
                target,
                src,
                pre,
                post,
                backup,
                mode: parse_mode(f.mode.as_deref().unwrap_or("0644"))?,
            });
        }
        report(InstallStage::Backup, "Target backups completed", 0.60);

        // 5. Execute (0.62 - 0.85)
        report(InstallStage::Execute, "Applying configuration payload", 0.62);
        self.db.set_status(txn, "EXECUTING", None)?;
        for (n, pf) in prepared.iter().enumerate() {
            let op = format!("OP-{}-{}", txn, n);
            self.db.conn.execute(
                "INSERT INTO operations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'PLANNED',NULL)",
                params![
                    op,
                    txn,
                    n as i64 + 1,
                    if pf.pre.is_some() {
                        "MODIFY_FILE"
                    } else {
                        "CREATE_FILE"
                    },
                    pf.target.to_string_lossy(),
                    pf.pre,
                    pf.post,
                    pf.backup
                ],
            )?;
            atomic_replace(&pf.src, &pf.target, pf.mode)?;
            self.db.conn.execute(
                "UPDATE operations SET status='COMPLETED' WHERE op_id=?1",
                params![op],
            )?;

            let exec_fraction = 0.62 + 0.23 * ((n + 1) as f32 / total_files as f32);
            report(InstallStage::Execute, "Applying configuration payload", exec_fraction);
        }
        report(InstallStage::Execute, "Payload files applied", 0.85);

        // 6. Verify (0.88 - 0.95)
        report(InstallStage::Verify, "Verifying installed target files", 0.88);
        for pf in &prepared {
            if !pf.target.exists() {
                return Err(WoderriseError::Other(format!(
                    "verification failed: target file missing {}",
                    pf.target.display()
                )));
            }
            let actual = sha256_file(&pf.target)?;
            if actual != pf.post {
                return Err(WoderriseError::Checksum {
                    path: pf.target.to_string_lossy().to_string(),
                    expected: pf.post.clone(),
                    actual,
                });
            }
        }
        report(InstallStage::Verify, "All targets verified successfully", 0.95);

        Ok(())
    }

    fn compensate(&self, txn: &str) -> Result<()> {
        let mut st = self.db.conn.prepare(
            "SELECT target_path,pre_sha256,backup_id FROM operations WHERE txn_id=?1 AND status='COMPLETED' ORDER BY sequence_num DESC",
        )?;
        let rows = st
            .query_map(params![txn], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (path, pre, bid) in rows {
            if let Some(id) = bid {
                let storage: String = self.db.conn.query_row(
                    "SELECT storage_path FROM backup_snapshots WHERE backup_id=?1",
                    params![id],
                    |r| r.get(0),
                )?;
                self.backups.restore(storage, path, 0o644)?;
            } else if pre.is_none() {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }

    pub fn rollback(&self, txn: &str, force: bool) -> Result<String> {
        let (status, _target_pkg): (String, String) = self
            .db
            .conn
            .query_row(
                "SELECT status, package_id FROM transactions WHERE txn_id = ?1",
                params![txn],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    WoderriseError::Other(format!("transaction {txn} not found"))
                }
                other => WoderriseError::Database(other),
            })?;

        if status != "COMMITTED" {
            return Err(WoderriseError::Other(format!(
                "cannot rollback transaction {txn} with status '{status}' (must be COMMITTED)"
            )));
        }

        let mut st = self.db.conn.prepare(
            "SELECT target_path,pre_sha256,post_sha256,backup_id FROM operations WHERE txn_id=?1 ORDER BY sequence_num DESC",
        )?;
        let rows = st
            .query_map(params![txn], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Pass 1: Conflict check for all files before any disk mutation
        if !force {
            for (path, _pre, post, _bid) in &rows {
                if sha256_file(path).ok().as_deref() != Some(post) {
                    return Err(WoderriseError::Conflict(path.clone()));
                }
            }
        }

        let new = self
            .db
            .begin_transaction("rollback", txn, "ROLLBACK", "user_cli")?;
        self.db.set_status(&new, "EXECUTING", None)?;

        // Pass 2: Revert files from backups or delete created files
        for (path, pre, _post, bid) in &rows {
            if let Some(id) = bid {
                let storage: String = self.db.conn.query_row(
                    "SELECT storage_path FROM backup_snapshots WHERE backup_id=?1",
                    params![id],
                    |r| r.get(0),
                )?;
                atomic_replace(Path::new(&storage), Path::new(path), 0o644)?;
            } else if pre.is_none() {
                let _ = fs::remove_file(path);
            }
        }

        // Clean up installed_packages table
        self.db.conn.execute(
            "DELETE FROM installed_packages WHERE install_txn_id = ?1",
            params![txn],
        )?;

        self.db.set_status(&new, "COMMITTED", None)?;
        self.db.set_status(txn, "ROLLED_BACK", None)?;
        Ok(new)
    }

    pub fn recover(&self) -> Result<usize> {
        let mut st = self.db.conn.prepare(
            "SELECT txn_id, package_id FROM transactions WHERE status IN ('PLANNED','PREPARED','EXECUTING','FAILED')",
        )?;
        let entries = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let n = entries.len();
        for (id, _pkg_id) in entries {
            self.compensate(&id).ok();
            self.db
                .conn
                .execute(
                    "DELETE FROM installed_packages WHERE install_txn_id = ?1",
                    params![id],
                )
                .ok();
            self.db
                .set_status(&id, "ABORTED_RECOVERED", Some("startup recovery"))?;
        }
        Ok(n)
    }
}
fn validate_install_sources(dir: &Path, manifest: &Manifest) -> Result<()> {
    if !manifest.install.config_injections.is_empty()
        || !manifest.install.service_triggers.is_empty()
    {
        return Err(WoderriseError::Validation(
            "config injections and service triggers are not supported by this installer".into(),
        ));
    }
    for artifact in &manifest.artifacts {
        let path = dir.join(&artifact.path);
        let actual = sha256_file(&path)?;
        if actual != artifact.sha256 {
            return Err(WoderriseError::Checksum {
                path: artifact.path.clone(),
                expected: artifact.sha256.clone(),
                actual,
            });
        }
    }
    for file in &manifest.install.files {
        let artifact = manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.path == file.source)
            .ok_or_else(|| {
                WoderriseError::Validation(format!(
                    "install source is not declared as an artifact: {}",
                    file.source
                ))
            })?;
        let path = dir.join(&file.source);
        let actual = sha256_file(&path)?;
        if actual != artifact.sha256 {
            return Err(WoderriseError::Checksum {
                path: file.source.clone(),
                expected: artifact.sha256.clone(),
                actual,
            });
        }
    }
    Ok(())
}
pub fn expand_target(target: &str, home: &Path) -> Result<PathBuf> {
    let p = Path::new(target);
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(WoderriseError::UnsafePath(target.into()));
    }
    if target == "~" {
        return Ok(home.to_path_buf());
    }
    let rel = if let Some(rest) = target.strip_prefix("~/") {
        Path::new(rest)
    } else {
        p
    };
    if rel.is_absolute()
        || rel.components().any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(WoderriseError::UnsafePath(target.into()));
    }
    let resolved = home.join(rel);
    if !resolved.starts_with(home) {
        return Err(WoderriseError::UnsafePath(target.into()));
    }
    Ok(resolved)
}
pub struct RollbackEngine<'a> {
    pub engine: &'a InstallationEngine,
}
impl<'a> RollbackEngine<'a> {
    pub fn rollback(&self, transaction: &str, force: bool) -> Result<String> {
        self.engine.rollback(transaction, force)
    }
}
pub struct StartupRecoveryScanner<'a> {
    pub engine: &'a InstallationEngine,
}
impl<'a> StartupRecoveryScanner<'a> {
    pub fn scan(&self) -> Result<usize> {
        self.engine.recover()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(".woderrise-test-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn test_expand_target_valid_and_unsafe_traversals() {
        let home = Path::new("/home/testuser");

        // Valid target expansions
        assert_eq!(expand_target("~", home).unwrap(), home);
        assert_eq!(expand_target("~/", home).unwrap(), home);
        assert_eq!(
            expand_target("~/.config/waybar/config", home).unwrap(),
            home.join(".config/waybar/config")
        );
        assert_eq!(
            expand_target(".config/waybar/config", home).unwrap(),
            home.join(".config/waybar/config")
        );
        assert_eq!(
            expand_target("foo/bar", home).unwrap(),
            home.join("foo/bar")
        );
        assert_eq!(
            expand_target("foo..bar", home).unwrap(),
            home.join("foo..bar")
        );
        assert_eq!(
            expand_target("..hidden", home).unwrap(),
            home.join("..hidden")
        );
        assert_eq!(
            expand_target("file...tar.gz", home).unwrap(),
            home.join("file...tar.gz")
        );

        // Path traversal attempts using ParentDir component (must return Err(UnsafePath))
        assert!(matches!(
            expand_target("~/../../etc/shadow", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("../../etc/shadow", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("..", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("~/..", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("~//..", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("~/foo/../bar", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("foo/../bar", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("foo/bar/..", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target(".config/../../etc/passwd", home),
            Err(WoderriseError::UnsafePath(_))
        ));

        // Absolute path escapes (must return Err(UnsafePath))
        assert!(matches!(
            expand_target("/etc/shadow", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("~//etc/shadow", home),
            Err(WoderriseError::UnsafePath(_))
        ));
        assert!(matches!(
            expand_target("///etc/shadow", home),
            Err(WoderriseError::UnsafePath(_))
        ));
    }
    #[test]
    fn parses_and_rejects_manifest() {
        let y="schema_version: '1.0.0'\npackage:\n  id: org.test.x\n  name: X\n  version: '1.0.0'\n  type: config\n  category: c\n  license: MIT\n  description: d\ninstall:\n  files: []";
        assert!(Manifest::parse(y).is_ok());
        assert!(
            Manifest::parse(&y.replace("schema_version: '1.0.0'", "schema_version: '2.0.0'"))
                .is_err()
        );
    }
    #[test]
    fn rejects_invalid_preview_palette_color() {
        let yaml = "schema_version: '1.0.0'\npackage:\n  id: org.test.preview\n  name: Preview\n  version: '1.0.0'\n  type: theme\n  category: test\n  license: MIT\n  description: test\npreviews:\n  thumbnail: preview.png\n  palette:\n    - name: broken\n      hex: not-a-color\ninstall:\n  files: []";
        assert!(Manifest::parse(yaml).is_err());
    }
    #[test]
    fn backup_deduplicates() {
        let p = root("backup");
        let f = p.join("a");
        fs::write(&f, b"hello").unwrap();
        let db = Database::in_memory().unwrap();
        let b = BackupManager::new(p.join("backups")).unwrap();
        let a = b.snapshot(&f, "t", &db).unwrap();
        let c = b.snapshot(&f, "t", &db).unwrap();
        assert_ne!(a, c);
        assert_eq!(fs::read_dir(p.join("backups")).unwrap().count(), 1);
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn installs_manifest_file_and_records_package() {
        let package = root("install");
        let home = package.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(package.join("payload"), b"payload").unwrap();
        let payload_hash = sha256_bytes(b"payload");
        fs::write(
            package.join("fixture.yaml"),
            "schema_version: '1.0.0'\npackage:\n  id: fixture.install\n  name: Install Fixture\n  version: '1.0.0'\n  type: config\n  category: test\n  license: MIT\n  description: test\ninstall:\n  files:\n    - source: payload\n      target: .config/woderrise-test\n",
        )
        .unwrap();
        let manifest_path = package.join("fixture.yaml");
        let mut manifest = fs::read_to_string(&manifest_path).unwrap();
        manifest = manifest.replace(
            "install:\n",
            &format!("artifacts:\n  - path: payload\n    sha256: {payload_hash}\ninstall:\n"),
        );
        fs::write(manifest_path, manifest).unwrap();
        let data = package.join("data");
        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };
        let txn = engine
            .install(package.join("fixture.yaml"), "test")
            .unwrap();
        assert!(home.join(".config/woderrise-test").exists());
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT active_version FROM installed_packages WHERE package_id='fixture.install'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "1.0.0"
        );
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT status FROM transactions WHERE txn_id=?1",
                    [&txn],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "COMMITTED"
        );
        let _ = fs::remove_dir_all(package);
    }

    #[test]
    fn rejects_unlisted_install_source_without_modifying_home() {
        let package = root("unlisted-source");
        let home = package.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(package.join("payload"), b"payload").unwrap();
        fs::write(
            package.join("fixture.yaml"),
            "schema_version: '1.0.0'\npackage:\n  id: fixture.unlisted\n  name: Unlisted Fixture\n  version: '1.0.0'\n  type: config\n  category: test\n  license: MIT\n  description: test\nartifacts: []\ninstall:\n  files:\n    - source: payload\n      target: .config/should-not-exist\n",
        )
        .unwrap();
        let data = package.join("data");
        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };
        assert!(engine
            .install(package.join("fixture.yaml"), "test")
            .is_err());
        assert!(!home.join(".config/should-not-exist").exists());
        let _ = fs::remove_dir_all(package);
    }

    #[test]
    fn failed_second_operation_compensates_first_operation() {
        let package = root("partial-failure");
        let home = package.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(package.join("first"), b"first").unwrap();
        fs::write(package.join("second"), b"second").unwrap();
        let first_hash = sha256_bytes(b"first");
        let second_hash = sha256_bytes(b"second");
        let manifest = format!(
            "schema_version: '1.0.0'\npackage:\n  id: fixture.partial\n  name: Partial Fixture\n  version: '1.0.0'\n  type: config\n  category: test\n  license: MIT\n  description: test\nartifacts:\n  - path: first\n    sha256: {first_hash}\n  - path: second\n    sha256: {second_hash}\ninstall:\n  files:\n    - source: first\n      target: .config/first\n    - source: second\n      target: .config\n"
        );
        fs::write(package.join("fixture.yaml"), manifest).unwrap();
        let data = package.join("data");
        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };
        assert!(engine
            .install(package.join("fixture.yaml"), "test")
            .is_err());
        assert!(!home.join(".config/first").exists());
        assert!(!home.join(".config").join("second").exists());
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT status FROM transactions ORDER BY started_at DESC LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "ABORTED_RECOVERED"
        );
        let _ = fs::remove_dir_all(package);
    }

    #[test]
    fn test_root_allocates_in_temp_dir_not_workspace() {
        let p = root("hygiene-check");
        assert!(p.starts_with(std::env::temp_dir()));
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn test_diff_computation_comprehensive() {
        // Empty strings
        let diff = compute_line_diff("", "");
        assert!(diff.is_empty());

        // Identical strings
        let diff = compute_line_diff("alpha\nbeta\ngamma", "alpha\nbeta\ngamma");
        assert_eq!(diff.len(), 3);
        assert!(diff.iter().all(|l| l.kind == DiffLineKind::Context));
        assert_eq!(diff[0].left_line_no, Some(1));
        assert_eq!(diff[0].right_line_no, Some(1));

        // Pure additions
        let diff = compute_line_diff("", "line1\nline2");
        assert_eq!(diff.len(), 2);
        assert!(diff.iter().all(|l| l.kind == DiffLineKind::Addition));
        assert_eq!(diff[0].left_line_no, None);
        assert_eq!(diff[0].right_line_no, Some(1));
        assert_eq!(diff[1].right_line_no, Some(2));

        // Pure deletions
        let diff = compute_line_diff("line1\nline2", "");
        assert_eq!(diff.len(), 2);
        assert!(diff.iter().all(|l| l.kind == DiffLineKind::Deletion));
        assert_eq!(diff[0].left_line_no, Some(1));
        assert_eq!(diff[0].right_line_no, None);
        assert_eq!(diff[1].left_line_no, Some(2));

        // Modifications with prefix, suffix, and middle edits
        let left = "common_head\nleft_old\ncommon_tail";
        let right = "common_head\nright_new\ncommon_tail";
        let diff = unified_diff(left, right);
        assert_eq!(diff.len(), 4);
        assert_eq!(diff[0].kind, DiffLineKind::Context);
        assert_eq!(diff[0].text, "common_head");
        assert_eq!(diff[1].kind, DiffLineKind::Deletion);
        assert_eq!(diff[1].text, "left_old");
        assert_eq!(diff[2].kind, DiffLineKind::Addition);
        assert_eq!(diff[2].text, "right_new");
        assert_eq!(diff[3].kind, DiffLineKind::Context);
        assert_eq!(diff[3].text, "common_tail");
    }

    #[test]
    fn test_compute_plan_multi_file_and_backups() {
        let pkg_dir = root("plan-test");
        let home = pkg_dir.join("home");
        let backups = pkg_dir.join("backups");
        fs::create_dir_all(&home).unwrap();

        // 1. Existing file to be modified
        let target_existing = home.join(".config/app/config.yaml");
        fs::create_dir_all(target_existing.parent().unwrap()).unwrap();
        fs::write(&target_existing, b"old_config: true\nmode: dark\n").unwrap();
        let old_hash = sha256_bytes(b"old_config: true\nmode: dark\n");

        // 2. Existing file that is identical
        let target_same = home.join(".config/app/same.txt");
        fs::write(&target_same, b"unchanged").unwrap();
        let same_hash = sha256_bytes(b"unchanged");

        // Package sources
        fs::write(pkg_dir.join("new_config.yaml"), b"new_config: true\nmode: dark\n").unwrap();
        let new_hash = sha256_bytes(b"new_config: true\nmode: dark\n");
        fs::write(pkg_dir.join("same.txt"), b"unchanged").unwrap();
        fs::write(pkg_dir.join("created.txt"), b"brand new file").unwrap();
        let created_hash = sha256_bytes(b"brand new file");

        let manifest_content = format!(
            r#"schema_version: '1.0.0'
package:
  id: test.multi.plan
  name: Plan Test
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: test plan
artifacts:
  - path: new_config.yaml
    sha256: {new_hash}
  - path: same.txt
    sha256: {same_hash}
  - path: created.txt
    sha256: {created_hash}
install:
  files:
    - source: new_config.yaml
      target: .config/app/config.yaml
      backup_before_write: true
    - source: same.txt
      target: .config/app/same.txt
      backup_before_write: true
    - source: created.txt
      target: .config/app/created.txt
      backup_before_write: false
"#
        );
        fs::write(pkg_dir.join("woderrise.yaml"), manifest_content).unwrap();

        let db = Database::open(pkg_dir.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(&backups).unwrap(),
            home: home.clone(),
        };

        let plan = engine.compute_plan(&pkg_dir).unwrap();
        assert_eq!(plan.package_id, "test.multi.plan");
        assert_eq!(plan.version, "1.0.0");
        assert_eq!(plan.file_diffs.len(), 3);
        assert_eq!(plan.will_backup_count, 2);
        assert!(plan.has_changes());

        // File 1: Modified with backup prediction
        let f1 = &plan.file_diffs[0];
        assert_eq!(f1.action, FileDiffAction::Modify);
        assert_eq!(f1.planned_backup_path, Some(backups.join(&old_hash)));
        assert!(f1.has_changes());

        // File 2: Unchanged with backup prediction
        let f2 = &plan.file_diffs[1];
        assert_eq!(f2.action, FileDiffAction::Unchanged);
        assert_eq!(f2.planned_backup_path, Some(backups.join(&same_hash)));
        assert!(!f2.has_changes());

        // File 3: Created without backup
        let f3 = &plan.file_diffs[2];
        assert_eq!(f3.action, FileDiffAction::Create);
        assert_eq!(f3.planned_backup_path, None);
        assert!(f3.has_changes());

        let _ = fs::remove_dir_all(pkg_dir);
    }

    #[test]
    fn test_rollback_cleans_up_installed_packages_and_updates_statuses() {
        let pkg_dir = root("rollback-cleanup");
        let home = pkg_dir.join("home");
        let data = pkg_dir.join("data");
        fs::create_dir_all(&home).unwrap();

        let target_file = home.join(".config/app/theme.css");
        fs::create_dir_all(target_file.parent().unwrap()).unwrap();
        fs::write(&target_file, b"/* original theme */").unwrap();

        fs::write(pkg_dir.join("theme.css"), b"/* catppuccin theme */").unwrap();
        let theme_hash = sha256_bytes(b"/* catppuccin theme */");

        let manifest = format!(
            r#"schema_version: '1.0.0'
package:
  id: com.test.theme
  name: Test Theme
  version: '2.0.0'
  type: theme
  category: test
  license: MIT
  description: test
artifacts:
  - path: theme.css
    sha256: {theme_hash}
install:
  files:
    - source: theme.css
      target: .config/app/theme.css
      backup_before_write: true
"#
        );
        fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };

        let txn = engine.install(&pkg_dir, "test").unwrap();
        assert_eq!(fs::read(&target_file).unwrap(), b"/* catppuccin theme */");

        // Verify installed_packages row exists
        let count: i64 = engine
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.test.theme'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Perform rollback
        let rollback_txn = engine.rollback(&txn, false).unwrap();

        // 1. Verify file restored to original
        assert_eq!(fs::read(&target_file).unwrap(), b"/* original theme */");

        // 2. Verify installed_packages row is DELETED
        let count_after: i64 = engine
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'com.test.theme'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_after, 0);

        // 3. Verify original transaction status is ROLLED_BACK
        let orig_status: String = engine
            .db
            .conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                [&txn],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orig_status, "ROLLED_BACK");

        // 4. Verify rollback transaction status is COMMITTED and type is ROLLBACK
        let (rb_status, rb_type): (String, String) = engine
            .db
            .conn
            .query_row(
                "SELECT status, txn_type FROM transactions WHERE txn_id = ?1",
                [&rollback_txn],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(rb_status, "COMMITTED");
        assert_eq!(rb_type, "ROLLBACK");

        let _ = fs::remove_dir_all(pkg_dir);
    }

    #[test]
    fn test_rollback_fails_fast_on_conflict_without_partial_restoration() {
        let pkg_dir = root("rollback-conflict");
        let home = pkg_dir.join("home");
        let data = pkg_dir.join("data");
        fs::create_dir_all(&home).unwrap();

        let f1 = home.join(".config/file1.txt");
        let f2 = home.join(".config/file2.txt");
        fs::create_dir_all(f1.parent().unwrap()).unwrap();
        fs::write(&f1, b"orig1").unwrap();
        fs::write(&f2, b"orig2").unwrap();

        fs::write(pkg_dir.join("p1"), b"installed1").unwrap();
        fs::write(pkg_dir.join("p2"), b"installed2").unwrap();
        let h1 = sha256_bytes(b"installed1");
        let h2 = sha256_bytes(b"installed2");

        let manifest = format!(
            r#"schema_version: '1.0.0'
package:
  id: com.test.conflict
  name: Conflict Test
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: test
artifacts:
  - path: p1
    sha256: {h1}
  - path: p2
    sha256: {h2}
install:
  files:
    - source: p1
      target: .config/file1.txt
      backup_before_write: true
    - source: p2
      target: .config/file2.txt
      backup_before_write: true
"#
        );
        fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };

        let txn = engine.install(&pkg_dir, "test").unwrap();
        assert_eq!(fs::read(&f1).unwrap(), b"installed1");
        assert_eq!(fs::read(&f2).unwrap(), b"installed2");

        // Mutate file 2 externally to create conflict
        fs::write(&f2, b"user_edited_file2").unwrap();

        // Attempt rollback with force = false -> MUST FAIL on conflict
        let res = engine.rollback(&txn, false);
        assert!(matches!(res, Err(WoderriseError::Conflict(_))));

        // Two-pass check: File 1 MUST NOT have been reverted (no partial disk mutation!)
        assert_eq!(fs::read(&f1).unwrap(), b"installed1");
        assert_eq!(fs::read(&f2).unwrap(), b"user_edited_file2");

        // Now force rollback -> MUST SUCCEED
        let rb_res = engine.rollback(&txn, true);
        assert!(rb_res.is_ok());
        assert_eq!(fs::read(&f1).unwrap(), b"orig1");
        assert_eq!(fs::read(&f2).unwrap(), b"orig2");

        let _ = fs::remove_dir_all(pkg_dir);
    }

    #[test]
    fn test_rollback_blocks_non_committed_or_already_rolled_back() {
        let pkg_dir = root("rollback-blocks");
        let home = pkg_dir.join("home");
        let data = pkg_dir.join("data");
        fs::create_dir_all(&home).unwrap();

        fs::write(pkg_dir.join("payload"), b"data").unwrap();
        let h = sha256_bytes(b"data");
        let manifest = format!(
            r#"schema_version: '1.0.0'
package:
  id: com.test.block
  name: Block
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: test
artifacts:
  - path: payload
    sha256: {h}
install:
  files:
    - source: payload
      target: .config/block.txt
"#
        );
        fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };

        // Non-existent transaction
        assert!(engine.rollback("TXN-NONEXISTENT", false).is_err());

        // Install and rollback once
        let txn = engine.install(&pkg_dir, "test").unwrap();
        assert!(engine.rollback(&txn, false).is_ok());

        // Second rollback attempt on already ROLLED_BACK txn must fail
        assert!(engine.rollback(&txn, false).is_err());

        let _ = fs::remove_dir_all(pkg_dir);
    }

    #[test]
    fn test_install_with_progress_captures_all_seven_stages_monotonically() {
        use std::sync::{Arc, Mutex};

        let pkg_dir = root("progress-test");
        let home = pkg_dir.join("home");
        let data = pkg_dir.join("data");
        fs::create_dir_all(&home).unwrap();

        let target_file = home.join(".config/progress.txt");
        fs::create_dir_all(target_file.parent().unwrap()).unwrap();
        fs::write(&target_file, b"existing content").unwrap();

        fs::write(pkg_dir.join("payload.txt"), b"new content").unwrap();
        let h = sha256_bytes(b"new content");
        let manifest = format!(
            r#"schema_version: '1.0.0'
package:
  id: com.test.progress
  name: Progress Test
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: test
artifacts:
  - path: payload.txt
    sha256: {h}
install:
  files:
    - source: payload.txt
      target: .config/progress.txt
      backup_before_write: true
"#
        );
        fs::write(pkg_dir.join("woderrise.yaml"), manifest).unwrap();

        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);

        let cb: ProgressCallback = Box::new(move |stage, msg, frac| {
            events_clone
                .lock()
                .unwrap()
                .push((stage, msg.to_string(), frac));
        });

        let txn = engine
            .install_with_progress(&pkg_dir, "tester", Some(cb))
            .unwrap();
        assert!(!txn.is_empty());

        let recorded = events.lock().unwrap().clone();
        assert!(!recorded.is_empty());

        let stages: Vec<InstallStage> = recorded.iter().map(|(s, _, _)| *s).collect();
        assert!(stages.contains(&InstallStage::Resolve));
        assert!(stages.contains(&InstallStage::Validate));
        assert!(stages.contains(&InstallStage::Plan));
        assert!(stages.contains(&InstallStage::Backup));
        assert!(stages.contains(&InstallStage::Execute));
        assert!(stages.contains(&InstallStage::Verify));
        assert!(stages.contains(&InstallStage::Commit));

        // Assert fractions are strictly monotonic and end at 1.0
        let fractions: Vec<f32> = recorded.iter().map(|(_, _, f)| *f).collect();
        for window in fractions.windows(2) {
            assert!(
                window[0] <= window[1],
                "Progress fractions not monotonic: {} > {}",
                window[0],
                window[1]
            );
        }
        assert_eq!(*fractions.last().unwrap(), 1.0);

        let _ = fs::remove_dir_all(pkg_dir);
    }

    #[test]
    fn test_recovery_recovers_planned_transactions_and_cleans_interrupted_state() {
        let pkg_dir = root("recovery-test");
        let data = pkg_dir.join("data");
        fs::create_dir_all(&data).unwrap();

        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: pkg_dir.join("home"),
        };

        let txn = engine
            .db
            .begin_transaction("interrupted.pkg", "1.0.0", "INSTALL", "recovery_test")
            .unwrap();
        // Insert orphan record in installed_packages as if interrupted during crash
        engine
            .db
            .conn
            .execute(
                "INSERT INTO installed_packages(package_id,active_version,package_type,installed_at,updated_at,is_explicit,ref_count,install_txn_id) VALUES(?1,'1.0.0','config',0,0,1,1,?2)",
                params!["interrupted.pkg", txn],
            )
            .unwrap();

        let recovered_count = engine.recover().unwrap();
        assert_eq!(recovered_count, 1);

        let status: String = engine
            .db
            .conn
            .query_row(
                "SELECT status FROM transactions WHERE txn_id = ?1",
                [&txn],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "ABORTED_RECOVERED");

        let pkg_count: i64 = engine
            .db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM installed_packages WHERE package_id = 'interrupted.pkg'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pkg_count, 0);

        let _ = fs::remove_dir_all(pkg_dir);
    }

    #[test]
    fn test_rollback_older_transaction_does_not_remove_active_upgraded_package() {
        use rusqlite::OptionalExtension;

        let pkg_dir1 = root("multi-v1");
        let pkg_dir2 = root("multi-v2");
        let home = pkg_dir1.join("home");
        let data = pkg_dir1.join("data");
        fs::create_dir_all(&home).unwrap();

        let target_file = home.join(".config/app/config.txt");
        fs::create_dir_all(target_file.parent().unwrap()).unwrap();

        // 1. Install v1.0.0
        fs::write(pkg_dir1.join("payload.txt"), b"v1 content").unwrap();
        let v1_hash = sha256_bytes(b"v1 content");
        fs::write(
            pkg_dir1.join("woderrise.yaml"),
            format!(
                r#"schema_version: '1.0.0'
package:
  id: com.test.multiversion
  name: Test Multi
  version: '1.0.0'
  type: config
  category: test
  license: MIT
  description: test
artifacts:
  - path: payload.txt
    sha256: {v1_hash}
install:
  files:
    - source: payload.txt
      target: .config/app/config.txt
      backup_before_write: true
"#
            ),
        ).unwrap();

        let db = Database::open(data.join("journal.db")).unwrap();
        let engine = InstallationEngine {
            db,
            backups: BackupManager::new(data.join("backups")).unwrap(),
            home: home.clone(),
        };

        let txn1 = engine.install(&pkg_dir1, "user").unwrap();

        // 2. Install / Upgrade to v2.0.0
        fs::write(pkg_dir2.join("payload.txt"), b"v2 content").unwrap();
        let v2_hash = sha256_bytes(b"v2 content");
        fs::write(
            pkg_dir2.join("woderrise.yaml"),
            format!(
                r#"schema_version: '1.0.0'
package:
  id: com.test.multiversion
  name: Test Multi
  version: '2.0.0'
  type: config
  category: test
  license: MIT
  description: test
artifacts:
  - path: payload.txt
    sha256: {v2_hash}
install:
  files:
    - source: payload.txt
      target: .config/app/config.txt
      backup_before_write: true
"#
            ),
        ).unwrap();

        let txn2 = engine.install(&pkg_dir2, "user").unwrap();

        // Verify active state in installed_packages is v2.0.0
        let (active_ver, active_txn): (String, String) = engine
            .db
            .conn
            .query_row(
                "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = 'com.test.multiversion'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(active_ver, "2.0.0");
        assert_eq!(active_txn, txn2);

        // 3. Rollback the older transaction (txn1) with force: true
        let _ = engine.rollback(&txn1, true).unwrap();

        // 4. Invariant: active package v2.0.0 must NOT be removed from installed_packages!
        let post_record: Option<(String, String)> = engine
            .db
            .conn
            .query_row(
                "SELECT active_version, install_txn_id FROM installed_packages WHERE package_id = 'com.test.multiversion'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap();
        assert_eq!(
            post_record,
            Some(("2.0.0".to_string(), txn2)),
            "Rolling back an older transaction must not remove the active upgraded package row"
        );

        let _ = fs::remove_dir_all(pkg_dir1);
        let _ = fs::remove_dir_all(pkg_dir2);
    }
}

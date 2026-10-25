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
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Other(String),
}
pub type Result<T> = std::result::Result<T, WoderriseError>;

pub mod catalog;
pub mod hyprland;
pub mod package_manager;
pub mod resolver;
pub mod sandbox;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub schema_version: String,
    pub package: PackageSpec,
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
        for p in self
            .artifacts
            .iter()
            .map(|a| &a.path)
            .chain(self.install.files.iter().map(|f| &f.source))
        {
            validate_relative(p)?;
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
    let r = fs::rename(&tmp, target);
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
        return Err(r.unwrap_err().into());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub struct InstallationEngine {
    pub db: Database,
    pub backups: BackupManager,
    pub home: PathBuf,
}
impl InstallationEngine {
    pub fn install(&self, package_dir: impl AsRef<Path>, initiator: &str) -> Result<String> {
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
        let txn = self.db.begin_transaction(
            &manifest.package.id,
            &manifest.package.version,
            "INSTALL",
            initiator,
        )?;
        let result = self.install_inner(base_dir, &manifest, &txn);
        match result {
            Ok(()) => {
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
    fn install_inner(&self, dir: &Path, m: &Manifest, txn: &str) -> Result<()> {
        self.db.set_status(txn, "PREPARED", None)?;
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
            let op = format!("OP-{}-{}", txn, n);
            self.db.conn.execute(
                "INSERT INTO operations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'PLANNED',NULL)",
                params![
                    op,
                    txn,
                    n as i64 + 1,
                    if pre.is_some() {
                        "MODIFY_FILE"
                    } else {
                        "CREATE_FILE"
                    },
                    target.to_string_lossy(),
                    pre,
                    post,
                    backup
                ],
            )?;
            self.db.set_status(txn, "EXECUTING", None)?;
            atomic_replace(
                &src,
                &target,
                parse_mode(f.mode.as_deref().unwrap_or("0644"))?,
            )?;
            self.db.conn.execute(
                "UPDATE operations SET status='COMPLETED' WHERE op_id=?1",
                params![op],
            )?;
        }
        self.db.set_status(txn, "EXECUTING", None)?;
        Ok(())
    }
    fn compensate(&self, txn: &str) -> Result<()> {
        let mut st=self.db.conn.prepare("SELECT target_path,pre_sha256,backup_id FROM operations WHERE txn_id=?1 AND status='COMPLETED' ORDER BY sequence_num DESC")?;
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
        let new = self
            .db
            .begin_transaction("rollback", txn, "ROLLBACK", "user_cli")?;
        let mut st=self.db.conn.prepare("SELECT target_path,pre_sha256,post_sha256,backup_id FROM operations WHERE txn_id=?1 ORDER BY sequence_num DESC")?;
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
        for (path, pre, post, bid) in rows {
            if !force && sha256_file(&path).ok().as_deref() != Some(&post) {
                return Err(WoderriseError::Conflict(path));
            }
            if let Some(id) = bid {
                let storage: String = self.db.conn.query_row(
                    "SELECT storage_path FROM backup_snapshots WHERE backup_id=?1",
                    params![id],
                    |r| r.get(0),
                )?;
                atomic_replace(Path::new(&storage), Path::new(&path), 0o644)?;
            } else if pre.is_none() {
                fs::remove_file(&path)?;
            }
        }
        self.db.set_status(&new, "ROLLED_BACK", None)?;
        self.db.set_status(txn, "ROLLED_BACK", None)?;
        Ok(new)
    }
    pub fn recover(&self) -> Result<usize> {
        let mut st = self.db.conn.prepare(
            "SELECT txn_id FROM transactions WHERE status IN ('PREPARED','EXECUTING','FAILED')",
        )?;
        let ids = st
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let n = ids.len();
        for id in ids {
            self.compensate(&id).ok();
            self.db
                .set_status(&id, "ABORTED_RECOVERED", Some("startup recovery"))?;
        }
        Ok(n)
    }
}
fn expand_target(target: &str, home: &Path) -> Result<PathBuf> {
    if target == "~" {
        return Ok(home.to_path_buf());
    }
    if let Some(rest) = target.strip_prefix("~/") {
        return Ok(home.join(rest));
    }
    if Path::new(target).is_absolute() {
        return Err(WoderriseError::UnsafePath(target.into()));
    }
    Ok(home.join(target))
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
        let p = std::env::current_dir()
            .unwrap()
            .join(format!(".woderrise-test-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn parses_and_rejects_manifest() {
        let y="schema_version: '1.0.0'\npackage:\n  id: x\n  name: X\n  version: '1.0.0'\n  type: config\n  category: c\n  license: MIT\n  description: d\ninstall:\n  files: []";
        assert!(Manifest::parse(y).is_ok());
        assert!(
            Manifest::parse(&y.replace("schema_version: '1.0.0'", "schema_version: '2.0.0'"))
                .is_err()
        );
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
        fs::write(
            package.join("fixture.yaml"),
            "schema_version: '1.0.0'\npackage:\n  id: fixture.install\n  name: Install Fixture\n  version: '1.0.0'\n  type: config\n  category: test\n  license: MIT\n  description: test\ninstall:\n  files:\n    - source: payload\n      target: .config/woderrise-test\n",
        )
        .unwrap();
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
}

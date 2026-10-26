use crate::{sha256_file, Database, Manifest, Result, WoderriseError};
use rusqlite::params;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub package_type: String,
    pub category: String,
    pub tags: Vec<String>,
    pub license: String,
    pub repository: Option<String>,
    pub manifest_path: PathBuf,
    pub manifest_sha256: String,
    pub validated: bool,
    pub yanked: bool,
}

pub fn migrate(db: &Database) -> Result<()> {
    db.conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS catalog(
            id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            version TEXT NOT NULL,
            package_type TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL,
            tags TEXT NOT NULL DEFAULT '',
            license TEXT NOT NULL DEFAULT '',
            repository TEXT,
            manifest_path TEXT NOT NULL DEFAULT '',
            manifest_sha256 TEXT NOT NULL DEFAULT '',
            validated INTEGER NOT NULL DEFAULT 0,
            yanked INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            PRIMARY KEY(id, version)
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS catalog_fts USING fts5(
            id UNINDEXED, version UNINDEXED, name, description, category, tags, content=''
        );",
    )?;
    let has_extended_schema: bool = db.conn.query_row(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('catalog') WHERE name='package_type'",
        [],
        |row| row.get(0),
    )?;
    if !has_extended_schema {
        db.conn.execute_batch(
            "DROP TABLE IF EXISTS catalog_fts;
             ALTER TABLE catalog RENAME TO catalog_legacy;
             CREATE TABLE catalog(
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                version TEXT NOT NULL,
                package_type TEXT NOT NULL DEFAULT '',
                category TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '',
                license TEXT NOT NULL DEFAULT '',
                repository TEXT,
                manifest_path TEXT NOT NULL DEFAULT '',
                manifest_sha256 TEXT NOT NULL DEFAULT '',
                validated INTEGER NOT NULL DEFAULT 0,
                yanked INTEGER NOT NULL DEFAULT 0,
                error TEXT,
                PRIMARY KEY(id, version)
             );
             INSERT INTO catalog(id,name,description,version,category,validated)
             SELECT id,name,description,version,category,0 FROM catalog_legacy;
             DROP TABLE catalog_legacy;
             CREATE VIRTUAL TABLE catalog_fts USING fts5(
                id UNINDEXED, version UNINDEXED, name, description, category, tags, content=''
             );",
        )?;
    }
    Ok(())
}

pub fn import_registry(db: &Database, root: impl AsRef<Path>) -> Result<usize> {
    migrate(db)?;
    let mut entries = Vec::new();
    collect_manifests(root.as_ref(), &mut entries)?;
    if entries.is_empty() {
        return Err(WoderriseError::Registry(format!(
            "no installable manifests found under {}",
            root.as_ref().display()
        )));
    }

    let tx = db.conn.unchecked_transaction()?;
    tx.execute("DELETE FROM catalog", [])?;
    tx.execute(
        "INSERT INTO catalog_fts(catalog_fts) VALUES('delete-all')",
        [],
    )?;
    for path in &entries {
        let entry = load_entry(path)?;
        if entry.yanked {
            continue;
        }
        tx.execute(
            "INSERT INTO catalog(
                id,name,description,version,package_type,category,tags,license,
                repository,manifest_path,manifest_sha256,validated,yanked,error
            ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,?12,NULL)",
            params![
                entry.id,
                entry.name,
                entry.description,
                entry.version,
                entry.package_type,
                entry.category,
                entry.tags.join("\u{1f}"),
                entry.license,
                entry.repository,
                entry.manifest_path.to_string_lossy(),
                entry.manifest_sha256,
                entry.yanked
            ],
        )?;
        tx.execute(
            "INSERT INTO catalog_fts(rowid,id,version,name,description,category,tags)
             SELECT rowid,id,version,name,description,category,tags
             FROM catalog WHERE id=?1 AND version=?2",
            params![entry.id, entry.version],
        )?;
    }
    tx.commit()?;
    Ok(entries.len())
}

/// Synchronize a configured GitHub registry into a local cache.
///
/// The registry remains an ordinary versioned directory tree; importing it is
/// still performed by `import_registry`, which validates manifests and bytes
/// before replacing the SQLite index.
pub fn sync_git_registry(source: &str, cache_dir: impl AsRef<Path>) -> Result<PathBuf> {
    if !(source.starts_with("https://github.com/") && !source.contains(char::is_whitespace)) {
        return Err(WoderriseError::Registry(
            "registry source must be an HTTPS GitHub repository URL".into(),
        ));
    }
    let cache_dir = cache_dir.as_ref();
    if cache_dir.exists() && !cache_dir.join(".git").is_dir() {
        return Err(WoderriseError::Registry(format!(
            "registry cache is not a Git repository: {}",
            cache_dir.display()
        )));
    }
    if let Some(parent) = cache_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = cache_dir.join(".git").is_dir();
    let cache_string = cache_dir.to_string_lossy().into_owned();
    let mut command = if existing {
        let mut command = Command::new("git");
        command.args(["-C", &cache_string, "fetch", "--depth", "1", "origin"]);
        command
    } else {
        let mut command = Command::new("git");
        command.args([
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            source,
            &cache_string,
        ]);
        command
    };
    run_bounded(&mut command, Duration::from_secs(30))?;
    if existing {
        let mut reset = Command::new("git");
        reset.args(["-C", &cache_string, "reset", "--hard", "FETCH_HEAD"]);
        run_bounded(&mut reset, Duration::from_secs(10))?;
    }
    let packages = cache_dir.join("packages");
    Ok(if packages.is_dir() {
        packages
    } else {
        cache_dir.to_path_buf()
    })
}

fn run_bounded(command: &mut Command, timeout: Duration) -> Result<()> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| WoderriseError::Registry(format!("could not start git: {error}")))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let output = child.wait_with_output()?;
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(WoderriseError::Registry(if detail.is_empty() {
                format!("git exited with {status}")
            } else {
                detail
            }));
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            let _ = child.wait();
            return Err(WoderriseError::Registry(format!(
                "git operation timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn upsert(db: &Database, e: &CatalogEntry) -> Result<()> {
    migrate(db)?;
    db.conn.execute(
        "INSERT OR REPLACE INTO catalog(
            id,name,description,version,package_type,category,tags,license,repository,
            manifest_path,manifest_sha256,validated,yanked,error
        ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,NULL)",
        params![
            e.id,
            e.name,
            e.description,
            e.version,
            e.package_type,
            e.category,
            e.tags.join("\u{1f}"),
            e.license,
            e.repository,
            e.manifest_path.to_string_lossy(),
            e.manifest_sha256,
            e.validated,
            e.yanked
        ],
    )?;
    rebuild_fts(db)?;
    Ok(())
}

pub fn search(db: &Database, query: &str, limit: usize) -> Result<Vec<CatalogEntry>> {
    migrate(db)?;
    let pattern = query.trim();
    let sql = if pattern.is_empty() {
        "SELECT id,version FROM catalog WHERE validated=1 AND yanked=0 ORDER BY name LIMIT ?1"
    } else {
        "SELECT c.id,c.version FROM catalog c
         JOIN catalog_fts f ON f.rowid=c.rowid
         WHERE catalog_fts MATCH ?1 AND c.validated=1 AND c.yanked=0 LIMIT ?2"
    };
    let mut statement = db.conn.prepare(sql)?;
    let ids = if pattern.is_empty() {
        statement
            .query_map(params![limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        statement
            .query_map(params![fts_query(pattern), limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    ids.into_iter()
        .map(|(id, version)| get(db, &id, &version))
        .collect()
}

pub fn get(db: &Database, id: &str, version: &str) -> Result<CatalogEntry> {
    migrate(db)?;
    db.conn
        .query_row(
            "SELECT id,name,description,version,package_type,category,tags,license,
                    repository,manifest_path,manifest_sha256,validated,yanked
             FROM catalog WHERE id=?1 AND version=?2",
            params![id, version],
            |row| {
                let tags: String = row.get(6)?;
                Ok(CatalogEntry {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    version: row.get(3)?,
                    package_type: row.get(4)?,
                    category: row.get(5)?,
                    tags: if tags.is_empty() {
                        Vec::new()
                    } else {
                        tags.split('\u{1f}').map(str::to_string).collect()
                    },
                    license: row.get(7)?,
                    repository: row.get(8)?,
                    manifest_path: PathBuf::from(row.get::<_, String>(9)?),
                    manifest_sha256: row.get(10)?,
                    validated: row.get::<_, i64>(11)? != 0,
                    yanked: row.get::<_, i64>(12)? != 0,
                })
            },
        )
        .map_err(Into::into)
}

pub fn manifest(db: &Database, id: &str, version: &str) -> Result<Manifest> {
    let entry = get(db, id, version)?;
    if !entry.validated || entry.yanked {
        return Err(WoderriseError::Registry(format!(
            "package {id}@{version} is not installable"
        )));
    }
    let actual = sha256_file(&entry.manifest_path)?;
    if actual != entry.manifest_sha256 {
        return Err(WoderriseError::Registry(format!(
            "manifest changed after import: {}",
            entry.manifest_path.display()
        )));
    }
    let manifest = Manifest::from_file(&entry.manifest_path)?;
    verify_artifacts(
        &manifest,
        entry.manifest_path.parent().unwrap_or(Path::new(".")),
    )?;
    Ok(manifest)
}

pub fn verify_artifacts(manifest: &Manifest, root: &Path) -> Result<()> {
    if !manifest.install.config_injections.is_empty()
        || !manifest.install.service_triggers.is_empty()
    {
        return Err(WoderriseError::Registry(
            "manifest uses unsupported config injections or service triggers".into(),
        ));
    }
    for artifact in &manifest.artifacts {
        let path = root.join(&artifact.path);
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
        if !root.join(&file.source).is_file() {
            return Err(WoderriseError::Registry(format!(
                "install source is missing: {}",
                file.source
            )));
        }
    }
    if let Some(previews) = &manifest.previews {
        for path in std::iter::once(&previews.thumbnail).chain(
            previews
                .screenshots
                .iter()
                .map(|screenshot| &screenshot.path),
        ) {
            if !root.join(path).is_file() {
                return Err(WoderriseError::Registry(format!(
                    "preview asset is missing: {path}"
                )));
            }
        }
    }
    Ok(())
}

fn load_entry(path: &Path) -> Result<CatalogEntry> {
    let manifest = Manifest::from_file(path)?;
    verify_artifacts(&manifest, path.parent().unwrap_or(Path::new(".")))?;
    let manifest_sha256 = sha256_file(path)?;
    Ok(CatalogEntry {
        id: manifest.package.id.clone(),
        name: manifest.package.name.clone(),
        description: manifest.package.description.clone(),
        version: manifest.package.version.clone(),
        package_type: manifest.package.package_type.clone(),
        category: manifest.package.category.clone(),
        tags: manifest.package.tags.clone(),
        license: manifest.package.license.clone(),
        repository: manifest.package.repository.clone(),
        manifest_path: path.to_path_buf(),
        manifest_sha256,
        validated: true,
        yanked: manifest.yanked,
    })
}

fn collect_manifests(root: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !root.is_dir() {
        return Err(WoderriseError::Registry(format!(
            "registry root is not a directory: {}",
            root.display()
        )));
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_manifests(&path, output)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("woderrise.yaml") {
            output.push(path);
        }
    }
    output.sort();
    Ok(())
}

fn rebuild_fts(db: &Database) -> Result<()> {
    db.conn.execute(
        "INSERT INTO catalog_fts(catalog_fts) VALUES('delete-all')",
        [],
    )?;
    db.conn.execute(
        "INSERT INTO catalog_fts(rowid,id,version,name,description,category,tags)
         SELECT rowid,id,version,name,description,category,tags FROM catalog",
        [],
    )?;
    Ok(())
}

fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|word| format!("\"{}\"*", word.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(root: &Path, id: &str, version: &str, body: &[u8]) -> PathBuf {
        let dir = root.join(id).join(version);
        fs::create_dir_all(dir.join("artifacts")).unwrap();
        let artifact = dir.join("artifacts/file.txt");
        fs::write(&artifact, body).unwrap();
        let hash = crate::sha256_bytes(body);
        let manifest = format!(
            "schema_version: '1.0.0'\npackage:\n  id: {id}\n  name: Test {id}\n  version: '{version}'\n  type: config\n  category: test\n  license: MIT\n  description: test package\n  tags: [test]\nartifacts:\n  - path: artifacts/file.txt\n    sha256: {hash}\ninstall:\n  files:\n    - source: artifacts/file.txt\n      target: ~/.config/test/file.txt\n"
        );
        let path = dir.join("woderrise.yaml");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(manifest.as_bytes()).unwrap();
        path
    }

    #[test]
    fn refresh_indexes_manifest_and_resolves_exact_version() {
        let root = std::env::temp_dir().join(format!("woderrise-catalog-{}", uuid::Uuid::new_v4()));
        let path = package(&root, "org.test.one", "1.0.0", b"one");
        let db = Database::in_memory().unwrap();
        assert_eq!(import_registry(&db, &root).unwrap(), 1);
        let entry = get(&db, "org.test.one", "1.0.0").unwrap();
        assert_eq!(entry.manifest_path, path);
        assert_eq!(
            manifest(&db, "org.test.one", "1.0.0")
                .unwrap()
                .package
                .version,
            "1.0.0"
        );
        assert_eq!(search(&db, "test", 10).unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_refresh_preserves_last_known_good_catalog() {
        let root = std::env::temp_dir().join(format!("woderrise-catalog-{}", uuid::Uuid::new_v4()));
        package(&root, "org.test.one", "1.0.0", b"one");
        let db = Database::in_memory().unwrap();
        import_registry(&db, &root).unwrap();
        let invalid = root.join("org.test.bad/1.0.0");
        fs::create_dir_all(&invalid).unwrap();
        fs::write(invalid.join("woderrise.yaml"), "not: a manifest").unwrap();
        assert!(import_registry(&db, &root).is_err());
        assert_eq!(search(&db, "", 10).unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_artifact_hash_mismatch() {
        let root = std::env::temp_dir().join(format!("woderrise-catalog-{}", uuid::Uuid::new_v4()));
        let path = package(&root, "org.test.one", "1.0.0", b"one");
        let mut content = fs::read_to_string(&path).unwrap();
        content = content.replace(&crate::sha256_bytes(b"one"), &crate::sha256_bytes(b"other"));
        fs::write(&path, content).unwrap();
        let db = Database::in_memory().unwrap();
        assert!(import_registry(&db, &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_untrusted_registry_sources_before_network_access() {
        let cache =
            std::env::temp_dir().join(format!("woderrise-registry-{}", uuid::Uuid::new_v4()));
        let error = sync_git_registry("http://example.invalid/registry", &cache).unwrap_err();
        assert!(error.to_string().contains("HTTPS GitHub"));
        assert!(!cache.exists());
    }

    #[test]
    fn rejects_duplicate_package_versions_atomically() {
        let root = std::env::temp_dir().join(format!("woderrise-catalog-{}", uuid::Uuid::new_v4()));
        package(&root, "org.test.one", "1.0.0", b"one");
        package(&root.join("duplicate"), "org.test.one", "1.0.0", b"two");
        let db = Database::in_memory().unwrap();
        assert!(import_registry(&db, &root).is_err());
        assert!(search(&db, "", 10).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn genuine_package_installs_journals_and_rolls_back_in_isolated_home() {
        let registry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/packages");
        let package = registry.join("com.catppuccin.waybar/1.0.0");
        let manifest_path = package.join("woderrise.yaml");
        let artifact_path = package.join("artifacts/mocha.css");
        let original = b"original user configuration\n";
        let temp = std::env::temp_dir().join(format!("woderrise-e2e-{}", uuid::Uuid::new_v4()));
        let home = temp.join("home");
        let target = home.join(".config/waybar/catppuccin-mocha.css");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, original).unwrap();

        let db = Database::open(temp.join("journal.db")).unwrap();
        assert!(import_registry(&db, &registry).unwrap() >= 2);
        let record = get(&db, "com.catppuccin.waybar", "1.0.0").unwrap();
        assert_eq!(record.manifest_path, manifest_path);
        let manifest = manifest(&db, "com.catppuccin.waybar", "1.0.0").unwrap();
        verify_artifacts(&manifest, package.as_path()).unwrap();
        assert_eq!(
            sha256_file(&artifact_path).unwrap(),
            "a6e30597ccc828e898844eb0cea9df7d73879ff5672a6a550f3acc24c93edd17"
        );

        let engine = crate::InstallationEngine {
            db,
            backups: crate::BackupManager::new(temp.join("backups")).unwrap(),
            home: home.clone(),
        };
        let transaction = engine.install(&manifest_path, "acceptance-test").unwrap();
        assert_eq!(
            fs::read(&target).unwrap(),
            fs::read(&artifact_path).unwrap()
        );
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT status FROM transactions WHERE txn_id=?1",
                    [&transaction],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "COMMITTED"
        );
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM operations WHERE txn_id=?1 AND status='COMPLETED'",
                    [&transaction],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM backup_snapshots WHERE txn_id=?1",
                    [&transaction],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );

        let rollback = engine.rollback(&transaction, false).unwrap();
        assert_eq!(fs::read(&target).unwrap(), original);
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT status FROM transactions WHERE txn_id=?1",
                    [&transaction],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "ROLLED_BACK"
        );
        assert_eq!(
            engine
                .db
                .conn
                .query_row(
                    "SELECT txn_type FROM transactions WHERE txn_id=?1",
                    [&rollback],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "ROLLBACK"
        );
        drop(engine);
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn reimport_and_upsert_on_contentless_fts5_succeeds() {
        let root = std::env::temp_dir().join(format!("woderrise-catalog-{}", uuid::Uuid::new_v4()));
        package(&root, "org.test.one", "1.0.0", b"one");
        let db = Database::in_memory().unwrap();
        assert_eq!(import_registry(&db, &root).unwrap(), 1);

        // Re-import on non-empty catalog_fts must succeed via delete-all
        assert_eq!(import_registry(&db, &root).unwrap(), 1);

        // Upsert on non-empty catalog_fts must rebuild FTS cleanly without error
        let mut entry = get(&db, "org.test.one", "1.0.0").unwrap();
        entry.description = "updated description".to_string();
        assert!(upsert(&db, &entry).is_ok());
        assert_eq!(search(&db, "updated", 10).unwrap().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }
}

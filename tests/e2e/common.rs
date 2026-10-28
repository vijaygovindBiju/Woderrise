use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static COUNTER: AtomicU64 = AtomicU64::new(1);

pub const REAL_CATPPUCCIN_DIR: &str = "registry/packages/com.catppuccin.waybar/1.0.0";
pub const REAL_CATPPUCCIN_ID: &str = "com.catppuccin.waybar";
pub const REAL_CATPPUCCIN_VER: &str = "1.0.0";
pub const WAYBAR_TARGET_REL: &str = ".config/waybar/catppuccin-mocha.css";

/// RAII Temporary Directory Guard ensuring automatic cleanup on drop.
pub struct TempDirGuard(pub PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Deref for TempDirGuard {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

pub struct TestEnvironment {
    pub guard: TempDirGuard,
    pub home: PathBuf,
    pub data: PathBuf,
}

impl TestEnvironment {
    pub fn new(prefix: &str) -> Self {
        let seq = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("woderrise-e2e-{prefix}-{nonce}-{seq}"));
        let home = root.join("home");
        let data = root.join("data");
        fs::create_dir_all(&home).expect("failed to create temp home");
        fs::create_dir_all(&data).expect("failed to create temp data");
        fs::create_dir_all(home.join(".config")).expect("failed to create temp .config");
        fs::create_dir_all(home.join(".cache")).expect("failed to create temp .cache");
        fs::create_dir_all(home.join(".local/state")).expect("failed to create temp .local/state");
        Self {
            guard: TempDirGuard(root),
            home,
            data,
        }
    }

    pub fn workspace_root() -> PathBuf {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if manifest_dir.join("Cargo.toml").is_file() {
            if let Ok(content) = fs::read_to_string(manifest_dir.join("Cargo.toml")) {
                if content.contains("[workspace]") {
                    return manifest_dir.to_path_buf();
                }
            }
        }
        let mut curr = manifest_dir;
        while let Some(parent) = curr.parent() {
            if parent.join("Cargo.toml").is_file() {
                if let Ok(content) = fs::read_to_string(parent.join("Cargo.toml")) {
                    if content.contains("[workspace]") {
                        return parent.to_path_buf();
                    }
                }
            }
            curr = parent;
        }
        manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(manifest_dir)
            .to_path_buf()
    }

    pub fn run(&self, args: &[&str]) -> std::process::Output {
        let bin_path = env!("CARGO_BIN_EXE_woderrise");
        let ws = Self::workspace_root();
        let reg_dir = ws.join("registry/packages");
        Command::new(bin_path)
            .current_dir(&ws)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("HOME", &self.home)
            .env("WODERRISE_DATA_DIR", &self.data)
            .env("WODERRISE_REGISTRY_DIR", &reg_dir)
            .arg("--data-dir")
            .arg(&self.data)
            .args(args)
            .output()
            .expect("failed to execute woderrise binary")
    }

    pub fn run_auth(&self, args: &[&str]) -> std::process::Output {
        let bin_path = env!("CARGO_BIN_EXE_woderrise");
        let ws = Self::workspace_root();
        Command::new(bin_path)
            .current_dir(&ws)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("HOME", &self.home)
            .arg("auth")
            .args(args)
            .output()
            .expect("failed to execute woderrise auth binary")
    }

    pub fn run_cli(&self, args: &[&str]) -> std::process::Output {
        let bin_path = env!("CARGO_BIN_EXE_woderrise");
        let ws = Self::workspace_root();
        Command::new(bin_path)
            .current_dir(&ws)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("HOME", &self.home)
            .args(args)
            .output()
            .expect("failed to execute woderrise binary")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.home.join(".config/woderrise/settings.json")
    }

    pub fn target_file(&self) -> PathBuf {
        self.home.join(WAYBAR_TARGET_REL)
    }

    pub fn db_path(&self) -> PathBuf {
        self.data.join("journal.db")
    }

    pub fn open_db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.db_path()).expect("failed to open test journal.db")
    }
}

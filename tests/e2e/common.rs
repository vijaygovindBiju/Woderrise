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
        Self {
            guard: TempDirGuard(root),
            home,
            data,
        }
    }

    pub fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }

    pub fn run(&self, args: &[&str]) -> std::process::Output {
        let bin_path = env!("CARGO_BIN_EXE_woderrise");
        let ws = Self::workspace_root();
        let reg_dir = ws.join("registry/packages");
        Command::new(bin_path)
            .current_dir(&ws)
            .env("HOME", &self.home)
            .env("WODERRISE_DATA_DIR", &self.data)
            .env("WODERRISE_REGISTRY_DIR", &reg_dir)
            .arg("--data-dir")
            .arg(&self.data)
            .args(args)
            .output()
            .expect("failed to execute woderrise binary")
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

use camino::Utf8PathBuf;
use tempfile::TempDir;
use woderrise_core::paths::XdgPaths;

#[allow(dead_code)]
pub struct TestEnv {
    pub _temp_dir: TempDir,
    pub base_path: Utf8PathBuf,
    pub xdg: XdgPaths,
}

impl TestEnv {
    pub fn new() -> Self {
        let temp = TempDir::new().expect("Failed to create temporary test directory");
        let base_path = Utf8PathBuf::from_path_buf(temp.path().to_path_buf())
            .expect("Valid UTF-8 path for test env");
        let xdg = XdgPaths::custom(&base_path);
        Self {
            _temp_dir: temp,
            base_path,
            xdg,
        }
    }
}

//! D-Bus contract. The transport is intentionally separate so headless builds
//! can test the same typed API without requiring a running system bus.
use std::sync::{Arc, Mutex};
use woderrise_core::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub transaction: String,
    pub stage: String,
    pub percent: u8,
    pub message: String,
}
pub trait InstallerApi: Send + Sync {
    fn install(&self, manifest: String) -> Result<String>;
    fn rollback(&self, transaction: String) -> Result<String>;
}
#[derive(Clone, Default)]
pub struct Service<A> {
    api: Arc<A>,
    progress: Arc<Mutex<Vec<Progress>>>,
}
impl<A: InstallerApi> Service<A> {
    pub fn new(api: A) -> Self {
        Self {
            api: Arc::new(api),
            progress: Default::default(),
        }
    }
    pub async fn install(&self, manifest: String) -> Result<String> {
        self.api.install(manifest)
    }
    pub async fn rollback(&self, transaction: String) -> Result<String> {
        self.api.rollback(transaction)
    }
    pub fn emit_progress(&self, progress: Progress) {
        self.progress.lock().unwrap().push(progress);
    }
    pub fn progress(&self) -> Vec<Progress> {
        self.progress.lock().unwrap().clone()
    }
}

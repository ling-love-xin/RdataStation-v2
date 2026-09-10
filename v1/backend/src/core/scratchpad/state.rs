use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::error::CoreError;
use crate::core::scratchpad::ScratchpadStore;

pub struct ScratchpadState {
    pub store: Arc<Mutex<Option<ScratchpadStore>>>,
    pub watcher_active: Arc<AtomicBool>,
}

impl ScratchpadState {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
            watcher_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn init(&self, project_path: PathBuf) -> Result<(), CoreError> {
        let store = ScratchpadStore::new(project_path);
        {
            let mut guard = self.store.lock().await;
            *guard = Some(store.clone());
        }
        if let Err(e) = store.ensure_dir().await {
            tracing::warn!(
                "[Scratchpad] ensure_dir failed during init (will retry on first write): {}",
                e
            );
        }
        Ok(())
    }

    pub fn is_watching(&self) -> bool {
        self.watcher_active.load(Ordering::Relaxed)
    }

    pub fn set_watching(&self, active: bool) {
        self.watcher_active.store(active, Ordering::Relaxed);
    }
}

impl Drop for ScratchpadState {
    fn drop(&mut self) {
        self.set_watching(false);
    }
}

impl Default for ScratchpadState {
    fn default() -> Self {
        Self::new()
    }
}

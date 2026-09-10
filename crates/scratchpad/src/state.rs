use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::ScratchpadStore;
use shared::error::CoreError;

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
        // v2 语义：`project_path` 即项目根（草稿箱根）；内部元数据落 `.RSmeta/scratchpad`。
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

    /// 当前草稿箱存储句柄（未初始化时为 `None`）。
    pub async fn store(&self) -> Option<ScratchpadStore> {
        self.store.lock().await.clone()
    }

    /// 当前项目根目录（即草稿箱根，未初始化时为 `None`）。
    pub async fn project_root(&self) -> Option<PathBuf> {
        self.store
            .lock()
            .await
            .as_ref()
            .map(|s| s.scratchpad_dir().to_path_buf())
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

/// Tauri 状态管理模块
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use tokio_util::sync::CancellationToken;

use crate::core::dbi::engine::duckdb_engine::DuckDBEngine;
use crate::core::ConnectionManager;

/// 预热进度状态
#[derive(Debug, Clone)]
pub struct WarmingProgressState {
    pub is_warming: bool,
    pub current_step: String,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub progress_percentage: f64,
    pub current_database: Option<String>,
    pub current_schema: Option<String>,
    pub current_table: Option<String>,
}

/// 预热任务状态
pub struct WarmingTask {
    pub cancel_token: CancellationToken,
    pub progress: Mutex<WarmingProgressState>,
}

/// 预热任务管理器
pub struct WarmingTaskManager {
    tasks: RwLock<HashMap<String, Arc<WarmingTask>>>,
}

impl Default for WarmingTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WarmingTaskManager {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
        }
    }

    /// 创建预热任务
    pub fn create_task(&self, connection_id: &str) -> Arc<WarmingTask> {
        let task = Arc::new(WarmingTask {
            cancel_token: CancellationToken::new(),
            progress: Mutex::new(WarmingProgressState {
                is_warming: true,
                current_step: "初始化".to_string(),
                total_steps: 0,
                completed_steps: 0,
                progress_percentage: 0.0,
                current_database: None,
                current_schema: None,
                current_table: None,
            }),
        });
        match self.tasks.write() {
            Ok(mut map) => {
                map.insert(connection_id.to_string(), Arc::clone(&task));
            }
            Err(e) => {
                tracing::error!("Failed to acquire write lock for warming tasks: {}", e);
            }
        }
        task
    }

    /// 获取预热任务
    pub fn get_task(&self, connection_id: &str) -> Option<Arc<WarmingTask>> {
        self.tasks
            .read()
            .map_err(|e| tracing::error!("Failed to acquire read lock for warming tasks: {}", e))
            .ok()
            .and_then(|map| map.get(connection_id).cloned())
    }

    /// 取消预热任务
    pub fn cancel_task(&self, connection_id: &str) -> bool {
        match self.tasks.write() {
            Ok(mut map) => {
                if let Some(task) = map.remove(connection_id) {
                    task.cancel_token.cancel();
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to acquire write lock for cancel warming task: {}",
                    e
                );
                false
            }
        }
    }

    /// 更新预热进度
    pub fn update_progress(&self, connection_id: &str, progress: WarmingProgressState) {
        let task = match self.tasks.read() {
            Ok(map) => map.get(connection_id).cloned(),
            Err(e) => {
                tracing::error!(
                    "Failed to acquire read lock for update warming progress: {}",
                    e
                );
                return;
            }
        };
        if let Some(task) = task {
            match task.progress.lock() {
                Ok(mut p) => *p = progress,
                Err(e) => {
                    tracing::error!("Failed to lock warming progress mutex: {}", e);
                }
            }
        }
    }

    /// 完成预热任务
    pub fn complete_task(&self, connection_id: &str) {
        match self.tasks.write() {
            Ok(mut map) => {
                map.remove(connection_id);
            }
            Err(e) => {
                tracing::error!(
                    "Failed to acquire write lock for complete warming task: {}",
                    e
                );
            }
        }
    }
}

/// 应用状态
pub struct AppState {
    /// 连接管理器
    pub connection_manager: Arc<ConnectionManager>,
    /// 预热任务管理器
    pub warming_task_manager: Arc<WarmingTaskManager>,
    /// DuckDB 加速引擎（联邦查询）
    pub duckdb_engine: Arc<tokio::sync::Mutex<DuckDBEngine>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            connection_manager,
            warming_task_manager: Arc::new(WarmingTaskManager::new()),
            duckdb_engine: Arc::new(tokio::sync::Mutex::new(DuckDBEngine::new())),
        }
    }
}

/// 状态扩展trait
#[allow(dead_code)]
trait StateExt {
    /// 获取连接管理器
    fn connection_manager(&self) -> &Arc<ConnectionManager>;
}

/// 为tauri::State实现扩展trait
impl<'a> StateExt for tauri::State<'a, AppState> {
    /// 获取连接管理器
    fn connection_manager(&self) -> &Arc<ConnectionManager> {
        &self.inner().connection_manager
    }
}

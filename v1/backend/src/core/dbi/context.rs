/**
 * 查询上下文模块
 *
 * 传递查询执行所需的上下文信息，包括：
 * - 连接信息
 * - 执行配置
 * - 权限控制
 */
use crate::core::dbi::engine::ExecutionMode;

/// 查询上下文
///
/// 包含执行查询所需的所有上下文信息
#[derive(Debug, Clone)]
pub struct QueryContext {
    /// 连接 ID
    pub connection_id: Option<String>,
    /// 执行模式
    pub mode: ExecutionMode,
    /// 是否只读
    pub read_only: bool,
    /// 超时时间（毫秒）
    pub timeout_ms: Option<u64>,
    /// 结果集限制
    pub limit: Option<usize>,
}

impl QueryContext {
    /// 创建新的查询上下文
    pub fn new(connection_id: Option<String>, mode: ExecutionMode) -> Self {
        Self {
            connection_id,
            mode,
            read_only: false,
            timeout_ms: None,
            limit: None,
        }
    }

    /// 设置为只读模式
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// 设置超时时间
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// 设置结果集限制
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// 检查是否是 DuckDB 加速模式
    pub fn is_duckdb_accelerated(&self) -> bool {
        matches!(self.mode, ExecutionMode::DuckDB)
    }
}

/// 执行上下文
///
/// 包含执行过程中的运行时信息
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// 查询开始时间
    pub start_time: std::time::Instant,
    /// 当前步骤
    pub current_step: String,
    /// 是否已取消
    pub cancelled: bool,
}

impl ExecutionContext {
    /// 创建新的执行上下文
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
            current_step: "initializing".to_string(),
            cancelled: false,
        }
    }

    /// 更新当前步骤
    pub fn set_step(&mut self, step: String) {
        self.current_step = step;
    }

    /// 取消执行
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// 获取执行耗时（毫秒）
    pub fn elapsed_ms(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64() * 1000.0
    }
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

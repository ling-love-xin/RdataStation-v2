//! 洞察设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

/// - `status_usage`：中间页工具栏是否显示分析引擎内存占用（「内存 X MB / 上限」）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    #[serde(default = "default_insight_memory_limit_mb")]
    pub memory_limit_mb: i64,
    #[serde(default = "default_insight_full_scan_confirm")]
    pub full_scan_confirm: bool,
    #[serde(default = "default_insight_work_set_limit")]
    pub work_set_limit: i64,
    #[serde(default = "default_insight_status_usage")]
    pub status_usage: bool,
}

impl Default for Insight {
    fn default() -> Self {
        Self {
            memory_limit_mb: default_insight_memory_limit_mb(),
            full_scan_confirm: default_insight_full_scan_confirm(),
            work_set_limit: default_insight_work_set_limit(),
            status_usage: default_insight_status_usage(),
        }
    }
}

/// 分析引擎内存上限（MB）：默认 2GB，与 `DuckDBManager::DEFAULT_MEMORY_LIMIT` 一致。
fn default_insight_memory_limit_mb() -> i64 {
    2048
}

/// 全量统计前确认：默认开启（大表全量是慢操作，值得一道确认）。
fn default_insight_full_scan_confirm() -> bool {
    true
}

/// 洞察集合提示阈值：默认 30 张（工作集超过 30 张提示先清理）。
fn default_insight_work_set_limit() -> i64 {
    30
}

/// 工具栏内存占用显示：默认开启。
fn default_insight_status_usage() -> bool {
    true
}

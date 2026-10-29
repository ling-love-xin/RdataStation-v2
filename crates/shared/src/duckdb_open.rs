//! DuckDB 文件打开助手
//!
//! 自 engine `duckdb::manager::DuckDBManager::open_file_with_retry` 下沉
//! （2026-10-29，engine 瘦身第二刀）：rds-store 的全局 DuckDB 长连接需要它，
//! 但 store 不能依赖 engine（会成环），故放 shared（engine 与 store 两侧共用）。

use std::path::PathBuf;

use duckdb::Connection;

use crate::error::{CommonError, CoreError};

/// 打开 DuckDB 文件（自动创建父目录；失败带路径上下文的 CoreError）
///
/// 与 engine `DuckDBManager::ensure_parent_dir` 语义一致：父目录不存在则建，
/// 建不成就报带路径的错误，避免 `Connection::open` 的裸错误难定位。
pub fn open_file_with_retry(path: &str) -> Result<Connection, CoreError> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "创建目录失败 {:?}: {}",
                    parent, e
                )))
            })?;
        }
    }
    Connection::open(&path).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "打开 DuckDB 文件失败 {}: {}",
            path.display(),
            e
        )))
    })
}

//! 本体层：`{项目}/resources/` 受管文件 + 历史内容副本（架构 §3 / §5.2）。
//!
//! 职责边界：
//! - **只管文件系统**：路径守卫、归档搬运、只读标记、内容指纹、版本副本；不碰 `project.db`
//!   （索引层在 `resource.rs` / `folder.rs` / `tag.rs` / `version.rs`）。
//! - **文件系统是本体权威，登记表是索引**：任何不一致以文件为准，由 `indexer` 侧检测修复。
//!
//! 目录约定（与草稿箱一致：内容可见、内部态隐藏）：
//! ```text
//! {项目}/resources/                          ← 受管内容（可见、归档后只读）
//! {项目}/.RSmeta/resources/versions/<id>/<v>/ ← 历史内容副本（隐藏，按 keep 份数裁剪）
//! ```

pub(crate) use std::io::Read;
pub(crate) use std::path::{Component, Path, PathBuf};

pub(crate) use sha2::{Digest, Sha256};
pub(crate) use shared::error::{CoreError, StorageError};
pub(crate) use tokio::fs;
pub(crate) use tokio::io::AsyncReadExt;

/// 受管内容目录名（项目根下**可见**的模块目录）。
pub const RESOURCES_DIR_NAME: &str = "resources";
/// 项目内部元数据目录名。
///
/// 直接引用 engine 已有常量，而不是再写一份字面量：`.RSmeta` 这一拼写当前有多处各自声明
/// （`project::store` / `project::lock` / `engine::connection_org_store` / `engine::project_db`
/// 内的字面量 / `scratchpad::store`），单一来源收敛见开发方案 P0.3。
pub const META_DIR_NAME: &str = engine::persistence::connection_org_store::RS_META_DIR_NAME;
/// 历史内容副本目录名（位于元数据目录内）。
const VERSIONS_DIR_NAME: &str = "versions";
/// 计算指纹时的读取块大小。
const HASH_CHUNK: usize = 64 * 1024;

/// 本体存储：按**项目根**实例化（项目态不得放进程单例，沿用草稿箱的隔离口径）。
#[derive(Clone)]
pub struct PayloadStore {
    project_root: PathBuf,
}

mod inspect;
mod paths;
mod transfer;
mod versions;

fn safe_segment(raw: &str) -> Result<String, CoreError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('.')
        || trimmed.contains(['/', '\\'])
        || trimmed.contains("..")
    {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "analytics_resources".to_string(),
            operation: "safe_segment".to_string(),
            reason: format!("非法 id：{raw}"),
        }));
    }
    Ok(trimmed.to_string())
}

fn invalid_rel(rel: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: "resolve_rel".to_string(),
        reason: format!("非法相对路径 {rel:?}：{reason}"),
    })
}

fn io_err(path: &Path, operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation,
        reason.to_string(),
    ))
}

#[cfg(test)]
mod tests;

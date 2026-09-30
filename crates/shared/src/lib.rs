//! RdataStation v2 共享基础层（shared crate）
//!
//! 自 v1 `core/` 基础层迁移（error / models / utils / crypto / drag）。
//! 遵循 GPUI-kit 编码指南：**只有 ≥2 个真实使用方的稳定能力**才进入本 crate。
//!
//! 收敛记录（2026-09-23）：首轮迁移是**全量复制**，带进来六个**零消费者**的 v1 残留 ——
//! `api_version` / `arrow` / `macros` / `port_negotiation` / `stream` / `types`
//! （其中 `types` 26 项与 `macros` 9 个宏**一个引用都没有**；`arrow::ArrowHandler`
//! 还被文档当成「Arrow 数据通路已通」的证据，其实从没人调）。本轮按上面那条口径删除，
//! 同批删掉 `utils` 里 5 个零调用函数。要与 v1 对照时看仓库根的 `v1/` 目录。
//!
//! 有意保留的一处：`error::CacheError`（缓存错误域）—— 它没有构造点（v2 的缓存错误
//! 实际走 `Storage`），但删它会牵动 `CoreError` / `ErrorCategory` 的匹配面，
//! 而错误域的完整性是设计的一部分。

pub mod crypto;
pub mod drag;
pub mod error;
pub mod models;
pub mod utils;

// 重新导出常用错误类型（与 v1 core/mod.rs 一致）
pub use error::{
    common_err, conn_err, invalid_arg, not_supported, query_err, storage_err, timeout, CommonError,
    ConnectionError, CoreError, CoreResult, DatabaseError, ErrorCategory, StorageError,
    TransactionState,
};

// 重新导出模型层
pub use models::{QueryResult, Row, Value};

// 重新导出拖动载体（编辑器 / 草稿箱的文件拖入）
pub use drag::InsertFileDrag;

// 重新导出工具模块
pub use utils::{hash, string, time};

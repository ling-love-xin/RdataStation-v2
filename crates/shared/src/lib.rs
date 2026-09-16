//! RdataStation v2 共享基础层（shared crate）
//!
//! 自 v1 `core/` 基础层迁移（error/models/arrow/stream/utils/crypto/port_negotiation 等）。
//! 遵循 GPUI-kit 编码指南：只有 ≥2 个真实使用方的稳定能力才进入本 crate。
//!
//! TODO(migration): 首轮迁移完成，保持 v1 实现原貌；后续随 Feature 迁移收敛 API。

pub mod api_version;
pub mod arrow;
pub mod crypto;
pub mod drag;
pub mod error;
pub mod macros;
pub mod models;
pub mod port_negotiation;
pub mod stream;
pub mod types;
pub mod utils;

// 重新导出常用错误类型（与 v1 core/mod.rs 一致）
pub use error::{
    common_err, conn_err, invalid_arg, not_supported, query_err, storage_err, timeout, CommonError,
    ConnectionError, CoreError, CoreResult, DatabaseError, ErrorCategory, StorageError,
    TransactionState,
};

// 重新导出模型层
pub use models::{QueryResult, Row, Value};

// 重新导出 Arrow 相关
pub use arrow::{ArrowBatch, ArrowBatchStream, ArrowHandler};
pub use drag::InsertFileDrag;

// 重新导出流相关
pub use stream::{ArrowBatchStream as CoreArrowBatchStream, Stream, StreamQueryResult};

// 重新导出工具模块
pub use utils::{hash, string, time};

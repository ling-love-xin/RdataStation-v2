//! RDS Rust 原生数据库驱动 sidecar 共享协议内核。
//!
//! 宿主契约见 `docs/architecture/plugin/plugin-dev-plan.md` §4.2：
//! - stdio 帧 `[u32 BE total_len][u8 kind][payload]`，`total_len` 含 5 字节头；
//! - JSON-RPC 2.0（kind `0x01`）+ Arrow IPC stream 分片（kind `0x02`）；
//! - 小结果内联 JSON 行（对象），大结果走附件；空结果集也必须带 `columns`；
//! - stdin EOF 等于宿主已死，进程立即退出。
//!
//! 这里只做协议与编码，具体数据库能力由各驱动的 [`Driver`] 实现提供。

mod error;
mod frame;
mod meta;
mod page;
mod server;

pub use error::{DriverError, ErrorCode};
pub use frame::{Frame, FrameReader, FrameWriter, KIND_ARROW, KIND_JSON, MAX_FRAME_BYTES};
pub use meta::{
    normalize_kind, MetaColumn, MetaIndex, MetaObject, MetaObjectDetail, MetaSchema,
};
pub use page::{Canonical, ColumnDef, ExecuteRequest, Page, SessionInfo};
pub use server::{serve, Driver};

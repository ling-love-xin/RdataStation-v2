//! 视图模型层：面板与对话框共用的人读快照，只依赖领域模型，不依赖任何视图模块。
//!
//! 2026-11-12 收口：本模块整体 `pub(crate)`，旧路径
//! （`resource_view::ArchiveRow`、`detail_view::ArchiveDetail` 等）由各视图模块自己
//! `pub use` 承接，外部契约不变。原先堆在这里的兼容重导出在本 crate 内无人使用，已删除。

pub mod detail;
pub mod dialog_rows;
pub mod row;

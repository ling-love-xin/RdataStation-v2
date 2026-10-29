//! 宿主端口实现（P2-a）：原 workbench `services/editor_*` 下沉至此。
//!
//! ## 为什么下沉
//!
//! 这些文件是 `crate::shared::EditorShared` 各 `attach_*` 槽位的**宿主实现**
//! （连接列表 / 执行器 / 补全 / schema / 会话存储 / 语言服务等），此前放在
//! `workbench::services` 下——改任何一个都会触发 workbench 全量重编
//! （26k 行，`-j 2` 下每次 2–4 分钟）。下沉后 workbench 只依赖本目录的
//! **重导**（`services/mod.rs` 的 `pub use`），编辑 / 连接 / 执行逻辑的改动
//! 只重编 `rds-editor`。
//!
//! ## 宿主能力从哪来
//!
//! 本目录的代码不再依赖 `workbench::panels::Shared` 的具体类型，只依赖
//! [`crate::host::EditorHost`]（workbench 实现并注入）。依赖方向：
//! `workbench → editor → engine / database / shared / workbench_shell`，无环。

pub mod editor_catalog;
pub mod editor_channels;
pub mod editor_completion;
pub mod editor_connections;
pub mod editor_env;
pub mod editor_exec;
pub mod editor_files;
pub mod editor_history;
pub mod editor_insight;
pub mod editor_kernels;
pub mod editor_lsp;
pub mod editor_project;
pub mod editor_schema;
pub mod editor_session;
pub mod editor_sources;
pub mod editor_table_keys;
pub mod result_service;

//! 工作台服务层（自 v1 `core/services` 迁移）
//!
//! - `connection_service`：用户数据源连接管理（测试/CRUD/路由）
//! - `data_source_service`：数据源连接服务（Phase A：元数据 + 测试 + CRUD）
//! - `driver_service`：驱动管理（注册/文件/数据源类型）
//! - `editor_*`：编辑器的宿主端口实现（执行 / 连接 / 补全 / schema / 会话 / 文件对话框 /
//!   语言服务 / 通道 / 联邦源等，见 `editor::ports`）
//!
//! 下沉（2026-10-15）：`connection_service` / `data_source_service` / `driver_service` /
//! `workspace_loader` / `secret_integration` 已迁至 `rds-connection-dialog`（只依赖 engine /
//! connection / shared / workbench_shell）。此处 `pub use` 重导，保持 `crate::services::*` 路径不变，
//! 本 crate 内既有引用点（connection_dialog / nav_host / editor_exec / nav_runtime / view 等）零改动。
//!
//! 下沉（2026-10-29，P2-a）：`editor_env/exec/files/kernels/lsp/session/sources`、`result_service`
//! 与全部 Shared 依赖端口（catalog/channels/completion/connections/history/insight/project/
//! schema/table_keys）已迁至 `rds-editor::ports`（不再依赖 `workbench::panels::Shared`，
//! 只依赖 `editor::host::{EditorHost, FederatedSourcePort}`）。此处 `pub use` 重导保持
//! `crate::services::*` 路径不变，本 crate 内既有引用点（view / resource_host 等）零改动。

pub use connection_dialog::services::connection_service;
pub use connection_dialog::services::data_source_service;
pub mod db_navigator;
pub use connection_dialog::services::driver_service;
pub use editor::ports::editor_catalog;
pub use editor::ports::editor_channels;
pub use editor::ports::editor_completion;
pub use editor::ports::editor_connections;
pub use editor::ports::editor_env;
pub use editor::ports::editor_exec;
pub use editor::ports::editor_files;
pub use editor::ports::editor_history;
pub use editor::ports::editor_insight;
pub use editor::ports::editor_kernels;
pub use editor::ports::editor_lsp;
pub use editor::ports::editor_project;
pub use editor::ports::editor_schema;
pub use editor::ports::editor_session;
pub use editor::ports::editor_sources;
pub use editor::ports::editor_table_keys;
pub mod mock_generator;
pub mod mock_jobs;
pub mod nav_runtime;
pub mod project_session;
pub mod query_export;
pub mod query_history;
pub mod resource_jobs;
pub use editor::ports::result_service;
pub mod scratchpad_meta;
pub use connection_dialog::services::secret_integration;

pub use connection_dialog::services::workspace_loader;

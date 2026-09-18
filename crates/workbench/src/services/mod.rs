//! 工作台服务层（自 v1 `core/services` 迁移）
//!
//! - `connection_service`：用户数据源连接管理（测试/CRUD/路由）
//! - `data_source_service`：数据源连接服务（Phase A：元数据 + 测试 + CRUD）
//! - `result_service`：结果集服务（SQL 过滤 / DuckDB 分析 / 临时表 / 导出；洞察已归 `crates/insight`）
//! - `driver_service`：驱动管理（注册/文件/数据源类型）
//! - `editor_exec`：编辑器执行端口的工作台实现（编辑器 A14 最小执行）
//! - `editor_session`：编辑器会话存储的工作台实现（编辑器 A12 光标/模式持久化）
//! - `editor_files`：编辑器的系统文件对话框（A9 打开 / 另存为）
//! - `editor_connections`：编辑器的连接端口（B1 连接列表 + 自动建连）
//! - `editor_channels`：【B13】编辑器的执行通道端口（源库 / 本地加速 / 联邦 的门控真值）

pub mod connection_service;
pub mod data_source_service;
pub mod db_navigator;
pub mod driver_service;
pub mod editor_channels;
pub mod editor_connections;
pub mod editor_exec;
pub mod editor_files;
pub mod editor_insight;
pub mod editor_session;
pub mod editor_sources;
pub mod mock_generator;
pub mod mock_jobs;
pub mod nav_runtime;
pub mod project_session;
pub mod query_export;
pub mod query_history;
pub mod resource_jobs;
pub mod result_service;
pub mod scratchpad_meta;
pub mod secret_integration;

pub mod workspace_loader;

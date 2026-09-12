//! 工作台服务层（自 v1 `core/services` 迁移）
//!
//! - `connection_service`：用户数据源连接管理（测试/CRUD/路由）
//! - `data_source_service`：数据源连接服务（Phase A：元数据 + 测试 + CRUD）
//! - `result_service`：结果集服务 + 洞察计算编排
//! - `persistence_service`：工作台持久化（结果/画像回写）
//! - `driver_service`：驱动管理（注册/文件/数据源类型）

pub mod connection_service;
pub mod data_source_service;
pub mod db_navigator;
pub mod driver_service;
pub mod mock_generator;
pub mod nav_jobs;
pub mod nav_runtime;
pub mod nav_store;
pub mod persistence_service;
pub mod project_session;
pub mod query_export;
pub mod query_history;
pub mod query_runner;
pub mod result_service;
pub mod secret_integration;
pub mod workspace_loader;

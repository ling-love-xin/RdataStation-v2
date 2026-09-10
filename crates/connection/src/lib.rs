//! RdataStation v2 connection crate（M3 数据源连接）
//!
//! 自 v1 `core/driver/connection` 迁移（配置/连接器/SSH/SSL/Proxy/流）。
//! 后续待迁移：`core/services/connection_service.rs`（连接生命周期服务）、
//! DuckDB Secret 本地加速通道（`secret.rs`，M3 核心差异化能力）。
//!
//! 依赖方向：connection → rds-shared；Feature crates → connection。

pub mod config;
pub mod connector;
pub mod factory;
pub mod known_hosts;
pub mod stream;

// 占位模块（TODO(migration): 按 GPUI-kit 规范实现命令与视图）
pub mod model;
pub mod commands;
pub mod connection_view;
pub mod connection_dialog;
pub mod secret;

pub use config::ConnectionConfig;
pub use connector::Connection;
pub use factory::ConnectionFactory;
pub use stream::ConnectionStream;
pub use model::{ConnectionScope, DataSource, DataSourceSaveInput, DeleteResult, TestResult};

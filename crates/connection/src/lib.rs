//! RdataStation v2 connection crate（M3 数据源连接）
//!
//! 自 v1 `core/driver/connection` 迁移（配置/连接器/SSH/SSL/Proxy/流）。
//!
//! 职责边界：
//! - 本 crate = 传输层 + 领域模型（`config` / `connector` / `factory` /
//!   `stream` / `known_hosts` / `secret` / `model`），只依赖 `rds-shared`；
//! - 服务编排（DataSourceService）与 GPUI 视图在 `workbench`（UI/编排层），
//!   因 engine 依赖 connection，连接编排需 engine 持久化，故不反向依赖。
//!
//! 依赖方向：connection → rds-shared；Feature crates → connection。

pub mod config;
pub mod connector;
pub mod factory;
pub mod known_hosts;
pub mod model;
pub mod secret;
pub mod stream;

pub use config::ConnectionConfig;
pub use connector::Connection;
pub use factory::ConnectionFactory;
pub use model::{ConnectionScope, DataSource, DataSourceSaveInput, DeleteResult, TestResult};
pub use stream::ConnectionStream;

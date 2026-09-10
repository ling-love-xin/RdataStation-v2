//! 连接管理模块
//!
//! 提供连接配置、连接器、连接流等核心功能
//!
//! # 模块职责
//!
//! - `config` - 连接配置和验证
//! - `stream` - 连接流抽象
//! - `connector` - 连接器接口和实现
//! - `factory` - 连接工厂
//! - `known_hosts` - SSH known_hosts 文件解析与校验

pub mod config;
pub mod connector;
pub mod factory;
pub mod known_hosts;
pub mod stream;

// 常用类型重导出
pub use config::ConnectionConfig;
pub use connector::Connection;
pub use factory::ConnectionFactory;
pub use stream::ConnectionStream;

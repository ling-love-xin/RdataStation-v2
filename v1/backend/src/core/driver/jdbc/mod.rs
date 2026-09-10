//! JDBC 驱动模块
//!
//! 通过 JVM 桥接支持 JDBC 兼容的数据库
//!
//! # 架构
//!
//! ```
//! Rust Core
//!     │
//!     ▼
//! JDBC Driver (core/driver/jdbc/)
//!     │
//!     ▼
//! JVM (via JNI)
//!     │
//!     ▼
//! JDBC Driver (.jar)
//! ```
//!
//! # 支持数据库
//!
//! - Oracle
//! - SQL Server
//! - DB2
//! - 其他 JDBC 兼容数据库

pub mod connection;
pub mod driver;
pub mod executor;
pub mod jvm_manager;

pub use connection::JdbcConnection;
pub use driver::JdbcDriver;
pub use jvm_manager::JvmManager;

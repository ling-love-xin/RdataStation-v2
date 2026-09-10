//! 项目（Project）管理模块
//!
//! 项目是 RdataStation 的核心工作单元，支持：
//! - 本地项目（现阶段）
//! - 网络项目（DuckLake 预留）
//!
//! ## 数据分层
//!
//! ```
//! Project
//! ├── SQLite (meta/project.db)    - 元数据索引、事务性信息
//! ├── DuckDB (analytics/data.duckdb) - 分析数据、版本载体
//! └── Config (config/*.json)      - 连接配置、SQL文件
//! ```
//!
//! ## 版本化支持
//!
//! 所有核心模型都支持版本化，为 DuckLake 多人协同预留：
//! - Versioned<T> 包装器
//! - 版本链（parent/child）
//! - 用户标识（created_by）
//! - 数据校验（checksum）

pub mod models;
pub mod store;

pub use models::{
    ConnectionRef, Project, ProjectConfig, ProjectInfo, ProjectPath, ProjectStatus, QueryRef,
    Version, VersionInfo, Versioned,
};
pub use store::{ProjectManager, ProjectStore};

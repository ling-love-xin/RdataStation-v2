//! RdataStation v2 项目 crate（project，M1）
//!
//! 承载双层数据架构（项目级物理隔离）：
//! - SQLite（meta/project.db）：元数据索引、事务信息
//! - DuckDB（analytics/data.duckdb）：分析数据、版本载入
//! - Config（config/*.json）：连接配置、SQL 文件
//! - 版本化支持（Versioned<T>，为 DuckLake 多人协同预留）

//! ## 数据分层
//!
//! Project
//! ├── SQLite (meta/project.db)    - 元数据索引、事务性信息
//! ├── DuckDB (analytics/data.duckdb) - 分析数据、版本载体
//! └── Config (config/*.json)      - 连接配置、SQL文件
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


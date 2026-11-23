//! RdataStation v2 项目 crate（project，M1）
//!
//! 一个实例一个项目：名册在全局库（`project_info` 表），项目磁盘元数据在项目根的
//! `.RSmeta/` 下（扁平布局，无子目录分层）：
//!
//! - `project.json` — 项目信息**权威源**（名称 / 描述 / 状态 / 时间戳）
//! - `project.db` — SQLite 镜像（`project` 表）+ `project_versions` 版本台账
//! - `analytics.duckdb` — 项目分析库（DuckDB 原生格式）
//! - `config/settings.json` — 项目配置（默认连接、`rds.build_tag` 等扩展键）
//! - `project.lock` / `project.lock.owner` — 实例锁与占用者信息
//!
//! 路径约定只有 `store` 一处权威（[`store::RS_META_DIR_NAME`] / `ProjectStore::project_db_path`），
//! 其它模块不要再自带一套推导。
//!
//! ## 版本能力
//!
//! 在用的是 **`project_versions` 手动快照台账**（`service::{list_versions, create_version}`）：
//! 一行一条 message，不承载数据内容。`Versioned<T>` / 版本链 / checksum 是 DuckLake
//! 多人协同的预留，**未接入任何业务数据**（见 `docs/architecture/project/project-wiring-matrix.md`）。

pub mod lock;
pub mod models;
pub mod service;
pub mod store;
pub mod ui;

pub use lock::{AcquireOutcome, LockInfo, ProjectLock};
pub use models::{
    ConnectionRef, Project, ProjectConfig, ProjectInfo, ProjectPath, ProjectStatus, QueryRef,
    Version, VersionInfo, Versioned,
};
pub use service::{OpenOutcome, ProjectSummary};
pub use store::ProjectStore;
pub use ui::{OpenProject, ProjectUiHost};

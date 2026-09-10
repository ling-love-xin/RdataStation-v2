//! Minicatalogs — 系统目录离线预置
//!
//! 对标 DataGrip 2026.1 Minicatalogs 特性。
//! 预嵌入常用数据库的系统 Schema 定义（information_schema / pg_catalog / sqlite_master），
//! 使得代码补全和导航在离线状态下也能工作。
//!
//! ## 设计约束
//!
//! | 约束 | 说明 |
//! |------|------|
//! | 嵌入方式 | `include_str!()` 编译期嵌入 JSON 定义文件 |
//! | 更新机制 | 应用升级时随二进制一起更新 |
//! | 加载时机 | 首次访问连接时懒加载到 L2 缓存 |
//! | 覆盖范围 | MySQL 8.0, PostgreSQL 16+, SQLite 3.x 系统目录 |
//! | 大小限制 | 每个 DB 类型 ≤ 50KB 编译产物 |
//!
//! ## 实施计划
//!
//! - [x] 模块定义（本文件）
//! - [ ] JSON 定义文件（`definitions/mysql_sys.json` 等）
//! - [ ] MinicatalogLoader（编译期嵌入 + 运行时懒加载）
//! - [ ] L2 缓存集成（标记来源 `SystemBuiltin`）
//! - [ ] Tauri 命令 `load_minicatalog(conn_id, schema)`
//! - [ ] 前端自动完成集成（Monaco Editor 补全提供者）

use serde::{Deserialize, Serialize};

/// 内建系统目录条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinicatalogEntry {
    /// 对象名（表名 / 视图名）
    pub name: String,
    /// 对象类型（TABLE / VIEW / FUNCTION）
    pub object_type: String,
    /// 所属 Schema
    pub schema_name: String,
    /// 列定义（可选，延迟加载）
    pub columns: Option<Vec<MinicatalogColumn>>,
    /// 来源标记
    pub source: CatalogSource,
}

/// 内建列定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinicatalogColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary: bool,
    pub comment: Option<String>,
}

/// 目录来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogSource {
    /// 系统内建（编译期嵌入）
    SystemBuiltin,
    /// 运行时从实际数据库加载
    RuntimeIntrospection,
}

/// 支持的数据库类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MinicatalogDbType {
    MySQL,
    PostgreSQL,
    SQLite,
}

impl MinicatalogDbType {
    /// 获取该数据库类型的内建系统 Schema 列表
    pub fn system_schemas(&self) -> &[&str] {
        match self {
            Self::MySQL => &["information_schema", "mysql", "performance_schema", "sys"],
            Self::PostgreSQL => &["pg_catalog", "information_schema"],
            Self::SQLite => &["main"],
        }
    }
}

/// Minicatalog 加载器状态
#[derive(Debug, Default)]
pub struct MinicatalogRegistry {
    /// 已加载的目录（db_type → schema → Vec<entry>）
    loaded: std::collections::HashMap<MinicatalogDbType, Vec<MinicatalogEntry>>,
}

impl MinicatalogRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取指定数据库类型的系统目录（懒加载）
    pub fn get_catalog(&mut self, db_type: MinicatalogDbType) -> &[MinicatalogEntry] {
        self.loaded
            .entry(db_type)
            .or_insert_with(|| load_builtin_catalog(db_type))
    }
}

/// 从编译期嵌入的定义加载内建目录
///
/// TODO: 当 JSON 定义文件就绪后，替换为 include_str! 读取。
fn load_builtin_catalog(_db_type: MinicatalogDbType) -> Vec<MinicatalogEntry> {
    // 占位：后续从 definitions/mysql_sys.json 等文件加载
    // let raw = include_str!("../../../../definitions/mysql_sys.json");
    // serde_json::from_str(raw).unwrap_or_default()
    Vec::new()
}

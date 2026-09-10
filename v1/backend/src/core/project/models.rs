//! 项目模型定义
//!
//! 定义项目的核心数据结构，支持版本化和 DuckLake 扩展。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// 项目路径类型
///
/// 支持本地路径和网络路径（DuckLake 预留）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProjectPath {
    /// 本地项目路径
    Local { path: PathBuf },
    /// 网络项目路径（DuckLake 预留）
    Remote { url: String, project_id: String },
}

impl ProjectPath {
    /// 创建本地项目路径
    pub fn local<P: Into<PathBuf>>(path: P) -> Self {
        ProjectPath::Local { path: path.into() }
    }

    /// 创建远程项目路径（DuckLake 预留）
    pub fn remote(url: impl Into<String>, project_id: impl Into<String>) -> Self {
        ProjectPath::Remote {
            url: url.into(),
            project_id: project_id.into(),
        }
    }

    /// 检查是否为本地项目
    pub fn is_local(&self) -> bool {
        matches!(self, ProjectPath::Local { .. })
    }

    /// 检查是否为远程项目
    pub fn is_remote(&self) -> bool {
        matches!(self, ProjectPath::Remote { .. })
    }

    /// 获取本地路径（如果是本地项目）
    pub fn local_path(&self) -> Option<&PathBuf> {
        match self {
            ProjectPath::Local { path } => Some(path),
            _ => None,
        }
    }

    /// 获取项目元数据目录路径（本地项目）
    pub fn meta_dir(&self) -> Option<PathBuf> {
        self.local_path().map(|p| p.join(".RSmeta"))
    }

    /// 获取 SQLite 元数据数据库路径
    pub fn sqlite_path(&self) -> Option<PathBuf> {
        self.meta_dir().map(|p| p.join("meta").join("project.db"))
    }

    /// 获取 DuckDB 分析数据库路径
    pub fn duckdb_path(&self) -> Option<PathBuf> {
        self.meta_dir()
            .map(|p| p.join("analytics").join("data.duckdb"))
    }

    /// 获取配置目录路径
    pub fn config_dir(&self) -> Option<PathBuf> {
        self.meta_dir().map(|p| p.join("config"))
    }

    /// 获取 SQL 文件目录路径
    pub fn queries_dir(&self) -> Option<PathBuf> {
        self.meta_dir().map(|p| p.join("queries"))
    }
}

impl Default for ProjectPath {
    fn default() -> Self {
        ProjectPath::Local {
            path: PathBuf::new(),
        }
    }
}

/// 项目状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    /// 活跃状态
    #[default]
    Active,
    /// 已归档
    Archived,
    /// 同步中（DuckLake 预留）
    Syncing,
    /// 离线状态（DuckLake 预留）
    Offline,
}

/// 项目信息（轻量级元数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// 项目唯一标识
    pub id: String,
    /// 项目名称
    pub name: String,
    /// 项目描述
    pub description: Option<String>,
    /// 项目路径
    pub path: ProjectPath,
    /// 项目状态
    pub status: ProjectStatus,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后修改时间
    pub updated_at: DateTime<Utc>,
    /// 最后打开时间
    pub last_opened_at: Option<DateTime<Utc>>,
    /// 创建者（DuckLake 预留）
    pub created_by: Option<String>,
    /// 版本数量
    pub version_count: u32,
}

/// 项目配置
#[derive(Debug, Clone, Serialize, Deserialize, Default, specta::Type)]
pub struct ProjectConfig {
    /// 项目设置
    #[specta(skip)]
    pub settings: HashMap<String, serde_json::Value>,
    /// 默认连接ID
    pub default_connection_id: Option<String>,
    /// 扩展配置
    #[specta(skip)]
    pub extensions: HashMap<String, serde_json::Value>,
}

/// 版本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    /// 版本唯一标识（UUID）
    pub id: String,
    /// 父版本ID（支持版本链）
    pub parent: Option<String>,
    /// 版本说明
    pub message: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 创建者（DuckLake 预留）
    pub created_by: Option<String>,
}

/// 版本包装器
///
/// 为所有核心模型提供版本化支持
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versioned<T> {
    /// 实际数据
    pub data: T,
    /// 版本信息
    pub version: Version,
    /// 数据校验和
    pub checksum: String,
}

impl<T> Versioned<T> {
    /// 创建新的版本化数据
    pub fn new(data: T, message: impl Into<String>) -> Self
    where
        T: Serialize,
    {
        let checksum = calculate_checksum(&data);
        Self {
            data,
            version: Version {
                id: uuid::Uuid::new_v4().to_string(),
                parent: None,
                message: message.into(),
                created_at: Utc::now(),
                created_by: None,
            },
            checksum,
        }
    }

    /// 创建子版本
    pub fn create_child(&self, data: T, message: impl Into<String>) -> Self
    where
        T: Serialize,
    {
        let checksum = calculate_checksum(&data);
        Self {
            data,
            version: Version {
                id: uuid::Uuid::new_v4().to_string(),
                parent: Some(self.version.id.clone()),
                message: message.into(),
                created_at: Utc::now(),
                created_by: None,
            },
            checksum,
        }
    }

    /// 验证数据完整性
    pub fn verify_checksum(&self) -> bool
    where
        T: Serialize,
    {
        calculate_checksum(&self.data) == self.checksum
    }
}

/// 计算数据校验和
fn calculate_checksum<T: Serialize>(data: &T) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let json = serde_json::to_string(data).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Failed to serialize data for checksum calculation");
        String::new()
    });
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 版本元数据（存储在 SQLite 中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    pub parent_id: Option<String>,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub entity_type: String, // "connection", "query", etc.
    pub entity_id: String,
}

/// 连接引用（存储在项目中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRef {
    pub id: String,
    pub name: String,
    pub db_type: String,
    /// 连接配置（加密存储）
    pub config: String,
    /// 版本信息
    pub version: Version,
}

/// 查询引用（存储在项目中）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRef {
    pub id: String,
    pub name: String,
    /// 文件路径（相对项目根目录）
    pub file_path: PathBuf,
    /// 版本信息
    pub version: Version,
}

/// 项目（完整结构）
#[derive(Debug, Clone)]
pub struct Project {
    /// 项目信息
    pub info: ProjectInfo,
    /// 项目配置
    pub config: ProjectConfig,
    /// 连接列表（从 SQLite 加载）
    pub connections: Vec<ConnectionRef>,
    /// 查询列表（从 SQLite 加载）
    pub queries: Vec<QueryRef>,
}

impl Project {
    /// 创建新项目
    pub fn new(name: impl Into<String>, path: ProjectPath) -> Self {
        let now = Utc::now();
        let id = uuid::Uuid::new_v4().to_string();

        Self {
            info: ProjectInfo {
                id,
                name: name.into(),
                description: None,
                path,
                status: ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                last_opened_at: None,
                created_by: None,
                version_count: 0,
            },
            config: ProjectConfig::default(),
            connections: Vec::new(),
            queries: Vec::new(),
        }
    }

    /// 获取项目元数据目录
    pub fn meta_dir(&self) -> Option<PathBuf> {
        self.info.path.meta_dir()
    }

    /// 获取 SQLite 数据库路径
    pub fn sqlite_path(&self) -> Option<PathBuf> {
        self.info.path.sqlite_path()
    }

    /// 获取 DuckDB 数据库路径
    pub fn duckdb_path(&self) -> Option<PathBuf> {
        self.info.path.duckdb_path()
    }

    /// 更新最后打开时间
    pub fn touch(&mut self) {
        self.info.last_opened_at = Some(Utc::now());
        self.info.updated_at = Utc::now();
    }

    /// 添加连接引用
    pub fn add_connection(&mut self, connection: ConnectionRef) {
        self.connections.push(connection);
        self.info.updated_at = Utc::now();
    }

    /// 添加查询引用
    pub fn add_query(&mut self, query: QueryRef) {
        self.queries.push(query);
        self.info.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_path_local() {
        let path = ProjectPath::local("/tmp/my-project");
        assert!(path.is_local());
        assert!(!path.is_remote());
        assert!(path.local_path().is_some());
    }

    #[test]
    fn test_project_path_remote() {
        let path = ProjectPath::remote("https://ducklake.io", "proj-123");
        assert!(!path.is_local());
        assert!(path.is_remote());
        assert!(path.local_path().is_none());
    }

    #[test]
    fn test_versioned_data() {
        let data = "test data".to_string();
        let versioned = Versioned::new(data.clone(), "Initial version");

        assert_eq!(versioned.data, data);
        assert!(versioned.verify_checksum());
        assert!(versioned.version.parent.is_none());

        let child = versioned.create_child("modified data".to_string(), "Second version");
        assert_eq!(child.version.parent, Some(versioned.version.id));
    }

    #[test]
    fn test_project_creation() {
        let path = ProjectPath::local("/tmp/test-project");
        let project = Project::new("Test Project", path);

        assert_eq!(project.info.name, "Test Project");
        assert_eq!(project.info.status, ProjectStatus::Active);
        assert!(project.connections.is_empty());
        assert!(project.queries.is_empty());
    }
}

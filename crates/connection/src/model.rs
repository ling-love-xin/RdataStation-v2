//! rds-connection — 数据源连接领域模型（Phase A）。
//!
//! 字段对齐 `global_connections` 表列（engine/persistence/global_db.rs）与
//! v1 `DATA-SOURCE-MODULE.md` 表设计；ID 前缀双轨制见 engine `id_prefix.rs`：
//!   - `G_xxx`   仅全局（global.db / global_connections）
//!   - `P_xxx`   仅项目（project.db / connections）
//!   - `GP_xxx`  项目引用全局快照（保存全局定义 + 项目共享快照）

use serde::{Deserialize, Serialize};
use specta::Type;

/// 连接作用域（双轨制，可"全局+项目"同属）。
///
/// 后端语义：
/// - `Global`：仅存全局（G_xxx）
/// - `Project`：仅存当前项目（P_xxx）
/// - `GlobalAndProject`：保存全局定义（G_xxx）+ 共享至当前项目（GP_xxx 快照）；
///   规则：项目可引用全局，全局不可引用项目私有配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionScope {
    /// 仅全局
    #[default]
    Global,
    /// 仅项目
    Project,
    /// 全局＋项目（G_ 定义 + GP_ 快照）
    GlobalAndProject,
}

impl ConnectionScope {
    /// 是否包含全局侧
    pub fn includes_global(&self) -> bool {
        matches!(self, Self::Global | Self::GlobalAndProject)
    }

    /// 是否包含项目侧
    pub fn includes_project(&self) -> bool {
        matches!(self, Self::Project | Self::GlobalAndProject)
    }

    /// UI 提示文案（与原型 Header 作用域提示行一致）
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Global => "仅全局 → 保存至 global_connections（G_xxx）",
            Self::Project => "仅项目 → 保存至当前项目 project.db（P_xxx）",
            Self::GlobalAndProject => "全局＋项目 → 保存全局（G_xxx）并共享至当前项目（GP_xxx）",
        }
    }
}

/// 数据源连接（领域模型）。
///
/// 由 `DataSourceSaveInput` 保存后回读得到；`password` 仅以密文形式出现
/// （`password_encrypted`，engine `shared::crypto::encrypt_password`）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DataSource {
    /// 连接 ID（G_/P_/GP_ 前缀，engine id_prefix 生成）
    pub id: String,
    /// 连接名称（全局唯一，大写检查见 global_db）
    pub name: String,
    /// 驱动类型（"mysql" | "postgres" | "sqlite" | "duckdb" | …）
    pub db_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub schema_name: Option<String>,
    pub username: Option<String>,
    /// AES-256-GCM 加密后的密码（列表/导出场景禁止回显明文）
    pub password_encrypted: Option<String>,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    /// 驱动属性 JSON（key-value）
    pub driver_properties: Option<String>,
    /// 高级选项 JSON
    pub advanced_options: Option<String>,
    /// 额外选项 JSON
    pub options: Option<String>,
    /// 标签 JSON 数组
    pub tags: Option<String>,
    /// DuckDB 本地加速（联邦查询直连源库）
    pub use_duckdb_fed: bool,
    /// DuckDB 联邦元数据缓存路径（global_connections.metadata_path）
    pub metadata_path: Option<String>,
    /// 测试/连接时探测到的服务器版本
    pub server_version: Option<String>,
    /// 作用域
    pub scope: ConnectionScope,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 新建/更新连接的保存输入（Phase A 对话框提交对象）。
///
/// `url` 为完整 URI（可含 user:pass 凭据）；`username`/`password` 单独提供时
/// 若 URL 无凭据则注入（service 层处理）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DataSourceSaveInput {
    /// 连接名称
    pub name: String,
    /// 驱动类型
    pub db_type: String,
    /// 连接 URL（如 mysql://host:3306/db）
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    /// 作用域（默认仅全局）
    #[serde(default)]
    pub scope: ConnectionScope,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub options: Option<String>,
    pub tags: Option<String>,
    pub use_duckdb_fed: Option<bool>,
    pub schema_name: Option<String>,
    /// DuckDB 联邦加速的元数据缓存路径（对应 global_connections.metadata_path）。
    pub metadata_path: Option<String>,
}

impl DataSourceSaveInput {
    /// 快速构造（对话框默认路径）
    pub fn new(name: impl Into<String>, db_type: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            db_type: db_type.into(),
            url: url.into(),
            username: None,
            password: None,
            scope: ConnectionScope::Global,
            description: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
            options: None,
            tags: None,
            use_duckdb_fed: None,
            schema_name: None,
            metadata_path: None,
        }
    }
}

/// 测试连接结果。
///
/// 与原型「测试连接 → 成功（版本＋延迟）」反馈一致；失败时 `message` 承载可读原因。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
    /// 建连+探测总耗时（毫秒）
    pub latency_ms: Option<u64>,
    /// 探测到的服务器版本（如 "8.4.0" / "16.4"）
    pub version: Option<String>,
}

impl TestResult {
    pub fn ok(message: impl Into<String>, latency_ms: u64, version: Option<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            latency_ms: Some(latency_ms),
            version,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            latency_ms: None,
            version: None,
        }
    }
}

/// 删除结果（删除连接时同时清理 Secret 联动，见 service 层）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DeleteResult {
    pub conn_id: String,
    pub removed_secret: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_semantics() {
        assert!(ConnectionScope::Global.includes_global());
        assert!(!ConnectionScope::Global.includes_project());
        assert!(ConnectionScope::Project.includes_project());
        assert!(!ConnectionScope::Project.includes_global());
        assert!(ConnectionScope::GlobalAndProject.includes_global());
        assert!(ConnectionScope::GlobalAndProject.includes_project());
        assert!(ConnectionScope::Global.hint().contains("G_"));
        assert!(ConnectionScope::GlobalAndProject.hint().contains("GP_"));
    }

    #[test]
    fn test_test_result_ok_err() {
        let ok = TestResult::ok("成功", 12, Some("8.4.0".into()));
        assert!(ok.success);
        assert_eq!(ok.latency_ms, Some(12));
        assert_eq!(ok.version.as_deref(), Some("8.4.0"));

        let err = TestResult::err("连接被拒绝");
        assert!(!err.success);
        assert_eq!(err.latency_ms, None);
    }

    #[test]
    fn test_save_input_serde_roundtrip() {
        let input = DataSourceSaveInput {
            scope: ConnectionScope::GlobalAndProject,
            ..DataSourceSaveInput::new("demo", "postgres", "postgres://h:5432/db")
        };
        let json = serde_json::to_string(&input).expect("serialize");
        assert!(json.contains("\"global_and_project\""));
        let back: DataSourceSaveInput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.scope, ConnectionScope::GlobalAndProject);
        assert_eq!(back.name, "demo");
    }
}

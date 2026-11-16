use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AnalyticsResource {
    pub id: String,
    /// v1 遗留列（`resource_type`）：权威词表是 `kind`，本字段只被写、不被读。
    ///
    /// 留着是因为列在 `analytics_resources` 上且非空；新代码判断种类一律看 `kind`。
    pub resource_type: String,
    pub name: String,
    pub alias: Option<String>,
    /// v1 遗留列（`config`）：v2 已无消费方，读面恒为 [`Value::Null`]（不再解析 JSON）。
    ///
    /// 不删字段是不想为此加一次表迁移；但它不再参与任何判断或展示。
    pub config: Value,
    pub scope: String,
    pub row_count: Option<i32>,
    pub column_count: Option<i32>,
    pub file_size: Option<i32>,
    pub version: i32,
    pub parent_version_id: Option<String>,
    pub parent_resource_id: Option<String>,
    pub source_query: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,

    // ===== 迁移 020 新增：分析存档语义（表结构镜像，领域态见 `model.rs`）=====
    /// 存档种类：`file` / `analysis` / `table_ref`（用 `ArchiveKind::from_db_str` 解析）。
    pub kind: String,
    /// 内容指纹（sha256）；旧行或 `table_ref` 为空。
    pub content_hash: Option<String>,
    /// 本体在 `resources/` 下的相对路径（`kind = file`）。
    pub file_rel_path: Option<String>,
    /// File 档归档粒度：`copy` / `reference`（迁移 023；用 `PayloadMode::from_db_str` 解析）。
    pub payload_mode: String,
    /// reference 模式的源绝对路径（迁移 023；copy 模式为 `None`）。
    pub source_path: Option<String>,
    /// 归档后只读标记（应用层守卫为主）。
    pub readonly: i32,
    /// 来源草稿相对路径。
    pub promoted_from: Option<String>,
    /// 来源连接 id。
    pub source_connection_id: Option<String>,
    /// 来源表 `schema.table`。
    pub source_table: Option<String>,
    /// `kind = analysis` 的重建定义。
    pub definition_sql: Option<String>,
    /// 归档时刻（与 `updated_at` 区分）。
    pub archived_at: Option<DateTime<Utc>>,

    // ===== 迁移 031/034 新增：表型本体与出处（取舍见迁移文件注释）=====
    /// 本体表名——**表型资产的权威判据**（非空即「本体是本项目/全局分析库里的表」）。
    ///
    /// `kind` 串暂时仍写 `analysis`（库层 CHECK 未扩：迁移在事务内执行、`foreign_keys`
    /// 关不掉，重建表要独立窗口），所以**不要直接读 `kind` 下结论**，
    /// 真实档位一律经 `ArchiveKind::resolve(kind, analytics_table)` 还原。
    pub analytics_table: Option<String>,
    /// 出处种类：`scratchpad` / `connection_table` / `temp_table` / `result_set` /
    /// `archive` / `global`（迁移把旧的 `promoted_from` 两义拆进这里）。
    pub origin_kind: Option<String>,
    /// 出处引用（JSON，形状随 `origin_kind` 而定）。
    pub origin_ref: Option<String>,
    /// 能否被 DuckDB 直接加载（1 = 结构化数据文件）；归档时探测一次即定。
    pub structured: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AnalyticsFolder {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub parent_folder_id: Option<String>,
    pub sort_order: i32,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AnalyticsTag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResourceVersion {
    pub id: String,
    pub resource_id: String,
    pub version: i32,
    pub snapshot: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CreateFolderRequest {
    pub name: String,
    pub scope: String,
    pub parent_folder_id: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CreateTagRequest {
    pub name: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub scope: String,
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AnalyticsResource {
    pub id: String,
    pub resource_type: String,
    pub name: String,
    pub alias: Option<String>,
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
pub struct AnalyticsRecycleItem {
    pub id: String,
    pub resource_id: String,
    pub resource_type: String,
    pub resource_name: String,
    pub resource_data: Value,
    pub deleted_by: Option<String>,
    pub deleted_at: DateTime<Utc>,
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
pub struct CreateResourceRequest {
    pub resource_type: String,
    pub name: String,
    pub alias: Option<String>,
    pub config: Value,
    pub scope: String,
    pub row_count: Option<i32>,
    pub column_count: Option<i32>,
    pub file_size: Option<i32>,
    pub parent_resource_id: Option<String>,
    pub source_query: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ListResourcesOutput {
    pub items: Vec<AnalyticsResource>,
    pub total: i32,
    pub page: i32,
    pub page_size: i32,
    pub total_pages: i32,
}

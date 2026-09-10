//! TypeScript 类型导出模块
//!
//! 这个模块包含所有需要与前端共享的类型定义，
//! 并通过 specta 自动生成 TypeScript 类型文件。
//!
//! v3.0: ts-rs → specta 迁移，类型通过 specta::collect_types! + ts::export 导出

use serde::{Deserialize, Serialize};
use specta::Type;

// ============================================================================
// 元数据缓存服务类型 (metadata_commands.rs)
// ============================================================================

/// 数据库元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct DatabaseMeta {
    pub name: String,
}

/// 数据库元数据 (别名)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct CatalogMeta {
    pub name: String,
}

/// Schema 元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct SchemaMeta {
    pub name: String,
}

/// 表元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct TableMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub table_type: String,
}

/// 视图元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct ViewMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub view_type: String,
}

/// 列元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct ColumnMeta {
    pub name: String,
    #[serde(rename = "dataType")]
    pub data_type: String,
    #[serde(rename = "isNullable")]
    pub is_nullable: bool,
    #[serde(rename = "defaultValue")]
    pub default_value: Option<String>,
    #[serde(rename = "isPrimaryKey")]
    pub is_primary_key: bool,
    #[serde(rename = "isForeignKey")]
    pub is_foreign_key: bool,
    pub comment: Option<String>,
}

/// 存储过程元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct ProcedureMeta {
    pub name: String,
}

/// 函数元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct FunctionMeta {
    pub name: String,
}

/// 例程源码元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct RoutineSourceMeta {
    pub name: String,
    #[serde(rename = "routineKind")]
    pub routine_kind: String,
    #[serde(rename = "sourceCode")]
    pub source_code: Option<String>,
}

/// 索引元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct IndexMeta {
    pub name: String,
    #[serde(rename = "tableName")]
    pub table_name: String,
    #[serde(rename = "columnNames")]
    pub column_names: Vec<String>,
    #[serde(rename = "isUnique")]
    pub is_unique: bool,
    #[serde(rename = "isPrimary")]
    pub is_primary: bool,
    #[serde(rename = "indexType")]
    pub index_type: Option<String>,
    pub comment: Option<String>,
}

/// 约束元数据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct ConstraintMeta {
    pub name: String,
    #[serde(rename = "tableName")]
    pub table_name: String,
    #[serde(rename = "constraintType")]
    pub constraint_type: String,
    #[serde(rename = "columnNames")]
    pub column_names: Vec<String>,
    #[serde(rename = "referencedTable")]
    pub referenced_table: Option<String>,
    #[serde(rename = "referencedColumns")]
    pub referenced_columns: Vec<String>,
    #[serde(rename = "updateRule")]
    pub update_rule: Option<String>,
    #[serde(rename = "deleteRule")]
    pub delete_rule: Option<String>,
}

/// 缓存统计信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct CacheStats {
    pub l1_hits: u32,
    pub l1_misses: u32,
    pub l2_hits: u32,
    pub l2_misses: u32,
    pub db_queries: u32,
    pub l1_hit_avg_us: u32,
    pub l2_hit_avg_us: u32,
    pub db_query_avg_us: u32,
    pub l1_hit_rate: f64,
    pub l2_hit_rate: f64,
    pub overall_hit_rate: f64,
}

// ============================================================================
// 元数据缓存服务类型 (metadata_cache_commands.rs)
// ============================================================================

/// 缓存状态响应
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct CacheStatusResponse {
    pub is_valid: bool,
    pub last_sync: Option<i32>,
    pub stats: Option<CacheStatsResponse>,
}

/// 缓存统计响应
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct CacheStatsResponse {
    pub table_count: u32,
    pub column_count: u32,
    pub last_sync: Option<i32>,
}

/// 刷新缓存请求
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct RefreshCacheInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub database_name: String,
    pub schema_name: Option<String>,
}

/// 清除缓存请求
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct ClearCacheInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub database_name: String,
    pub schema_name: Option<String>,
}

/// 表元数据输入
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct TableInput {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
}

/// 列元数据输入
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct ColumnInput {
    pub id: String,
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary: bool,
    pub is_unique: bool,
}

/// DDL 事件输入
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct DDLEventInput {
    #[serde(rename = "type")]
    pub ddl_type: String,
    pub connection_id: String,
    pub connection_type: Option<String>,
    pub project_path: Option<String>,
    pub database_name: String,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
    pub column_name: Option<String>,
    pub executed_at: Option<f64>,
}

/// 同步状态信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct SyncStatusInfo {
    pub in_progress: bool,
    pub total_tables: u32,
    pub completed_tables: u32,
    pub last_sync_time: Option<i32>,
}

// ============================================================================
// 缓存预热服务类型 (cache_warming_commands.rs)
// ============================================================================

/// 预热进度响应
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct WarmingProgressResponse {
    pub connection_id: String,
    pub is_warming: bool,
    pub current_step: String,
    pub total_steps: u32,
    pub completed_steps: u32,
    pub progress_percentage: f64,
    pub current_database: Option<String>,
    pub current_schema: Option<String>,
    pub current_table: Option<String>,
}

/// 预热请求
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct WarmCacheInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub databases: Vec<String>,
}

/// 取消预热请求
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct CancelWarmingInput {
    pub connection_id: String,
}

/// 版本迁移响应
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct MigrationResponse {
    pub from_version: u32,
    pub to_version: u32,
    pub success: bool,
    pub duration_ms: Option<u32>,
    pub message: String,
}

/// V7: 构建缓存索引请求（支持增量模式）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct BuildCacheIndexInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub source_connection_id: String,
    pub database: String,
    pub schema: Option<String>,
    pub incremental: Option<bool>,
}

/// V7: 索引构建响应（支持增量模式）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct IndexBuildResponse {
    pub success: bool,
    pub schema_count: u32,
    pub table_count: u32,
    pub column_count: u32,
    pub total_entries: u32,
    pub message: String,
    pub incremental: Option<bool>,
    pub create_count: Option<u32>,
    pub update_count: Option<u32>,
    pub delete_count: Option<u32>,
}

/// V7: Schema 对象数量统计响应
#[derive(Debug, Clone, Serialize, Deserialize, Type)]

pub struct SchemaObjectCountsResponse {
    pub table_count: u32,
    pub view_count: u32,
    pub column_count: u32,
    pub routine_count: u32,
    pub total: u32,
}

// ⚠️ 类型导出已迁移至 lib.rs 中的 specta::collect_types! + tauri_specta::ts::export
// 废弃原因: ts-rs → specta 迁移 (v3.0)

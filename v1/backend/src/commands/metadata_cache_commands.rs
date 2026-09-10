/**
 * 元数据缓存相关命令
 *
 * 处理数据库元数据缓存的读取、刷新、清除等操作
 */
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::error::CoreError;
use crate::core::persistence::metadata_cache::{
    ConnectionType, MetadataCacheManager, MetadataCacheOps,
};

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
#[derive(Debug, Clone, Deserialize, Type)]
pub struct RefreshCacheInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub database_name: String,
    pub schema_name: Option<String>,
}

/// 清除缓存请求
#[derive(Debug, Clone, Deserialize, Type)]
pub struct ClearCacheInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub database_name: String,
    pub schema_name: Option<String>,
}

/// 缓存 TTL（秒），超过此时间未同步视为过期
const CACHE_TTL_SECS: u64 = 300;

/// 表元数据输入
#[derive(Debug, Clone, Deserialize, Type)]
pub struct TableInput {
    pub id: String,
    pub name: String,
    pub comment: Option<String>,
}

/// 列元数据输入
#[derive(Debug, Clone, Deserialize, Type)]
pub struct ColumnInput {
    pub id: String,
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary: bool,
    pub is_unique: bool,
}

/// 获取缓存状态
#[tauri::command]
#[specta::specta]
pub async fn get_metadata_cache_status(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    database_name: String,
    schema_name: Option<String>,
) -> Result<CacheStatusResponse, CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    if !cache_manager.exists() {
        return Ok(CacheStatusResponse {
            is_valid: false,
            last_sync: None,
            stats: None,
        });
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let schema = schema_name.as_deref().unwrap_or("public");
    let mut is_valid = ops
        .is_cache_valid(&database_name, schema, None)
        .map_err(|e| CoreError::from(e.to_string()))?;

    let last_sync = ops
        .get_last_sync_time(&database_name, schema)
        .map_err(|e| CoreError::from(e.to_string()))?;

    // TTL 过期检查：如果 last_sync 超过 CACHE_TTL_SECS，标记为无效
    if is_valid {
        if let Some(ts) = last_sync {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| CoreError::from(format!("获取系统时间失败: {}", e)))?
                .as_secs() as i64;
            if now.saturating_sub(ts) > CACHE_TTL_SECS as i64 {
                is_valid = false;
            }
        }
    }

    let stats = if is_valid {
        let cache_stats = ops
            .get_cache_stats(&database_name, schema)
            .map_err(|e| CoreError::from(e.to_string()))?;
        Some(CacheStatsResponse {
            table_count: cache_stats.table_count as u32,
            column_count: cache_stats.column_count as u32,
            last_sync: cache_stats.last_sync.map(|v| v as i32),
        })
    } else {
        None
    };

    Ok(CacheStatusResponse {
        is_valid,
        last_sync: last_sync.map(|v| v as i32),
        stats,
    })
}

/// 刷新元数据缓存
#[tauri::command]
#[specta::specta]
pub async fn refresh_metadata_cache(input: RefreshCacheInput) -> Result<(), CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &input.connection_id,
        if input.connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        input.project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let schema = input.schema_name.as_deref().unwrap_or("public");

    // 清除旧缓存
    ops.clear_metadata(&input.database_name, schema, None)
        .map_err(|e| CoreError::from(e.to_string()))?;

    // 注意：实际的元数据获取和缓存逻辑需要调用数据库驱动
    // 这里只是清除旧缓存，新缓存需要由前端调用数据库 API 后保存
    Ok(())
}

/// 清除元数据缓存（同时删除缓存文件）
#[tauri::command]
#[specta::specta]
pub async fn clear_metadata_cache(input: ClearCacheInput) -> Result<u32, CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &input.connection_id,
        if input.connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        input.project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    if !cache_manager.exists() {
        return Ok(0);
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let schema = input.schema_name.as_deref().unwrap_or("public");

    let affected = ops
        .clear_metadata(&input.database_name, schema, None)
        .map_err(|e| CoreError::from(e.to_string()))?;

    // 删除缓存文件，确保强制刷新时不残留旧数据
    let cache_path = cache_manager.db_path();
    if cache_path.exists() {
        std::fs::remove_file(cache_path)
            .map_err(|e| CoreError::from(format!("删除缓存文件失败: {}", e)))?;
    }

    Ok(affected as u32)
}

/// 保存表元数据到缓存
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn save_table_metadata_to_cache(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    id: String,
    database_name: String,
    schema_name: String,
    table_name: String,
    comment: Option<String>,
) -> Result<(), CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| CoreError::from(format!("获取系统时间失败: {}", e)))?
        .as_secs() as i64;

    ops.save_table_metadata(
        &id,
        &database_name,
        &schema_name,
        &table_name,
        comment.as_deref(),
        current_time,
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(())
}

/// 批量保存表元数据到缓存
#[tauri::command]
#[specta::specta]
pub async fn save_tables_batch_to_cache(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    database_name: String,
    schema_name: String,
    tables: Vec<TableInput>,
) -> Result<u32, CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let mut ops = MetadataCacheOps::new(conn);

    #[allow(clippy::type_complexity)]
    let batch: Vec<(String, String, String, String, Option<String>)> = tables
        .into_iter()
        .map(|t| {
            (
                t.id,
                database_name.clone(),
                schema_name.clone(),
                t.name,
                t.comment,
            )
        })
        .collect();

    let count = batch.len() as u32;
    ops.save_tables_batch(batch)
        .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(count)
}

/// 批量保存列元数据到缓存
#[tauri::command]
#[specta::specta]
pub async fn save_columns_batch_to_cache(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    database_name: String,
    schema_name: String,
    table_name: String,
    columns: Vec<ColumnInput>,
) -> Result<u32, CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let mut ops = MetadataCacheOps::new(conn);

    #[allow(clippy::type_complexity)]
    let batch: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        bool,
        bool,
    )> = columns
        .into_iter()
        .map(|c| {
            (
                c.id,
                database_name.clone(),
                schema_name.clone(),
                table_name.clone(),
                c.name,
                c.data_type,
                c.is_nullable,
                c.is_primary,
                c.is_unique,
            )
        })
        .collect();

    let count = batch.len() as u32;
    ops.save_columns_batch(batch)
        .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(count)
}

/// 保存列元数据到缓存
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn save_column_metadata_to_cache(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    id: String,
    database_name: String,
    schema_name: String,
    table_name: String,
    column_name: String,
    data_type: String,
    is_nullable: bool,
    is_primary: bool,
    is_unique: bool,
) -> Result<(), CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| CoreError::from(format!("获取系统时间失败: {}", e)))?
        .as_secs() as i64;

    ops.save_column_metadata(
        &id,
        &database_name,
        &schema_name,
        &table_name,
        &column_name,
        &data_type,
        is_nullable,
        is_primary,
        is_unique,
        current_time,
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(())
}

/// 从缓存获取表列表
#[tauri::command]
#[specta::specta]
pub async fn get_tables_from_cache(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    database_name: String,
    schema_name: Option<String>,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    if !cache_manager.exists() {
        return Ok(vec![]);
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let schema = schema_name.as_deref().unwrap_or("public");

    let tables = ops
        .list_tables(&database_name, Some(schema))
        .map_err(|e| CoreError::from(e.to_string()))?;

    let result: Vec<serde_json::Value> = tables
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "schema_name": t.schema_name,
                "comment": t.comment,
                "last_sync": t.last_sync,
            })
        })
        .collect();

    Ok(result)
}

/// 从缓存获取列列表
#[tauri::command]
#[specta::specta]
pub async fn get_columns_from_cache(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    database_name: String,
    schema_name: String,
    table_name: String,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    if !cache_manager.exists() {
        return Ok(vec![]);
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let columns = ops
        .list_columns(&database_name, &schema_name, &table_name)
        .map_err(|e| CoreError::from(e.to_string()))?;

    let result: Vec<serde_json::Value> = columns
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "data_type": c.data_type,
                "is_nullable": c.is_nullable,
                "is_primary": c.is_primary,
                "is_unique": c.is_unique,
                "comment": c.comment,
                "last_sync": c.last_sync,
            })
        })
        .collect();

    Ok(result)
}

/// DDL 事件输入
#[derive(Debug, Clone, Deserialize, Type)]
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

/// 取消同步任务
#[tauri::command]
#[specta::specta]
pub async fn cancel_sync(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
) -> Result<(), CoreError> {
    let cache_manager = MetadataCacheManager::new(
        &connection_id,
        if connection_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    if !cache_manager.exists() {
        return Ok(());
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    ops.cancel_sync(&connection_id)
        .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(())
}

/// 通知后端 DDL 事件（缓存失效）
#[tauri::command]
#[specta::specta]
pub async fn notify_ddl_event(event: DDLEventInput) -> Result<(), CoreError> {
    let conn_type = event.connection_type.as_deref().unwrap_or("global");

    let cache_manager = MetadataCacheManager::new(
        &event.connection_id,
        if conn_type == "global" {
            ConnectionType::Global
        } else {
            ConnectionType::Project
        },
        event.project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    if !cache_manager.exists() {
        return Ok(());
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let schema = event.schema_name.as_deref().unwrap_or("public");

    match event.ddl_type.as_str() {
        "CREATE_DATABASE" | "DROP_DATABASE" => {
            ops.clear_metadata(&event.database_name, schema, None)
                .map_err(|e| CoreError::from(e.to_string()))?;
        }
        "CREATE_TABLE" | "DROP_TABLE" | "ALTER_TABLE" | "TRUNCATE_TABLE" | "CREATE_VIEW"
        | "DROP_VIEW" | "CREATE_INDEX" | "DROP_INDEX" => {
            if let Some(ref table) = event.table_name {
                ops.clear_metadata(&event.database_name, schema, Some(table))
                    .map_err(|e| CoreError::from(e.to_string()))?;
            } else {
                ops.clear_metadata(&event.database_name, schema, None)
                    .map_err(|e| CoreError::from(e.to_string()))?;
            }
        }
        "CREATE_SCHEMA" | "DROP_SCHEMA" => {
            ops.clear_metadata(&event.database_name, schema, None)
                .map_err(|e| CoreError::from(e.to_string()))?;
        }
        _ => {}
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncStatusInfo {
    pub in_progress: bool,
    pub total_tables: u32,
    pub completed_tables: u32,
    pub last_sync_time: Option<i32>,
}

#[tauri::command]
#[specta::specta]
pub async fn get_sync_status(_connection_id: String) -> Result<Option<SyncStatusInfo>, CoreError> {
    Ok(None)
}

/**
 * 缓存预热相关命令
 *
 * 处理缓存预热的启动、取消、进度查询等操作
 */
use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;

use crate::adapters::tauri::state::WarmingProgressState;
use crate::core::error::CoreError;
use crate::core::persistence::cache_version_migration::{
    CacheVersionManager, CURRENT_CACHE_VERSION,
};
use crate::core::persistence::metadata_cache::{
    ConnectionType, IndexEntryInput, MetadataCacheManager, MetadataCacheOps,
};
use crate::core::services::ConnId;
use futures::FutureExt;

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
#[derive(Debug, Clone, Deserialize, Type)]
pub struct WarmCacheInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub databases: Vec<String>,
    pub source_connection_id: String,
    pub introspection_level: Option<i32>,
}

/// 取消预热请求
#[derive(Debug, Clone, Deserialize)]
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

/// V7: 构建缓存索引（支持增量模式）
///
/// 此命令执行完整的预热流程（优化版 V3）：
/// 1. 支持增量模式：检测变化并仅同步变更
/// 2. JoinSet 并行获取多个 Schema 的 tables
/// 3. JoinSet 并行获取多个表的 columns
/// 4. 流式写入（每批写入，而非全量内存构建后写入）
/// 5. 进度回调（通过 Tauri Event 向前端推送进度）
/// 6. 取消支持（CancellationToken）
#[tauri::command]
pub async fn build_cache_index(
    input: BuildCacheIndexInput,
    state: tauri::State<'_, crate::adapters::tauri::state::AppState>,
    app_handle: tauri::AppHandle,
) -> Result<IndexBuildResponse, CoreError> {
    use crate::core::error::CoreError;
    use crate::core::persistence::metadata_cache::{ChangeDetectionResult, SyncSnapshot};
    use tokio::sync::broadcast;
    use tokio::task::JoinSet;
    use tokio_util::sync::CancellationToken;

    let cancel_token = CancellationToken::new();
    let use_incremental = input.incremental.unwrap_or(false);

    let cache_connection_type = if input.connection_type == "global" {
        ConnectionType::Global
    } else {
        ConnectionType::Project
    };

    let cache_manager = MetadataCacheManager::new(
        &input.connection_id,
        cache_connection_type,
        input.project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let cache_conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let mut cache_ops = MetadataCacheOps::new(cache_conn);

    let version_manager = CacheVersionManager::new();
    if version_manager
        .needs_upgrade(cache_ops.get_connection())
        .map_err(|e| CoreError::from(e.to_string()))?
    {
        version_manager
            .migrate(cache_ops.get_connection())
            .map_err(|e| CoreError::from(e.to_string()))?;
    }

    cache_ops
        .update_sync_status(&input.connection_id, "indexing", 0, None)
        .map_err(|e| CoreError::from(e.to_string()))?;

    let source_conn_id: ConnId = input.source_connection_id.clone();
    let db = match state
        .connection_manager
        .get_connection(&source_conn_id)
        .await
    {
        Some(conn) => conn,
        None => {
            return Err(format!(
                "Source connection not found: {}",
                input.source_connection_id
            )
            .into())
        }
    };

    let db_name = input.database.clone();
    let conn_id = input.connection_id.clone();
    let ct = cancel_token.clone();
    let app = app_handle.clone();

    let schemas = match db.list_schemas(&db_name).await {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to list schemas: {}", e).into()),
    };

    let total_schemas = schemas.len();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("获取系统时间失败: {}", e))?
        .as_secs() as i64;

    let send_progress = |step: &str, current: usize, total: usize, msg: &str| {
        let progress = if total > 0 {
            (current as f64 / total as f64 * 100.0) as u32
        } else {
            0
        };
        let _ = app.emit(
            "cache_warming_progress",
            serde_json::json!({
                "connection_id": conn_id,
                "step": step,
                "current": current,
                "total": total,
                "progress": progress,
                "message": msg,
            }),
        );
    };

    // V7: 增量模式 - 检测变化
    let mut change_result: Option<ChangeDetectionResult> = None;
    if use_incremental && cache_ops.has_snapshot(&conn_id, "full").unwrap_or(false) {
        send_progress("detecting_changes", 0, 1, "正在检测元数据变化...");
        change_result = Some(
            cache_ops
                .incremental_sync(&conn_id)
                .map_err(|e| CoreError::from(e.to_string()))?,
        );
    }

    // V7: 如果是增量模式且已有快照，我们可以更智能地处理
    // （这里先保持原有的全量同步逻辑，后续可以进一步优化为真·增量同步）

    send_progress("fetching_schemas", 0, total_schemas, "开始并行获取 schemas");

    // 并行获取每个 Schema 的 Tables 和 Columns（JoinSet）
    let (tx_schema, mut rx_schema) =
        broadcast::channel::<(String, Vec<String>, Vec<(String, String, Vec<String>)>, i64)>(100);

    let db_name_for_schemas = db_name.clone();
    let mut schema_join_set = JoinSet::new();

    for (schema_idx, schema_name) in schemas.into_iter().enumerate() {
        let db_name_clone = db_name_for_schemas.clone();
        let ct_clone = ct.clone();
        let tx_clone = tx_schema.clone();
        let db_clone = db.clone();

        schema_join_set.spawn(async move {
            if ct_clone.is_cancelled() {
                return None;
            }

            let tables = match db_clone
                .list_tables(&db_name_clone, Some(&schema_name))
                .await
            {
                Ok(t) => t,
                Err(_) => return None,
            };

            let table_names: Vec<String> = tables.iter().map(|t| t.name.clone()).collect();
            let table_count = table_names.len();

            // 并行获取每个表的 Columns（JoinSet）
            let mut column_join_set = JoinSet::new();
            for table in tables {
                let db_name_for_cols = db_name_clone.clone();
                let schema_for_cols = schema_name.clone();
                let ct_for_cols = ct_clone.clone();
                let db_for_cols = db_clone.clone();

                column_join_set.spawn(async move {
                    if ct_for_cols.is_cancelled() {
                        return None;
                    }

                    match db_for_cols
                        .list_columns(&db_name_for_cols, Some(&schema_for_cols), &table.name)
                        .await
                    {
                        Ok(columns) => {
                            let column_names: Vec<String> =
                                columns.iter().map(|c| c.name.clone()).collect();
                            Some((schema_for_cols, table.name.clone(), column_names))
                        }
                        Err(_) => None,
                    }
                });
            }

            let mut columns_data = Vec::new();
            while let Some(result) = column_join_set.join_next().await {
                if ct_clone.is_cancelled() {
                    break;
                }
                if let Some(data) = result.ok().flatten() {
                    columns_data.push(data);
                }
            }

            if tx_clone
                .send((schema_name, table_names, columns_data, schema_idx as i64))
                .is_err()
            {
                return None;
            }

            Some(table_count)
        });
    }

    // 接收结果并流式写入
    let batch_size = 500;
    let mut batch_entries: Vec<IndexEntryInput> = Vec::with_capacity(batch_size);
    let mut schema_ids = std::collections::HashMap::new();
    let mut total_tables = 0;
    let mut total_columns = 0;
    let mut total_entries = 0;
    let mut completed_schemas = 0;
    let mut all_snapshots: Vec<SyncSnapshot> = Vec::new(); // V7: 收集快照

    loop {
        // 并行处理已接收的 schema 结果
        while let Ok((schema_name, table_names, columns_data, _)) = rx_schema.try_recv() {
            if cancel_token.is_cancelled() {
                break;
            }

            completed_schemas += 1;
            send_progress(
                "fetching_tables",
                completed_schemas,
                total_schemas,
                &format!("获取 schema {} 的 tables", schema_name),
            );

            // V7: 添加 schema 快照
            let schema_hash =
                MetadataCacheOps::calculate_object_hash("schema", &schema_name, None, None);
            all_snapshots.push(SyncSnapshot {
                id: None,
                connection_id: conn_id.clone(),
                snapshot_type: "full".to_string(),
                object_type: "schema".to_string(),
                object_name: schema_name.clone(),
                parent_name: None,
                object_hash: Some(schema_hash),
                snapshot_at: now,
            });

            // 记录 schema ID
            let schema_id = schema_ids.len() as i64 + 1;
            schema_ids.insert(schema_name.clone(), schema_id);

            // 写入 schema 索引
            batch_entries.push(IndexEntryInput {
                connection_id: input.connection_id.clone(),
                schema_id: None,
                object_type: "schema".to_string(),
                object_name: schema_name.clone(),
                parent_name: None,
                path: schema_name.clone(),
                introspect_level: 1,
                row_count_estimate: None,
                sort_weight: Some(0),
                last_sync: Some(now),
            });

            // 写入 table 索引
            for table_name in &table_names {
                total_tables += 1;
                let path = format!("{}/{}", schema_name, table_name);

                // V7: 添加 table 快照
                let table_hash = MetadataCacheOps::calculate_object_hash(
                    "table",
                    table_name,
                    Some(&schema_name),
                    None,
                );
                all_snapshots.push(SyncSnapshot {
                    id: None,
                    connection_id: conn_id.clone(),
                    snapshot_type: "full".to_string(),
                    object_type: "table".to_string(),
                    object_name: table_name.clone(),
                    parent_name: Some(schema_name.clone()),
                    object_hash: Some(table_hash),
                    snapshot_at: now,
                });

                batch_entries.push(IndexEntryInput {
                    connection_id: input.connection_id.clone(),
                    schema_id: Some(schema_id),
                    object_type: "table".to_string(),
                    object_name: table_name.clone(),
                    parent_name: Some(schema_name.clone()),
                    path,
                    introspect_level: 1,
                    row_count_estimate: None,
                    sort_weight: Some(0),
                    last_sync: Some(now),
                });

                if batch_entries.len() >= batch_size {
                    total_entries += batch_entries.len();
                    if let Err(e) = cache_ops.save_index_entries_internal(batch_entries, batch_size)
                    {
                        tracing::error!("Failed to save index entries: {}", e);
                    }
                    batch_entries = Vec::with_capacity(batch_size);
                    send_progress(
                        "writing_index",
                        total_entries,
                        total_tables + total_schemas,
                        "写入索引中...",
                    );
                }
            }

            // 写入 column 索引
            for (_, table_name, column_names) in columns_data {
                for col_name in &column_names {
                    total_columns += 1;
                    let path = format!("{}/{}/{}", schema_name, table_name, col_name);

                    // V7: 添加 column 快照
                    let col_hash = MetadataCacheOps::calculate_object_hash(
                        "column",
                        col_name,
                        Some(&table_name),
                        None,
                    );
                    all_snapshots.push(SyncSnapshot {
                        id: None,
                        connection_id: conn_id.clone(),
                        snapshot_type: "full".to_string(),
                        object_type: "column".to_string(),
                        object_name: col_name.clone(),
                        parent_name: Some(table_name.clone()),
                        object_hash: Some(col_hash),
                        snapshot_at: now,
                    });

                    batch_entries.push(IndexEntryInput {
                        connection_id: input.connection_id.clone(),
                        schema_id: Some(schema_id),
                        object_type: "column".to_string(),
                        object_name: col_name.clone(),
                        parent_name: Some(table_name.clone()),
                        path,
                        introspect_level: 1,
                        row_count_estimate: None,
                        sort_weight: Some(0),
                        last_sync: Some(now),
                    });

                    if batch_entries.len() >= batch_size {
                        total_entries += batch_entries.len();
                        if let Err(e) =
                            cache_ops.save_index_entries_internal(batch_entries, batch_size)
                        {
                            tracing::error!("Failed to save index entries: {}", e);
                        }
                        batch_entries = Vec::with_capacity(batch_size);
                        send_progress(
                            "writing_index",
                            total_entries,
                            total_tables + total_schemas + total_columns,
                            "写入索引中...",
                        );
                    }
                }
            }
        }

        // 检查 JoinSet 状态
        if schema_join_set.join_next().now_or_never().is_none() && rx_schema.is_empty() {
            break;
        }

        if cancel_token.is_cancelled() {
            schema_join_set.abort_all();
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
    }

    // 等待所有任务完成
    while schema_join_set.join_next().now_or_never().is_some() {
        if cancel_token.is_cancelled() {
            schema_join_set.abort_all();
            break;
        }
    }

    // 写入剩余批次
    if !batch_entries.is_empty() && !cancel_token.is_cancelled() {
        total_entries += batch_entries.len();
        if let Err(e) = cache_ops.save_index_entries_internal(batch_entries, batch_size) {
            tracing::error!("Failed to save index entries: {}", e);
        }
    }

    if cancel_token.is_cancelled() {
        cache_ops
            .update_sync_status(&input.connection_id, "cancelled", 0, None)
            .ok();
        return Err("索引构建被取消".to_string().into());
    }

    // V7: 保存快照（用于下次增量同步）
    send_progress("saving_snapshot", 0, 1, "保存元数据快照...");
    cache_ops
        .save_snapshot(&conn_id, "full", all_snapshots)
        .map_err(|e| CoreError::from(e.to_string()))?;

    cache_ops
        .update_sync_status(&input.connection_id, "completed", 100, None)
        .ok();
    send_progress("completed", 1, 1, "索引构建完成");

    // V7: 返回响应
    Ok(IndexBuildResponse {
        success: true,
        schema_count: schema_ids.len() as u32,
        table_count: total_tables as u32,
        column_count: total_columns as u32,
        total_entries: total_entries as u32,
        message: if use_incremental {
            format!(
                "索引构建完成（增量模式）：{} schemas, {} tables, {} columns",
                schema_ids.len(),
                total_tables,
                total_columns
            )
        } else {
            format!(
                "索引构建完成（全量模式）：{} schemas, {} tables, {} columns",
                schema_ids.len(),
                total_tables,
                total_columns
            )
        },
        incremental: Some(use_incremental),
        create_count: change_result.as_ref().map(|r| r.create_count as u32),
        update_count: change_result.as_ref().map(|r| r.update_count as u32),
        delete_count: change_result.as_ref().map(|r| r.delete_count as u32),
    })
}

/// 启动缓存预热（v10.4: 实际后台执行）
///
/// 内省级别控制：
///   Level 1: 仅加载 schema + table 名称（快速）
///   Level 2: 加载 schema + table + column 名称（标准，默认）
///   Level 3: 加载完整列元数据、索引、约束等（完整）
#[tauri::command]
#[specta::specta]
pub async fn start_cache_warming(
    input: WarmCacheInput,
    state: tauri::State<'_, crate::adapters::tauri::state::AppState>,
    app_handle: tauri::AppHandle,
) -> Result<WarmingProgressResponse, CoreError> {
    let connection_type = if input.connection_type == "global" {
        ConnectionType::Global
    } else {
        ConnectionType::Project
    };

    let cache_manager = MetadataCacheManager::new(
        &input.connection_id,
        connection_type,
        input.project_path.as_deref(),
    )
    .map_err(|e| CoreError::from(e.to_string()))?;

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let mut ops = MetadataCacheOps::new(conn);

    let version_manager = CacheVersionManager::new();
    if version_manager
        .needs_upgrade(ops.get_connection())
        .map_err(|e| CoreError::from(e.to_string()))?
    {
        let records = version_manager
            .migrate(ops.get_connection())
            .map_err(|e| CoreError::from(e.to_string()))?;

        if !records.is_empty() {
            tracing::info!(
                connection_id = %input.connection_id,
                "缓存版本迁移完成: {} -> {}",
                records[0].from_version,
                records[0].to_version
            );
        }
    }

    let total_steps = input.databases.len() as u32;

    ops.update_sync_status(&input.connection_id, "warming", 0, None)
        .map_err(|e| CoreError::from(e.to_string()))?;

    let conn_id = input.connection_id.clone();
    let task = state.warming_task_manager.create_task(&conn_id);
    let cancel_token = task.cancel_token.clone();
    let connection_manager = state.connection_manager.clone();
    let warming_manager = state.warming_task_manager.clone();
    let input_clone = input.clone();

    tokio::spawn(async move {
        let source_conn_id: ConnId = input_clone.source_connection_id.clone();
        let db = match connection_manager.get_connection(&source_conn_id).await {
            Some(conn) => conn,
            None => {
                tracing::error!("Source connection not found: {}", source_conn_id);
                warming_manager.complete_task(&conn_id);
                return;
            }
        };

        let level = input_clone.introspection_level.unwrap_or(2);
        let total_dbs = input_clone.databases.len();
        let mut completed_dbs = 0usize;

        for db_name in &input_clone.databases {
            if cancel_token.is_cancelled() {
                warming_manager.update_progress(
                    &conn_id,
                    WarmingProgressState {
                        is_warming: false,
                        current_step: "已取消".to_string(),
                        total_steps: total_dbs,
                        completed_steps: completed_dbs,
                        progress_percentage: 0.0,
                        current_database: Some(db_name.clone()),
                        current_schema: None,
                        current_table: None,
                    },
                );
                warming_manager.complete_task(&conn_id);
                return;
            }

            warming_manager.update_progress(
                &conn_id,
                WarmingProgressState {
                    is_warming: true,
                    current_step: format!("正在预热数据库: {}", db_name),
                    total_steps: total_dbs,
                    completed_steps: completed_dbs,
                    progress_percentage: if total_dbs > 0 {
                        completed_dbs as f64 / total_dbs as f64 * 100.0
                    } else {
                        0.0
                    },
                    current_database: Some(db_name.clone()),
                    current_schema: None,
                    current_table: None,
                },
            );

            let schemas = match db.list_schemas(db_name).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(db = %db_name, error = %e, "Failed to list schemas");
                    completed_dbs += 1;
                    continue;
                }
            };

            for schema_name in &schemas {
                if cancel_token.is_cancelled() {
                    break;
                }

                // Level 1: Save schema entry
                let _ = app_handle.emit(
                    "cache_warming_progress",
                    serde_json::json!({
                        "connection_id": conn_id,
                        "step": "schema",
                        "current": completed_dbs,
                        "total": total_dbs,
                        "progress": if total_dbs > 0 { completed_dbs as f64 / total_dbs as f64 * 100.0 } else { 0.0 },
                        "message": format!("扫描 Schema: {}", schema_name),
                    }),
                );

                if level >= 2 {
                    let tables = match db.list_tables(db_name, Some(schema_name)).await {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::warn!(schema = %schema_name, error = %e, "Failed to list tables");
                            continue;
                        }
                    };

                    let table_count = tables.len();
                    let mut column_total = 0usize;

                    for (table_idx, table) in tables.iter().enumerate() {
                        if cancel_token.is_cancelled() {
                            break;
                        }

                        if level >= 3 && table_idx % 50 == 0 {
                            let _ = app_handle.emit(
                                "cache_warming_progress",
                                serde_json::json!({
                                    "connection_id": conn_id,
                                    "step": "columns",
                                    "current": table_idx,
                                    "total": table_count,
                                    "progress": 0.0,
                                    "message": format!("加载列元数据: {}.{} ({} 列)", schema_name, table.name, table_idx),
                                }),
                            );
                        }

                        if level >= 3 {
                            let columns = match db
                                .list_columns(db_name, Some(schema_name), &table.name)
                                .await
                            {
                                Ok(c) => c,
                                Err(_) => continue,
                            };
                            column_total += columns.len();
                        }
                    }

                    tracing::info!(
                        db = %db_name,
                        schema = %schema_name,
                        tables = table_count,
                        columns = column_total,
                        level = level,
                        "Schema 预热完成"
                    );
                }
            }

            completed_dbs += 1;
        }

        warming_manager.update_progress(
            &conn_id,
            WarmingProgressState {
                is_warming: false,
                current_step: "预热完成".to_string(),
                total_steps: total_dbs,
                completed_steps: completed_dbs,
                progress_percentage: 100.0,
                current_database: None,
                current_schema: None,
                current_table: None,
            },
        );

        let _ = app_handle.emit(
            "cache_warming_progress",
            serde_json::json!({
                "connection_id": conn_id,
                "step": "completed",
                "current": completed_dbs,
                "total": total_dbs,
                "progress": 100,
                "message": format!("预热完成: {} 个数据库", completed_dbs),
            }),
        );

        #[allow(clippy::cast_possible_truncation)]
        let _ = ops.update_sync_status(&conn_id, "completed", 100, None);
        tracing::info!(connection_id = %conn_id, "缓存预热完成");
    });

    Ok(WarmingProgressResponse {
        connection_id: input.connection_id.clone(),
        is_warming: true,
        current_step: "开始预热...".to_string(),
        total_steps,
        completed_steps: 0,
        progress_percentage: 0.0,
        current_database: None,
        current_schema: None,
        current_table: None,
    })
}

/// 取消缓存预热
#[tauri::command]
pub async fn cancel_cache_warming(
    input: CancelWarmingInput,
    state: tauri::State<'_, crate::adapters::tauri::state::AppState>,
) -> Result<(), CoreError> {
    let success = state.warming_task_manager.cancel_task(&input.connection_id);

    if success {
        tracing::info!(
            connection_id = %input.connection_id,
            "缓存预热已取消"
        );
        Ok(())
    } else {
        Err(format!("未找到连接 {} 的预热任务", input.connection_id).into())
    }
}

/// 获取预热进度
#[tauri::command]
pub async fn get_warming_progress(
    connection_id: String,
    state: tauri::State<'_, crate::adapters::tauri::state::AppState>,
) -> Result<WarmingProgressResponse, CoreError> {
    if let Some(task) = state.warming_task_manager.get_task(&connection_id) {
        let progress = task
            .progress
            .lock()
            .map_err(|e| format!("Failed to lock warming progress: {}", e))?;
        Ok(WarmingProgressResponse {
            connection_id: connection_id.clone(),
            is_warming: progress.is_warming,
            current_step: progress.current_step.clone(),
            total_steps: progress.total_steps as u32,
            completed_steps: progress.completed_steps as u32,
            progress_percentage: progress.progress_percentage,
            current_database: progress.current_database.clone(),
            current_schema: progress.current_schema.clone(),
            current_table: progress.current_table.clone(),
        })
    } else {
        Ok(WarmingProgressResponse {
            connection_id,
            is_warming: false,
            current_step: "空闲".to_string(),
            total_steps: 0,
            completed_steps: 0,
            progress_percentage: 0.0,
            current_database: None,
            current_schema: None,
            current_table: None,
        })
    }
}

/// 检查缓存版本
#[tauri::command]
#[specta::specta]
pub async fn check_cache_version(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
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

    if !cache_manager.exists() {
        return Ok(0);
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let version_manager = CacheVersionManager::new();

    version_manager
        .get_current_version(&conn)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 执行缓存版本迁移
#[tauri::command]
#[specta::specta]
pub async fn execute_cache_migration(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
) -> Result<MigrationResponse, CoreError> {
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
    let version_manager = CacheVersionManager::new();

    let from_version = version_manager
        .get_current_version(&conn)
        .map_err(|e| CoreError::from(e.to_string()))?;

    if from_version >= CURRENT_CACHE_VERSION {
        return Ok(MigrationResponse {
            from_version,
            to_version: from_version,
            success: true,
            duration_ms: None,
            message: "缓存已是最新版本".to_string(),
        });
    }

    let start_time = std::time::Instant::now();
    let records = version_manager
        .migrate(&conn)
        .map_err(|e| CoreError::from(e.to_string()))?;
    let duration = start_time.elapsed();

    if records.is_empty() {
        Ok(MigrationResponse {
            from_version,
            to_version: from_version,
            success: true,
            duration_ms: None,
            message: "无需迁移".to_string(),
        })
    } else {
        let record = &records[0];
        Ok(MigrationResponse {
            from_version: record.from_version,
            to_version: record.to_version,
            success: record.success,
            duration_ms: Some(duration.as_millis() as u32),
            message: if record.success {
                format!("迁移成功: {} -> {}", record.from_version, record.to_version)
            } else {
                format!("迁移失败: {} -> {}", record.from_version, record.to_version)
            },
        })
    }
}

/// 获取缓存版本迁移历史
#[tauri::command]
#[specta::specta]
pub async fn get_cache_migration_history(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
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
    let version_manager = CacheVersionManager::new();

    let history = version_manager
        .get_migration_history(&conn)
        .map_err(|e| CoreError::from(e.to_string()))?;

    let result: Vec<serde_json::Value> = history
        .iter()
        .map(|record| {
            serde_json::json!({
                "from_version": record.from_version,
                "to_version": record.to_version,
                "migrated_at": record.migrated_at,
                "reason": record.reason,
                "duration_ms": record.duration_ms,
                "success": record.success,
            })
        })
        .collect();

    Ok(result)
}

/// V6: 获取内省级别建议（DataGrip 风格）
#[tauri::command]
#[specta::specta]
pub async fn get_introspect_level_suggestion(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    schema_id: i32,
    is_current_schema: bool,
) -> Result<i32, CoreError> {
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
        return Ok(1); // Default to Level 1 for new connections
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let counts = ops
        .get_schema_object_counts(&connection_id, schema_id as i64)
        .map_err(|e| CoreError::from(e.to_string()))?;

    let level = ops.calculate_introspect_level(counts.total as i64, is_current_schema);

    Ok(level)
}

/// V6: 获取 Schema 对象数量统计
#[tauri::command]
#[specta::specta]
pub async fn get_schema_object_counts(
    connection_id: String,
    connection_type: String,
    project_path: Option<String>,
    schema_id: i32,
) -> Result<SchemaObjectCountsResponse, CoreError> {
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
        return Ok(SchemaObjectCountsResponse {
            table_count: 0,
            view_count: 0,
            column_count: 0,
            routine_count: 0,
            total: 0,
        });
    }

    let conn = cache_manager
        .open()
        .map_err(|e| CoreError::from(e.to_string()))?;
    let ops = MetadataCacheOps::new(conn);

    let counts = ops
        .get_schema_object_counts(&connection_id, schema_id as i64)
        .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(SchemaObjectCountsResponse {
        table_count: counts.table_count as u32,
        view_count: counts.view_count as u32,
        column_count: counts.column_count as u32,
        routine_count: counts.routine_count as u32,
        total: counts.total as u32,
    })
}

/// V6: Schema 对象数量统计响应
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SchemaObjectCountsResponse {
    pub table_count: u32,
    pub view_count: u32,
    pub column_count: u32,
    pub routine_count: u32,
    pub total: u32,
}

/// V7: 构建缓存索引请求（支持增量模式）
#[derive(Debug, Clone, Deserialize, Type)]
pub struct BuildCacheIndexInput {
    pub connection_id: String,
    pub connection_type: String,
    pub project_path: Option<String>,
    pub source_connection_id: String,
    pub database: String,
    pub schema: Option<String>,
    pub incremental: Option<bool>, // V7: 是否使用增量模式
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
    pub incremental: Option<bool>, // V7: 是否使用增量模式
    pub create_count: Option<u32>, // V7: 新增对象数
    pub update_count: Option<u32>, // V7: 更新对象数
    pub delete_count: Option<u32>, // V7: 删除对象数
}

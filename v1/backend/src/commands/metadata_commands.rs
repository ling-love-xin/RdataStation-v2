use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::core::cache::CacheManager;
use crate::core::error::{CacheError, CommonError, CoreError};
use crate::core::get_connection_manager;
use crate::core::persistence::metadata_cache::{MetadataCacheManager, MetadataCacheOps};
use crate::core::services::MetadataService;
use crate::core::types::{FunctionMeta, ProcedureMeta, RoutineSourceMeta};

use crate::core::driver::{get_level, remove_level, set_level, IntrospectionLevel};

#[tauri::command]
#[specta::specta]
pub async fn set_introspection_level(conn_id: String, level: String) -> Result<(), CoreError> {
    let il = match level.as_str() {
        "level1" => IntrospectionLevel::Level1,
        "level2" => IntrospectionLevel::Level2,
        "level3" => IntrospectionLevel::Level3,
        _ => {
            return Err(CoreError::common(CommonError::General(format!(
                "Unknown introspection level: {}. Use level1/level2/level3",
                level
            ))))
        }
    };
    set_level(&conn_id, il);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_introspection_level(conn_id: String) -> Result<String, CoreError> {
    Ok(get_level(&conn_id).to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn remove_introspection_level(conn_id: String) -> Result<(), CoreError> {
    remove_level(&conn_id);
    Ok(())
}

static L1_HIT_COUNT: AtomicU64 = AtomicU64::new(0);
static L1_MISS_COUNT: AtomicU64 = AtomicU64::new(0);
static L2_HIT_COUNT: AtomicU64 = AtomicU64::new(0);
static L2_MISS_COUNT: AtomicU64 = AtomicU64::new(0);
static DB_QUERY_COUNT: AtomicU64 = AtomicU64::new(0);
static L1_HIT_TIME_US: AtomicU64 = AtomicU64::new(0);
static L2_HIT_TIME_US: AtomicU64 = AtomicU64::new(0);
static DB_QUERY_TIME_US: AtomicU64 = AtomicU64::new(0);

fn check_l1_cache<T>(
    get_fn: impl FnOnce(&mut crate::core::cache::MetadataCache) -> Option<T>,
) -> Result<Option<T>, CoreError> {
    let instance = CacheManager::instance();
    let cm = instance
        .lock()
        .map_err(|e| CoreError::cache(CacheError::internal(format!("CacheManager lock: {}", e))))?;
    let mc_arc = cm.metadata_cache();
    let mut mc = mc_arc.lock().map_err(|e| {
        CoreError::cache(CacheError::internal(format!("MetadataCache lock: {}", e)))
    })?;
    Ok(get_fn(&mut mc))
}

fn write_l1_cache(
    set_fn: impl FnOnce(&mut crate::core::cache::MetadataCache),
) -> Result<(), CoreError> {
    let instance = CacheManager::instance();
    let cm = instance
        .lock()
        .map_err(|e| CoreError::cache(CacheError::internal(format!("CacheManager lock: {}", e))))?;
    let mc_arc = cm.metadata_cache();
    let mut mc = mc_arc.lock().map_err(|e| {
        CoreError::cache(CacheError::internal(format!("MetadataCache lock: {}", e)))
    })?;
    set_fn(&mut mc);
    Ok(())
}

fn parse_connection_type(t: &str) -> crate::core::persistence::metadata_cache::ConnectionType {
    match t.to_lowercase().as_str() {
        "project" => crate::core::persistence::metadata_cache::ConnectionType::Project,
        _ => crate::core::persistence::metadata_cache::ConnectionType::Global,
    }
}

fn open_l2_cache(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
) -> Result<MetadataCacheOps, CoreError> {
    let ct = parse_connection_type(connection_type);
    let mgr = MetadataCacheManager::new(conn_id, ct, project_path)?;
    if !mgr.exists() {
        return Err(CoreError::cache(CacheError::internal(
            "L2 disk cache not found".to_string(),
        )));
    }
    let conn = mgr.open()?;
    Ok(MetadataCacheOps::new(conn))
}

fn try_l2_databases(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
) -> Result<Option<Vec<DatabaseMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let conn = ops.get_connection();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT catalog_name FROM schemata WHERE catalog_name IS NOT NULL ORDER BY catalog_name",
    )
    .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 databases query: {}", e))))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 databases rows: {}", e))))?
        .filter_map(|r| r.ok())
        .collect();
    if names.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            names
                .into_iter()
                .map(|name| DatabaseMeta { name })
                .collect(),
        ))
    }
}

fn try_l2_schemas(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
) -> Result<Option<Vec<SchemaMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let schema_infos = ops
        .list_schemas(Some(database))
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 schemas list: {}", e))))?;
    if schema_infos.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            schema_infos
                .into_iter()
                .map(|s| SchemaMeta::basic(s.schema_name))
                .collect(),
        ))
    }
}

fn try_l2_tables(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
) -> Result<Option<Vec<TableMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let conn = ops.get_connection();
    let mut stmt = conn
        .prepare(
            "SELECT t.table_name, t.table_type FROM tables t
         INNER JOIN schemata s ON t.schema_id = s.id
         WHERE s.catalog_name = ?1 AND s.schema_name = ?2
         ORDER BY t.table_name",
        )
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 tables query: {}", e))))?;
    let tables: Vec<TableMeta> = stmt
        .query_map(rusqlite::params![database, schema_name], |row| {
            Ok(TableMeta::basic(
                row.get(0)?,
                row.get::<_, String>(1)
                    .unwrap_or_else(|_| "TABLE".to_string()),
            ))
        })
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 tables rows: {}", e))))?
        .filter_map(|r| r.ok())
        .collect();
    if tables.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tables))
    }
}

fn try_l2_columns(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
    table_name: &str,
) -> Result<Option<Vec<ColumnMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let column_infos = ops
        .list_columns(database, schema_name, table_name)
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 columns list: {}", e))))?;
    if column_infos.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            column_infos
                .into_iter()
                .map(|c| ColumnMeta {
                    name: c.name,
                    data_type: c.data_type,
                    is_nullable: c.is_nullable,
                    default_value: None,
                    is_primary_key: c.is_primary,
                    is_foreign_key: false,
                    comment: c.comment,
                })
                .collect(),
        ))
    }
}

fn try_l2_procedures(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
) -> Result<Option<Vec<ProcedureMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let conn = ops.get_connection();
    let mut stmt = conn
        .prepare(
            "SELECT r.routine_name FROM routines r
         INNER JOIN schemata s ON r.schema_id = s.id
         WHERE s.catalog_name = ?1 AND s.schema_name = ?2 AND r.routine_type = 'PROCEDURE'
         ORDER BY r.routine_name",
        )
        .map_err(|e| {
            CoreError::cache(CacheError::internal(format!("L2 procedures query: {}", e)))
        })?;
    let names: Vec<String> = stmt
        .query_map(rusqlite::params![database, schema_name], |row| row.get(0))
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 procedures rows: {}", e))))?
        .filter_map(|r| r.ok())
        .collect();
    if names.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            names
                .into_iter()
                .map(|name| ProcedureMeta { name })
                .collect(),
        ))
    }
}

fn try_l2_functions(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
) -> Result<Option<Vec<FunctionMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let conn = ops.get_connection();
    let mut stmt = conn
        .prepare(
            "SELECT r.routine_name FROM routines r
         INNER JOIN schemata s ON r.schema_id = s.id
         WHERE s.catalog_name = ?1 AND s.schema_name = ?2 AND r.routine_type = 'FUNCTION'
         ORDER BY r.routine_name",
        )
        .map_err(|e| {
            CoreError::cache(CacheError::internal(format!("L2 functions query: {}", e)))
        })?;
    let names: Vec<String> = stmt
        .query_map(rusqlite::params![database, schema_name], |row| row.get(0))
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 functions rows: {}", e))))?
        .filter_map(|r| r.ok())
        .collect();
    if names.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            names
                .into_iter()
                .map(|name| FunctionMeta { name })
                .collect(),
        ))
    }
}

fn try_l2_sequences(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
) -> Result<Option<Vec<SequenceMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let conn = ops.get_connection();
    let mut stmt = conn
        .prepare(
            "SELECT seq.sequence_name FROM sequences seq
         INNER JOIN schemata s ON seq.schema_id = s.id
         WHERE s.catalog_name = ?1 AND s.schema_name = ?2
         ORDER BY seq.sequence_name",
        )
        .map_err(|e| {
            CoreError::cache(CacheError::internal(format!("L2 sequences query: {}", e)))
        })?;
    let names: Vec<String> = stmt
        .query_map(rusqlite::params![database, schema_name], |row| row.get(0))
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 sequences rows: {}", e))))?
        .filter_map(|r| r.ok())
        .collect();
    if names.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            names
                .into_iter()
                .map(|name| SequenceMeta { name })
                .collect(),
        ))
    }
}

fn try_l2_triggers(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
) -> Result<Option<Vec<TriggerMeta>>, CoreError> {
    let ops = open_l2_cache(conn_id, connection_type, project_path)?;
    let conn = ops.get_connection();
    let mut stmt = conn
        .prepare(
            "SELECT trg.trigger_name, t.table_name, trg.trigger_event FROM triggers trg
         INNER JOIN tables t ON trg.table_id = t.id
         INNER JOIN schemata s ON t.schema_id = s.id
         WHERE s.catalog_name = ?1 AND s.schema_name = ?2
         ORDER BY trg.trigger_name",
        )
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 triggers query: {}", e))))?;
    let triggers: Vec<TriggerMeta> = stmt
        .query_map(rusqlite::params![database, schema_name], |row| {
            Ok(TriggerMeta {
                name: row.get(0)?,
                table_name: row.get(1)?,
                event: row.get(2)?,
            })
        })
        .map_err(|e| CoreError::cache(CacheError::internal(format!("L2 triggers rows: {}", e))))?
        .filter_map(|r| r.ok())
        .collect();
    if triggers.is_empty() {
        Ok(None)
    } else {
        Ok(Some(triggers))
    }
}

fn write_l2_tables_after_load(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
    tables: &[TableMeta],
) {
    if let Ok(mut ops) = open_l2_cache_for_write(conn_id, connection_type, project_path) {
        let batch: Vec<(String, String, String, String, Option<String>)> = tables
            .iter()
            .map(|t| {
                (
                    database.to_string(),
                    schema_name.to_string(),
                    t.name.clone(),
                    t.table_type.clone(),
                    None::<String>,
                )
            })
            .collect();
        let _ = ops.save_tables_batch(batch);
    }
}

fn write_l2_columns_after_load(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
    table_name: &str,
    columns: &[ColumnMeta],
) {
    type ColumnBatchEntry = (
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        bool,
        bool,
    );
    if let Ok(mut ops) = open_l2_cache_for_write(conn_id, connection_type, project_path) {
        let batch: Vec<ColumnBatchEntry> = columns
            .iter()
            .map(|c| {
                (
                    database.to_string(),
                    schema_name.to_string(),
                    table_name.to_string(),
                    c.name.clone(),
                    c.data_type.clone(),
                    if c.is_nullable {
                        "YES".to_string()
                    } else {
                        "NO".to_string()
                    },
                    c.is_primary_key,
                    c.is_foreign_key,
                    false,
                )
            })
            .collect();
        let _ = ops.save_columns_batch(batch);
    }
}

fn open_l2_cache_for_write(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
) -> Result<MetadataCacheOps, CoreError> {
    let ct = parse_connection_type(connection_type);
    let mgr = MetadataCacheManager::new(conn_id, ct, project_path)?;
    let conn = mgr.open()?;
    Ok(MetadataCacheOps::new(conn))
}

fn write_l2_indexes_after_load(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
    table_name: &str,
    indexes: Vec<crate::core::driver::IndexDetail>,
) {
    if let Ok(mut ops) = open_l2_cache_for_write(conn_id, connection_type, project_path) {
        let _ = ops.save_indexes_for_table(conn_id, database, schema_name, table_name, indexes);
    }
}

fn write_l2_constraints_after_load(
    conn_id: &str,
    connection_type: &str,
    project_path: Option<&str>,
    database: &str,
    schema_name: &str,
    table_name: &str,
    constraints: Vec<crate::core::driver::ConstraintDetail>,
) {
    if let Ok(mut ops) = open_l2_cache_for_write(conn_id, connection_type, project_path) {
        let _ =
            ops.save_constraints_for_table(conn_id, database, schema_name, table_name, constraints);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn invalidate_metadata_cache(conn_id: String) -> Result<(), CoreError> {
    let instance = CacheManager::instance();
    let cm = instance
        .lock()
        .map_err(|e| CoreError::cache(CacheError::internal(format!("CacheManager lock: {}", e))))?;
    cm.invalidate_connection(&conn_id);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DatabaseMeta {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SchemaMeta {
    pub name: String,
    // V10: Schema 聚合统计（可选，仅当从 schema_stats 视图读取时填充）
    #[serde(rename = "totalTables", skip_serializing_if = "Option::is_none")]
    pub total_tables: Option<i32>,
    #[serde(rename = "totalViews", skip_serializing_if = "Option::is_none")]
    pub total_views: Option<i32>,
    #[serde(rename = "totalProcedures", skip_serializing_if = "Option::is_none")]
    pub total_procedures: Option<i32>,
    #[serde(rename = "totalFunctions", skip_serializing_if = "Option::is_none")]
    pub total_functions: Option<i32>,
    #[serde(rename = "totalSizeBytes", skip_serializing_if = "Option::is_none")]
    pub total_size_bytes: Option<i64>,
    #[serde(rename = "rowCountTotal", skip_serializing_if = "Option::is_none")]
    pub row_count_total: Option<i64>,
}

impl SchemaMeta {
    pub fn basic(name: String) -> Self {
        Self {
            name,
            total_tables: None,
            total_views: None,
            total_procedures: None,
            total_functions: None,
            total_size_bytes: None,
            row_count_total: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TableMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub table_type: String,
    // V10: 企业级统计（可选，仅当查询系统表成功时填充）
    #[serde(rename = "rowCountEstimate", skip_serializing_if = "Option::is_none")]
    pub row_count_estimate: Option<i64>,
    #[serde(rename = "dataLength", skip_serializing_if = "Option::is_none")]
    pub data_length: Option<i64>,
    #[serde(rename = "indexLength", skip_serializing_if = "Option::is_none")]
    pub index_length: Option<i64>,
    #[serde(rename = "displayOrder", skip_serializing_if = "Option::is_none")]
    pub display_order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(rename = "colorLabel", skip_serializing_if = "Option::is_none")]
    pub color_label: Option<String>,
    #[serde(rename = "userComment", skip_serializing_if = "Option::is_none")]
    pub user_comment: Option<String>,
}

impl TableMeta {
    /// 从基础信息创建（兼容旧代码路径）
    pub fn basic(name: String, table_type: String) -> Self {
        Self {
            name,
            table_type,
            row_count_estimate: None,
            data_length: None,
            index_length: None,
            display_order: None,
            hidden: None,
            favorite: None,
            color_label: None,
            user_comment: None,
        }
    }

    /// 从持久化层 TableDetailInfo 创建（含 V10 统计）
    pub fn from_detail(info: &crate::core::persistence::metadata_cache::TableDetailInfo) -> Self {
        Self {
            name: info.table_name.clone(),
            table_type: info.table_type.clone(),
            row_count_estimate: info.row_count_estimate,
            data_length: info.data_length,
            index_length: info.index_length,
            display_order: info.display_order,
            hidden: info.hidden,
            favorite: info.favorite,
            color_label: info.color_label.clone(),
            user_comment: info.user_comment.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ViewMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub view_type: String,
}

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
    #[serde(rename = "comment")]
    pub comment: Option<String>,
}

fn new_metadata_service() -> MetadataService {
    MetadataService::new(get_connection_manager().clone())
}

#[tauri::command]
#[specta::specta]
pub async fn load_databases(
    conn_id: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<DatabaseMeta>, CoreError> {
    let t0 = Instant::now();
    let cached = check_l1_cache(|mc| mc.get_catalogs(&conn_id))?;
    if let Some(names) = cached {
        L1_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        L1_HIT_TIME_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
        return Ok(names
            .into_iter()
            .map(|name| DatabaseMeta { name })
            .collect());
    }
    L1_MISS_COUNT.fetch_add(1, Ordering::Relaxed);

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    let t1 = Instant::now();
    if let Ok(Some(databases)) = try_l2_databases(&conn_id, ct, pp) {
        L2_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        L2_HIT_TIME_US.fetch_add(t1.elapsed().as_micros() as u64, Ordering::Relaxed);
        let names: Vec<String> = databases.iter().map(|d| d.name.clone()).collect();
        let _ = write_l1_cache(|mc| mc.set_catalogs(&conn_id, names));
        return Ok(databases);
    }
    L2_MISS_COUNT.fetch_add(1, Ordering::Relaxed);

    let service = new_metadata_service();
    let t2 = Instant::now();
    let names = service.list_catalogs(&conn_id).await?;
    DB_QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
    DB_QUERY_TIME_US.fetch_add(t2.elapsed().as_micros() as u64, Ordering::Relaxed);

    let _ = write_l1_cache(|mc| mc.set_catalogs(&conn_id, names.clone()));

    Ok(names
        .into_iter()
        .map(|name| DatabaseMeta { name })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CatalogMeta {
    pub name: String,
}

#[tauri::command]
#[specta::specta]
pub async fn load_catalogs(
    conn_id: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<CatalogMeta>, CoreError> {
    let cached = check_l1_cache(|mc| mc.get_catalogs(&conn_id))?;
    if let Some(databases) = cached {
        return Ok(databases
            .into_iter()
            .map(|name| CatalogMeta { name })
            .collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    if let Ok(Some(databases)) = try_l2_databases(&conn_id, ct, pp) {
        let names: Vec<String> = databases.iter().map(|d| d.name.clone()).collect();
        let _ = write_l1_cache(|mc| mc.set_catalogs(&conn_id, names));
        return Ok(databases
            .into_iter()
            .map(|d| CatalogMeta { name: d.name })
            .collect());
    }

    let service = new_metadata_service();
    let names = service.list_catalogs(&conn_id).await?;

    let _ = write_l1_cache(|mc| mc.set_catalogs(&conn_id, names.clone()));

    Ok(names.into_iter().map(|name| CatalogMeta { name }).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn load_schemas(
    conn_id: String,
    db_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<SchemaMeta>, CoreError> {
    let cached = check_l1_cache(|mc| mc.get_schemas(&conn_id, &db_name))?;
    if let Some(names) = cached {
        return Ok(names.into_iter().map(SchemaMeta::basic).collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    if let Ok(Some(schemas)) = try_l2_schemas(&conn_id, ct, pp, &db_name) {
        let names: Vec<String> = schemas.iter().map(|s| s.name.clone()).collect();
        let _ = write_l1_cache(|mc| mc.set_schemas(&conn_id, &db_name, names));
        return Ok(schemas);
    }

    let service = new_metadata_service();
    let names = service.list_schemas(&conn_id, &db_name).await?;

    let _ = write_l1_cache(|mc| mc.set_schemas(&conn_id, &db_name, names.clone()));

    Ok(names.into_iter().map(SchemaMeta::basic).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn load_tables(
    conn_id: String,
    db_name: String,
    schema_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<TableMeta>, CoreError> {
    let cached = check_l1_cache(|mc| mc.get_tables(&conn_id, &db_name, Some(&schema_name)))?;
    if let Some(objects) = cached {
        return Ok(objects
            .into_iter()
            .map(|obj| TableMeta::basic(obj.name, "TABLE".to_string()))
            .collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    if let Ok(Some(tables)) = try_l2_tables(&conn_id, ct, pp, &db_name, &schema_name) {
        let objects: Vec<crate::core::driver::SchemaObject> = tables
            .iter()
            .map(|t| crate::core::driver::SchemaObject {
                name: t.name.clone(),
                kind: crate::core::driver::SchemaObjectKind::Table,
                children: None,
                comment: None,
                table_name: None,
                event: None,
            })
            .collect();
        let _ = write_l1_cache(|mc| mc.set_tables(&conn_id, &db_name, Some(&schema_name), objects));
        return Ok(tables);
    }

    let service = new_metadata_service();
    let objects = service
        .list_tables(&conn_id, &db_name, &schema_name)
        .await?;

    let _ =
        write_l1_cache(|mc| mc.set_tables(&conn_id, &db_name, Some(&schema_name), objects.clone()));

    let table_metas: Vec<TableMeta> = objects
        .iter()
        .map(|obj| TableMeta::basic(obj.name.clone(), "TABLE".to_string()))
        .collect();
    write_l2_tables_after_load(&conn_id, ct, pp, &db_name, &schema_name, &table_metas);

    Ok(table_metas)
}

#[tauri::command]
#[specta::specta]
pub async fn load_views(
    conn_id: String,
    db_name: String,
    schema_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<ViewMeta>, CoreError> {
    let cached = check_l1_cache(|mc| mc.get_tables(&conn_id, &db_name, Some(&schema_name)))?;
    if let Some(objects) = cached {
        return Ok(objects
            .into_iter()
            .filter(|obj| matches!(obj.kind, crate::core::driver::SchemaObjectKind::View))
            .map(|obj| ViewMeta {
                name: obj.name,
                view_type: "VIEW".to_string(),
            })
            .collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    if let Ok(Some(tables)) = try_l2_tables(&conn_id, ct, pp, &db_name, &schema_name) {
        let objects: Vec<crate::core::driver::SchemaObject> = tables
            .iter()
            .map(|t| crate::core::driver::SchemaObject {
                name: t.name.clone(),
                kind: if t.table_type.to_uppercase() == "VIEW" {
                    crate::core::driver::SchemaObjectKind::View
                } else {
                    crate::core::driver::SchemaObjectKind::Table
                },
                children: None,
                comment: None,
                table_name: None,
                event: None,
            })
            .collect();
        let _ = write_l1_cache(|mc| mc.set_tables(&conn_id, &db_name, Some(&schema_name), objects));
        return Ok(tables
            .into_iter()
            .filter(|t| t.table_type.to_uppercase() == "VIEW")
            .map(|t| ViewMeta {
                name: t.name,
                view_type: "VIEW".to_string(),
            })
            .collect());
    }

    let service = new_metadata_service();
    let objects = service
        .list_tables(&conn_id, &db_name, &schema_name)
        .await?;

    let _ =
        write_l1_cache(|mc| mc.set_tables(&conn_id, &db_name, Some(&schema_name), objects.clone()));

    let table_metas: Vec<TableMeta> = objects
        .iter()
        .map(|obj| {
            TableMeta::basic(
                obj.name.clone(),
                if matches!(obj.kind, crate::core::driver::SchemaObjectKind::View) {
                    "VIEW".to_string()
                } else {
                    "TABLE".to_string()
                },
            )
        })
        .collect();
    write_l2_tables_after_load(&conn_id, ct, pp, &db_name, &schema_name, &table_metas);

    Ok(objects
        .into_iter()
        .filter(|obj| matches!(obj.kind, crate::core::driver::SchemaObjectKind::View))
        .map(|obj| ViewMeta {
            name: obj.name,
            view_type: "VIEW".to_string(),
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn load_columns(
    conn_id: String,
    db_name: String,
    schema_name: String,
    table_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<ColumnMeta>, CoreError> {
    let cached = check_l1_cache(|mc| {
        mc.get_columns_detail(&conn_id, &db_name, Some(&schema_name), &table_name)
    })?;
    if let Some(columns) = cached {
        return Ok(columns
            .into_iter()
            .map(|col| ColumnMeta {
                name: col.name,
                data_type: col.data_type,
                is_nullable: col.nullable,
                default_value: col.default_value,
                is_primary_key: col.is_primary_key,
                is_foreign_key: col.is_foreign_key,
                comment: col.comment,
            })
            .collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    if let Ok(Some(columns)) = try_l2_columns(&conn_id, ct, pp, &db_name, &schema_name, &table_name)
    {
        let details: Vec<crate::core::driver::ColumnDetail> = columns
            .iter()
            .map(|c| crate::core::driver::ColumnDetail {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
                nullable: c.is_nullable,
                is_primary_key: c.is_primary_key,
                is_foreign_key: c.is_foreign_key,
                default_value: c.default_value.clone(),
                comment: c.comment.clone(),
                extra: std::collections::HashMap::new(),
            })
            .collect();
        let _ = write_l1_cache(|mc| {
            mc.set_columns_detail(&conn_id, &db_name, Some(&schema_name), &table_name, details)
        });
        return Ok(columns);
    }

    let service = new_metadata_service();
    let columns_detail = service
        .list_columns(&conn_id, &db_name, &schema_name, &table_name)
        .await?;

    let column_metas: Vec<ColumnMeta> = columns_detail
        .iter()
        .map(|col| ColumnMeta {
            name: col.name.clone(),
            data_type: col.data_type.clone(),
            is_nullable: col.nullable,
            default_value: col.default_value.clone(),
            is_primary_key: col.is_primary_key,
            is_foreign_key: col.is_foreign_key,
            comment: col.comment.clone(),
        })
        .collect();

    let _ = write_l1_cache(|mc| {
        mc.set_columns_detail(
            &conn_id,
            &db_name,
            Some(&schema_name),
            &table_name,
            columns_detail,
        )
    });

    write_l2_columns_after_load(
        &conn_id,
        ct,
        pp,
        &db_name,
        &schema_name,
        &table_name,
        &column_metas,
    );

    Ok(column_metas)
}

#[tauri::command]
#[specta::specta]
pub async fn load_procedures(
    conn_id: String,
    db_name: String,
    schema_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<ProcedureMeta>, CoreError> {
    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    let cached = check_l1_cache(|mc| mc.get_procedures(&conn_id, &db_name, Some(&schema_name)))?;
    if let Some(objects) = cached {
        L1_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(objects
            .into_iter()
            .map(|obj| ProcedureMeta { name: obj.name })
            .collect());
    }

    if let Ok(Some(procedures)) = try_l2_procedures(&conn_id, ct, pp, &db_name, &schema_name) {
        L2_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(procedures);
    }
    L2_MISS_COUNT.fetch_add(1, Ordering::Relaxed);

    let service = new_metadata_service();
    let objects = service
        .list_procedures(&conn_id, &db_name, &schema_name)
        .await?;

    let _ = write_l1_cache(|mc| {
        mc.set_procedures(&conn_id, &db_name, Some(&schema_name), objects.clone())
    });

    Ok(objects
        .into_iter()
        .map(|obj| ProcedureMeta { name: obj.name })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn load_functions(
    conn_id: String,
    db_name: String,
    schema_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<FunctionMeta>, CoreError> {
    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    let cached = check_l1_cache(|mc| mc.get_functions(&conn_id, &db_name, Some(&schema_name)))?;
    if let Some(objects) = cached {
        L1_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(objects
            .into_iter()
            .map(|obj| FunctionMeta { name: obj.name })
            .collect());
    }

    if let Ok(Some(functions)) = try_l2_functions(&conn_id, ct, pp, &db_name, &schema_name) {
        L2_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(functions);
    }
    L2_MISS_COUNT.fetch_add(1, Ordering::Relaxed);

    let service = new_metadata_service();
    let objects = service
        .list_functions(&conn_id, &db_name, &schema_name)
        .await?;

    let _ = write_l1_cache(|mc| {
        mc.set_functions(&conn_id, &db_name, Some(&schema_name), objects.clone())
    });

    Ok(objects
        .into_iter()
        .map(|obj| FunctionMeta { name: obj.name })
        .collect())
}

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

impl CacheStats {
    fn snapshot() -> Self {
        let l1_hits = L1_HIT_COUNT.load(Ordering::Relaxed);
        let l1_misses = L1_MISS_COUNT.load(Ordering::Relaxed);
        let l2_hits = L2_HIT_COUNT.load(Ordering::Relaxed);
        let l2_misses = L2_MISS_COUNT.load(Ordering::Relaxed);
        let db_queries = DB_QUERY_COUNT.load(Ordering::Relaxed);
        let l1_hit_time = L1_HIT_TIME_US.load(Ordering::Relaxed);
        let l2_hit_time = L2_HIT_TIME_US.load(Ordering::Relaxed);
        let db_query_time = DB_QUERY_TIME_US.load(Ordering::Relaxed);

        let l1_total = l1_hits + l1_misses;
        let l2_total = l2_hits + l2_misses;

        Self {
            l1_hits: l1_hits as u32,
            l1_misses: l1_misses as u32,
            l2_hits: l2_hits as u32,
            l2_misses: l2_misses as u32,
            db_queries: db_queries as u32,
            l1_hit_avg_us: (l1_hit_time.checked_div(l1_hits).unwrap_or(0)) as u32,
            l2_hit_avg_us: (l2_hit_time.checked_div(l2_hits).unwrap_or(0)) as u32,
            db_query_avg_us: (db_query_time.checked_div(db_queries).unwrap_or(0)) as u32,
            l1_hit_rate: if l1_total > 0 {
                l1_hits as f64 / l1_total as f64
            } else {
                0.0
            },
            l2_hit_rate: if l2_total > 0 {
                l2_hits as f64 / l2_total as f64
            } else {
                0.0
            },
            overall_hit_rate: if l1_total > 0 {
                (l1_hits + l2_hits) as f64 / l1_total as f64
            } else {
                0.0
            },
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_cache_stats() -> Result<CacheStats, CoreError> {
    Ok(CacheStats::snapshot())
}

#[tauri::command]
#[specta::specta]
pub async fn reset_cache_stats() -> Result<(), CoreError> {
    L1_HIT_COUNT.store(0, Ordering::Relaxed);
    L1_MISS_COUNT.store(0, Ordering::Relaxed);
    L2_HIT_COUNT.store(0, Ordering::Relaxed);
    L2_MISS_COUNT.store(0, Ordering::Relaxed);
    DB_QUERY_COUNT.store(0, Ordering::Relaxed);
    L1_HIT_TIME_US.store(0, Ordering::Relaxed);
    L2_HIT_TIME_US.store(0, Ordering::Relaxed);
    DB_QUERY_TIME_US.store(0, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn load_routine_source(
    conn_id: String,
    db_name: String,
    schema_name: String,
    routine_name: String,
    routine_kind: String,
) -> Result<RoutineSourceMeta, CoreError> {
    let kind_str = routine_kind.clone();
    let cached = check_l1_cache(|mc| {
        mc.get_routine_source(
            &conn_id,
            &db_name,
            Some(&schema_name),
            &routine_name,
            &routine_kind,
        )
    })?;
    if let Some(source) = cached {
        return Ok(RoutineSourceMeta {
            name: routine_name,
            routine_kind: kind_str,
            source_code: Some(source),
        });
    }

    let service = new_metadata_service();
    let kind = match routine_kind.as_str() {
        "procedure" => crate::core::driver::SchemaObjectKind::Procedure,
        "function" => crate::core::driver::SchemaObjectKind::Function,
        _ => {
            return Err(CoreError::common(CommonError::invalid_argument(
                "routine_kind",
                format!(
                    "Unknown routine kind: {}. Expected 'procedure' or 'function'",
                    routine_kind
                ),
            )))
        }
    };

    let source = service
        .get_routine_source(&conn_id, &db_name, &schema_name, &routine_name, kind)
        .await?;

    if let Some(ref src) = source {
        let _ = write_l1_cache(|mc| {
            mc.set_routine_source(
                &conn_id,
                &db_name,
                Some(&schema_name),
                &routine_name,
                &routine_kind,
                src.clone(),
            )
        });
    }

    Ok(RoutineSourceMeta {
        name: routine_name,
        routine_kind,
        source_code: source,
    })
}

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
    #[serde(rename = "comment")]
    pub comment: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn load_indexes(
    conn_id: String,
    db_name: String,
    schema_name: String,
    table_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<IndexMeta>, CoreError> {
    let cached =
        check_l1_cache(|mc| mc.get_indexes(&conn_id, &db_name, Some(&schema_name), &table_name))?;
    if let Some(indexes) = cached {
        return Ok(indexes
            .into_iter()
            .map(|idx| IndexMeta {
                name: idx.name,
                table_name: idx.table_name,
                column_names: idx.column_names,
                is_unique: idx.is_unique,
                is_primary: idx.is_primary,
                index_type: idx.index_type,
                comment: idx.comment,
            })
            .collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();

    let service = new_metadata_service();
    let indexes = service
        .list_indexes(&conn_id, &db_name, &schema_name, &table_name)
        .await?;

    let index_metas: Vec<IndexMeta> = indexes
        .iter()
        .map(|idx| IndexMeta {
            name: idx.name.clone(),
            table_name: idx.table_name.clone(),
            column_names: idx.column_names.clone(),
            is_unique: idx.is_unique,
            is_primary: idx.is_primary,
            index_type: idx.index_type.clone(),
            comment: idx.comment.clone(),
        })
        .collect();

    let _ = write_l1_cache(|mc| {
        mc.set_indexes(
            &conn_id,
            &db_name,
            Some(&schema_name),
            &table_name,
            indexes.clone(),
        )
    });

    write_l2_indexes_after_load(
        &conn_id,
        ct,
        pp,
        &db_name,
        &schema_name,
        &table_name,
        indexes,
    );

    Ok(index_metas)
}

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

#[tauri::command]
#[specta::specta]
pub async fn load_constraints(
    conn_id: String,
    db_name: String,
    schema_name: String,
    table_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<ConstraintMeta>, CoreError> {
    let cached = check_l1_cache(|mc| {
        mc.get_constraints(&conn_id, &db_name, Some(&schema_name), &table_name)
    })?;
    if let Some(constraints) = cached {
        return Ok(constraints
            .into_iter()
            .map(|c| ConstraintMeta {
                name: c.name,
                table_name: c.table_name,
                constraint_type: c.constraint_type,
                column_names: c.column_names,
                referenced_table: c.referenced_table,
                referenced_columns: c.referenced_columns,
                update_rule: c.update_rule,
                delete_rule: c.delete_rule,
            })
            .collect());
    }

    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();

    let service = new_metadata_service();
    let constraints = service
        .list_constraints(&conn_id, &db_name, &schema_name, &table_name)
        .await?;

    let constraint_metas: Vec<ConstraintMeta> = constraints
        .iter()
        .map(|c| ConstraintMeta {
            name: c.name.clone(),
            table_name: c.table_name.clone(),
            constraint_type: c.constraint_type.clone(),
            column_names: c.column_names.clone(),
            referenced_table: c.referenced_table.clone(),
            referenced_columns: c.referenced_columns.clone(),
            update_rule: c.update_rule.clone(),
            delete_rule: c.delete_rule.clone(),
        })
        .collect();

    let _ = write_l1_cache(|mc| {
        mc.set_constraints(
            &conn_id,
            &db_name,
            Some(&schema_name),
            &table_name,
            constraints.clone(),
        )
    });

    write_l2_constraints_after_load(
        &conn_id,
        ct,
        pp,
        &db_name,
        &schema_name,
        &table_name,
        constraints,
    );

    Ok(constraint_metas)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SequenceMeta {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TriggerMeta {
    pub name: String,
    #[serde(rename = "tableName")]
    pub table_name: Option<String>,
    #[serde(rename = "event")]
    pub event: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn load_sequences(
    conn_id: String,
    db_name: String,
    schema_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<SequenceMeta>, CoreError> {
    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    let cached = check_l1_cache(|mc| mc.get_sequences(&conn_id, &db_name, Some(&schema_name)))?;
    if let Some(objects) = cached {
        L1_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(objects
            .into_iter()
            .map(|s| SequenceMeta {
                name: s.name.clone(),
            })
            .collect());
    }

    if let Ok(Some(sequences)) = try_l2_sequences(&conn_id, ct, pp, &db_name, &schema_name) {
        L2_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(sequences);
    }
    L2_MISS_COUNT.fetch_add(1, Ordering::Relaxed);

    let service = new_metadata_service();
    let sequences = service
        .list_sequences(&conn_id, &db_name, &schema_name)
        .await?;

    let _ = write_l1_cache(|mc| {
        mc.set_sequences(&conn_id, &db_name, Some(&schema_name), sequences.clone())
    });

    let sequence_metas: Vec<SequenceMeta> = sequences
        .iter()
        .map(|s| SequenceMeta {
            name: s.name.clone(),
        })
        .collect();

    Ok(sequence_metas)
}

#[tauri::command]
#[specta::specta]
pub async fn load_triggers(
    conn_id: String,
    db_name: String,
    schema_name: String,
    connection_type: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<TriggerMeta>, CoreError> {
    let ct = connection_type.as_deref().unwrap_or("global");
    let pp = project_path.as_deref();
    let cached = check_l1_cache(|mc| mc.get_triggers(&conn_id, &db_name, Some(&schema_name)))?;
    if let Some(objects) = cached {
        L1_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(objects
            .into_iter()
            .map(|t| TriggerMeta {
                name: t.name.clone(),
                table_name: t.table_name.clone(),
                event: t.event.clone(),
            })
            .collect());
    }

    if let Ok(Some(triggers)) = try_l2_triggers(&conn_id, ct, pp, &db_name, &schema_name) {
        L2_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok(triggers);
    }
    L2_MISS_COUNT.fetch_add(1, Ordering::Relaxed);

    let service = new_metadata_service();
    let triggers = service
        .list_triggers(&conn_id, &db_name, &schema_name)
        .await?;

    let _ = write_l1_cache(|mc| {
        mc.set_triggers(&conn_id, &db_name, Some(&schema_name), triggers.clone())
    });

    let trigger_metas: Vec<TriggerMeta> = triggers
        .iter()
        .map(|t| TriggerMeta {
            name: t.name.clone(),
            table_name: t.table_name.clone(),
            event: t.event.clone(),
        })
        .collect();

    Ok(trigger_metas)
}

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::error::{CoreError, StorageError};

/// 数据源类型（MySQL, PostgreSQL, Oracle 等数据库大类）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DataSourceType {
    pub id: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

/// 驱动定义，描述驱动的元数据、连接参数 Schema、下载地址和版本信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Driver {
    pub id: String,
    pub type_id: String,
    pub name: String,
    pub driver_kind: String,
    pub is_file: bool,
    pub default_port: Option<i32>,
    pub url_template: Option<String>,
    pub download_url: Option<String>,
    pub download_checksum: Option<String>,
    pub version: Option<String>,
    pub config_schema: String,
    pub supported_auth_types: Option<String>,
    pub capabilities: Option<String>,
    pub driver_properties: Option<String>,
    pub enabled: bool,
}

/// 本机已安装的外部驱动文件注册信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverFile {
    pub id: String,
    pub driver_id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: Option<i32>,
    pub checksum: Option<String>,
    pub version: String,
    pub installed_at: String,
    pub updated_at: String,
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "driver_store".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 获取所有已启用的数据源类型，按分类和名称排序
pub fn get_data_source_types(conn: &Connection) -> Result<Vec<DataSourceType>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, category, icon, enabled, created_at
             FROM data_source_types ORDER BY category, name",
        )
        .map_err(|e| storage_err("prepare_get_data_source_types", e.to_string()))?;

    let items = stmt
        .query_map([], |row| {
            Ok(DataSourceType {
                id: row.get(0)?,
                name: row.get(1)?,
                category: row.get(2)?,
                icon: row.get(3)?,
                enabled: row.get::<_, i32>(4)? != 0,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| storage_err("query_data_source_types", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(items)
}

/// 根据驱动 ID 获取单个驱动定义
pub fn get_driver(conn: &Connection, id: &str) -> Result<Option<Driver>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, type_id, name, driver_kind, is_file, default_port,
                    url_template, download_url, download_checksum, version,
                    config_schema, supported_auth_types, capabilities, driver_properties, enabled
             FROM drivers WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_driver", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(Driver {
            id: row.get(0)?,
            type_id: row.get(1)?,
            name: row.get(2)?,
            driver_kind: row.get(3)?,
            is_file: row.get::<_, i32>(4)? != 0,
            default_port: row.get(5)?,
            url_template: row.get(6)?,
            download_url: row.get(7)?,
            download_checksum: row.get(8)?,
            version: row.get(9)?,
            config_schema: row.get(10)?,
            supported_auth_types: row.get(11)?,
            capabilities: row.get(12)?,
            driver_properties: row.get(13)?,
            enabled: row.get::<_, i32>(14)? != 0,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_driver", e.to_string()))
}

/// 获取所有驱动定义
pub fn get_all_drivers(conn: &Connection) -> Result<Vec<Driver>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, type_id, name, driver_kind, is_file, default_port,
                    url_template, download_url, download_checksum, version,
                    config_schema, supported_auth_types, capabilities, driver_properties, enabled
             FROM drivers ORDER BY name",
        )
        .map_err(|e| storage_err("prepare_get_all_drivers", e.to_string()))?;

    let items = stmt
        .query_map([], |row| {
            Ok(Driver {
                id: row.get(0)?,
                type_id: row.get(1)?,
                name: row.get(2)?,
                driver_kind: row.get(3)?,
                is_file: row.get::<_, i32>(4)? != 0,
                default_port: row.get(5)?,
                url_template: row.get(6)?,
                download_url: row.get(7)?,
                download_checksum: row.get(8)?,
                version: row.get(9)?,
                config_schema: row.get(10)?,
                supported_auth_types: row.get(11)?,
                capabilities: row.get(12)?,
                driver_properties: row.get(13)?,
                enabled: row.get::<_, i32>(14)? != 0,
            })
        })
        .map_err(|e| storage_err("query_all_drivers", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(items)
}

/// 获取指定数据源类型下的所有已启用驱动
pub fn get_drivers_by_type(conn: &Connection, type_id: &str) -> Result<Vec<Driver>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, type_id, name, driver_kind, is_file, default_port,
                    url_template, download_url, download_checksum, version,
                    config_schema, supported_auth_types, capabilities, driver_properties, enabled
             FROM drivers WHERE type_id = ?1 AND enabled = 1 ORDER BY name",
        )
        .map_err(|e| storage_err("prepare_get_drivers_by_type", e.to_string()))?;

    let items = stmt
        .query_map(params![type_id], |row| {
            Ok(Driver {
                id: row.get(0)?,
                type_id: row.get(1)?,
                name: row.get(2)?,
                driver_kind: row.get(3)?,
                is_file: row.get::<_, i32>(4)? != 0,
                default_port: row.get(5)?,
                url_template: row.get(6)?,
                download_url: row.get(7)?,
                download_checksum: row.get(8)?,
                version: row.get(9)?,
                config_schema: row.get(10)?,
                supported_auth_types: row.get(11)?,
                capabilities: row.get(12)?,
                driver_properties: row.get(13)?,
                enabled: row.get::<_, i32>(14)? != 0,
            })
        })
        .map_err(|e| storage_err("query_drivers_by_type", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(items)
}

/// 获取指定驱动在本机已安装的所有版本文件
pub fn list_driver_files(conn: &Connection, driver_id: &str) -> Result<Vec<DriverFile>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, driver_id, file_path, file_name, file_size, checksum, version, installed_at, updated_at
             FROM driver_files WHERE driver_id = ?1 ORDER BY version DESC",
        )
        .map_err(|e| storage_err("prepare_list_driver_files", e.to_string()))?;

    let items = stmt
        .query_map(params![driver_id], |row| {
            Ok(DriverFile {
                id: row.get(0)?,
                driver_id: row.get(1)?,
                file_path: row.get(2)?,
                file_name: row.get(3)?,
                file_size: row.get(4)?,
                checksum: row.get(5)?,
                version: row.get(6)?,
                installed_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| storage_err("query_driver_files", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(items)
}

/// 注册驱动文件到本机安装记录（INSERT OR REPLACE）
pub fn register_driver_file(conn: &Connection, df: &DriverFile) -> Result<(), CoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO driver_files (id, driver_id, file_path, file_name, file_size, checksum, version, installed_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            df.id,
            df.driver_id,
            df.file_path,
            df.file_name,
            df.file_size,
            df.checksum,
            df.version,
            df.installed_at,
            df.updated_at,
        ],
    )
    .map_err(|e| storage_err("register_driver_file", e.to_string()))?;

    Ok(())
}

/// 检查指定版本的驱动文件是否已在本机安装
pub fn is_driver_file_installed(
    conn: &Connection,
    driver_id: &str,
    version: &str,
) -> Result<bool, CoreError> {
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM driver_files WHERE driver_id = ?1 AND version = ?2",
            params![driver_id, version],
            |row| row.get(0),
        )
        .map_err(|e| storage_err("check_driver_file_installed", e.to_string()))?;

    Ok(count > 0)
}

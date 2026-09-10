//! Round 21：工作台真实数据加载器
//!
//! 从全局系统库（`GlobalDatabaseManager`，M3 数据源连接模块的持久化层）读取
//! 连接元数据，映射为视图 `ConnectionItem`。加载失败时降级为空列表 + 错误提示，
//! 不阻塞工作台启动。

use std::path::{Path, PathBuf};

use engine::persistence::global_db::GlobalDatabaseManager;

use crate::view::ConnectionItem;

/// 默认全局库目录：`%APPDATA%\rdata-station\global`（测试环境可注入其他目录）。
pub fn default_global_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("rdata-station")
        .join("global")
}

/// 从默认目录加载持久化连接。
pub fn load_persisted_connections() -> (Vec<ConnectionItem>, Option<String>) {
    let dir = default_global_dir();
    load_persisted_connections_from(&dir)
}

/// 从指定目录加载持久化连接（目录可注入，便于测试与后续切换数据目录）。
///
/// 返回 `(连接列表, 错误提示)`：错误提示仅在有失败时填充，连接列表为空不代表失败。
pub fn load_persisted_connections_from(dir: &Path) -> (Vec<ConnectionItem>, Option<String>) {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => return (Vec::new(), Some(format!("无法启动异步运行时: {e}"))),
    };

    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    match runtime.block_on(async {
        let manager = GlobalDatabaseManager::new(sqlite, duckdb, 2).await?;
        manager.get_global_connections(None, None).await
    }) {
        Ok(infos) => {
            let items = infos
                .into_iter()
                .map(|c| ConnectionItem {
                    id: c.id,
                    name: c.name,
                    driver: c.driver,
                    connected: c.is_active,
                    host: c.host,
                    port: c.port,
                    database: c.database,
                    schema: c.schema_name,
                    description: c.description,
                    use_duckdb_fed: c.use_duckdb_fed,
                    created_at: c.created_at,
                    updated_at: c.updated_at,
                })
                .collect();
            (items, None)
        }
        Err(e) => (Vec::new(), Some(format!("加载全局连接失败: {e}"))),
    }
}

/// 新建连接（M3 写路径）：保存到全局系统库。
///
/// 返回 `Err(提示)` 表示保存失败；成功后调用方可重新 `load_persisted_connections` 刷新列表。
pub fn save_connection(
    name: &str,
    db_type: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let dir = default_global_dir();
    save_connection_at(&dir, name, db_type, url, username, password)
}
/// 删除指定目录中的全局连接（目录可注入，便于测试）。
///
/// 成功返回 `Ok(())`，失败返回 `Err(中文提示)`。
pub fn delete_connection_at(dir: &Path, conn_id: &str) -> Result<(), String> {
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => return Err(format!("无法启动异步运行时: {e}")),
    };

    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    runtime.block_on(async {
        match GlobalDatabaseManager::new(sqlite, duckdb, 2).await {
            Ok(manager) => manager
                .delete_global_connection(conn_id)
                .await
                .map_err(|e| format!("删除连接失败: {e}")),
            Err(e) => Err(format!("初始化全局库失败: {e}")),
        }
    })
}

/// 删除默认全局目录中的连接。
pub fn delete_connection(conn_id: &str) -> Result<(), String> {
    let dir = default_global_dir();
    delete_connection_at(&dir, conn_id)
}


/// 保存连接到指定目录（目录可注入，便于测试与后续数据目录切换）。
pub fn save_connection_at(
    dir: &Path,
    name: &str,
    db_type: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    // conn_id 由时间戳生成（毫秒，避免与已有 id 冲突）。
    let conn_id = format!(
        "conn-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    let username = if username.is_empty() { None } else { Some(username) };
    let password = if password.is_empty() { None } else { Some(password) };

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let manager = GlobalDatabaseManager::new(sqlite, duckdb, 2)
            .await
            .map_err(|e| e.to_string())?;
        manager
            .save_global_connection(engine::persistence::global_db::GlobalConnectionSaveInput {
                conn_id: &conn_id,
                name,
                db_type,
                url,
                username,
                password,
                tags: None,
                server_version: None,
                description: None,
                driver_id: None,
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                options: None,
                driver_properties: None,
                advanced_options: None,
                use_duckdb_fed: Some(true),
                metadata_path: None,
                schema_name: None,
            })
            .await
            .map_err(|e| e.to_string())
    })
}

# -*- coding: utf-8 -*-
"""Round 23：loader 增加 save_connection（M3 写路径）+ 表单集成。"""
import pathlib

# ---- 1. workspace_loader.rs：save_connection ----
w = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\workspace_loader.rs')
t = w.read_text(encoding='utf-8')

add = '''
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
'''

t = t.rstrip() + '\n' + add
w.write_text(t, encoding='utf-8')
print('loader save_connection added')

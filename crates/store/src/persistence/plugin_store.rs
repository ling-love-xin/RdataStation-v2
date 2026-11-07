use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, StorageError};

/// 插件信息结构（全局存储）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Plugin {
    pub id: String,
    pub code: String,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub plugin_type: String,
    pub manifest_json: Option<String>,
    pub install_path: String,
    pub is_enabled: bool,
    pub is_builtin: bool,
    pub installed_at: String,
    pub updated_at: String,
}

/// 插件依赖项
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PluginDependency {
    pub plugin_id: String,
    pub dep_code: String,
    pub dep_version_range: String,
    pub is_optional: bool,
}

/// 插件全局配置项
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PluginGlobalConfig {
    pub plugin_id: String,
    pub key: String,
    pub value: Option<String>,
    pub updated_at: String,
}

/// 项目使用的插件（项目级存储）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProjectUsedPlugin {
    pub plugin_code: String,
    pub plugin_version: String,
    pub enabled: bool,
    pub required: bool,
}

/// 项目插件配置项
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProjectPluginConfig {
    pub plugin_code: String,
    pub plugin_version: String,
    pub key: String,
    pub value: Option<String>,
    pub updated_at: String,
}

fn storage_err(operation: &str, reason: String) -> CoreError {
    CoreError::Storage(StorageError::Persistence {
        store: "plugin_store".to_string(),
        operation: operation.to_string(),
        reason,
    })
}

// ==================== 全局插件存储函数 ====================

/// 注册插件到全局插件中心
///
/// **一个插件 code 只保留一行**：表的主键 `id` 是安装实例 uuid（每次安装新生成），
/// 而运行时身份是清单里的 `plugin.id`（= `code`）。同 code 升级到新版本时，
/// `UNIQUE(code, version)` 不冲突，旧行会留下来指向同一个 `install_path`，
/// 于是 `get_all_plugins` 出现重复、卸载后会留幽灵记录。这里按 code 合并：
/// 沿用最近一行的 `is_enabled` 与 `installed_at`——用户停用的插件重装后不该自己变回启用。
pub fn register_plugin(conn: &Connection, plugin: &Plugin) -> Result<(), CoreError> {
    register_plugin_with_dependencies(conn, plugin, &[])
}

/// 注册插件并整体替换它的依赖行（同一事务）。
///
/// 依赖行必须跟着插件登记一起落库：只写 `plugins` 不写 `plugin_dependencies`，
/// 「被依赖不让卸载」的反向检查永远查不到引用者；分开写又会出现「插件已登记、
/// 依赖没写进去」的半截状态。同 code 旧行的依赖在下面一并清掉再写新行。
pub fn register_plugin_with_dependencies(
    conn: &Connection,
    plugin: &Plugin,
    deps: &[PluginDependency],
) -> Result<(), CoreError> {
    let previous: Option<(i32, String)> = conn
        .query_row(
            "SELECT is_enabled, installed_at FROM plugins WHERE code = ?1
             ORDER BY updated_at DESC LIMIT 1",
            params![plugin.code],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| storage_err("register_plugin_existing", e.to_string()))?;

    let (is_enabled, installed_at) = match previous {
        Some((enabled, installed_at)) => (enabled != 0, installed_at),
        None => (plugin.is_enabled, plugin.installed_at.clone()),
    };

    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| storage_err("register_plugin_begin", e.to_string()))?;
    let outcome = (|| -> Result<(), CoreError> {
        // 依赖行按安装实例 uuid 关联；删旧插件行前先连带清掉它的依赖，
        // 否则换 uuid 主键后这些行会永远留在库里指向一个不存在的插件。
        conn.execute(
            "DELETE FROM plugin_dependencies
             WHERE plugin_id IN (SELECT id FROM plugins WHERE code = ?1)",
            params![plugin.code],
        )
        .map_err(|e| storage_err("register_plugin_deps", e.to_string()))?;
        conn.execute("DELETE FROM plugins WHERE code = ?1", params![plugin.code])
            .map_err(|e| storage_err("register_plugin_dedupe", e.to_string()))?;
        conn.execute(
            "INSERT INTO plugins
             (id, code, name, version, author, description, repo_url, plugin_type, manifest_json, install_path, is_enabled, is_builtin, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                plugin.id,
                plugin.code,
                plugin.name,
                plugin.version,
                plugin.author,
                plugin.description,
                plugin.repo_url,
                plugin.plugin_type,
                plugin.manifest_json,
                plugin.install_path,
                is_enabled as i32,
                plugin.is_builtin as i32,
                installed_at,
                plugin.updated_at,
            ],
        )
        .map_err(|e| storage_err("register_plugin", e.to_string()))?;
        for dep in deps {
            // 依赖行统一挂到本次登记的安装实例 uuid 上（同 code 旧行已删）。
            conn.execute(
                "INSERT OR REPLACE INTO plugin_dependencies
                 (plugin_id, dep_code, dep_version_range, is_optional)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    plugin.id,
                    dep.dep_code,
                    dep.dep_version_range,
                    dep.is_optional as i32,
                ],
            )
            .map_err(|e| storage_err("register_plugin_dep_insert", e.to_string()))?;
        }
        Ok(())
    })();

    match outcome {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map_err(|e| storage_err("register_plugin_commit", e.to_string())),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// 根据 ID 获取插件
pub fn get_plugin(conn: &Connection, id: &str) -> Result<Option<Plugin>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, code, name, version, author, description, repo_url, plugin_type, manifest_json, install_path, is_enabled, is_builtin, installed_at, updated_at
             FROM plugins WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_plugin", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(Plugin {
            id: row.get(0)?,
            code: row.get(1)?,
            name: row.get(2)?,
            version: row.get(3)?,
            author: row.get(4)?,
            description: row.get(5)?,
            repo_url: row.get(6)?,
            plugin_type: row.get(7)?,
            manifest_json: row.get(8)?,
            install_path: row.get(9)?,
            is_enabled: row.get::<_, i32>(10)? != 0,
            is_builtin: row.get::<_, i32>(11)? != 0,
            installed_at: row.get(12)?,
            updated_at: row.get(13)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_plugin", e.to_string()))
}

/// 根据 code 和 version 获取插件
pub fn get_plugin_by_code_version(
    conn: &Connection,
    code: &str,
    version: &str,
) -> Result<Option<Plugin>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, code, name, version, author, description, repo_url, plugin_type, manifest_json, install_path, is_enabled, is_builtin, installed_at, updated_at
             FROM plugins WHERE code = ?1 AND version = ?2",
        )
        .map_err(|e| storage_err("prepare_get_plugin_by_code_version", e.to_string()))?;

    stmt.query_row(params![code, version], |row| {
        Ok(Plugin {
            id: row.get(0)?,
            code: row.get(1)?,
            name: row.get(2)?,
            version: row.get(3)?,
            author: row.get(4)?,
            description: row.get(5)?,
            repo_url: row.get(6)?,
            plugin_type: row.get(7)?,
            manifest_json: row.get(8)?,
            install_path: row.get(9)?,
            is_enabled: row.get::<_, i32>(10)? != 0,
            is_builtin: row.get::<_, i32>(11)? != 0,
            installed_at: row.get(12)?,
            updated_at: row.get(13)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_plugin_by_code_version", e.to_string()))
}

/// 根据清单 id（= `code`）获取插件
///
/// 运行时（加载 / 启停 / 卸载）认的是插件 `id`（目录名与清单 id），不是安装实例 uuid；
/// 按 uuid 查会静默查不到，于是"停用"只停在内存、重启又变回启用。
pub fn get_plugin_by_code(conn: &Connection, code: &str) -> Result<Option<Plugin>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, code, name, version, author, description, repo_url, plugin_type, manifest_json, install_path, is_enabled, is_builtin, installed_at, updated_at
             FROM plugins WHERE code = ?1",
        )
        .map_err(|e| storage_err("prepare_get_plugin_by_code", e.to_string()))?;

    stmt.query_row(params![code], |row| {
        Ok(Plugin {
            id: row.get(0)?,
            code: row.get(1)?,
            name: row.get(2)?,
            version: row.get(3)?,
            author: row.get(4)?,
            description: row.get(5)?,
            repo_url: row.get(6)?,
            plugin_type: row.get(7)?,
            manifest_json: row.get(8)?,
            install_path: row.get(9)?,
            is_enabled: row.get::<_, i32>(10)? != 0,
            is_builtin: row.get::<_, i32>(11)? != 0,
            installed_at: row.get(12)?,
            updated_at: row.get(13)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_plugin_by_code", e.to_string()))
}

/// 按清单 id（`code`）更新启用状态（运行时入口用它，安装实例 uuid 只用于主键）
pub fn update_plugin_enabled_by_code(
    conn: &Connection,
    code: &str,
    is_enabled: bool,
) -> Result<(), CoreError> {
    conn.execute(
        "UPDATE plugins SET is_enabled = ?1, updated_at = CURRENT_TIMESTAMP WHERE code = ?2",
        params![is_enabled as i32, code],
    )
    .map_err(|e| storage_err("update_plugin_enabled_by_code", e.to_string()))?;

    Ok(())
}

/// 按清单 id（`code`）删除登记（卸载）
pub fn delete_plugin_by_code(conn: &Connection, code: &str) -> Result<(), CoreError> {
    conn.execute(
        "DELETE FROM plugin_dependencies
         WHERE plugin_id IN (SELECT id FROM plugins WHERE code = ?1)",
        params![code],
    )
    .map_err(|e| storage_err("delete_plugin_by_code_deps", e.to_string()))?;
    conn.execute("DELETE FROM plugins WHERE code = ?1", params![code])
        .map_err(|e| storage_err("delete_plugin_by_code", e.to_string()))?;
    Ok(())
}

/// 获取所有已安装插件
pub fn get_all_plugins(conn: &Connection) -> Result<Vec<Plugin>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, code, name, version, author, description, repo_url, plugin_type, manifest_json, install_path, is_enabled, is_builtin, installed_at, updated_at
             FROM plugins ORDER BY code, updated_at DESC",
        )
        .map_err(|e| storage_err("prepare_get_all_plugins", e.to_string()))?;

    let plugins = stmt
        .query_map([], |row| {
            Ok(Plugin {
                id: row.get(0)?,
                code: row.get(1)?,
                name: row.get(2)?,
                version: row.get(3)?,
                author: row.get(4)?,
                description: row.get(5)?,
                repo_url: row.get(6)?,
                plugin_type: row.get(7)?,
                manifest_json: row.get(8)?,
                install_path: row.get(9)?,
                is_enabled: row.get::<_, i32>(10)? != 0,
                is_builtin: row.get::<_, i32>(11)? != 0,
                installed_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })
        .map_err(|e| storage_err("query_all_plugins", e.to_string()))?;

    plugins
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("collect_plugins", e.to_string()))
}

/// 更新插件启用状态
pub fn update_plugin_enabled(
    conn: &Connection,
    id: &str,
    is_enabled: bool,
) -> Result<(), CoreError> {
    conn.execute(
        "UPDATE plugins SET is_enabled = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        params![is_enabled as i32, id],
    )
    .map_err(|e| storage_err("update_plugin_enabled", e.to_string()))?;

    Ok(())
}

/// 删除插件
pub fn delete_plugin(conn: &Connection, id: &str) -> Result<(), CoreError> {
    conn.execute(
        "DELETE FROM plugin_dependencies WHERE plugin_id = ?1",
        params![id],
    )
    .map_err(|e| storage_err("delete_plugin_deps", e.to_string()))?;
    conn.execute("DELETE FROM plugins WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_plugin", e.to_string()))?;
    Ok(())
}

/// 注册插件依赖
pub fn register_plugin_dependency(
    conn: &Connection,
    dep: &PluginDependency,
) -> Result<(), CoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO plugin_dependencies
         (plugin_id, dep_code, dep_version_range, is_optional)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            dep.plugin_id,
            dep.dep_code,
            dep.dep_version_range,
            dep.is_optional as i32,
        ],
    )
    .map_err(|e| storage_err("register_plugin_dependency", e.to_string()))?;

    Ok(())
}

/// 获取插件的所有依赖
pub fn get_plugin_dependencies(
    conn: &Connection,
    plugin_id: &str,
) -> Result<Vec<PluginDependency>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT plugin_id, dep_code, dep_version_range, is_optional
             FROM plugin_dependencies WHERE plugin_id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_plugin_dependencies", e.to_string()))?;

    let deps = stmt
        .query_map(params![plugin_id], |row| {
            Ok(PluginDependency {
                plugin_id: row.get(0)?,
                dep_code: row.get(1)?,
                dep_version_range: row.get(2)?,
                is_optional: row.get::<_, i32>(3)? != 0,
            })
        })
        .map_err(|e| storage_err("query_plugin_dependencies", e.to_string()))?;

    deps.collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("collect_dependencies", e.to_string()))
}

/// 设置插件全局配置
pub fn set_plugin_global_config(
    conn: &Connection,
    config: &PluginGlobalConfig,
) -> Result<(), CoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO plugin_global_config
         (plugin_id, key, value, updated_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)",
        params![config.plugin_id, config.key, config.value,],
    )
    .map_err(|e| storage_err("set_plugin_global_config", e.to_string()))?;

    Ok(())
}

/// 获取插件全局配置
pub fn get_plugin_global_configs(
    conn: &Connection,
    plugin_id: &str,
) -> Result<Vec<PluginGlobalConfig>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT plugin_id, key, value, updated_at
             FROM plugin_global_config WHERE plugin_id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_plugin_global_configs", e.to_string()))?;

    let configs = stmt
        .query_map(params![plugin_id], |row| {
            Ok(PluginGlobalConfig {
                plugin_id: row.get(0)?,
                key: row.get(1)?,
                value: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(|e| storage_err("query_plugin_global_configs", e.to_string()))?;

    configs
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("collect_global_configs", e.to_string()))
}

// ==================== 项目级插件存储函数 ====================

/// 添加插件到项目（启用）
pub fn project_add_plugin(
    conn: &Connection,
    used_plugin: &ProjectUsedPlugin,
) -> Result<(), CoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO project_used_plugins
         (plugin_code, plugin_version, enabled, required)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            used_plugin.plugin_code,
            used_plugin.plugin_version,
            used_plugin.enabled as i32,
            used_plugin.required as i32,
        ],
    )
    .map_err(|e| storage_err("project_add_plugin", e.to_string()))?;

    Ok(())
}

/// 从项目移除插件
pub fn project_remove_plugin(
    conn: &Connection,
    code: &str,
    version: &str,
) -> Result<(), CoreError> {
    conn.execute(
        "DELETE FROM project_used_plugins WHERE plugin_code = ?1 AND plugin_version = ?2",
        params![code, version],
    )
    .map_err(|e| storage_err("project_remove_plugin", e.to_string()))?;
    Ok(())
}

/// 获取项目使用的所有插件
pub fn project_get_plugins(conn: &Connection) -> Result<Vec<ProjectUsedPlugin>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT plugin_code, plugin_version, enabled, required
             FROM project_used_plugins ORDER BY plugin_code",
        )
        .map_err(|e| storage_err("prepare_project_get_plugins", e.to_string()))?;

    let plugins = stmt
        .query_map([], |row| {
            Ok(ProjectUsedPlugin {
                plugin_code: row.get(0)?,
                plugin_version: row.get(1)?,
                enabled: row.get::<_, i32>(2)? != 0,
                required: row.get::<_, i32>(3)? != 0,
            })
        })
        .map_err(|e| storage_err("query_project_plugins", e.to_string()))?;

    plugins
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("collect_project_plugins", e.to_string()))
}

/// 更新项目插件启用状态
pub fn project_update_plugin_enabled(
    conn: &Connection,
    code: &str,
    version: &str,
    enabled: bool,
) -> Result<(), CoreError> {
    conn.execute(
        "UPDATE project_used_plugins SET enabled = ?1 WHERE plugin_code = ?2 AND plugin_version = ?3",
        params![enabled as i32, code, version],
    )
    .map_err(|e| storage_err("project_update_plugin_enabled", e.to_string()))?;

    Ok(())
}

/// 设置项目插件配置
pub fn project_set_plugin_config(
    conn: &Connection,
    config: &ProjectPluginConfig,
) -> Result<(), CoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO project_plugin_config
         (plugin_code, plugin_version, key, value, updated_at)
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)",
        params![
            config.plugin_code,
            config.plugin_version,
            config.key,
            config.value,
        ],
    )
    .map_err(|e| storage_err("project_set_plugin_config", e.to_string()))?;

    Ok(())
}

/// 获取项目插件配置
pub fn project_get_plugin_configs(
    conn: &Connection,
    code: &str,
    version: &str,
) -> Result<Vec<ProjectPluginConfig>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT plugin_code, plugin_version, key, value, updated_at
             FROM project_plugin_config WHERE plugin_code = ?1 AND plugin_version = ?2",
        )
        .map_err(|e| storage_err("prepare_project_get_plugin_configs", e.to_string()))?;

    let configs = stmt
        .query_map(params![code, version], |row| {
            Ok(ProjectPluginConfig {
                plugin_code: row.get(0)?,
                plugin_version: row.get(1)?,
                key: row.get(2)?,
                value: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|e| storage_err("query_project_plugin_configs", e.to_string()))?;

    configs
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("collect_project_configs", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/global/001_init.sql"))
            .unwrap();
        conn
    }

    fn plugin(id: &str, version: &str, enabled: bool, installed_at: &str) -> Plugin {
        Plugin {
            id: id.to_string(),
            code: "com.example.demo".to_string(),
            name: "Demo".to_string(),
            version: version.to_string(),
            author: None,
            description: None,
            repo_url: None,
            plugin_type: "sidecar".to_string(),
            manifest_json: None,
            install_path: "/plugins/com.example.demo".to_string(),
            is_enabled: enabled,
            is_builtin: false,
            installed_at: installed_at.to_string(),
            updated_at: "2026-11-07T00:00:00Z".to_string(),
        }
    }

    /// 升级到新版本时旧行必须被替换：主键是安装实例 uuid，仅 INSERT OR REPLACE 会按
    /// `UNIQUE(code, version)` 判重，新版本不冲突，旧行会留下来变成重复登记。
    #[test]
    fn a_new_version_replaces_the_previous_row_of_the_same_code() {
        let conn = migrated_conn();
        register_plugin(
            &conn,
            &plugin("uuid-1", "1.0.0", true, "2026-10-01T00:00:00Z"),
        )
        .unwrap();
        register_plugin(
            &conn,
            &plugin("uuid-2", "1.1.0", true, "2026-10-01T00:00:00Z"),
        )
        .unwrap();

        let all = get_all_plugins(&conn).unwrap();
        assert_eq!(all.len(), 1, "同一 code 只应留一行");
        assert_eq!(all[0].version, "1.1.0");
        assert_eq!(all[0].id, "uuid-2");
    }

    /// 重装不该把用户停用的插件悄悄改回启用，首次安装时间也应保留。
    #[test]
    fn a_reinstall_keeps_the_disabled_state_and_install_time() {
        let conn = migrated_conn();
        let first = plugin("uuid-1", "1.0.0", true, "2026-10-01T00:00:00Z");
        register_plugin(&conn, &first).unwrap();
        update_plugin_enabled_by_code(&conn, &first.code, false).unwrap();

        let reinstall = plugin("uuid-2", "1.1.0", true, "2026-11-07T00:00:00Z");
        register_plugin(&conn, &reinstall).unwrap();

        let row = get_plugin_by_code(&conn, &first.code).unwrap().unwrap();
        assert_eq!(row.id, "uuid-2");
        assert!(!row.is_enabled, "重装后仍应保持停用");
        assert_eq!(row.installed_at, "2026-10-01T00:00:00Z");
    }

    /// 运行时只知道清单 id（= code），不知道安装 uuid；按 code 启停 / 删除必须命中。
    #[test]
    fn runtime_operations_work_by_code_not_by_install_uuid() {
        let conn = migrated_conn();
        register_plugin(
            &conn,
            &plugin("uuid-1", "1.0.0", true, "2026-10-01T00:00:00Z"),
        )
        .unwrap();

        update_plugin_enabled_by_code(&conn, "com.example.demo", false).unwrap();
        assert!(
            !get_plugin_by_code(&conn, "com.example.demo")
                .unwrap()
                .unwrap()
                .is_enabled
        );

        delete_plugin_by_code(&conn, "com.example.demo").unwrap();
        assert!(
            get_plugin_by_code(&conn, "com.example.demo")
                .unwrap()
                .is_none()
        );
        assert!(get_plugin(&conn, "uuid-1").unwrap().is_none());
    }

    /// 依赖行按安装实例 uuid 关联：换 uuid 升级或按 code 卸载后，旧依赖行必须一起消失，
    /// 否则库里会留下指向不存在插件的孤儿记录。
    #[test]
    fn dependencies_are_dropped_with_the_plugin_row() {
        let conn = migrated_conn();
        let first = plugin("uuid-1", "1.0.0", true, "2026-10-01T00:00:00Z");
        register_plugin(&conn, &first).unwrap();
        register_plugin_dependency(
            &conn,
            &PluginDependency {
                plugin_id: "uuid-1".to_string(),
                dep_code: "com.example.core".to_string(),
                dep_version_range: ">=1.0.0".to_string(),
                is_optional: false,
            },
        )
        .unwrap();
        assert_eq!(get_plugin_dependencies(&conn, "uuid-1").unwrap().len(), 1);

        // 升级换 uuid：旧行的依赖不能留
        register_plugin(
            &conn,
            &plugin("uuid-2", "1.1.0", true, "2026-10-01T00:00:00Z"),
        )
        .unwrap();
        assert!(
            get_plugin_dependencies(&conn, "uuid-1").unwrap().is_empty(),
            "旧安装实例的依赖行应随旧插件行一起删除"
        );

        // 按 code 卸载：新行的依赖也要清掉
        register_plugin_dependency(
            &conn,
            &PluginDependency {
                plugin_id: "uuid-2".to_string(),
                dep_code: "com.example.core".to_string(),
                dep_version_range: ">=1.0.0".to_string(),
                is_optional: false,
            },
        )
        .unwrap();
        delete_plugin_by_code(&conn, "com.example.demo").unwrap();
        assert!(get_plugin_dependencies(&conn, "uuid-2").unwrap().is_empty());
    }

    /// 历史库里已经存在的重复行：迁移 028 收敛成每个 code 一行（保留最新 updated_at），
    /// 并加上唯一索引——否则任何绕过 `register_plugin` 的写入路径都能再造出重复登记。
    #[test]
    fn the_code_unique_migration_collapses_historical_duplicates() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/global/001_init.sql"))
            .unwrap();
        insert_legacy_row(&conn, "uuid-old", "1.0.0", "2026-10-01T00:00:00Z")
            .expect("构造历史重复行（旧）");
        insert_legacy_row(&conn, "uuid-new", "1.1.0", "2026-11-01T00:00:00Z")
            .expect("构造历史重复行（新）");

        conn.execute_batch(include_str!(
            "../../migrations/global/028_plugin_code_unique.sql"
        ))
        .unwrap();

        let all = get_all_plugins(&conn).unwrap();
        assert_eq!(all.len(), 1, "同一 code 的历史重复行应只留一行");
        assert_eq!(all[0].id, "uuid-new", "应保留 updated_at 最新的那一行");

        assert!(
            insert_legacy_row(&conn, "uuid-third", "1.2.0", "2026-12-01T00:00:00Z").is_err(),
            "唯一索引应拒绝再写入同 code 的第二行"
        );
    }

    /// 模拟「升级前」的写入：直接 INSERT，不经过 `register_plugin` 的按 code 合并。
    fn insert_legacy_row(
        conn: &Connection,
        id: &str,
        version: &str,
        updated_at: &str,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO plugins
             (id, code, name, version, author, description, repo_url, plugin_type, manifest_json, install_path, is_enabled, is_builtin, installed_at, updated_at)
             VALUES (?1, 'com.example.demo', 'Demo', ?2, NULL, NULL, NULL, 'sidecar', NULL, '/plugins/com.example.demo', 1, 0, '2026-10-01T00:00:00Z', ?3)",
            params![id, version, updated_at],
        )
    }
}

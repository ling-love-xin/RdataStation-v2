use std::collections::HashMap;
use std::sync::Mutex;

use crate::core::error::{CommonError, CoreError};

/// 插件权限级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginPermissionLevel {
    /// 只读：仅允许 SELECT，禁止写入
    ReadOnly,
    /// 读写：可创建临时表，允许增删改
    ReadWrite,
    /// 管理：支持联邦 ATTACH、远程表物化等高级操作
    Admin,
}

impl PluginPermissionLevel {
    /// 获取权限级别名称
    pub fn name(&self) -> &str {
        match self {
            PluginPermissionLevel::ReadOnly => "readonly",
            PluginPermissionLevel::ReadWrite => "readwrite",
            PluginPermissionLevel::Admin => "admin",
        }
    }

    /// 检查是否允许写入操作
    pub fn can_write(&self) -> bool {
        match self {
            PluginPermissionLevel::ReadOnly => false,
            PluginPermissionLevel::ReadWrite | PluginPermissionLevel::Admin => true,
        }
    }

    /// 检查是否允许联邦查询操作
    pub fn can_federate(&self) -> bool {
        match self {
            PluginPermissionLevel::ReadOnly | PluginPermissionLevel::ReadWrite => false,
            PluginPermissionLevel::Admin => true,
        }
    }
}

/// 插件连接信息
pub struct PluginConnection {
    /// 插件 ID
    pub plugin_id: String,
    /// 权限级别
    pub permission_level: PluginPermissionLevel,
    /// 已注册的临时表
    pub temp_tables: Vec<String>,
}

impl PluginConnection {
    /// 创建新的插件连接。
    pub fn new(plugin_id: String, permission_level: PluginPermissionLevel) -> Self {
        PluginConnection {
            plugin_id,
            permission_level,
            temp_tables: Vec::new(),
        }
    }

    /// 注册临时表。
    pub fn register_temp_table(&mut self, table_name: String) {
        self.temp_tables.push(table_name);
    }

    /// 获取所有临时表。
    pub fn temp_tables(&self) -> &[String] {
        &self.temp_tables
    }
}

/// 插件系统接口管理器
///
/// 为 Extism WASM + Go Sidecar 插件提供安全沙箱式 DuckDB 访问，屏蔽底层连接细节。
///
/// # 权限级别
/// - **只读**: SQL 格式化、数据脱敏、Schema Diff - 仅允许 SELECT
/// - **读写**: 数据导入导出、Mock 增强 - 可创建临时表，允许增删改
/// - **管理**: 官方内置插件（JDBC Bridge）- 支持联邦 ATTACH、远程表物化
pub struct PluginManager {
    /// 插件连接注册表: plugin_id -> PluginConnection
    connections: Mutex<HashMap<String, PluginConnection>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginManager {
    /// 创建新的插件管理器。
    pub fn new() -> Self {
        PluginManager {
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// 创建插件沙箱连接。
    ///
    /// # 参数
    /// - `plugin_id`: 插件唯一标识
    /// - `permission_level`: 权限级别
    ///
    /// # 返回
    /// - `Ok(())`: 连接创建成功
    /// - `Err(CoreError)`: 连接已存在或其他错误
    ///
    /// # 示例
    /// ```rust,ignore
    /// manager.create_plugin_connection("sql-formatter", PluginPermissionLevel::ReadOnly)?;
    /// ```
    pub fn create_plugin_connection(
        &self,
        plugin_id: &str,
        permission_level: PluginPermissionLevel,
    ) -> Result<(), CoreError> {
        let mut connections = self
            .connections
            .lock()
            .map_err(|e| CoreError::common(CommonError::General(format!("获取锁失败: {}", e))))?;

        if connections.contains_key(plugin_id) {
            return Err(CoreError::common(CommonError::General(format!(
                "插件 '{}' 连接已存在",
                plugin_id
            ))));
        }

        let conn = PluginConnection::new(plugin_id.to_string(), permission_level);
        connections.insert(plugin_id.to_string(), conn);

        tracing::info!(
            "[PluginManager] 创建插件连接: {} (权限: {})",
            plugin_id,
            permission_level.name()
        );

        Ok(())
    }

    /// 带权限校验执行 SQL。
    ///
    /// # 参数
    /// - `plugin_id`: 插件 ID
    /// - `sql`: SQL 语句
    ///
    /// # 返回
    /// - `Ok(())`: SQL 执行通过校验
    /// - `Err(CoreError)`: 权限不足或 SQL 不合法
    ///
    /// # 注意
    /// 此方法仅执行权限校验，实际 SQL 执行由 executor 处理
    pub fn validate_plugin_sql(&self, plugin_id: &str, sql: &str) -> Result<(), CoreError> {
        let connections = self
            .connections
            .lock()
            .map_err(|e| CoreError::common(CommonError::General(format!("获取锁失败: {}", e))))?;

        let conn = connections.get(plugin_id).ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "插件 '{}' 连接不存在",
                plugin_id
            )))
        })?;

        Self::check_sql_permission(&conn.permission_level, sql)
    }

    /// 注册插件临时表纳入生命周期管理。
    ///
    /// # 参数
    /// - `plugin_id`: 插件 ID
    /// - `table_name`: 临时表名
    ///
    /// # 返回
    /// - `Ok(())`: 注册成功
    /// - `Err(CoreError)`: 权限不足或连接不存在
    pub fn attach_plugin_temp_table(
        &self,
        plugin_id: &str,
        table_name: &str,
    ) -> Result<(), CoreError> {
        let mut connections = self
            .connections
            .lock()
            .map_err(|e| CoreError::common(CommonError::General(format!("获取锁失败: {}", e))))?;

        let conn = connections.get_mut(plugin_id).ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "插件 '{}' 连接不存在",
                plugin_id
            )))
        })?;

        if !conn.permission_level.can_write() {
            return Err(CoreError::common(CommonError::General(format!(
                "插件 '{}' 权限不足，无法创建临时表",
                plugin_id
            ))));
        }

        conn.register_temp_table(table_name.to_string());

        tracing::info!(
            "[PluginManager] 注册插件临时表: {} -> {}",
            plugin_id,
            table_name
        );

        Ok(())
    }

    /// 回收连接、清理插件临时表。
    ///
    /// # 参数
    /// - `plugin_id`: 插件 ID
    ///
    /// # 返回
    /// - `Ok(Vec<String>)`: 被清理的临时表列表
    /// - `Err(CoreError)`: 连接不存在
    pub fn revoke_plugin_connection(&self, plugin_id: &str) -> Result<Vec<String>, CoreError> {
        let mut connections = self
            .connections
            .lock()
            .map_err(|e| CoreError::common(CommonError::General(format!("获取锁失败: {}", e))))?;

        let conn = connections.remove(plugin_id).ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "插件 '{}' 连接不存在",
                plugin_id
            )))
        })?;

        let temp_tables = conn.temp_tables;

        tracing::info!(
            "[PluginManager] 回收插件连接: {} (清理 {} 个临时表)",
            plugin_id,
            temp_tables.len()
        );

        Ok(temp_tables)
    }

    /// 检查插件连接是否存在。
    ///
    /// # 参数
    /// - `plugin_id`: 插件 ID
    ///
    /// # 返回
    /// true 表示连接存在
    pub fn has_connection(&self, plugin_id: &str) -> bool {
        self.connections
            .lock()
            .map(|conns| conns.contains_key(plugin_id))
            .unwrap_or(false)
    }

    /// 获取插件权限级别。
    ///
    /// # 参数
    /// - `plugin_id`: 插件 ID
    ///
    /// # 返回
    /// 权限级别（如果连接存在）
    pub fn get_permission_level(&self, plugin_id: &str) -> Option<PluginPermissionLevel> {
        self.connections
            .lock()
            .ok()
            .and_then(|conns| conns.get(plugin_id).map(|c| c.permission_level))
    }

    /// 获取活跃插件连接数量。
    ///
    /// # 返回
    /// 活跃连接数
    pub fn active_connection_count(&self) -> usize {
        self.connections
            .lock()
            .map(|conns| conns.len())
            .unwrap_or(0)
    }

    /// 内部：检查 SQL 是否符合权限级别。
    ///
    /// # 参数
    /// - `level`: 权限级别
    /// - `sql`: SQL 语句
    ///
    /// # 返回
    /// - `Ok(())`: 权限通过
    /// - `Err(CoreError)`: 权限不足
    fn check_sql_permission(level: &PluginPermissionLevel, sql: &str) -> Result<(), CoreError> {
        let trimmed = sql.trim().to_uppercase();

        // 只读插件仅允许 SELECT/WITH/EXPLAIN
        if !level.can_write() {
            let allowed_prefixes = ["SELECT", "WITH", "EXPLAIN", "DESCRIBE", "PRAGMA"];
            let is_allowed = allowed_prefixes.iter().any(|p| trimmed.starts_with(p));

            if !is_allowed {
                return Err(CoreError::common(CommonError::General(format!(
                    "只读插件禁止执行非查询 SQL: {}",
                    trimmed.split_whitespace().next().unwrap_or("")
                ))));
            }
        }

        // 非管理员禁止联邦操作
        if !level.can_federate() {
            let forbidden = ["ATTACH", "DETACH", "INSTALL", "LOAD"];
            for kw in &forbidden {
                if trimmed.contains(kw) {
                    return Err(CoreError::common(CommonError::General(format!(
                        "插件权限不足，禁止操作: {}",
                        kw
                    ))));
                }
            }
        }

        Ok(())
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_level_properties() {
        assert_eq!(PluginPermissionLevel::ReadOnly.name(), "readonly");
        assert!(!PluginPermissionLevel::ReadOnly.can_write());
        assert!(!PluginPermissionLevel::ReadOnly.can_federate());

        assert!(PluginPermissionLevel::ReadWrite.can_write());
        assert!(!PluginPermissionLevel::ReadWrite.can_federate());

        assert!(PluginPermissionLevel::Admin.can_write());
        assert!(PluginPermissionLevel::Admin.can_federate());
    }

    #[test]
    fn test_create_plugin_connection() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        assert!(!manager.has_connection("test-plugin"));

        manager.create_plugin_connection("test-plugin", PluginPermissionLevel::ReadOnly)?;

        assert!(manager.has_connection("test-plugin"));
        assert_eq!(
            manager.get_permission_level("test-plugin"),
            Some(PluginPermissionLevel::ReadOnly)
        );
        Ok(())
    }

    #[test]
    fn test_create_duplicate_connection() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        manager.create_plugin_connection("test-plugin", PluginPermissionLevel::ReadOnly)?;

        assert!(manager
            .create_plugin_connection("test-plugin", PluginPermissionLevel::ReadWrite)
            .is_err());
        Ok(())
    }

    #[test]
    fn test_validate_plugin_sql_readonly() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        manager.create_plugin_connection("readonly-plugin", PluginPermissionLevel::ReadOnly)?;

        // 允许 SELECT
        assert!(manager
            .validate_plugin_sql("readonly-plugin", "SELECT * FROM users")
            .is_ok());

        // 禁止 INSERT
        assert!(manager
            .validate_plugin_sql("readonly-plugin", "INSERT INTO users VALUES (1)")
            .is_err());

        // 禁止 ATTACH
        assert!(manager
            .validate_plugin_sql("readonly-plugin", "ATTACH 'db.duckdb'")
            .is_err());
        Ok(())
    }

    #[test]
    fn test_validate_plugin_sql_readwrite() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        manager.create_plugin_connection("rw-plugin", PluginPermissionLevel::ReadWrite)?;

        // 允许 INSERT
        assert!(manager
            .validate_plugin_sql("rw-plugin", "INSERT INTO users VALUES (1)")
            .is_ok());

        // 禁止 ATTACH
        assert!(manager
            .validate_plugin_sql("rw-plugin", "ATTACH 'db.duckdb'")
            .is_err());
        Ok(())
    }

    #[test]
    fn test_validate_plugin_sql_admin() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        manager.create_plugin_connection("admin-plugin", PluginPermissionLevel::Admin)?;

        // 允许 ATTACH
        assert!(manager
            .validate_plugin_sql("admin-plugin", "ATTACH 'db.duckdb' AS db")
            .is_ok());
        Ok(())
    }

    #[test]
    fn test_attach_plugin_temp_table() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        manager.create_plugin_connection("rw-plugin", PluginPermissionLevel::ReadWrite)?;

        assert!(manager
            .attach_plugin_temp_table("rw-plugin", "tmp_p_test_20260512143040")
            .is_ok());

        // 只读插件禁止创建临时表
        manager.create_plugin_connection("ro-plugin", PluginPermissionLevel::ReadOnly)?;

        assert!(manager
            .attach_plugin_temp_table("ro-plugin", "tmp_p_test2_20260512143040")
            .is_err());
        Ok(())
    }

    #[test]
    fn test_revoke_plugin_connection() -> Result<(), CoreError> {
        let manager = PluginManager::new();

        manager.create_plugin_connection("test-plugin", PluginPermissionLevel::ReadWrite)?;
        manager.attach_plugin_temp_table("test-plugin", "tmp_p_test_20260512143040")?;

        let cleaned = manager.revoke_plugin_connection("test-plugin")?;

        assert_eq!(cleaned.len(), 1);
        assert!(cleaned.contains(&"tmp_p_test_20260512143040".to_string()));
        assert!(!manager.has_connection("test-plugin"));
        Ok(())
    }

    #[test]
    fn test_revoke_non_existent_connection() {
        let manager = PluginManager::new();
        assert!(manager.revoke_plugin_connection("non-existent").is_err());
    }

    #[test]
    fn test_active_connection_count() -> Result<(), CoreError> {
        let manager = PluginManager::new();
        assert_eq!(manager.active_connection_count(), 0);

        manager.create_plugin_connection("plugin1", PluginPermissionLevel::ReadOnly)?;
        manager.create_plugin_connection("plugin2", PluginPermissionLevel::ReadWrite)?;

        assert_eq!(manager.active_connection_count(), 2);

        manager.revoke_plugin_connection("plugin1")?;
        assert_eq!(manager.active_connection_count(), 1);
        Ok(())
    }
}

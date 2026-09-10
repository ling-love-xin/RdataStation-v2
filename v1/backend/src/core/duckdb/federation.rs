use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::collections::HashMap;
use std::sync::Mutex;

use duckdb::Connection;

use crate::core::error::{CommonError, CoreError};

/// 外部数据源类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataSourceType {
    /// DuckDB 数据库文件
    DuckDB,
    /// MySQL 数据库
    MySQL,
    /// PostgreSQL 数据库
    PostgreSQL,
    /// SQLite 数据库
    SQLite,
}

impl std::fmt::Display for DataSourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSourceType::DuckDB => write!(f, "duckdb"),
            DataSourceType::MySQL => write!(f, "mysql"),
            DataSourceType::PostgreSQL => write!(f, "postgres"),
            DataSourceType::SQLite => write!(f, "sqlite"),
        }
    }
}

/// 外部数据源配置
pub struct DataSourceConfig {
    /// 数据源类型
    pub source_type: DataSourceType,
    /// 连接字符串
    pub connection_string: String,
    /// 挂载别名
    pub alias: String,
    /// 附加参数（可选）
    pub extra_params: HashMap<String, String>,
}

impl DataSourceConfig {
    /// 创建 DuckDB 数据源配置
    pub fn duckdb(alias: &str, path: &str) -> Self {
        DataSourceConfig {
            source_type: DataSourceType::DuckDB,
            connection_string: path.to_string(),
            alias: alias.to_string(),
            extra_params: HashMap::new(),
        }
    }

    /// 创建 MySQL 数据源配置
    pub fn mysql(
        alias: &str,
        host: &str,
        port: u16,
        database: &str,
        user: &str,
        password: &str,
    ) -> Self {
        let encoded_user = utf8_percent_encode(user, NON_ALPHANUMERIC).to_string();
        let encoded_pass = utf8_percent_encode(password, NON_ALPHANUMERIC).to_string();
        let connection_string = format!(
            "mysql://{}:{}@{}:{}/{}",
            encoded_user, encoded_pass, host, port, database
        );
        DataSourceConfig {
            source_type: DataSourceType::MySQL,
            connection_string,
            alias: alias.to_string(),
            extra_params: HashMap::new(),
        }
    }

    /// 创建 PostgreSQL 数据源配置
    pub fn postgres(
        alias: &str,
        host: &str,
        port: u16,
        database: &str,
        user: &str,
        password: &str,
    ) -> Self {
        let encoded_user = utf8_percent_encode(user, NON_ALPHANUMERIC).to_string();
        let encoded_pass = utf8_percent_encode(password, NON_ALPHANUMERIC).to_string();
        let connection_string = format!(
            "postgresql://{}:{}@{}:{}/{}",
            encoded_user, encoded_pass, host, port, database
        );
        DataSourceConfig {
            source_type: DataSourceType::PostgreSQL,
            connection_string,
            alias: alias.to_string(),
            extra_params: HashMap::new(),
        }
    }

    /// 创建 SQLite 数据源配置
    pub fn sqlite(alias: &str, path: &str) -> Self {
        DataSourceConfig {
            source_type: DataSourceType::SQLite,
            connection_string: path.to_string(),
            alias: alias.to_string(),
            extra_params: HashMap::new(),
        }
    }
}

/// 联邦查询管理器
///
/// 负责 ATTACH 外部数据源、物化远程表。
///
/// # 单向规则
/// 仅项目 ATTACH 全局，全局永不挂载项目库。
///
/// # 使用示例
/// ```rust,ignore
/// let federation = FederationManager::new();
/// federation.attach_data_source(&manager, config)?;
/// let result = federation.execute_federated_query(&manager, "SELECT * FROM global.users")?;
/// ```
pub struct FederationManager {
    /// 已挂载的数据源: 别名 -> 配置
    attached_sources: Mutex<HashMap<String, DataSourceConfig>>,
}

impl Default for FederationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FederationManager {
    /// 创建新的联邦查询管理器。
    pub fn new() -> Self {
        FederationManager {
            attached_sources: Mutex::new(HashMap::new()),
        }
    }

    /// ATTACH 外部数据源。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `sql`: ATTACH SQL 语句
    ///
    /// # 返回
    /// - `Ok(())`: 挂载成功
    /// - `Err(CoreError)`: 挂载失败
    ///
    /// # 示例
    /// ```rust,ignore
    /// federation.attach_sql(&conn, "ATTACH '/path/to/global.duckdb' AS global")?;
    /// ```
    pub fn attach_sql(&self, conn: &Connection, sql: &str) -> Result<(), CoreError> {
        // 解析 SQL 提取别名
        let alias = Self::parse_attach_alias(sql)?;

        let mut sources = self
            .attached_sources
            .lock()
            .map_err(|e| CoreError::common(CommonError::General(format!("获取锁失败: {}", e))))?;

        if sources.contains_key(&alias) {
            return Err(CoreError::common(CommonError::General(format!(
                "数据源 '{}' 已挂载",
                alias
            ))));
        }

        // 实际执行 ATTACH SQL
        conn.execute_batch(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!("ATTACH 数据源失败: {}", e)))
        })?;

        sources.insert(alias.clone(), DataSourceConfig::duckdb(&alias, ""));

        tracing::info!("[FederationManager] ATTACH 数据源: {}", alias);
        Ok(())
    }

    /// 生成并执行 ATTACH 语句。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `config`: 数据源配置
    ///
    /// # 返回
    /// - `Ok(())`: 挂载成功
    /// - `Err(CoreError)`: 挂载失败
    pub fn attach_data_source(
        &self,
        conn: &Connection,
        config: &DataSourceConfig,
    ) -> Result<(), CoreError> {
        let sql = Self::generate_attach_sql(config);
        self.attach_sql(conn, &sql)
    }

    /// DETACH 外部数据源。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `alias`: 数据源别名
    ///
    /// # 返回
    /// - `Ok(())`: 卸载成功
    /// - `Err(CoreError)`: 卸载失败
    pub fn detach(&self, conn: &Connection, alias: &str) -> Result<(), CoreError> {
        let mut sources = self
            .attached_sources
            .lock()
            .map_err(|e| CoreError::common(CommonError::General(format!("获取锁失败: {}", e))))?;

        if !sources.contains_key(alias) {
            return Err(CoreError::common(CommonError::General(format!(
                "数据源 '{}' 未挂载",
                alias
            ))));
        }

        // 实际执行 DETACH SQL
        let detach_sql = format!("DETACH {}", alias);
        conn.execute_batch(&detach_sql).map_err(|e| {
            CoreError::common(CommonError::General(format!("DETACH 数据源失败: {}", e)))
        })?;

        sources.remove(alias);
        tracing::info!("[FederationManager] DETACH 数据源: {}", alias);
        Ok(())
    }

    /// 生成 ATTACH SQL 语句。
    fn generate_attach_sql(config: &DataSourceConfig) -> String {
        match config.source_type {
            DataSourceType::DuckDB => {
                format!("ATTACH '{}' AS {}", config.connection_string, config.alias)
            }
            DataSourceType::SQLite => {
                format!("ATTACH '{}' AS {}", config.connection_string, config.alias)
            }
            DataSourceType::MySQL | DataSourceType::PostgreSQL => {
                // 网络数据源需要特殊语法，这里简化处理
                format!("ATTACH '{}' AS {}", config.connection_string, config.alias)
            }
        }
    }

    /// 获取已挂载的数据源列表。
    ///
    /// # 返回
    /// 已挂载数据源别名列表
    pub fn list_attached(&self) -> Vec<String> {
        self.attached_sources
            .lock()
            .map(|sources| sources.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// 检查数据源是否已挂载。
    ///
    /// # 参数
    /// - `alias`: 数据源别名
    ///
    /// # 返回
    /// true 表示已挂载
    pub fn is_attached(&self, alias: &str) -> bool {
        self.attached_sources
            .lock()
            .map(|sources| sources.contains_key(alias))
            .unwrap_or(false)
    }

    /// 清空所有已挂载的数据源。
    pub fn detach_all(&self) {
        if let Ok(mut sources) = self.attached_sources.lock() {
            sources.clear();
        }
    }

    /// 内部：从 ATTACH SQL 中提取别名。
    ///
    /// # 参数
    /// - `sql`: ATTACH SQL 语句
    ///
    /// # 返回
    /// 数据源别名
    fn parse_attach_alias(sql: &str) -> Result<String, CoreError> {
        let upper = sql.trim().to_uppercase();

        if !upper.starts_with("ATTACH") {
            return Err(CoreError::common(CommonError::General(
                "不是有效的 ATTACH 语句".to_string(),
            )));
        }

        // 提取 AS 后面的别名
        if let Some(as_pos) = upper.find(" AS ") {
            let alias = sql[(as_pos + 4)..].split_whitespace().next().unwrap_or("");
            if alias.is_empty() {
                return Err(CoreError::common(CommonError::General(
                    "ATTACH 语句缺少别名".to_string(),
                )));
            }
            Ok(alias.to_string())
        } else {
            // 没有 AS，使用默认别名
            Ok("attached".to_string())
        }
    }

    /// 生成 ATTACH 全局 DuckDB 的 SQL 语句。
    ///
    /// # 参数
    /// - `global_db_path`: 全局 DuckDB 文件路径
    ///
    /// # 返回
    /// ATTACH SQL 语句
    pub fn generate_attach_global_sql(global_db_path: &str) -> String {
        format!("ATTACH '{}' AS global", global_db_path)
    }

    /// 生成物化远程表的 SQL 语句。
    ///
    /// # 参数
    /// - `source_alias`: 数据源别名
    /// - `remote_table`: 远程表名
    /// - `local_table`: 本地临时表名
    ///
    /// # 返回
    /// CREATE TABLE AS SELECT 语句
    pub fn generate_materialize_sql(
        source_alias: &str,
        remote_table: &str,
        local_table: &str,
    ) -> String {
        format!(
            "CREATE TABLE {} AS SELECT * FROM {}.{}",
            local_table, source_alias, remote_table
        )
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn setup_test_db() -> Result<(Connection, PathBuf), CoreError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::common(CommonError::General(format!("时间获取失败: {}", e))))?
            .as_nanos();
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("test_federation_{}.duckdb", timestamp));

        let _ = fs::remove_file(&db_path);

        let conn = Connection::open(&db_path).map_err(|e| {
            CoreError::common(CommonError::General(format!("创建测试数据库失败: {}", e)))
        })?;
        Ok((conn, db_path))
    }

    fn cleanup_test_db(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_data_source_config_duckdb() {
        let config = DataSourceConfig::duckdb("global", "/path/to/global.duckdb");
        assert_eq!(config.source_type, DataSourceType::DuckDB);
        assert_eq!(config.alias, "global");
        assert_eq!(config.connection_string, "/path/to/global.duckdb");
    }

    #[test]
    fn test_data_source_config_mysql() {
        let config =
            DataSourceConfig::mysql("mysql_src", "localhost", 3306, "mydb", "user", "pass");
        assert_eq!(config.source_type, DataSourceType::MySQL);
        assert!(config.connection_string.contains("mysql://"));
        assert!(config.connection_string.contains("localhost"));
    }

    #[test]
    fn test_data_source_config_postgres() {
        let config =
            DataSourceConfig::postgres("pg_src", "localhost", 5432, "mydb", "user", "pass");
        assert_eq!(config.source_type, DataSourceType::PostgreSQL);
        assert!(config.connection_string.contains("postgresql://"));
    }

    #[test]
    fn test_attach_and_detach() -> Result<(), CoreError> {
        let (conn, db_path) = setup_test_db()?;
        let federation = FederationManager::new();

        assert!(federation.list_attached().is_empty());

        // ATTACH 另一个 DuckDB 文件
        let temp_attach_db =
            std::env::temp_dir().join(format!("test_attach_source_{}.duckdb", std::process::id()));
        let _ = fs::remove_file(&temp_attach_db);
        let _source_conn = Connection::open(&temp_attach_db).map_err(|e| {
            CoreError::common(CommonError::General(format!("创建源数据库失败: {}", e)))
        })?;
        drop(_source_conn);

        let sql = format!("ATTACH '{}' AS global", temp_attach_db.display());
        federation.attach_sql(&conn, &sql)?;
        assert_eq!(federation.list_attached().len(), 1);
        assert!(federation.is_attached("global"));

        // 重复 ATTACH 应失败
        assert!(federation.attach_sql(&conn, &sql).is_err());

        // DETACH
        federation.detach(&conn, "global")?;
        assert!(federation.list_attached().is_empty());

        let _ = fs::remove_file(&temp_attach_db);
        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_detach_non_existent() -> Result<(), CoreError> {
        let (conn, db_path) = setup_test_db()?;
        let federation = FederationManager::new();
        assert!(federation.detach(&conn, "non_existent").is_err());
        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_detach_all() -> Result<(), CoreError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::common(CommonError::General(format!("时间获取失败: {}", e))))?
            .as_nanos();

        let (conn, db_path) = setup_test_db()?;
        let federation = FederationManager::new();

        let temp_db1 = std::env::temp_dir().join(format!("test_attach_db1_{}.duckdb", timestamp));
        let temp_db2 =
            std::env::temp_dir().join(format!("test_attach_db2_{}.duckdb", timestamp + 1));
        let _ = fs::remove_file(&temp_db1);
        let _ = fs::remove_file(&temp_db2);
        let _ = Connection::open(&temp_db1);
        let _ = Connection::open(&temp_db2);

        federation.attach_sql(&conn, &format!("ATTACH '{}' AS db1", temp_db1.display()))?;
        federation.attach_sql(&conn, &format!("ATTACH '{}' AS db2", temp_db2.display()))?;

        assert_eq!(federation.list_attached().len(), 2);

        federation.detach_all();
        assert!(federation.list_attached().is_empty());

        let _ = fs::remove_file(&temp_db1);
        let _ = fs::remove_file(&temp_db2);
        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_parse_attach_alias() -> Result<(), CoreError> {
        let alias =
            FederationManager::parse_attach_alias("ATTACH '/path/to/global.duckdb' AS global")?;
        assert_eq!(alias, "global");
        Ok(())
    }

    #[test]
    fn test_parse_attach_alias_without_as() -> Result<(), CoreError> {
        let alias = FederationManager::parse_attach_alias("ATTACH '/path/to/db.duckdb'")?;
        assert_eq!(alias, "attached");
        Ok(())
    }

    #[test]
    fn test_parse_invalid_attach() {
        assert!(FederationManager::parse_attach_alias("SELECT * FROM users").is_err());
    }

    #[test]
    fn test_generate_attach_global_sql() {
        let sql = FederationManager::generate_attach_global_sql("/path/to/global.duckdb");
        assert_eq!(sql, "ATTACH '/path/to/global.duckdb' AS global");
    }

    #[test]
    fn test_generate_materialize_sql() {
        let sql = FederationManager::generate_materialize_sql("global", "users", "tmp_q_users");
        assert_eq!(
            sql,
            "CREATE TABLE tmp_q_users AS SELECT * FROM global.users"
        );
    }
}

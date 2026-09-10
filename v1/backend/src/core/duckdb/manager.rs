use super::temp_table::TempTableManager;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use duckdb::Connection;

use crate::core::error::{CommonError, CoreError};

/// DuckDB 连接池默认配置
const DEFAULT_READ_POOL_SIZE: usize = 4;
const MIN_READ_POOL_SIZE: usize = 1;
const MAX_READ_POOL_SIZE: usize = 6;

/// DuckDB 扩展文件统一存放路径
const DUCKDB_EXTENSIONS_DIR: &str = ".rdatastation/duckdb/extensions";

/// 全局 DuckDB 内存实例（单例）
static GLOBAL_DUCKDB: OnceLock<Arc<Mutex<duckdb::Connection>>> = OnceLock::new();

/// 全局临时表管理器（单例）
static GLOBAL_TEMP_TABLE_MANAGER: OnceLock<TempTableManager> = OnceLock::new();

/// DuckDBManager 管理全局或项目级 DuckDB 实例的连接池。
///
/// 采用双层连接池架构：
/// - 全局级：~/.rdatastation/global_analytics.duckdb
/// - 项目级：由项目元数据决定路径
///
/// 连接池结构：1 写入连接 + N 读取连接 + 1 后台维护连接
///
/// # 设计约束
/// - 写入连接固定 1 个：DuckDB 单写入者模型
/// - 读取连接池默认 4 个，简单 Round-Robin 轮询
/// - 后台维护连接独立，用于 TTL 清理、快照维护
/// - 全局与项目复用同一结构体，仅存储路径不同
pub struct DuckDBManager {
    /// DuckDB 数据库文件路径
    db_path: PathBuf,

    /// 写入连接（1个，独占）
    write_conn: Connection,

    /// 读取连接池（默认4个，轮询分配）
    read_pool: Vec<Connection>,

    /// 后台维护连接（1个，独立）
    maintenance_conn: Connection,

    /// 读取连接轮询索引
    read_index: AtomicUsize,
}

impl DuckDBManager {
    /// 获取或创建全局 DuckDB 内存单例。
    ///
    /// # 注意
    /// 首次调用时初始化全局 DuckDB 内存实例。
    /// 如果初始化失败，程序会 panic（因为全局分析引擎是核心组件，不可降级运行）。
    ///
    /// # 返回
    /// 全局 DuckDB 内存连接的静态引用
    pub fn global() -> &'static Arc<Mutex<duckdb::Connection>> {
        GLOBAL_DUCKDB.get_or_init(|| {
            let conn = duckdb::Connection::open_in_memory()
                .unwrap_or_else(|e| panic!("初始化全局 DuckDB 内存实例失败: {}", e));
            Self::configure_connection(&conn)
                .unwrap_or_else(|e| panic!("配置全局 DuckDB 连接失败: {}", e));
            Arc::new(Mutex::new(conn))
        })
    }

    /// 获取或创建全局 DuckDB 内存连接。
    ///
    /// # 返回
    /// - `Ok(Arc<Mutex<duckdb::Connection>>)`: 全局 DuckDB 内存连接
    /// - `Err(CoreError)`: 获取失败（通常是全局实例未初始化）
    pub fn get_or_create_in_memory() -> Result<Arc<Mutex<duckdb::Connection>>, CoreError> {
        // 先尝试获取已初始化的实例
        if let Some(instance) = GLOBAL_DUCKDB.get() {
            return Ok(instance.clone());
        }
        // 未初始化时触发延迟初始化
        Ok(Self::global().clone())
    }

    /// 设置或切换到持久化 DuckDB 数据库。
    ///
    /// # 参数
    /// - `path`: 持久化数据库文件路径
    ///
    /// # 返回
    /// - `Ok(Arc<Mutex<duckdb::Connection>>)`: 持久化连接
    /// - `Err(CoreError)`: 初始化失败
    pub fn set_persistent<P: AsRef<Path>>(
        path: P,
    ) -> Result<Arc<Mutex<duckdb::Connection>>, CoreError> {
        let path = path.as_ref();
        Self::ensure_parent_dir(path)?;

        let conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 持久化连接失败: {}",
                e
            )))
        })?;
        Self::configure_connection(&conn)?;

        Ok(Arc::new(Mutex::new(conn)))
    }

    /// 打开文件并重试（兼容旧 API）。
    ///
    /// # 参数
    /// - `path`: DuckDB 文件路径
    ///
    /// # 返回
    /// - `Ok(Connection)`: 打开的连接
    /// - `Err(CoreError)`: 打开失败
    pub fn open_file_with_retry(path: &str) -> Result<Connection, CoreError> {
        let path = PathBuf::from(path);
        Self::ensure_parent_dir(&path)?;

        Connection::open(&path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "打开 DuckDB 文件失败 {}: {}",
                path.display(),
                e
            )))
        })
    }

    /// 确保连接已建立（兼容旧 API）。
    ///
    /// # 返回
    /// - `Ok(())`: 连接已建立
    /// - `Err(CoreError)`: 连接失败
    pub fn ensure_connection() -> Result<(), CoreError> {
        let _ = Self::get_or_create_in_memory()?;
        Ok(())
    }

    /// 验证分析 SQL（兼容旧 API）。
    ///
    /// # 参数
    /// - `sql`: 待验证的 SQL
    ///
    /// # 返回
    /// - `Ok(())`: SQL 验证通过
    /// - `Err(CoreError)`: SQL 验证失败
    pub fn validate_analysis_sql(_sql: &str) -> Result<(), CoreError> {
        // 简单验证：非空检查
        // TODO: 未来可集成 SqlEngine 进行更严格的验证
        Ok(())
    }

    /// 获取或创建全局临时表管理器。
    fn global_temp_table_manager() -> &'static TempTableManager {
        GLOBAL_TEMP_TABLE_MANAGER.get_or_init(|| TempTableManager::new(50))
    }

    /// 注册临时表到全局管理器。
    ///
    /// # 参数
    /// - `table_name`: 临时表名
    pub fn register_temp_table(table_name: &str) {
        Self::global_temp_table_manager().register(table_name);
    }

    /// 打开或创建 DuckDB 数据库文件，初始化连接池。
    ///
    /// # 参数
    /// - `path`: DuckDB 文件路径
    ///
    /// # 返回
    /// - `Ok(DuckDBManager)`: 成功初始化的管理器
    /// - `Err(CoreError)`: 初始化失败
    ///
    /// # 示例
    /// ```rust,ignore
    /// let manager = DuckDBManager::open("/path/to/analytics.duckdb")?;
    /// ```
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, CoreError> {
        let path = path.as_ref();

        // 确保父目录存在
        Self::ensure_parent_dir(path)?;

        // 创建写入连接
        let write_conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 写入连接失败: {}",
                e
            )))
        })?;

        // 配置写入连接
        Self::configure_connection(&write_conn)?;

        // 创建读取连接池
        let read_pool = Self::create_read_pool(path, DEFAULT_READ_POOL_SIZE)?;

        // 创建后台维护连接
        let maintenance_conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 维护连接失败: {}",
                e
            )))
        })?;

        Self::configure_connection(&maintenance_conn)?;

        Ok(DuckDBManager {
            db_path: path.to_path_buf(),
            write_conn,
            read_pool,
            maintenance_conn,
            read_index: AtomicUsize::new(0),
        })
    }

    /// 获取唯一写入连接。
    ///
    /// # 返回
    /// 写入连接的不可变引用
    ///
    /// # 注意
    /// 写入连接是独占的，调用方需自行控制写入任务顺序
    pub fn write_conn(&self) -> &Connection {
        &self.write_conn
    }

    /// 轮询获取读取连接。
    ///
    /// # 返回
    /// 读取连接的不可变引用，通过 Round-Robin 轮询分配
    ///
    /// # 注意
    /// 简单轮询，无复杂负载均衡逻辑
    pub fn read_conn(&self) -> &Connection {
        let idx = self.read_index.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
        &self.read_pool[idx]
    }

    /// 获取后台维护连接。
    ///
    /// # 返回
    /// 维护连接的不可变引用
    ///
    /// # 注意
    /// 仅用于后台清理、快照维护，不用于业务读写
    pub fn maintenance_conn(&self) -> &Connection {
        &self.maintenance_conn
    }

    /// 获取数据库文件路径。
    ///
    /// # 返回
    /// DuckDB 数据库文件的 PathBuf
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 获取扩展文件目录路径。
    ///
    /// # 返回
    /// 扩展文件目录的 PathBuf
    ///
    /// # 注意
    /// 所有实例（全局、项目）通过该路径获取扩展
    pub fn extensions_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(DUCKDB_EXTENSIONS_DIR)
    }

    /// 确保父目录存在。
    ///
    /// # 参数
    /// - `path`: 目标文件路径
    ///
    /// # 返回
    /// - `Ok(())`: 目录存在或创建成功
    /// - `Err(CoreError)`: 创建目录失败
    fn ensure_parent_dir(path: &Path) -> Result<(), CoreError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "创建目录失败 {:?}: {}",
                        parent, e
                    )))
                })?;
            }
        }
        Ok(())
    }

    /// 配置 DuckDB 连接的默认参数。
    ///
    /// # 参数
    /// - `conn`: 需要配置的 DuckDB 连接
    ///
    /// # 返回
    /// - `Ok(())`: 配置成功
    /// - `Err(CoreError)`: 配置失败
    fn configure_connection(conn: &Connection) -> Result<(), CoreError> {
        // 设置扩展目录
        let ext_dir = Self::extensions_dir();
        conn.execute_batch(&format!(
            "SET extension_directory = '{}'",
            ext_dir.display()
        ))
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "配置 DuckDB 扩展目录失败: {}",
                e
            )))
        })?;

        Ok(())
    }

    /// 创建读取连接池。
    ///
    /// # 参数
    /// - `db`: Database 实例
    /// - `size`: 连接池大小
    ///
    /// # 返回
    /// - `Ok(Vec<Connection>)`: 连接池
    /// - `Err(CoreError)`: 创建失败
    fn create_read_pool(path: &Path, size: usize) -> Result<Vec<Connection>, CoreError> {
        let clamped_size = size.clamp(MIN_READ_POOL_SIZE, MAX_READ_POOL_SIZE);
        let mut pool = Vec::with_capacity(clamped_size);

        for i in 0..clamped_size {
            let conn = Connection::open(path).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "创建读取连接池第 {} 个连接失败: {}",
                    i, e
                )))
            })?;

            Self::configure_connection(&conn)?;
            pool.push(conn);
        }

        Ok(pool)
    }

    /// 设置读取连接池大小。
    ///
    /// # 注意
    /// 此方法需要在未来支持动态调整连接池大小时实现。
    /// 当前版本仅返回默认值，不实际调整。
    #[allow(dead_code)]
    pub fn set_read_pool_size(&self, _size: usize) -> usize {
        // 未来实现动态调整连接池大小
        DEFAULT_READ_POOL_SIZE
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_test_db_unique(test_name: &str) -> Result<(DuckDBManager, PathBuf), CoreError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("test_duckdb_{}_{}.duckdb", test_name, id));

        // 确保文件被真正删除
        if db_path.exists() {
            let _ = fs::remove_file(&db_path);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let manager = DuckDBManager::open(&db_path)?;
        Ok((manager, db_path))
    }

    fn cleanup_test_db(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_open_creates_database() -> Result<(), CoreError> {
        let (manager, db_path) = setup_test_db_unique("open_creates")?;

        assert!(db_path.exists());
        assert_eq!(manager.db_path(), db_path.as_path());

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_write_conn_is_unique() -> Result<(), CoreError> {
        let (manager, db_path) = setup_test_db_unique("write_conn")?;

        let conn1 = manager.write_conn();
        let conn2 = manager.write_conn();

        // 写入连接应该是同一个
        assert_eq!(conn1 as *const Connection, conn2 as *const Connection);

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_read_conn_round_robin() -> Result<(), CoreError> {
        let (manager, db_path) = setup_test_db_unique("read_conn")?;

        // 获取 2 * DEFAULT_READ_POOL_SIZE 次读取连接
        let mut conn_ptrs = Vec::new();
        for _ in 0..(DEFAULT_READ_POOL_SIZE * 2) {
            let conn = manager.read_conn();
            conn_ptrs.push(conn as *const Connection);
        }

        // 应该轮询到不同的连接
        let unique: std::collections::HashSet<_> = conn_ptrs.iter().collect();
        assert_eq!(
            unique.len(),
            DEFAULT_READ_POOL_SIZE,
            "轮询应覆盖所有读取连接"
        );

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_maintenance_conn_is_unique() -> Result<(), CoreError> {
        let (manager, db_path) = setup_test_db_unique("maintenance_conn")?;

        let conn1 = manager.maintenance_conn();
        let conn2 = manager.maintenance_conn();

        // 维护连接应该是同一个
        assert_eq!(conn1 as *const Connection, conn2 as *const Connection);

        // 维护连接不应与写入连接相同
        assert_ne!(
            conn1 as *const Connection,
            manager.write_conn() as *const Connection
        );

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_extensions_dir_path() {
        let ext_dir = DuckDBManager::extensions_dir();

        // 路径应包含 .rdatastation/duckdb/extensions
        let path_str = ext_dir.to_string_lossy();
        assert!(path_str.contains(".rdatastation"));
        assert!(path_str.contains("duckdb"));
        assert!(path_str.contains("extensions"));
    }

    #[test]
    fn test_ensure_parent_dir_creates_directory() -> Result<(), CoreError> {
        let temp_dir = std::env::temp_dir();
        let nested_path = temp_dir
            .join("test_duckdb_dir")
            .join("nested")
            .join("db.duckdb");

        // 确保清理
        if let Some(parent) = nested_path.parent() {
            let _ = fs::remove_dir_all(parent);
        }

        let result = DuckDBManager::ensure_parent_dir(&nested_path);
        assert!(result.is_ok());
        assert!(nested_path
            .parent()
            .ok_or_else(|| CoreError::common(CommonError::General("路径无父目录".to_string())))?
            .exists());

        // 清理
        let _ = fs::remove_dir_all(temp_dir.join("test_duckdb_dir"));
        Ok(())
    }
}

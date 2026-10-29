use super::temp_table::{TempTableManager, TempTableSource, TempTableStats};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use duckdb::Connection;

use shared::error::{CommonError, CoreError};

/// DuckDB 连接池默认配置
const DEFAULT_READ_POOL_SIZE: usize = 4;
#[cfg(not(windows))]
const MIN_READ_POOL_SIZE: usize = 1;
#[cfg(not(windows))]
const MAX_READ_POOL_SIZE: usize = 6;

/// 全局 DuckDB 内存实例（单例）
static GLOBAL_DUCKDB: OnceLock<Arc<Mutex<duckdb::Connection>>> = OnceLock::new();

/// 全局临时表管理器（单例）
static GLOBAL_TEMP_TABLE_MANAGER: OnceLock<TempTableManager> = OnceLock::new();

/// 最后一次**显式**设置的内存上限（字节）；`0` = 尚未设置（走环境变量 / 默认路径）。
///
/// 为什么要记这一份：`current_setting('memory_limit')` 在 DuckDB 1.5+ 会把值重新
/// 格式化成「1.8 GiB」这种近似读数（十进制存、二进制显示，一位小数），直接拿它显示
/// 「内存 X / 上限」会出现「1.9 GB / 2 GB」的怪样。显式设置（设置页 / sink）时记
/// 精确字节，读数优先用它；`0` 才回退解析 DuckDB 的读数（环境变量覆盖路径）。
static LAST_MEMORY_LIMIT_BYTES: AtomicU64 = AtomicU64::new(0);

/// 内存库的可观测读数（状态栏 / 洞察工具栏「内存 X MB / 上限」）。
///
/// `database_bytes` 是 `pragma_database_size('memory')` 的口径——memory catalog 的
/// 逻辑大小，**近似**进程实际占用（DuckDB 不暴露进程 RSS），上限之下可信。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryStats {
    /// 当前生效的内存上限（字节；来自 `current_setting('memory_limit')`）。
    pub limit_bytes: u64,
    /// 内存 catalog 当前大小（字节）；`None` = 该 pragma 在当前版本不可用。
    pub database_bytes: Option<u64>,
    /// 分析临时表数量（查询结果 / 洞察 / Mock / 插件四类合计，登记表口径）。
    pub table_count: usize,
}

/// DuckDBManager 管理全局或项目级 DuckDB 实例的连接池。
///
/// 采用双层连接池架构：
/// - 全局级：`<RDS_HOME>/data/system/analytics.duckdb`
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
    maintenance_conn: Option<Connection>,

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
        shared::duckdb_open::open_file_with_retry(path)
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

    /// 分析引擎在本进程里起来了没有（**纯读**：不触发初始化、不拿锁）
    ///
    /// 界面门控用（B13「执行位置」每帧都要判一次）：`global()` 会顺手初始化——首次
    /// 可能有真实代价，渲染路径不能调它；`get_or_create_in_memory()` 也会初始化。
    /// 还没起来就是“尚不可用”，原因由调用方拼。
    pub fn is_initialized() -> bool {
        GLOBAL_DUCKDB.get().is_some()
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

    /// 全局临时表管理器（分析路径用它做登记 / 注销 / 惰性清理）。
    ///
    /// 公开它的理由：清理决策（TTL / 上限）在管理器里，而**真正的 DROP 只有持有连接
    /// 的地方能执行**——两边必须能对上，否则会出现「登记摘掉了、表还在库里」的孤儿
    /// （见 `duckdb::analysis` 与架构 K16）。
    pub fn temp_table_manager() -> &'static TempTableManager {
        Self::global_temp_table_manager()
    }

    /// 临时表登记的**数量概览**（日志 / 诊断用；权威来源仍是库本身）。
    ///
    /// 洞察的建表路径（`duckdb::analysis`）在接近上限时会拿它告警——这是「内存库在长大」
    /// 唯一的提前信号（架构 K16 ④）。
    pub fn temp_table_stats() -> TempTableStats {
        Self::global_temp_table_manager().stats()
    }

    /// 运行时调整内存库上限（MB）：在**已打开的全局内存连接**上 `SET memory_limit`。
    ///
    /// 为什么是运行时：上限是连接级设置，随时可改；设置页改完经装配层 sink 落到这里，
    /// 不必重启。DuckDB 到顶后溢写（`temp_directory` 已钉住），改小不释放已占用的内存——
    /// 调用方（hint 文案）要把这点讲给用户。
    /// 设置 sink 适配：把设置页的内存上限（MB）应用到分析引擎，供
    /// `settings::install_insight_memory_limit_sink` 注册（sink 签名是 `fn(i64)`，
    /// 不能返回 Result）。失败只记日志——设置页改完立即生效，失败时下一次
    /// 装配（进入洞察）会再次对齐。
    pub fn apply_memory_limit_mb(mb: i64) {
        if let Err(error) = Self::set_memory_limit_mb(mb.max(0) as u64) {
            tracing::error!("应用洞察内存上限失败（{mb}MB）: {error}");
        }
    }

    pub fn set_memory_limit_mb(mb: u64) -> Result<(), CoreError> {
        let conn = Self::get_or_create_in_memory()?;
        let guard = conn.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
        })?;
        guard.execute_batch(&format!("SET memory_limit = '{mb}MB'")).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "调整分析引擎内存上限失败（{mb}MB）: {e}"
            )))
        })?;
        // 记录精确字节（见 [`LAST_MEMORY_LIMIT_BYTES`] 的说明：读数要回退解析才会用到它）
        LAST_MEMORY_LIMIT_BYTES.store(mb.saturating_mul(1024 * 1024), Ordering::Relaxed);
        Ok(())
    }

    /// 内存库可观测读数：上限 + 实际大小 + 临时表数量。
    ///
    /// 纯查询、拿锁后一次读完；**调用方不得持有该锁**（与 [`Self::in_memory_temp_tables`]
    /// 同一约定）。`database_size` 读不到不致命（返回 `None`，界面显示「—」）。
    pub fn memory_stats() -> Result<MemoryStats, CoreError> {
        let conn = Self::get_or_create_in_memory()?;
        let guard = conn.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
        })?;
        // 上限：优先用显式设置记录的精确值；`0`（环境变量 / 默认路径）才回退解析
        // DuckDB 的读数（`1.8 GiB` 这类近似值，见 [`LAST_MEMORY_LIMIT_BYTES`]）。
        let limit_bytes = match LAST_MEMORY_LIMIT_BYTES.load(Ordering::Relaxed) {
            0 => guard
                .query_row(
                    "SELECT current_setting('memory_limit')",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "读取分析引擎内存上限失败: {e}"
                    )))
                })
                .and_then(|raw| {
                    parse_human_size(&raw).ok_or_else(|| {
                        CoreError::common(CommonError::General(format!(
                            "内存上限读数 `{raw}` 解析失败"
                        )))
                    })
                })?,
            recorded => recorded,
        };
        // 实际大小：memory catalog 的逻辑大小（近似内存占用，失败不致命）
        let database_bytes = guard
            .query_row(
                "SELECT database_size FROM pragma_database_size('memory')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|bytes| bytes.max(0) as u64);
        drop(guard);
        Ok(MemoryStats {
            limit_bytes,
            database_bytes,
            table_count: Self::global_temp_table_manager().stats().total(),
        })
    }

    /// 注册临时表到全局管理器。
    ///
    /// # 参数
    /// - `table_name`: 临时表名
    pub fn register_temp_table(table_name: &str) {
        Self::global_temp_table_manager().register(table_name);
    }

    /// 列出**内存库**里某来源的临时表（权威来源是库本身，不是注册表）。
    ///
    /// # 注意
    /// 内部要取内存库连接锁（`GLOBAL_DUCKDB` 是 `Mutex<Connection>`）：
    /// **调用方不得持有该锁**，否则同线程重入会死锁。
    pub fn in_memory_temp_tables(source: TempTableSource) -> Result<Vec<String>, CoreError> {
        let conn = Self::get_or_create_in_memory()?;
        let guard = conn.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
        })?;
        TempTableManager::list_by_source(&guard, source)
    }

    /// 删除**内存库**里某来源的全部临时表（同步注册表），返回被删表名。
    ///
    /// 用于「项目切换 / 关闭时清掉上一项目的临时产物」：内存库是**进程级单例**，
    /// 不随项目切换自动释放，同名重复生成会重建，但换名字就会逐张累积。
    ///
    /// # 注意
    /// 同 [`Self::in_memory_temp_tables`]：调用方不得持有内存库连接锁。
    pub fn drop_in_memory_temp_tables(source: TempTableSource) -> Result<Vec<String>, CoreError> {
        let conn = Self::get_or_create_in_memory()?;
        let guard = conn.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {e}")))
        })?;
        Self::global_temp_table_manager().drop_by_source(&guard, source)
    }

    /// [`Self::drop_in_memory_temp_tables`] 的**非阻塞**版：锁被占用时返回 `Ok(None)`。
    ///
    /// 用途：切项目时宿主在 UI 线程上清理，而出口任务（不可取消）可能整段持有连接锁——
    /// 在 UI 线程上等锁就是界面假死；拿不到就先跳过，留给调用方稍后重试。
    pub fn try_drop_in_memory_temp_tables(
        source: TempTableSource,
    ) -> Result<Option<Vec<String>>, CoreError> {
        let conn = Self::get_or_create_in_memory()?;
        let Ok(guard) = conn.try_lock() else {
            return Ok(None);
        };
        Self::global_temp_table_manager()
            .drop_by_source(&guard, source)
            .map(Some)
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

        // 创建唯一连接（Windows 平台限制：duckdb-rs bundled 同进程对同一
        // DuckDB 文件仅允许一个连接句柄，多连接在 Windows 报文件被占用；
        // 因此读/维护均复用该连接，由 DuckDB 自身支持单连接读写）。
        let write_conn = Connection::open(path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "创建 DuckDB 写入连接失败: {}",
                e
            )))
        })?;

        // 配置写入连接
        Self::configure_connection(&write_conn)?;

        Ok(DuckDBManager {
            db_path: path.to_path_buf(),
            write_conn,
            // Windows：读连接池留空，read_conn() 回退到唯一连接
            read_pool: Vec::new(),
            maintenance_conn: None,
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
        if self.read_pool.is_empty() {
            // Windows 单连接模式：读写共用唯一连接
            &self.write_conn
        } else {
            let idx = self.read_index.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
            &self.read_pool[idx]
        }
    }

    /// 获取后台维护连接。
    ///
    /// # 返回
    /// 维护连接的不可变引用
    ///
    /// # 注意
    /// 仅用于后台清理、快照维护，不用于业务读写
    pub fn maintenance_conn(&self) -> &Connection {
        match &self.maintenance_conn {
            Some(c) => c,
            None => &self.write_conn,
        }
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
    /// 所有实例（全局、项目）通过该路径获取扩展；位置由 `paths::extensions_dir()`
    /// 统一解析（`<RDS_HOME>/extensions`）——改造前这里与 `init_extensions` 各用
    /// 一个目录（`~/.rdatastation` vs `{data_dir}/duckdb`），查扩展会查错地方。
    pub fn extensions_dir() -> PathBuf {
        paths::extensions_dir()
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
    /// **所有长期存活的 DuckDB 连接都要过这里**（全局 / 项目 / 读取池 / 加速会话 / duckdb 驱动）。
    /// 实现已下沉 `rds_store::duckdb_config`（engine 瘦身第三刀：rds-driver 与 engine 两侧共用，
    /// driver 不能依赖 engine），此处保留委托，engine 内部调用点零改动。五件事（扩展目录 /
    /// 不静默联网 / 已装自动加载 / 内存闸 / 溢写口）的完整说明见 `rds_store::duckdb_config`。
    pub(crate) fn configure_connection(conn: &Connection) -> Result<(), CoreError> {
        rds_store::duckdb_config::configure_connection(conn)
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
    #[cfg(not(windows))]
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

/// 把 DuckDB 的 `current_setting('memory_limit')` 值（`2GB` / `2048MB` / `1.8 GiB` /
/// `1073741824`）解析成字节；认不得返回 `None`（调用方显示「—」而不是编一个数）。
///
/// DuckDB 1.5+ 对设置值做**人类可读重格式化**（`SET memory_limit='2GB'` 后读回是
/// `1.8 GiB`——它按十进制 GB 存 2,000,000,000 字节、按二进制单位显示），所以
/// 这里必须同时认二进制后缀（KiB / MiB / GiB / TiB，可带小数）与十进制后缀。
fn parse_human_size(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let value: f64 = num.parse().ok()?;
    let mult = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1.0,
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        "KIB" => 1024.0,
        "MIB" => 1024.0 * 1024.0,
        "GIB" => 1024.0 * 1024.0 * 1024.0,
        "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((value * mult) as u64)
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

        // 轮询语义：Windows 单连接模式下所有读取连接回退到唯一连接（池为空），
        // 非 Windows 多连接池模式应轮询覆盖全部读取连接
        #[cfg(windows)]
        {
            let unique: std::collections::HashSet<_> = conn_ptrs.iter().collect();
            assert_eq!(unique.len(), 1, "Windows 单连接模式应复用唯一连接");
            assert_eq!(
                conn_ptrs[0],
                manager.write_conn() as *const Connection,
                "单连接模式读取连接应等于写连接"
            );
        }
        #[cfg(not(windows))]
        {
            let unique: std::collections::HashSet<_> = conn_ptrs.iter().collect();
            assert_eq!(
                unique.len(),
                DEFAULT_READ_POOL_SIZE,
                "轮询应覆盖所有读取连接"
            );
        }

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

        // 维护连接与写入连接的关系：Windows 单连接模式二者相同（复用唯一连接），
        // 非 Windows 多连接模式下二者不同
        #[cfg(windows)]
        assert_eq!(
            conn1 as *const Connection,
            manager.write_conn() as *const Connection
        );
        #[cfg(not(windows))]
        assert_ne!(
            conn1 as *const Connection,
            manager.write_conn() as *const Connection
        );

        cleanup_test_db(&db_path);
        Ok(())
    }

    #[test]
    fn test_parse_human_size() {
        // `current_setting('memory_limit')` 的读数解析（大小写 / 空格不敏感）：
        // 十进制后缀按 10^n、二进制后缀（KiB/MiB/GiB）按 2^10n；DuckDB 1.5+ 回读
        // 设置值是「1.8 GiB」这种二进制格式化（十进制存、二进制显示，一位小数）。
        assert_eq!(parse_human_size("2GB"), Some(2_000_000_000));
        assert_eq!(parse_human_size("2048MB"), Some(2_048_000_000));
        assert_eq!(parse_human_size("512 mb"), Some(512_000_000));
        assert_eq!(parse_human_size("1073741824"), Some(1_073_741_824));
        assert_eq!(
            parse_human_size("1.8 GiB"),
            Some((1.8_f64 * 1024.0 * 1024.0 * 1024.0) as u64)
        );
        // 认不得返回 None（界面显示「—」，不编一个数）
        assert_eq!(parse_human_size("garbage"), None);
        assert_eq!(parse_human_size(""), None);
        assert_eq!(parse_human_size("-1MB"), None);
    }

    #[test]
    fn test_memory_stats_and_set_limit() -> Result<(), CoreError> {
        // 全局内存库单例：默认上限 2GB（无环境变量覆盖时）
        let stats = DuckDBManager::memory_stats()?;
        assert!(stats.limit_bytes > 0, "上限应可读");
        assert_eq!(
            stats.table_count,
            DuckDBManager::temp_table_stats().total(),
            "表数口径 = 登记表合计"
        );

        // 运行时改上限再读：应反映新值（1024 MB = 1 GiB）
        DuckDBManager::set_memory_limit_mb(1024)?;
        let after = DuckDBManager::memory_stats()?;
        assert_eq!(after.limit_bytes, 1024 * 1024 * 1024);

        // 恢复默认，避免影响后续测试（上限是连接级设置，改了不自动还原）
        DuckDBManager::set_memory_limit_mb(2048)?;
        Ok(())
    }

    #[test]
    fn test_configure_connection_applies_memory_and_temp_settings() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory().map_err(|e| {
            CoreError::common(CommonError::General(format!("打开内存库失败: {e}")))
        })?;
        DuckDBManager::configure_connection(&conn)?;

        let setting = |name: &str| -> Result<String, CoreError> {
            conn.query_row(&format!("SELECT current_setting('{name}')"), [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("读 {name} 失败: {e}")))
            })
        };

        // 内存闸真的设上了（DuckDB 会把它回显成 "1.9 GiB" 这类形式，故只断言非空）
        assert!(!setting("memory_limit")?.is_empty(), "memory_limit 应已设置");
        // 溢写口钉在数据根的 tmp/（路径比较忽略末尾分隔符）
        assert_eq!(
            PathBuf::from(setting("temp_directory")?),
            paths::temp_dir(),
            "temp_directory 应指向 paths::temp_dir()"
        );
        assert!(!setting("max_temp_directory_size")?.is_empty());
        // 溢写目录已经建出来了（否则 SET temp_directory 本身就会失败）
        assert!(paths::temp_dir().exists());

        // 扩展目录钉在数据根（“不碰 C 盘”与离线预置都靠它）；目录要已建出来
        assert_eq!(
            PathBuf::from(setting("extension_directory")?),
            paths::extensions_dir(),
            "extension_directory 应指向 paths::extensions_dir()"
        );
        assert!(paths::extensions_dir().exists(), "扩展目录应已建出来");

        // Secret 目录钉在 `{system}/secrets`（与凭据注册同一处；不设就见不到已注册的凭据）
        assert_eq!(
            PathBuf::from(setting("secret_directory")?),
            crate::migration::get_secrets_dir()?,
            "secret_directory 应指向 get_secrets_dir()"
        );
        assert!(
            crate::migration::get_secrets_dir()?.exists(),
            "Secret 目录应已建出来"
        );
        // 不静默联网；已装的自动加载保住日常体验（`allow_community_extensions` 改不了，
        // 它默认就是 true——真要关得在建库前用 `DBConfig`，见上面注释）
        // 注：布尔设置的 `current_setting` 回出来就是 BOOLEAN 类型（不能按 String 读）
        let bool_setting = |name: &str| -> Result<bool, CoreError> {
            conn.query_row(&format!("SELECT current_setting('{name}')"), [], |row| {
                row.get::<_, bool>(0)
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("读 {name} 失败: {e}")))
            })
        };
        assert!(!bool_setting("autoinstall_known_extensions")?);
        assert!(bool_setting("autoload_known_extensions")?);

        Ok(())
    }

    #[test]
    fn test_extensions_dir_path() {
        let ext_dir = DuckDBManager::extensions_dir();

        // 扩展目录跟随数据根：<RDS_HOME>/extensions
        assert_eq!(ext_dir, paths::extensions_dir());
        assert!(ext_dir.ends_with("extensions"));
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

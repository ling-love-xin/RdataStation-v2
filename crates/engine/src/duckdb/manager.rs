use super::temp_table::{TempTableManager, TempTableSource, TempTableStats};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use duckdb::Connection;

use shared::error::{CommonError, CoreError};

/// DuckDB 连接池默认配置
const DEFAULT_READ_POOL_SIZE: usize = 4;
#[cfg(not(windows))]
const MIN_READ_POOL_SIZE: usize = 1;
#[cfg(not(windows))]
const MAX_READ_POOL_SIZE: usize = 6;

/// 内存库的内存上限默认值（可用 [`ENV_MEMORY_LIMIT`] 覆盖）。
///
/// 为什么要有这道闸：内存库是**进程级单例**，查询结果表与洞察中间表都记在它头上，
/// 而 DuckDB 自己的默认值是「物理内存的 80%」——桌面应用把机器吃光不是可接受的失败方式。
/// 上限一到，DuckDB 会**溢写到 `temp_directory`**（而不是直接报错），所以换来的是「慢一点」。
const DEFAULT_MEMORY_LIMIT: &str = "2GB";

/// 覆盖内存上限的环境变量（例：`RDS_DUCKDB_MEMORY_LIMIT=4GB`）。非法值被忽略并告警。
const ENV_MEMORY_LIMIT: &str = "RDS_DUCKDB_MEMORY_LIMIT";

/// 溢写目录的总量上限默认值（可用 [`ENV_TEMP_SIZE_LIMIT`] 覆盖）。
///
/// DuckDB 默认 100GB；数据根的 `tmp/` 是我们自己会清理的目录，不该让它长到那种量级。
const DEFAULT_TEMP_SIZE_LIMIT: &str = "10GB";

/// 覆盖溢写目录上限的环境变量。
const ENV_TEMP_SIZE_LIMIT: &str = "RDS_DUCKDB_MAX_TEMP_SIZE";

/// 全局 DuckDB 内存实例（单例）
static GLOBAL_DUCKDB: OnceLock<Arc<Mutex<duckdb::Connection>>> = OnceLock::new();

/// 全局临时表管理器（单例）
static GLOBAL_TEMP_TABLE_MANAGER: OnceLock<TempTableManager> = OnceLock::new();

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
    /// 绕过它就等于绕过资源闸与扩展策略——审计时抳到过两次：`accel` 自己拼扩展目录
    /// （漏了内存闸与溢写口），duckdb 驱动干脆什么都不设（用户 SQL 里出现 `read_parquet`
    /// 这类函数时，扩展会被**静默下到 `~/.duckdb`**，写用户 C 盘）。
    ///
    /// 五件事：
    /// 1. **扩展目录**：一律 `<RDS_HOME>/extensions`（应用目录内，可离线预置）；
    /// 2. **不静默联网**：`autoinstall_known_extensions = false`——SQL 里出现未安装的扩展
    ///    函数时**报错**而不是偷偷下载（企业内网里那是“莫名卡住”）；`INSTALL` 一律由我们
    ///    显式做（见 `duckdb/extensions.rs` 与联邦的扩展门控）；
    /// 3. **已装的自动加载**：`autoload_known_extensions = true`——它只加载**本地已装**的
    ///    扩展，无网络副作用，保住日常体验（关掉它会让“之前能用”的查询突然报未加载）；
    ///    同时显式允许社区扩展（联邦要用 `mssql` / `oracle_scanner`，它们都是社区扩展）；
    /// 4. **内存闸**：`memory_limit`（DuckDB 默认是物理内存的 80%——桌面应用不该把机器吃光）；
    /// 5. **溢写口**：`temp_directory` 钉到数据根的 `tmp/`（到顶时溢写到这里，而不是系统
    ///    临时目录；那是用户清理不到的角落），并给溢写总量上也一把限。
    ///
    /// 两个大小值都可用环境变量覆盖（见 [`ENV_MEMORY_LIMIT`] / [`ENV_TEMP_SIZE_LIMIT`]）；
    /// **拼进 SQL 前一律过 [`parse_size_setting`] 白名单**，非法值回退默认并告警——
    /// 配错一个环境变量不该让程序起不来。
    ///
    /// # 参数
    /// - `conn`: 需要配置的 DuckDB 连接
    ///
    /// # 返回
    /// - `Ok(())`: 配置成功
    /// - `Err(CoreError)`: 配置失败
    pub(crate) fn configure_connection(conn: &Connection) -> Result<(), CoreError> {
        // 扩展目录：尽量先建出来（`INSTALL` 与离线预置都需要它）
        let extensions_dir = Self::extensions_dir();
        if let Err(e) = std::fs::create_dir_all(&extensions_dir) {
            tracing::warn!(
                "[duckdb] 扩展目录 {} 建不出来（{e}）：安装扩展时可能失败",
                extensions_dir.display()
            );
        }

        // 逐条收集再拼成一条 batch（`execute_batch` 仍按分号拆语句，换行不算分隔符）
        //
        // 注意 **不设 `allow_community_extensions`**：实测它在库跑起来之后不能改
        // （`Cannot change allow_community_extensions setting while database is running`），
        // 而它的默认值就是 `true`（联邦要用的 `mssql` / `oracle_scanner` 都是社区扩展）——
        // 要改它必须在建库前用 `DBConfig`，我们没这个需求，就不把它写进这里。
        let mut settings = vec![
            format!("SET extension_directory = '{}'", extensions_dir.display()),
            // 不静默联网（未装的扩展直接报错；装上是我们的事）
            "SET autoinstall_known_extensions = false".to_string(),
            // 已装的自动加载（无网络副作用）
            "SET autoload_known_extensions = true".to_string(),
        ];

        // 临时目录：建不出来就跳过溢写设置（退回 DuckDB 默认），不挡启动
        let temp_dir = paths::temp_dir();
        match std::fs::create_dir_all(&temp_dir) {
            Ok(()) => {
                settings.push(format!("SET temp_directory = '{}'", temp_dir.display()));
                settings.push(format!(
                    "SET max_temp_directory_size = '{}'",
                    size_setting(
                        std::env::var_os(ENV_TEMP_SIZE_LIMIT),
                        DEFAULT_TEMP_SIZE_LIMIT,
                        ENV_TEMP_SIZE_LIMIT,
                    )
                ));
            }
            Err(e) => tracing::warn!(
                "[duckdb] 溢写目录 {} 不可用（{e}）：将使用 DuckDB 默认临时目录",
                temp_dir.display()
            ),
        }

        settings.push(format!(
            "SET memory_limit = '{}'",
            size_setting(
                std::env::var_os(ENV_MEMORY_LIMIT),
                DEFAULT_MEMORY_LIMIT,
                ENV_MEMORY_LIMIT,
            )
        ));

        let sql = format!("{};", settings.join(";\n"));
        conn.execute_batch(&sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "配置 DuckDB 连接失败: {}（尝试的设置：{}）",
                e,
                settings.join("; ")
            )))
        })?;

        // 只在 debug 级：本函数对每个连接都会跑（非 Windows 还有读取连接池）
        tracing::debug!("[duckdb] 连接配置已应用：{}", settings.join("; "));
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

/// 取一个「大小」类设置：环境变量优先，非法值回退默认并告警。
///
/// # 参数
/// - `raw`: 环境变量原值（未设置时为 `None`）
/// - `default`: 回退值
/// - `env_name`: 环境变量名（仅用于告警文案）
fn size_setting(raw: Option<std::ffi::OsString>, default: &str, env_name: &str) -> String {
    let Some(raw) = raw else {
        return default.to_string();
    };
    let raw = raw.to_string_lossy();
    match parse_size_setting(&raw) {
        Some(ok) => ok,
        None => {
            tracing::warn!(
                "[duckdb] 环境变量 {env_name}=\"{}\" 不是合法大小，按默认值 {default} 处理",
                raw.trim()
            );
            default.to_string()
        }
    }
}

/// 校验并规整大小字符串（`2GB` / `512 mb` / `1073741824`）。
///
/// 白名单故意窄：这些值会被**拼进 SQL**（DuckDB 的 `SET` 不接受绑定参数），
/// 且窄白名单同时挡住了单位写错（`2 GBB`）与手滑。不合法返回 `None`。
fn parse_size_setting(raw: &str) -> Option<String> {
    let s = raw.trim();
    // 数字部分：数字与至多一个小数点；其余算单位（允许中间有空格）
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let unit = unit.trim();

    let digits_ok = !num.is_empty()
        && num.chars().any(|c| c.is_ascii_digit())
        && num.chars().filter(|c| *c == '.').count() <= 1;
    let unit_ok = matches!(
        unit.to_ascii_uppercase().as_str(),
        "" | "B" | "KB" | "MB" | "GB" | "TB"
    );

    (digits_ok && unit_ok).then(|| format!("{num}{}", unit.to_ascii_uppercase()))
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
    fn test_parse_size_setting_whitelist() {
        // 合法：数字 + 可选单位（大小写 / 空格不敏感）
        assert_eq!(parse_size_setting("2GB").as_deref(), Some("2GB"));
        assert_eq!(parse_size_setting(" 512 mb ").as_deref(), Some("512MB"));
        assert_eq!(parse_size_setting("1073741824").as_deref(), Some("1073741824"));
        assert_eq!(parse_size_setting("1.5gb").as_deref(), Some("1.5GB"));

        // 不合法：空 / 纯单位 / 负号 / 未知单位 / 带 SQL 尾巴——后者是白名单真正要挡的
        assert_eq!(parse_size_setting(""), None);
        assert_eq!(parse_size_setting("GB"), None);
        assert_eq!(parse_size_setting("-1"), None);
        assert_eq!(parse_size_setting("2PB"), None);
        assert_eq!(parse_size_setting("2GB'; DROP TABLE t; --"), None);
        assert_eq!(parse_size_setting("."), None);
    }

    #[test]
    fn test_size_setting_env_override_and_fallback() {
        use std::ffi::OsString;

        let default = "2GB";
        // 未设置 → 默认值
        assert_eq!(size_setting(None, default, "RDS_TEST"), default);
        // 合法覆盖 → 规整后的值
        assert_eq!(
            size_setting(Some(OsString::from("4 gb")), default, "RDS_TEST"),
            "4GB"
        );
        // 非法覆盖 → 回退默认（而不是拼进 SQL）
        assert_eq!(
            size_setting(Some(OsString::from("huge")), default, "RDS_TEST"),
            default
        );
        assert_eq!(
            size_setting(Some(OsString::from("   ")), default, "RDS_TEST"),
            default
        );
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

//! 本地加速通道（B13 切片二）：在 DuckDB 上直连源库（**只读挂载**）
//!
//! ## 它是什么
//!
//! 用户把「执行位置」切到**本地加速**后，语句**不发给源库驱动**，而是在 DuckDB 上执行：
//! 源库先用 `ATTACH … (TYPE mysql|postgres|sqlite, READ_ONLY)` 挂成一个 catalog，
//! 之后 `SELECT` 在 DuckDB 里跑（远端扫描与谓词下推由扩展负责）。
//!
//! ## 三条实测事实（决定了这个模块的形状，探针见
//! `crates/engine/tests/duckdb_extension_probe.rs`）
//!
//! 1. **表名解析**：DuckDB 的默认 catalog 是 `memory`，不写限定名**查不到**源表
//!    （`Table with name t does not exist`）。会话建立后 `USE <alias>` 把默认 catalog 指到
//!    附加源，用户的 `SELECT * FROM orders` 就直接可用；跨 schema 仍可显式写
//!    `<alias>.<schema>.<table>`。**不做 SQL 改写**——改写（`qualify_columns`）只是把
//!    同一件事换个地方做，还会引入“部分限定”的不确定行为。
//! 2. **写**：`READ_ONLY` 挂载由 DuckDB 自己拒（`INSERT` / `CREATE TABLE` 报
//!    "attached in read-only mode"），而**本地临时对象照常允许**（`CREATE TEMP TABLE`）——
//!    这正是“源库只读 / 会话变量可用”的分界。编辑器侧还有一道更早的闸（提交前就拒）。
//! 3. **新鲜度**：数据是**实时**的（源库插一行，同一 DuckDB 会话立刻能查到）；
//!    定型的是**表清单**——ATTACH 之后源库新建的表要重新 ATTACH 才可见。
//!    所以界面不能写“快照”；「重新 ATTACH」（[`AccelSession::refresh`]）的价值在表清单。
//!
//! ## 一条源一条连接
//!
//! 会话按 `conn_id` 缓存，理由两条：① `USE` 与临时表都挂在**连接**上，两个源共用一条连接
//! 会互相串味；② DuckDB 不允许同一文件在一条连接里挂两次（`Unique file handle conflict`）。
//!
//! 会话故意开在**内存库**上：分析库（`analytics.duckdb`）的写连接由 `DuckDBManager` 独占，
//! 这里再开文件会撞锁。代价是加速会话的临时表暂时与“分析会话”（1c）不共享——那是 1c 的
//! 话题（会话/通道归属），不在这里假装做到。
//!
//! ## 门控状态为什么要缓存
//!
//! 「执行位置」菜单每帧都要判可用性，而安装/加载扩展是 I/O（首次会**联网**取一次）。
//! 因此这里维护一份**内存状态**：没试过 = 不拦（`Unknown`），试过且失败 = 记下原因，
//! 菜单行尾就说那条原因（[`extension_state`]）；成功则清掉。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use duckdb::Connection;
use once_cell::sync::Lazy;

use shared::error::{CommonError, CoreError};
use shared::models::QueryResult;

/// 附加源的固定别名（用户 SQL 里可以显式写 `<alias>.<schema>.<table>` 跨 schema）
pub const SOURCE_ALIAS: &str = "rds_src";

/// 加速源的种类（决定要不要扩展、怎么挂）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccelKind {
    MySql,
    PostgreSql,
    Sqlite,
    DuckDb,
}

impl AccelKind {
    /// 连接的 `db_type` → 加速源种类
    ///
    /// `db_type` 来自驱动标识（`mysql_native` / `postgres_native` / `sqlite` / `duckdb`），
    /// 认不出就说出来（**不猜**：猜错会把语句发到一个不相干的库上）。
    pub fn from_db_type(db_type: &str) -> Result<Self, String> {
        let normalized = db_type.trim().to_ascii_lowercase();
        if normalized.starts_with("mysql") {
            Ok(Self::MySql)
        } else if normalized.starts_with("postgres") || normalized.starts_with("pg") {
            Ok(Self::PostgreSql)
        } else if normalized.starts_with("sqlite") {
            Ok(Self::Sqlite)
        } else if normalized.starts_with("duckdb") {
            Ok(Self::DuckDb)
        } else {
            Err(format!("这个驱动（{db_type}）暂时不能本地加速"))
        }
    }

    /// 界面文案
    pub fn label(self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::PostgreSql => "PostgreSQL",
            Self::Sqlite => "SQLite",
            Self::DuckDb => "DuckDB",
        }
    }

    /// 需要的 DuckDB 扩展（DuckDB 文件源不需要，是内核自带能力）
    fn extension(self) -> Option<&'static str> {
        match self {
            Self::MySql => Some("mysql"),
            Self::PostgreSql => Some("postgres"),
            Self::Sqlite => Some("sqlite"),
            Self::DuckDb => None,
        }
    }

    /// `ATTACH … (TYPE …)` 里的类型名（DuckDB 文件源不写 TYPE）
    fn attach_type(self) -> Option<&'static str> {
        match self {
            Self::MySql => Some("mysql"),
            Self::PostgreSql => Some("postgres"),
            Self::Sqlite => Some("sqlite"),
            Self::DuckDb => None,
        }
    }
}

/// 一个加速源（**宿主组装**：引擎不读连接库，也不解密口令）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccelSource {
    /// 源连接 id（会话按它缓存；结果集与历史也用它）
    pub conn_id: String,
    pub kind: AccelKind,
    /// 网络型 = 带凭据的 URL（`mysql://…` / `postgres://…`）；文件型 = **裸路径**
    pub connection_string: String,
}

impl AccelSource {
    /// 从宿主的连接信息组装
    ///
    /// `url` 是连接管理器里那条连接的 URL（`ConnectionInfo.url`）。文件型的 URL 可能是
    /// `sqlite://D:\x.db` 这种带 scheme 的写法，而 DuckDB 要的是裸路径——这里剥掉 scheme
    /// （`sqlite://` / `duckdb://` / `file://`）并去掉 Windows 路径前误加的前导 `/`。
    pub fn new(conn_id: &str, db_type: &str, url: &str) -> Result<Self, String> {
        let kind = AccelKind::from_db_type(db_type)?;
        let connection_string = match kind {
            AccelKind::Sqlite | AccelKind::DuckDb => normalize_file_path(url)?,
            _ => {
                let trimmed = url.trim();
                if trimmed.is_empty() {
                    return Err("这个连接没有可用的连接串".to_string());
                }
                trimmed.to_string()
            }
        };
        Ok(Self {
            conn_id: conn_id.to_string(),
            kind,
            connection_string,
        })
    }

    /// 挂载语句（只读；别名固定 [`SOURCE_ALIAS`]）
    fn attach_sql(&self) -> String {
        let literal = quote_literal(&self.connection_string);
        match self.kind.attach_type() {
            Some(kind) => format!("ATTACH {literal} AS {SOURCE_ALIAS} (TYPE {kind}, READ_ONLY)"),
            None => format!("ATTACH {literal} AS {SOURCE_ALIAS} (READ_ONLY)"),
        }
    }
}

/// 文件型 URL → 裸路径（剥 scheme、修 `sqlite:///C:/x.db` 的多余前导斜杠）
fn normalize_file_path(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("这个连接没有可用的文件路径".to_string());
    }
    let rest = ["sqlite://", "duckdb://", "file://"]
        .iter()
        .find_map(|scheme| trimmed.strip_prefix(scheme))
        .unwrap_or(trimmed);
    // `sqlite:///C:/x.db` 剥掉 scheme 后是 `/C:/x.db`：Windows 盘符前的那个斜杠要去掉
    let bytes = rest.as_bytes();
    let windows_drive = bytes.len() > 2
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && (bytes[2] == b':' || bytes[2] == b'|');
    let cleaned = if windows_drive { &rest[1..] } else { rest };
    if cleaned.is_empty() {
        return Err("这个连接没有可用的文件路径".to_string());
    }
    Ok(cleaned.to_string())
}

/// SQL 字符串字面量（单引号翻倍）
fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 一条加速会话（一个源一条专用 DuckDB 连接）
pub struct AccelSession {
    conn: Mutex<Connection>,
    kind: AccelKind,
    /// 挂载语句（`refresh` 用同一份口径重建）
    source_attach: String,
    /// 中断句柄（**建会话时就取出来**：真正跑查询时连接锁被占着，那时再取就晚了）
    interrupt: Arc<duckdb::InterruptHandle>,
    /// 现在有没有语句在跑（中断的“有没有可中断的”靠它，不凭猜）
    running: std::sync::atomic::AtomicBool,
    /// 建立时刻（诊断用：会话活了多久）
    attached_at: std::time::SystemTime,
}

/// “正在跑”的守卫：无论正常返回还是提前 `?` 返回，都把标记清掉
struct RunningGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for RunningGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl AccelSession {
    /// 在**这条会话**上执行一句（读语句出 Arrow 批，写语句出影响行数）
    ///
    /// 读写判定与源库驱动**同一套判据**（`driver::utils::returns_rows`）：两个通道对
    /// “这句是不是查询”的回答必须一致，否则同一句在两边会得到不同形状的结果。
    pub fn run(&self, sql: &str) -> Result<QueryResult, CoreError> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let _guard = RunningGuard(&self.running);
        let conn = self.lock()?;
        if !crate::driver::utils::returns_rows(sql) {
            let affected = conn.execute(sql, []).map_err(|error| query_error(sql, &error))?;
            return Ok(crate::driver::utils::affected_rows_result(affected as u64));
        }

        let mut stmt = conn.prepare(sql).map_err(|error| query_error(sql, &error))?;
        let mut rows = stmt.query([]).map_err(|error| query_error(sql, &error))?;
        let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
        while let Some(row) = rows.next().map_err(|error| query_error(sql, &error))? {
            let mut values: Vec<duckdb::types::Value> = Vec::new();
            for index in 0.. {
                match row.get::<usize, duckdb::types::Value>(index) {
                    Ok(value) => values.push(value),
                    Err(_) => break,
                }
            }
            data.push(values);
        }
        // duckdb-rs：列元数据要等语句真的跑过之后才可读
        let columns: Vec<String> = (0..stmt.column_count())
            .map(|index| stmt.column_name(index).map_or("unknown", |name| name).to_string())
            .collect();

        if data.is_empty() {
            return Ok(QueryResult {
                columns,
                is_read_only: Some(true),
                ..QueryResult::empty()
            });
        }
        let batch = crate::duckdb::row_to_arrow::duckdb_rows_to_arrow(&columns, &data)?;
        Ok(QueryResult {
            columns,
            batches: vec![batch],
            is_read_only: Some(true),
            ..QueryResult::empty()
        })
    }

    /// 重新挂载：源库侧**新建的表**要这样才看得见（数据本来就是实时的）
    ///
    /// 必须先 `USE memory`：默认 catalog 是附加源时 DuckDB 拒绝 DETACH 它。
    pub fn refresh(&self) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute_batch(&format!("USE memory; DETACH {SOURCE_ALIAS}"))
            .map_err(|error| attach_error("DETACH", &error))?;
        let attach = self.source_attach.clone();
        run_attach_steps(&conn, &attach)
    }

    /// 中断这条会话上正在跑的语句
    ///
    /// `Ok(false)` = 现在没有语句在跑（与源库通道同一语义：没可中断的不算错误，但要如实说）。
    /// DuckDB 的中断句柄在建会话时就取好了（跑查询时连接锁被占着，那时再取就晚了）。
    pub fn interrupt(&self) -> Result<bool, String> {
        if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(false);
        }
        self.interrupt.interrupt();
        Ok(true)
    }

    /// 挂载多久了（诊断）
    pub fn attached_for(&self) -> std::time::Duration {
        self.attached_at
            .elapsed()
            .unwrap_or(std::time::Duration::ZERO)
    }

    /// 源类型（诊断 / 测试）
    pub fn kind(&self) -> AccelKind {
        self.kind
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoreError> {
        self.conn.lock().map_err(|error| {
            CoreError::common(CommonError::General(format!(
                "加速会话连接锁被污染：{error}"
            )))
        })
    }
}

/// 进程内的加速会话表（按源连接 id）
static SESSIONS: Lazy<Mutex<HashMap<String, Arc<AccelSession>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 扩展状态（按种类）：试过、失败了才拦；没试过不拦
static EXTENSION_FAILURES: Lazy<Mutex<HashMap<AccelKind, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 扩展现在能不能用（**渲染路径可调**：纯内存读，不做 I/O）
pub fn extension_state(kind: AccelKind) -> Result<(), String> {
    match EXTENSION_FAILURES.lock() {
        Ok(failures) => match failures.get(&kind) {
            Some(reason) => Err(reason.clone()),
            None => Ok(()),
        },
        // 锁被污染：宁可不拦（执行时会再试一次并给出真实原因）
        Err(_) => Ok(()),
    }
}

/// 取（必要时建立）某个源的加速会话
///
/// **会做 I/O**：首次要 `INSTALL` 扩展（联网一次）、`ATTACH` 源库。所以它属于**事件路径**
/// （工作线程上的执行），不属于渲染路径；界面可用性看 [`extension_state`]。
pub fn ensure_session(source: &AccelSource) -> Result<Arc<AccelSession>, String> {
    if let Some(session) = session_for(&source.conn_id) {
        return Ok(session);
    }

    let conn = Connection::open_in_memory().map_err(|error| {
        format!("开本地分析连接失败：{error}")
    })?;
    let dir = paths::extensions_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return Err(format!("建扩展目录失败（{}）：{error}", dir.display()));
    }
    conn.execute_batch(&format!(
        "SET extension_directory = {}",
        quote_literal(&dir.to_string_lossy())
    ))
    .map_err(|error| format!("设置 DuckDB 扩展目录失败：{error}"))?;

    if let Some(extension) = source.kind.extension()
        && let Err(reason) = install_and_load(&conn, source.kind, extension)
    {
        return Err(reason);
    }

    let attach = source.attach_sql();
    run_attach_steps(&conn, &attach).map_err(|error| error.to_string())?;
    // 扩展这一关过了：清掉旧的失败记录（否则菜单会一直说“不可用”）
    if let Ok(mut failures) = EXTENSION_FAILURES.lock() {
        failures.remove(&source.kind);
    }

    let session = Arc::new(AccelSession {
        interrupt: conn.interrupt_handle(),
        conn: Mutex::new(conn),
        kind: source.kind,
        source_attach: attach,
        running: std::sync::atomic::AtomicBool::new(false),
        attached_at: std::time::SystemTime::now(),
    });
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(source.conn_id.clone(), session.clone());
    }
    tracing::info!(
        source = %source.conn_id,
        kind = source.kind.label(),
        "本地加速会话已建立（源库以只读方式挂载）"
    );
    Ok(session)
}

/// 首次安装 + 加载扩展；失败时**记下原因**给门控用
fn install_and_load(conn: &Connection, kind: AccelKind, extension: &str) -> Result<(), String> {
    let reason = match conn.execute_batch(&format!("INSTALL {extension}; LOAD {extension}")) {
        Ok(()) => return Ok(()),
        Err(error) => format!(
            "本地加速需要 DuckDB 扩展 {extension}，安装失败：{error}（首次需要能访问 DuckDB 扩展源；离线时请先把扩展放进 {}）",
            paths::extensions_dir().display()
        ),
    };
    if let Ok(mut failures) = EXTENSION_FAILURES.lock() {
        failures.insert(kind, reason.clone());
    }
    tracing::warn!(kind = kind.label(), "加速扩展不可用：{reason}");
    Err(reason)
}

/// 挂载两步：`ATTACH` 再 `USE`（默认 catalog 指到源库，用户才能不写限定名）
fn run_attach_steps(conn: &Connection, attach_sql: &str) -> Result<(), CoreError> {
    conn.execute_batch(attach_sql)
        .map_err(|error| attach_error("ATTACH", &error))?;
    conn.execute_batch(&format!("USE {SOURCE_ALIAS}"))
        .map_err(|error| attach_error("USE", &error))?;
    Ok(())
}

/// 已建立的会话（不做 I/O）
pub fn session_for(conn_id: &str) -> Option<Arc<AccelSession>> {
    SESSIONS.lock().ok()?.get(conn_id).cloned()
}

/// 放掉某个源的会话（测试 / 连接被删时用）
pub fn drop_session(conn_id: &str) -> bool {
    SESSIONS
        .lock()
        .map(|mut sessions| sessions.remove(conn_id).is_some())
        .unwrap_or(false)
}

/// 放掉全部会话（测试用：进程内的加速状态要能清干净）
pub fn drop_all() {
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.clear();
    }
    if let Ok(mut failures) = EXTENSION_FAILURES.lock() {
        failures.clear();
    }
}

/// 中断某个源上的语句（没有会话 = `None`，「谁在跑就中断谁」由调用方决定）
pub fn cancel(conn_id: &str) -> Option<Result<bool, String>> {
    session_for(conn_id).map(|session| session.interrupt())
}

fn query_error(sql: &str, error: &duckdb::Error) -> CoreError {
    CoreError::database(shared::error::DatabaseError::query(sql, error.to_string()))
}

fn attach_error(step: &str, error: &duckdb::Error) -> CoreError {
    CoreError::common(CommonError::General(format!("{step} 源库失败：{error}")))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        AccelKind, AccelSource, SOURCE_ALIAS, drop_all, drop_session, ensure_session,
        extension_state, normalize_file_path,
    };
    use duckdb::Connection;
    use std::sync::Arc;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_accel_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    /// 造一个有数据的 DuckDB 文件当“源”
    fn source_file(dir: &std::path::Path, rows: i64) -> std::path::PathBuf {
        let path = dir.join("source.duckdb");
        let conn = Connection::open(&path).expect("建源文件");
        conn.execute_batch(&format!(
            "CREATE TABLE orders AS SELECT i AS id FROM range({rows}) AS r(i)"
        ))
        .expect("建表");
        path
    }

    #[test]
    fn db_types_map_to_kinds_and_unknown_ones_say_so() {
        assert_eq!(AccelKind::from_db_type("mysql_native").unwrap(), AccelKind::MySql);
        assert_eq!(AccelKind::from_db_type("MySQL").unwrap(), AccelKind::MySql);
        assert_eq!(
            AccelKind::from_db_type("postgres_native").unwrap(),
            AccelKind::PostgreSql
        );
        assert_eq!(AccelKind::from_db_type("sqlite").unwrap(), AccelKind::Sqlite);
        assert_eq!(AccelKind::from_db_type("duckdb").unwrap(), AccelKind::DuckDb);
        // 认不出就不猜（猜错会把语句发到别的库上）
        let error = AccelKind::from_db_type("clickhouse").expect_err("应当拒绝");
        assert!(error.contains("clickhouse"), "{error}");
    }

    #[test]
    fn file_urls_become_bare_paths() {
        assert_eq!(normalize_file_path("D:\\data\\a.db").unwrap(), "D:\\data\\a.db");
        assert_eq!(normalize_file_path("sqlite:///C:/x.db").unwrap(), "C:/x.db");
        assert_eq!(normalize_file_path("duckdb://D:/y.duckdb").unwrap(), "D:/y.duckdb");
        assert!(normalize_file_path("   ").is_err());
    }

    #[test]
    fn attach_sql_is_read_only_and_typed() {
        let mysql = AccelSource::new("C_1", "mysql_native", "mysql://u:p@h:3306/db").unwrap();
        assert_eq!(
            mysql.attach_sql(),
            format!("ATTACH 'mysql://u:p@h:3306/db' AS {SOURCE_ALIAS} (TYPE mysql, READ_ONLY)")
        );
        let sqlite = AccelSource::new("C_2", "sqlite", "sqlite://D:/x.db").unwrap();
        assert_eq!(sqlite.connection_string, "D:/x.db");
        assert_eq!(
            sqlite.attach_sql(),
            format!("ATTACH 'D:/x.db' AS {SOURCE_ALIAS} (TYPE sqlite, READ_ONLY)")
        );
        // 单引号要翻倍（路径里真的可能有）
        let quoted = AccelSource {
            conn_id: "C_3".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: "D:/o'brien/a.duckdb".to_string(),
        };
        assert_eq!(
            quoted.attach_sql(),
            format!("ATTACH 'D:/o''brien/a.duckdb' AS {SOURCE_ALIAS} (READ_ONLY)")
        );
    }

    /// 会话建立 → 不写限定名就能查到源表（`USE` 生效的真实证明）
    #[test]
    fn a_session_resolves_source_tables_without_qualification() {
        drop_all();
        let dir = temp_dir("session");
        let path = source_file(&dir, 3);
        let source = AccelSource {
            conn_id: "C_session".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };

        let session = ensure_session(&source).expect("建会话");
        let result = session.run("SELECT count(*) AS n FROM orders").expect("查源表");
        let rows = result.to_rows();
        assert_eq!(rows.len(), 1, "源文件里的表要能直接查到");
        assert_eq!(rows[0][0].to_string(), "3");
        assert_eq!(session.kind(), AccelKind::DuckDb);

        // 同一源第二次取 = 复用会话（不会撞 “Unique file handle conflict”）
        let again = ensure_session(&source).expect("复用会话");
        assert!(Arc::ptr_eq(&session, &again), "同一 conn_id 应当复用同一条会话");
        assert!(drop_session("C_session"), "会话被放掉");
        assert!(!drop_session("C_session"), "再放一次就没有了");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 只读挂载：写源对象被 DuckDB 拒；本地临时对象允许
    #[test]
    fn the_source_is_read_only_but_local_temp_objects_are_allowed() {
        drop_all();
        let dir = temp_dir("readonly");
        let path = source_file(&dir, 1);
        let source = AccelSource {
            conn_id: "C_ro".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };
        let session = ensure_session(&source).expect("建会话");

        let write = session
            .run("INSERT INTO orders VALUES (99)")
            .expect_err("写源表必须被拒");
        assert!(
            write.to_string().contains("read-only"),
            "要说明是只读挂载：{write}"
        );
        // 本地临时对象 = 会话变量，允许
        session
            .run("CREATE TEMP TABLE vars AS SELECT 1 AS x")
            .expect("临时表应当允许");
        let value = session.run("SELECT x FROM vars").expect("查临时表");
        assert_eq!(value.to_rows()[0][0].to_string(), "1");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 重新挂载：`USE memory` → `DETACH` → `ATTACH` → `USE` 能跑通，且挂完还好使
    ///
    /// （「源侧新建的表要重挂才看得见」那条语义由真机探针验证：DuckDB 文件被挂住时
    /// 本进程另开写连接会被文件锁拦住，单元测试造不出那个现场。）
    #[test]
    fn refresh_reattaches_the_source_and_keeps_working() {
        drop_all();
        let dir = temp_dir("refresh");
        let path = source_file(&dir, 2);
        let source = AccelSource {
            conn_id: "C_refresh".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };
        let session = ensure_session(&source).expect("建会话");
        assert_eq!(
            session.run("SELECT count(*) FROM orders").unwrap().to_rows()[0][0].to_string(),
            "2"
        );

        session.refresh().expect("重新挂载");
        assert_eq!(
            session
                .run("SELECT count(*) FROM orders")
                .expect("重挂后仍能查到源表")
                .to_rows()[0][0]
                .to_string(),
            "2"
        );
        assert!(
            session.run("SELECT * FROM rds_src.orders").is_ok(),
            "限定名（别名）也要能走"
        );
        // 重挂后本地临时表还在吗？在——DETACH/ATTACH 不重建连接（会话变量不受影响）
        session
            .run("CREATE TEMP TABLE keep AS SELECT 7 AS x")
            .expect("建临时表");
        session.refresh().expect("再挂一次");
        assert_eq!(
            session.run("SELECT x FROM keep").unwrap().to_rows()[0][0].to_string(),
            "7",
            "重挂不该把会话里的临时表弄丢"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 没试过扩展 = 不拦（`Unknown`）；失败才记下来拦（首用联网失败的现场）
    #[test]
    fn extension_state_only_blocks_after_a_real_failure() {
        drop_all();
        assert!(extension_state(AccelKind::MySql).is_ok(), "没试过就不该拦");
        assert!(extension_state(AccelKind::DuckDb).is_ok());

        // 造一次真实失败：文件型的“源”指向不存在的路径 → 会话建不起来
        let source = AccelSource {
            conn_id: "C_missing".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: "Z:/definitely/missing.duckdb".to_string(),
        };
        assert!(ensure_session(&source).is_err(), "挂不上的源要报错");
        // 失败不写进“扩展失败”表（那是另一个维度：挂载失败由执行路径每次如实报）
        assert!(extension_state(AccelKind::DuckDb).is_ok());
    }
}

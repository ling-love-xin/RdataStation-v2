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
///
/// 前四个是 **L1（官方 scanner）**：`ATTACH` 一个连接串、带 `READ_ONLY`；
/// [`Self::Oracle`] 是 **L2（社区 scanner）**：凭据只能走**会话级 Secret**、
/// **不支持 `READ_ONLY`**、限定名是两段（真机台账：`docs/architecture/federation/federation-architecture.md` §2.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccelKind {
    MySql,
    PostgreSql,
    Sqlite,
    DuckDb,
    /// Oracle（社区扩展 `oracle_scanner`；**只允许做联邦源**，不做本地加速）
    Oracle,
}

impl AccelKind {
    /// 连接的 `db_type` → 加速源种类
    ///
    /// `db_type` 来自驱动标识（`mysql_native` / `postgres_native` / `sqlite` / `duckdb` / `oracle`），
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
        } else if normalized.starts_with("oracle") || normalized.starts_with("ora") {
            Ok(Self::Oracle)
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
            Self::Oracle => "Oracle",
        }
    }

    /// 需要的 DuckDB 扩展（DuckDB 文件源不需要，是内核自带能力）
    ///
    /// `pub(crate)`：联邦会话要拿同一份清单去装上（别在两处各写一张表）
    pub(crate) fn extension(self) -> Option<&'static str> {
        match self {
            Self::MySql => Some("mysql"),
            Self::PostgreSql => Some("postgres"),
            Self::Sqlite => Some("sqlite"),
            Self::DuckDb => None,
            Self::Oracle => Some("oracle_scanner"),
        }
    }

    /// 扩展从哪个仓库装（`None` = 官方仓库；L2 是社区扩展，必须 `INSTALL … FROM community`）
    pub(crate) fn extension_repository(self) -> Option<&'static str> {
        match self {
            Self::Oracle => Some("community"),
            _ => None,
        }
    }

    /// `ATTACH … (TYPE …)` 里的类型名（DuckDB 文件源不写 TYPE）
    pub(crate) fn attach_type(self) -> Option<&'static str> {
        match self {
            Self::MySql => Some("mysql"),
            Self::PostgreSql => Some("postgres"),
            Self::Sqlite => Some("sqlite"),
            Self::DuckDb => None,
            Self::Oracle => Some("oracle_scanner"),
        }
    }

    /// **扫描器认的** URL scheme——**不是**应用的驱动 id
    ///
    /// 应用里 `db_type` 可能是原生驱动的 id（`mysql_native` / `postgres_native`），而 DuckDB
    /// 的 scanner 只认 `mysql://` / `postgres://`：直接把驱动 id 当 scheme 递过去，会得到
    /// `Invalid dsn "mysql_native://…" - expected key=value pairs separated by spaces`
    /// （真机踩到）。所以连接串在交给 DuckDB 前要把 scheme 换成这一份。
    ///
    /// Oracle 这一项**不是给 DuckDB 的 URL**（它靠 Secret 挂载）：只是解析凭据时认前缀用。
    pub(crate) fn scheme(self) -> &'static str {
        match self {
            Self::MySql => "mysql",
            Self::PostgreSql => "postgres",
            Self::Sqlite => "sqlite",
            Self::DuckDb => "duckdb",
            Self::Oracle => "oracle",
        }
    }

    /// 挂载时能不能带 `READ_ONLY`（写成“反过来说”是因为要默认安全：**默认支持**，例外显式声明）
    ///
    /// Oracle 的社区扩展当前不支持（原话：*Oracle ATTACH does not accept option 'read_only' yet*），
    /// 所以那道引擎侧的写保护对它不成立——得靠会话层拒绝 + 编辑器闸门 + 只读账号（台账 §2.1）。
    pub(crate) fn supports_read_only_attach(self) -> bool {
        !matches!(self, Self::Oracle)
    }

    /// 挂载时是不是必须先把凭据建成本连接的 Secret（L2 的唯一入口）
    ///
    /// 公开给宿主：结果区那行小字要据此说清“这类源写在两段名上（`别名.表`）”。
    pub fn needs_secret(self) -> bool {
        matches!(self, Self::Oracle)
    }

    /// 这个种类能不能做**本地加速**（不能则给出原因：菜单行尾要显示的就是这句）
    ///
    /// 本地加速是“一条源一条连接、连接串直接进 `ATTACH`”：L2（Oracle）的凭据得先建成会话级
    /// Secret（那是联邦会话干的事），所以它只能做联邦源。判据与理由都放在引擎侧一处，
    /// 免得门控与组装各写一份、日后又说不一样的话。
    pub fn local_accel_support(self) -> Result<(), String> {
        if self.needs_secret() {
            return Err(format!(
                "{} 只能做联邦源（本地加速不支持）：它的凭据要走会话级 Secret",
                self.label()
            ));
        }
        Ok(())
    }
}

/// 一个加速源（**宿主组装**：引擎不读连接库，也不解密口令）
#[derive(Clone, PartialEq, Eq)]
pub struct AccelSource {
    /// 源连接 id（会话按它缓存；结果集与历史也用它）
    pub conn_id: String,
    pub kind: AccelKind,
    /// 网络型 = 带凭据的 URL（`mysql://…` / `postgres://…`）；文件型 = **裸路径**
    pub connection_string: String,
}

impl std::fmt::Debug for AccelSource {
    /// 手写 `Debug`：**连接串里有口令**，`{:?}` 不能把它原样印出来（日志 / 报错 / 测试失败信息）
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccelSource")
            .field("conn_id", &self.conn_id)
            .field("kind", &self.kind)
            .field(
                "connection_string",
                &connection::url::mask_password_in_url(&self.connection_string),
            )
            .finish()
    }
}

impl AccelSource {
    /// 从宿主的连接信息组装
    ///
    /// `url` 是这条连接的**运行时连接串**（含凭据：`mysql://user:pass@host:port/db`）。
    /// 为什么必须带凭据而不是连接管理器里的脱敏 URL（`user:******@`）：
    /// **DuckDB 1.5.5 的 mysql / postgres 扫描器不认 Secret**（实测见
    /// `tests/federation_credentials_probe.rs`：会话级 / 持久化、带 scope / 不带、`ATTACH ''`
    /// 各种写法都拿不到凭据），所以凭据只能随连接串进 `ATTACH`；
    /// 代价是**任何离开引擎的文本都要脱敏**（[`scrub_credentials`]）。
    ///
    /// 文件型的 URL 可能是 `sqlite://D:\x.db` 这种带 scheme 的写法，而 DuckDB 要的是裸路径——
    /// 这里剥掉 scheme（`sqlite://` / `duckdb://` / `file://`）并去掉 Windows 路径前误加的前导 `/`。
    pub fn new(conn_id: &str, db_type: &str, url: &str) -> Result<Self, String> {
        let kind = AccelKind::from_db_type(db_type)?;
        // 本地加速只支持 L1：Oracle 这类得先建会话级 Secret（联邦会话干的事），
        // 与其在这里再写一份凭据逻辑，不如直接说不支持（与“不猜”同一口径）
        kind.local_accel_support()?;
        let connection_string = match kind {
            AccelKind::Sqlite | AccelKind::DuckDb => normalize_file_path(url)?,
            _ => normalize_scheme(kind, url),
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
///
/// `pub(crate)`：联邦源组装要剥同一套 scheme（否则文件源挂不上）
pub(crate) fn normalize_file_path(url: &str) -> Result<String, String> {
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

/// 网络型连接串：把 scheme 换成**扫描器认的**那个（`mysql_native://…` → `mysql://…`）
///
/// 其余部分（凭据 / 主机 / 库名 / 查询串）原样保留：应用里 `db_type` 是驱动 id，
/// 而 DuckDB 的 scanner 只认 `mysql` / `postgres` 这类 scheme（见 [`AccelKind::scheme`]）。
/// 空串如实报错；认不出 scheme 的写法（没有 `://`）原样返回，让 DuckDB 自己报。
pub(crate) fn normalize_scheme(kind: AccelKind, url: &str) -> String {
    let trimmed = url.trim();
    match trimmed.split_once("://") {
        Some((_, rest)) => format!("{}://{rest}", kind.scheme()),
        None => trimmed.to_string(),
    }
}

/// SQL 字符串字面量（单引号翻倍）
///
/// `pub(crate)`：联邦源的 `ATTACH` 也在这里拼
pub(crate) fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 把文本里可能出现的连接凭据抹掉（**留给任何要离开引擎的文本**）
///
/// 为什么需要它：凭据随 `ATTACH` 串进 DuckDB，而 DuckDB 报错会把参数**原样回显**
/// （真机原话：`Failed to connect to MySQL database with parameters "mysql://root:***@…"`），
/// 这些文本会进结果区、失败卡片与历史（还会落盘）。两条规则：
///
/// 1. 整串替换成脱敏版（`user:******@host/…`）——报错回显的就是整串；
/// 2. `user:password` 片段替换（同一串被拼接 / 截断时仍兜得住）。
///
/// 没有凭据的连接串（文件型）原样返回。
pub(crate) fn scrub_credentials(connection_string: &str, text: &str) -> String {
    if connection_string.trim().is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    if out.contains(connection_string) {
        out = out.replace(
            connection_string,
            &connection::url::mask_password_in_url(connection_string),
        );
    }
    let (user, password) = connection::url::extract_credentials_from_url(connection_string);
    if let (Some(user), Some(password)) = (user, password)
        && !user.is_empty()
        && !password.is_empty()
    {
        out = out.replace(&format!("{user}:{password}"), &format!("{user}:******"));
    }
    out
}

/// 一条加速会话（一个源一条专用 DuckDB 连接）
pub struct AccelSession {
    conn: Mutex<Connection>,
    kind: AccelKind,
    /// 挂载语句（`refresh` 用同一份口径重建；**里面有凭据**，只在本进程里用）
    source_attach: String,
    /// 源连接串（脱敏用：错误文本要把里面的口令抹掉才能离开引擎）
    connection_string: String,
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

/// 在一条 DuckDB 连接上跑一句（读语句出 Arrow 批，写语句出影响行数）
///
/// **加速档与联邦档共用这一份**：两个通道对“这句是不是查询”的回答必须一致
/// （判据与源库驱动同一套：`driver::utils::returns_rows`），否则同一句在两个档上
/// 会得到不同形状的结果。
pub(crate) fn run_sql_on(
    conn: &Connection,
    running: &std::sync::atomic::AtomicBool,
    sql: &str,
) -> Result<QueryResult, CoreError> {
    running
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let _guard = RunningGuard(running);
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

impl AccelSession {
    /// 在**这条会话**上执行一句（读语句出 Arrow 批，写语句出影响行数）
    ///
    /// 错误文本先脱敏再往外走：驱动报错里可能带着建连参数（见 [`scrub_credentials`]）。
    pub fn run(&self, sql: &str) -> Result<QueryResult, CoreError> {
        let conn = self.lock()?;
        run_sql_on(&conn, &self.running, sql).map_err(|error| scrub_error(&self.connection_string, error))
    }

    /// 重新挂载：源库侧**新建的表**要这样才看得见（数据本来就是实时的）
    ///
    /// 必须先 `USE memory`：默认 catalog 是附加源时 DuckDB 拒绝 DETACH 它。
    pub fn refresh(&self) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute_batch(&format!("USE memory; DETACH {SOURCE_ALIAS}"))
            .map_err(|error| scrub_error(&self.connection_string, attach_error("DETACH", &error)))?;
        let attach = self.source_attach.clone();
        run_attach_steps(&conn, &attach)
            .map_err(|error| scrub_error(&self.connection_string, error))
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

/// 建立会话时的全局互斥（**只护“装扩展 + ATTACH”这一段**，不护查询）
///
/// 为什么需要它：`INSTALL` 与 `LOAD` 都是进程级资源（扩展目录里的同一个文件），
/// 两个源同时首次建立会话时会撞上“正在写同一个文件”。生产上执行器是单工作线程
/// （基本串行），但“重新挂载”走旁路线程、以后也可能有第二个执行者——这里兜住。
static MOUNT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// 联邦会话也用它把"装扩展 + 一批 ATTACH"串起来（同一个进程级资源）
pub(crate) fn mount_lock() -> std::sync::MutexGuard<'static, ()> {
    MOUNT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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
    // 挂载这一步串行（见 `mount_lock`）；查询不受它影响
    let _mount = mount_lock();
    // 拿锁期间可能已被别人建好了
    if let Some(session) = session_for(&source.conn_id) {
        return Ok(session);
    }

    let conn = Connection::open_in_memory().map_err(|error| {
        format!("开本地分析连接失败：{error}")
    })?;
    // 统一配置（扩展目录 / 内存闸 / 溢写口 / 不静默联网）——别再在这里自己拼 SET：
    // 会话是个长期存活的连接，绕开 `configure_connection` 就会漏掉其中某几项（审计抓到过）
    super::manager::DuckDBManager::configure_connection(&conn)
        .map_err(|error| format!("配置本地分析连接失败：{error}"))?;

    if let Some(extension) = source.kind.extension()
        && let Err(reason) = install_and_load(&conn, source.kind, extension)
    {
        return Err(reason);
    }

    let attach = source.attach_sql();
    run_attach_steps(&conn, &attach)
        .map_err(|error| scrub_credentials(&source.connection_string, &error.to_string()))?;
    // 扩展这一关过了：清掉旧的失败记录（否则菜单会一直说“不可用”）
    if let Ok(mut failures) = EXTENSION_FAILURES.lock() {
        failures.remove(&source.kind);
    }

    let session = Arc::new(AccelSession {
        interrupt: conn.interrupt_handle(),
        conn: Mutex::new(conn),
        kind: source.kind,
        source_attach: attach,
        connection_string: source.connection_string.clone(),
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

/// 装 + 加载一个扩展（失败原因里带上“离线怎么办”）
fn install_and_load(conn: &Connection, kind: AccelKind, extension: &str) -> Result<(), String> {
    let reason = match conn.execute_batch(&install_sql(kind, extension)) {
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

/// 装扩展的 SQL（**L2 要从社区仓库装**：`INSTALL oracle_scanner FROM community`）
///
/// `pub(crate)`：联邦会话装同一批扩展（别在两处各写一份）
pub(crate) fn install_sql(kind: AccelKind, extension: &str) -> String {
    match kind.extension_repository() {
        Some(repository) => format!("INSTALL {extension} FROM {repository}; LOAD {extension}"),
        None => format!("INSTALL {extension}; LOAD {extension}"),
    }
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

/// 错误文本脱敏（把 `connection_string` 里的口令抹掉）
///
/// `CoreError` 是各档共用的错误类型，这里按种类还原成同一种形状（只改文本）。
fn scrub_error(connection_string: &str, error: CoreError) -> CoreError {
    let text = error.to_string();
    let scrubbed = scrub_credentials(connection_string, &text);
    if scrubbed == text {
        return error;
    }
    CoreError::common(CommonError::General(scrubbed))
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
        source_file_named(dir, "source.duckdb", rows)
    }

    /// 同上，文件名可指定（并发用例要两个不同的源）
    fn source_file_named(dir: &std::path::Path, name: &str, rows: i64) -> std::path::PathBuf {
        let path = dir.join(name);
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

    /// 两个不同的源同时建立会话：装扩展 / 挂载串行，互不影响
    #[test]
    fn concurrent_mounts_do_not_race_on_the_extension_directory() {
        drop_all();
        let dir = temp_dir("concurrent");
        let first = AccelSource {
            conn_id: "C_conc_1".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: source_file_named(&dir, "first.duckdb", 1)
                .to_string_lossy()
                .to_string(),
        };
        let second = AccelSource {
            conn_id: "C_conc_2".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: source_file_named(&dir, "second.duckdb", 2)
                .to_string_lossy()
                .to_string(),
        };

        let handles: Vec<_> = [first, second]
            .into_iter()
            .map(|source| {
                std::thread::spawn(move || {
                    let session = super::ensure_session(&source).expect("建会话");
                    session.run("SELECT count(*) FROM orders").expect("查表")
                })
            })
            .collect();
        let counts: Vec<String> = handles
            .into_iter()
            .map(|handle| handle.join().expect("线程不该 panic").to_rows()[0][0].to_string())
            .collect();
        assert_eq!(counts.len(), 2, "两条会话都建起来了");
        drop_all();
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

    /// 凭据脱敏：DuckDB 的报错会把 `ATTACH` 参数原样回显，口令不能跟着出去
    #[test]
    fn error_text_never_carries_the_password() {
        let url = "mysql://root:s3cr3t@192.168.3.138:3306/shop";
        // 真机现场的形状：整串被回显
        let echoed = format!(
            "IO Error: Failed to connect to MySQL database with parameters \"{url}\": Access denied"
        );
        let scrubbed = super::scrub_credentials(url, &echoed);
        assert!(!scrubbed.contains("s3cr3t"), "口令不得出现：{scrubbed}");
        assert!(scrubbed.contains("root:******@192.168.3.138"), "{scrubbed}");
        assert!(scrubbed.contains("Access denied"), "原因要留着：{scrubbed}");

        // 片段形式（串被拼接 / 截断）也兜得住
        let fragment = "connect failed for root:s3cr3t at host";
        assert!(!super::scrub_credentials(url, fragment).contains("s3cr3t"));

        // 没有凭据的文件型：原样返回
        let file = "D:/data/x.db";
        assert_eq!(
            super::scrub_credentials(file, "Catalog Error: table not found"),
            "Catalog Error: table not found"
        );
        // 空串不做事（也不 panic）
        assert_eq!(super::scrub_credentials("", "boom"), "boom");
    }

    /// 驱动 id ≠ 扫描器 scheme：交结 DuckDB 前要把 scheme 换掉（真机踩到）
    #[test]
    fn the_scheme_handed_to_duckdb_is_the_scanners_one() {
        // 原生驱动的 id 不能直接当 scheme（DuckDB 报 Invalid dsn）
        let source = AccelSource::new(
            "C_native",
            "mysql_native",
            "mysql_native://root:pw@h:3306/db",
        )
        .expect("组装源");
        assert_eq!(source.connection_string, "mysql://root:pw@h:3306/db");
        assert!(source.attach_sql().contains("TYPE mysql"), "{}", source.attach_sql());

        let pg = AccelSource::new(
            "C_pg",
            "postgres_native",
            "postgres_native://u:p@h:5432/db?sslmode=require",
        )
        .expect("组装源");
        assert_eq!(
            pg.connection_string,
            "postgres://u:p@h:5432/db?sslmode=require",
            "查询串要原样留着"
        );

        // 已经是扫描器 scheme 的：保持不变（幂等）
        let same = AccelSource::new("C_same", "mysql", "mysql://root:pw@h:3306/db").expect("组装");
        assert_eq!(same.connection_string, "mysql://root:pw@h:3306/db");

        // 没有 scheme 的写法：原样留着（让 DuckDB 自己报错，不猜）
        assert_eq!(super::normalize_scheme(AccelKind::MySql, "h:3306/db"), "h:3306/db");
    }
}

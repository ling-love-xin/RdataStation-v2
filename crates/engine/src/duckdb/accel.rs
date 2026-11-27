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

use shared::error::CoreError;
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

    /// 该源所需的扩展**能力**（统一入口 `extensions::ensure` 的入参）
    ///
    /// 与 [`Self::extension`] 的分工：那个给的是扩展**文件名**（`ATTACH … (TYPE …)` 与
    /// 门控要用），这个给的是「我要什么能力」（准入与登记要用）。两者现在一一对应，
    /// 但只有能力这一层能容纳「同一能力换实现」而不改调用点。
    pub(crate) fn ability(self) -> Option<super::extensions::ExtensionAbility> {
        use super::extensions::ExtensionAbility;
        match self {
            Self::MySql => Some(ExtensionAbility::FederationMysql),
            Self::PostgreSql => Some(ExtensionAbility::FederationPostgres),
            Self::Sqlite => Some(ExtensionAbility::FederationSqlite),
            Self::DuckDb => None,
            Self::Oracle => Some(ExtensionAbility::FederationOracle),
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

    /// 该驱动类型是否有对应的 DuckDB 扩展（联邦扫描可用）——**公开门控**（Secret 注册等跨 crate 消费）。
    ///
    /// 2026-10-03 用户规则：DuckDB Secret 注册门控依据 = **是否有对应扩展**；
    /// `mysql` / `postgres` / `sqlite` 有官方 scanner 扩展；`oracle` 走社区扩展（oracle_scanner）；
    /// **duckdb 文件源是内核自带能力、无扩展 → 不算**（文件源不需要 Secret）。
    /// 未知 / 无法联邦的类型 → false（不猜：猜错会把凭据注册到不支持的 Secret 类型上）。
    pub fn has_federable_extension(db_type: &str) -> bool {
        Self::from_db_type(db_type)
            .map(|kind| kind.extension().is_some())
            .unwrap_or(false)
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

    /// 限定名是**两段**（`别名.表`）还是**三段**（`别名.schema.表`）
    ///
    /// 真机台账（架构 §2.1）：Oracle 的 scanner 把表挂进 `main`，引用写两段；
    /// 其余源三段（DuckDB 对三段名永远合法）。数据对象面板的「复制限定名」按这个
    /// 分支——**判据只有这一处**，UI 不各自猜。
    pub fn two_segment_names(self) -> bool {
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
    ///
    /// 生产路径不再由这里拼：单实例后挂载统一走 `federation::session::mount_into`
    /// （按计划里的别名）。留着是给用例做口径对照——「加速与联邦的 ATTACH 形状必须一致」。
    #[cfg(test)]
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
    running.store(true, std::sync::atomic::Ordering::SeqCst);
    let _guard = RunningGuard(running);
    if !crate::driver::utils::returns_rows(sql) {
        let affected = conn
            .execute(sql, [])
            .map_err(|error| query_error(sql, &error))?;
        return Ok(crate::driver::utils::affected_rows_result(affected as u64));
    }

    let mut stmt = conn
        .prepare(sql)
        .map_err(|error| query_error(sql, &error))?;
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
        .map(|index| {
            stmt.column_name(index)
                .map_or("unknown", |name| name)
                .to_string()
        })
        .collect();

    if data.is_empty() {
        return Ok(QueryResult::with_batches(columns, vec![], Some(true)));
    }
    let batch = shared::duckdb_rows::duckdb_rows_to_arrow(&columns, &data)?;
    Ok(QueryResult::with_batches(columns, vec![batch], Some(true)))
}

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

/// 加速源 → 挂载用的 `FederatedSource`（别名由调用方给：加速档要按源唯一）
fn federated_source(source: &AccelSource, alias: &str) -> super::federation::registry::FederatedSource {
    super::federation::registry::FederatedSource {
        conn_id: source.conn_id.clone(),
        alias: alias.to_string(),
        kind: source.kind,
        connection_string: source.connection_string.clone(),
        secret: None,
    }
}

/// 把一个加速源挂进**分析实例**，返回（该源在实例上的会话，实际别名）。
///
/// 单实例之后本地加速不再各自建内存库：源是**实例级**的 `ATTACH`，会话只负责 `USE`。
/// 内核的加速档与编辑器里直跑的加速档都走这里——别名申请、幂等判重、`USE` 只有一份口径，
/// 否则「同一个源两处各挂一遍」就会重演"第二个源指到第一个"的串库问题。
///
/// `owner` 是**这条会话的身份**（2026-11-26 起按文档，不再按连接）：`USE` / `search_path` /
/// 中断都是连接级状态，两条文档若共用一条会话就会互相污染。同时用同一连接的两条文档现在
/// 各有一条会话，源仍是实例级 `ATTACH`（catalog 共享），只有会话状态分开。
pub fn ensure_on_instance(
    source: &AccelSource,
    owner: &str,
) -> Result<(Arc<super::instance::AnalysisSession>, String), String> {
    use super::federation::registry::MountState;

    let instance = super::instance::instance()?;
    let alias = instance.reserve_alias(SOURCE_ALIAS, &source.conn_id, source.kind)?;
    let session = instance.ensure_session(owner, "本地加速")?;
    let mounted = session.mount_sources(&[federated_source(source, &alias)])?;
    // 挂不上就是这条源不能用：当场说清，而不是等查询报"表不存在"
    if let Some(entry) = mounted.first() {
        if let MountState::Failed(reason) = &entry.state {
            return Err(reason.clone());
        }
    }
    // 未限定名落在这一条源上（与旧加速档的 `USE rds_src` 同一语义），
    // `use_catalog` 同时把 `memory.rds_tmp` 钉回 `search_path`——切换之后临时表还在。
    session.use_catalog(&alias)?;
    // 加速通道只有这一条源：把上一通道（联邦）留在这条会话里的挂载记录清掉，
    // 否则面板「会话源」会把两套通道的源一起列出来（实例上的 `ATTACH` 不动，
    // 那是源段的事实，卸不卸由用户的两态按钮决定）。
    session.retain_mounted(std::slice::from_ref(&alias));
    Ok((session, alias))
}

/// 重新挂载某个加速源（界面「重新挂载源库」）：同一条会话里 `DETACH` 再挂回去。
///
/// 为什么不是关会话重开：关会话会连 `TEMP` 一起清掉，而重挂只该刷新源侧清单。
/// 源库侧新建的表在 2.0.1 上多数能直接看见（动态解析），这条留给"清单确实要重建"的场合。
pub fn remount_on_instance(source: &AccelSource, owner: &str) -> Result<String, String> {
    let instance = super::instance::instance()?;
    let alias = instance.reserve_alias(SOURCE_ALIAS, &source.conn_id, source.kind)?;
    let session = instance.ensure_session(owner, "本地加速")?;
    // 先回 memory：默认 catalog 停在源上时 DuckDB 会拒绝 DETACH 它
    session.with_connection(|conn| {
        match conn.execute_batch(&format!("USE memory; DETACH {alias}")) {
            Ok(()) => Ok(()),
            Err(error) => {
                let text = error.to_string();
                // 本来就没挂（首次调用）不算错误，接着挂上就是
                if text.contains("does not exist") {
                    Ok(())
                } else {
                    Err(format!("卸下 {alias} 失败：{text}"))
                }
            }
        }
    })?;
    let mounted = session.mount_sources(&[federated_source(source, &alias)])?;
    if let Some(super::federation::registry::MountedSource { state: super::federation::registry::MountState::Failed(reason), .. }) = mounted.first() {
        return Err(reason.clone());
    }
    session.use_catalog(&alias)?;
    Ok(alias)
}

/// 确保该源需要的扩展已就绪（统一走 [`super::extensions::ensure`]）
///
/// 以前这里自己拼 `INSTALL … ; LOAD …` 且**每次建会话都跑一遍**（已装的只是被 DuckDB
/// 内部跳过，仍要一次 SQL 往返）。统一后顺序变成「先 LOAD，缺文件才 INSTALL」，顺带
/// 把「装了什么」写进登记表。
///
/// 失败仍记进 `EXTENSION_FAILURES`（渲染路径要能纯内存读出「不可用」）。
pub(crate) fn ensure_extension(conn: &Connection, kind: AccelKind) -> Result<(), String> {
    // DuckDB 文件源不需要扩展（内核自带能力）
    let Some(ability) = kind.ability() else {
        return Ok(());
    };
    match super::extensions::ensure(conn, ability) {
        Ok(loaded) => {
            // 登记是 best-effort：写不进去不影响已经能用的扩展
            super::extensions::record_best_effort("runtime", &loaded);
            if let Ok(mut failures) = EXTENSION_FAILURES.lock() {
                failures.remove(&kind);
            }
            Ok(())
        }
        Err(error) => {
            let reason = format!("本地加速需要{}，加载失败：{error}", ability.label());
            if let Ok(mut failures) = EXTENSION_FAILURES.lock() {
                failures.insert(kind, reason.clone());
            }
            tracing::warn!(kind = kind.label(), "加速扩展不可用：{reason}");
            Err(reason)
        }
    }
}

/// 挂载源里的一张表 / 视图（数据对象面板「源行展开」的叶子数据）
///
/// `qualified` 是**可直接粘进 SQL 的限定名**：按源种类给三段（`别名.schema.表`）或
/// 两段（`别名.表`，判据 [`AccelKind::two_segment_names`]）——命名规则只有这一处，
/// 界面不重复拼。标识符含特殊字符时加双引号（DuckDB 的转义规则）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedTable {
    pub schema: String,
    pub name: String,
    pub is_view: bool,
    /// 限定名（写 SQL 用；面板叶子的「复制」就是它）
    pub qualified: String,
}

/// 列出一条 DuckDB 连接里某个挂载别名下的表 / 视图（按 schema、名字排序）
///
/// 过滤口径与联邦的 `count_tables` 同一份（`duckdb_tables()` / `duckdb_views()` 按别名），
/// 加速档与联邦档共用这一实现——两处各写一遍迟早给出不一样的清单。
/// 表与视图同名时**保表**（DuckDB 里同 schema 同名本就不可能，这里是防外源元数据抖动）。
pub(crate) fn list_mounted_tables(
    conn: &Connection,
    alias: &str,
    kind: AccelKind,
) -> Result<Vec<MountedTable>, String> {
    let qualified_of = |schema: &str, name: &str| -> String {
        let alias_part = quote_ident(alias);
        if kind.two_segment_names() {
            // L2：表挂进 main，引用写两段（真机台账）；schema 名不进限定名
            format!("{alias_part}.{}", quote_ident(name))
        } else {
            format!(
                "{alias_part}.{}.{}",
                quote_ident(schema),
                quote_ident(name)
            )
        }
    };

    let mut out: Vec<MountedTable> = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT schema_name, table_name FROM duckdb_tables() \
             WHERE database_name = ? ORDER BY schema_name, table_name",
        )
        .map_err(|error| format!("列源 {alias} 的表失败：{error}"))?;
    let mut rows = stmt
        .query([alias])
        .map_err(|error| format!("列源 {alias} 的表失败：{error}"))?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let (schema, name) = (
            row.get::<_, String>(0).map_err(|error| error.to_string())?,
            row.get::<_, String>(1).map_err(|error| error.to_string())?,
        );
        let qualified = qualified_of(&schema, &name);
        out.push(MountedTable {
            schema,
            name,
            is_view: false,
            qualified,
        });
    }

    let mut stmt = conn
        .prepare(
            "SELECT schema_name, view_name FROM duckdb_views() \
             WHERE database_name = ? ORDER BY schema_name, view_name",
        )
        .map_err(|error| format!("列源 {alias} 的视图失败：{error}"))?;
    let mut rows = stmt
        .query([alias])
        .map_err(|error| format!("列源 {alias} 的视图失败：{error}"))?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let (schema, name) = (
            row.get::<_, String>(0).map_err(|error| error.to_string())?,
            row.get::<_, String>(1).map_err(|error| error.to_string())?,
        );
        // 同名去重：表优先（正常不会发生，防外源元数据把同名报两遍）
        if out
            .iter()
            .any(|entry| entry.schema == schema && entry.name == name)
        {
            continue;
        }
        let qualified = qualified_of(&schema, &name);
        out.push(MountedTable {
            schema,
            name,
            is_view: true,
            qualified,
        });
    }
    Ok(out)
}

/// 标识符转双引号形式（含 `"` 的加转义；普通名也加，DuckDB 一律认）
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}


fn query_error(sql: &str, error: &duckdb::Error) -> CoreError {
    CoreError::database(shared::error::DatabaseError::query(sql, error.to_string()))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        AccelKind, AccelSource, SOURCE_ALIAS, extension_state, normalize_file_path,
    };
    use crate::duckdb::federation::registry::MountState;
    use crate::duckdb::instance::{AnalysisInstance, AnalysisSession};
    use duckdb::Connection;
    use std::sync::Arc;

    /// 把一个源挂进**私有实例**（用例之间互不共享全局单例，别名也不会互相让位）
    fn mount(
        tag: &str,
        source: &AccelSource,
    ) -> (Arc<AnalysisInstance>, Arc<AnalysisSession>, String) {
        let instance = AnalysisInstance::open_tagged(tag, std::time::Duration::from_secs(30))
            .expect("开实例");
        let alias = instance
            .reserve_alias(SOURCE_ALIAS, &source.conn_id, source.kind)
            .expect("取别名");
        let session = instance
            .session_for_conn(&source.conn_id)
            .expect("取会话");
        let mounted = session
            .mount_sources(&[super::federated_source(source, &alias)])
            .expect("挂载");
        assert!(
            mounted
                .iter()
                .any(|entry| entry.alias() == alias && entry.is_ready()),
            "源要真挂上"
        );
        session.use_catalog(&alias).expect("USE");
        (instance, session, alias)
    }

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
        assert_eq!(
            AccelKind::from_db_type("mysql_native").unwrap(),
            AccelKind::MySql
        );
        assert_eq!(AccelKind::from_db_type("MySQL").unwrap(), AccelKind::MySql);
        assert_eq!(
            AccelKind::from_db_type("postgres_native").unwrap(),
            AccelKind::PostgreSql
        );
        assert_eq!(
            AccelKind::from_db_type("sqlite").unwrap(),
            AccelKind::Sqlite
        );
        assert_eq!(
            AccelKind::from_db_type("duckdb").unwrap(),
            AccelKind::DuckDb
        );
        // 认不出就不猜（猜错会把语句发到别的库上）
        let error = AccelKind::from_db_type("clickhouse").expect_err("应当拒绝");
        assert!(error.contains("clickhouse"), "{error}");
    }

    #[test]
    fn file_urls_become_bare_paths() {
        assert_eq!(
            normalize_file_path("D:\\data\\a.db").unwrap(),
            "D:\\data\\a.db"
        );
        assert_eq!(normalize_file_path("sqlite:///C:/x.db").unwrap(), "C:/x.db");
        assert_eq!(
            normalize_file_path("duckdb://D:/y.duckdb").unwrap(),
            "D:/y.duckdb"
        );
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
        let dir = temp_dir("session");
        let path = source_file(&dir, 3);
        let source = AccelSource {
            conn_id: "C_session".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };

        let (instance, session, _alias) = mount("t_accel_unqualified", &source);
        let result = session
            .run("SELECT count(*) AS n FROM orders")
            .expect("查源表");
        let rows = result.to_rows();
        assert_eq!(rows.len(), 1, "源文件里的表要能直接查到");
        assert_eq!(rows[0][0].to_string(), "3");

        // 同一源第二次取 = 复用同一条会话（不会撞 “Unique file handle conflict”）
        let again = instance.session_for_conn("C_session").expect("复用会话");
        assert!(
            Arc::ptr_eq(&session, &again),
            "同一 conn_id 应当复用同一条会话"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 数据对象面板用：`mounted_conns()` 能枚举挂载，`list_mounted_tables_for_conn` 给出可粘贴的限定名
    #[test]
    fn owners_enumerate_sessions_and_tables_list_with_qualified_names() {
        let dir = temp_dir("owners");
        let path = source_file(&dir, 2);
        let source = AccelSource {
            conn_id: "C_owners".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };
        let (instance, session, _alias) = mount("t_accel_owners", &source);

        let conns = instance.mounted_conns();
        assert!(
            conns
                .iter()
                .any(|(id, _alias, kind)| id == "C_owners" && *kind == AccelKind::DuckDb),
            "枚举里要有这条挂载：{conns:?}"
        );

        let tables = instance
            .list_mounted_tables_for_conn("C_owners")
            .expect("列源里的表");
        let orders = tables.iter().find(|t| t.name == "orders").expect("有 orders 表");
        assert!(!orders.is_view);
        // 限定名按挂载别名给三段（DuckDB 文件源不是两段名），粘进 SQL 要真能查
        assert!(
            orders.qualified.starts_with(&format!("\"{SOURCE_ALIAS}\".")),
            "限定名应带挂载别名：{}",
            orders.qualified
        );
        let result = session
            .run(&format!("SELECT count(*) AS n FROM {}", orders.qualified))
            .expect("限定名可查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        // 没挂过的连接：如实报错（面板据此给「为什么展不开」）
        assert!(instance.list_mounted_tables_for_conn("C_nope").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 只读挂载：写源对象被 DuckDB 拒；本地临时对象允许
    #[test]
    fn the_source_is_read_only_but_local_temp_objects_are_allowed() {
        let dir = temp_dir("readonly");
        let path = source_file(&dir, 1);
        let source = AccelSource {
            conn_id: "C_ro".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };
        let (_instance, session, _alias) = mount("t_accel_readonly", &source);

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
        let dir = temp_dir("refresh");
        let path = source_file(&dir, 2);
        let source = AccelSource {
            conn_id: "C_refresh".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: path.to_string_lossy().to_string(),
        };
        let (_instance, session, alias) = mount("t_accel_refresh", &source);
        assert_eq!(
            session
                .run("SELECT count(*) FROM orders")
                .unwrap()
                .to_rows()[0][0]
                .to_string(),
            "2"
        );

        // 「重新挂载」在单实例下的等价动作：本会话 `DETACH` 再挂回去（会话不重建，
        // 所以 `TEMP` 与本地变量都还在）。旧 `AccelSession::refresh` 就是这个序列。
        session
            .with_connection(|conn| {
                conn.execute_batch(&format!("USE memory; DETACH {alias}"))
                    .map_err(|error| error.to_string())
            })
            .expect("卸下");
        let mounted = session
            .mount_sources(&[super::federated_source(&source, &alias)])
            .expect("再挂回去");
        assert!(
            mounted.iter().any(|entry| entry.is_ready()),
            "重挂后要 Ready"
        );
        session.use_catalog(&alias).expect("再 USE");
        assert_eq!(
            session
                .run("SELECT count(*) FROM orders")
                .expect("重挂后仍能查到源表")
                .to_rows()[0][0]
                .to_string(),
            "2"
        );
        assert!(
            session.run(&format!("SELECT * FROM {alias}.orders")).is_ok(),
            "限定名（别名）也要能走"
        );
        // 重挂不重建连接：会话里的临时表还在
        session
            .run("CREATE TEMP TABLE keep AS SELECT 7 AS x")
            .expect("建临时表");
        session
            .with_connection(|conn| {
                conn.execute_batch(&format!("USE memory; DETACH {alias}"))
                    .map_err(|error| error.to_string())
            })
            .expect("再卸一次");
        session
            .mount_sources(&[super::federated_source(&source, &alias)])
            .expect("再挂一次");
        session.use_catalog(&alias).expect("USE");
        assert_eq!(
            session
                .run("SELECT x FROM keep")
                .unwrap()
                .to_rows()[0][0]
                .to_string(),
            "7",
            "重挂不该把会话里的临时表弄丢"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 两个不同的源同时建立会话：装扩展 / 挂载串行，互不影响
    #[test]
    fn concurrent_mounts_do_not_race_on_the_extension_directory() {
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
                let tag = format!("t_accel_conc_{}", source.conn_id);
                std::thread::spawn(move || {
                    let (_instance, session, _alias) = mount(&tag, &source);
                    session.run("SELECT count(*) FROM orders").expect("查表")
                })
            })
            .collect();
        let counts: Vec<String> = handles
            .into_iter()
            .map(|handle| handle.join().expect("线程不该 panic").to_rows()[0][0].to_string())
            .collect();
        assert_eq!(counts.len(), 2, "两条会话都建起来了");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 没试过扩展 = 不拦（`Unknown`）；失败才记下来拦（首用联网失败的现场）
    #[test]
    fn extension_state_only_blocks_after_a_real_failure() {
        assert!(extension_state(AccelKind::MySql).is_ok(), "没试过就不该拦");
        assert!(extension_state(AccelKind::DuckDb).is_ok());

        // 造一次真实失败：文件型的“源”指向不存在的路径 → 挂载状态是 Failed
        let source = AccelSource {
            conn_id: "C_missing".to_string(),
            kind: AccelKind::DuckDb,
            connection_string: "Z:/definitely/missing.duckdb".to_string(),
        };
        let instance = AnalysisInstance::open_tagged("t_accel_missing", std::time::Duration::from_secs(30))
            .expect("开实例");
        let alias = instance
            .reserve_alias(SOURCE_ALIAS, &source.conn_id, source.kind)
            .expect("取别名");
        let session = instance.session_for_conn("C_missing").expect("取会话");
        let mounted = session
            .mount_sources(&[super::federated_source(&source, &alias)])
            .expect("单个源挂不上不整批失败");
        assert!(
            mounted
                .iter()
                .any(|entry| matches!(entry.state, MountState::Failed(_))),
            "挂不上的源要报失败"
        );
        // 失败不写进“扩展失败”表（那是另一个维度：挂载失败由执行路径每次如实报）
        assert!(extension_state(AccelKind::DuckDb).is_ok());
    }

    /// 扩展门控（2026-10-03 用户规则：Secret 注册按"是否有对应扩展"放行）
    #[test]
    fn has_federable_extension_gates_by_scanner_extension() {
        // 官方 scanner 扩展：mysql / postgres / sqlite → 可注册
        assert!(AccelKind::has_federable_extension("mysql"));
        assert!(AccelKind::has_federable_extension("postgres"));
        assert!(AccelKind::has_federable_extension("sqlite"));
        assert!(
            AccelKind::has_federable_extension("postgres_native"),
            "驱动 id 也应归一"
        );
        // duckdb 文件源：内核自带能力、无扩展 → 不注册（默认不注册）
        assert!(!AccelKind::has_federable_extension("duckdb"));
        // 未知 / 无法联邦 → 不注册（不猜）
        assert!(!AccelKind::has_federable_extension(""));
        assert!(!AccelKind::has_federable_extension("nosql"));
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
        assert!(
            source.attach_sql().contains("TYPE mysql"),
            "{}",
            source.attach_sql()
        );

        let pg = AccelSource::new(
            "C_pg",
            "postgres_native",
            "postgres_native://u:p@h:5432/db?sslmode=require",
        )
        .expect("组装源");
        assert_eq!(
            pg.connection_string, "postgres://u:p@h:5432/db?sslmode=require",
            "查询串要原样留着"
        );

        // 已经是扫描器 scheme 的：保持不变（幂等）
        let same = AccelSource::new("C_same", "mysql", "mysql://root:pw@h:3306/db").expect("组装");
        assert_eq!(same.connection_string, "mysql://root:pw@h:3306/db");

        // 没有 scheme 的写法：原样留着（让 DuckDB 自己报错，不猜）
        assert_eq!(
            super::normalize_scheme(AccelKind::MySql, "h:3306/db"),
            "h:3306/db"
        );
    }
}

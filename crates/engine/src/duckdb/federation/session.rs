//! 联邦会话：把多个源挂到**同一条** DuckDB 连接上（见 `docs/architecture/federation/`）
//!
//! ## 与 `accel` 的关系（先说清楚，避免又长出一套）
//!
//! `accel` 是“一条源一条连接”（整库 `ATTACH … (READ_ONLY)` + `USE`）；联邦是它的**推广**：
//! 同一条连接挂**多条**源、每条一个别名，`USE` 指向**主源**。所以这里直接复用 accel 的：
//!
//! 1. [`mount_lock`]（`INSTALL` / `LOAD` 是进程级资源，并发建会话会撞文件）；
//! 2. [`run_sql_on`]（读写分流与 Arrow 转换与加速档**同一份**，同一句在两个档上形状一致）；
//! 3. 写保护双保险（`ATTACH … (READ_ONLY)` 让 DuckDB 自己拒 + 编辑器侧提交前拒）；
//!    L2 没有第一道，改用会话层判定（见 [`FederatedSession::l2_write_refusal`]）。
//!
//! ## 三条会话内规则
//!
//! - **挂不上的源不阻断**：记下原话继续挂下一个（部分可用好过整体失败）；
//! - **主源**：未限定名只在主源解析；请求的主源不可用 → 回退到第一个可用源，
//!   并留下 `primary_note`（界面上要说出来，不能悄悄换）；
//! - **只读**：L1 源一律 `READ_ONLY` 挂载（DuckDB 自己拒写）；**L2 源不支持 `READ_ONLY`**
//!   （真机台账 §2.1），那一路由会话层 [`FederatedSession::run`] 拒 + 编辑器闸门 + 只读账号；
//!   本地临时对象（`CREATE TEMP TABLE`）照常允许。
//!
//! ## 会话生命周期（进程内缓存）
//!
//! 会话按**会话主人**（当前文档绑定的连接 id）缓存，命中条件是**源集合指纹**一致
//! （[`source_fingerprint`]）。执行路径调 [`ensure_session`]，界面与中断路径读
//! [`snapshot_for`] / [`cancel`]（纯内存，不做 I/O）。
//!
//! 两条要记住的语义：
//!
//! 1. **换主源不重建会话**（只是会话上的 `USE`）：用户建的本地临时对象不丢；
//! 2. **源清单变了会重建会话**（旧会话连同它的临时对象一起消失，日志里写一条）——
//!    这是“改了参与源就当新会话”的代价，比让旧表清单继续骗人强。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use duckdb::Connection;
use once_cell::sync::Lazy;

use shared::error::CoreError;
use shared::models::QueryResult;

use super::super::accel::{AccelKind, install_sql, mount_lock, run_sql_on, scrub_credentials};
use super::super::manager::DuckDBManager;
use super::registry::{FederatedSource, MountState, MountedSource, SessionSnapshot};

/// 一条联邦会话：DuckDB 内存库 + 一批只读挂载
pub struct FederatedSession {
    conn: Mutex<Connection>,
    /// 中断句柄（**建会话时就取出来**：真正跑查询时连接锁被占着，那时再取就晚了）
    interrupt: Arc<duckdb::InterruptHandle>,
    /// 现在有没有语句在跑（中断的“有没有可中断的”靠它，不凭猜）
    running: AtomicBool,
    mounted: Mutex<Vec<MountedSource>>,
    primary: Mutex<Option<String>>,
    primary_note: Mutex<Option<String>>,
    mounted_at: std::time::SystemTime,
}

impl std::fmt::Debug for FederatedSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 连接没法 Debug：打印可读摘要（主源 + 源数），测试与日志都用它
        f.debug_struct("FederatedSession")
            .field("primary", &self.primary.lock().ok().and_then(|p| p.clone()))
            .field(
                "sources",
                &self.mounted.lock().map(|m| m.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl FederatedSession {
    /// 建会话并**逐个**挂载（挂不上的记原因，不中断）
    ///
    /// 会做 I/O（首次装扩展要联网一次、`ATTACH` 要连源库），属于**事件路径**；
    /// 界面可用性看 [`Self::snapshot`]（纯内存读）。
    pub fn open(
        sources: &[FederatedSource],
        requested_primary: Option<&str>,
    ) -> Result<Self, String> {
        // 会话内重名先拦：两个源叫一个名会静默指到其中一个（那是最难查的一类错）
        let mut seen: Vec<String> = Vec::new();
        for source in sources {
            if seen.iter().any(|alias| alias == &source.alias) {
                return Err(format!("别名 {} 在这次的源里出现了两次", source.alias));
            }
            seen.push(source.alias.clone());
        }

        let conn = open_configured()?;

        // 装扩展与一批 ATTACH 串行（`INSTALL` 写目录、`LOAD` 是连接级，都要独占）
        let _mount = mount_lock();
        let mut installed: HashSet<&'static str> = HashSet::new();
        for source in sources {
            if let Some(extension) = source.kind.extension()
                && installed.insert(extension)
            {
                install_and_load(&conn, source.kind, extension)?;
            }
        }

        let mut mounted = Vec::with_capacity(sources.len());
        for source in sources {
            mounted.push(mount_one(&conn, source));
        }

        let (primary, primary_note) = pick_primary(&mounted, requested_primary);
        if let Some(alias) = primary.as_deref() {
            use_catalog(&conn, alias)?;
        }

        let interrupt = conn.interrupt_handle();
        Ok(Self {
            conn: Mutex::new(conn),
            interrupt,
            running: AtomicBool::new(false),
            mounted: Mutex::new(mounted),
            primary: Mutex::new(primary),
            primary_note: Mutex::new(primary_note),
            mounted_at: std::time::SystemTime::now(),
        })
    }

    /// 在本地库上执行一句（读语句出 Arrow 批，写语句出影响行数）
    ///
    /// 源库对象的写在引擎侧就被 `READ_ONLY` 拒（这里不做第二套判定：判据只有一处）——
    /// **除了 L2 源**：Oracle 的扩展不支持 `READ_ONLY`，对它的写只能由会话层拦（[`Self::l2_write_refusal`]）。
    /// 错误文本先脱敏：源连接串是带凭据的，驱动报错可能把它回显出来。
    pub fn run(&self, sql: &str) -> Result<QueryResult, CoreError> {
        if let Some(reason) = self.l2_write_refusal(sql) {
            return Err(CoreError::common(shared::error::CommonError::General(reason)));
        }
        let conn = self.lock()?;
        run_sql_on(&conn, &self.running, sql).map_err(|error| self.scrub_error(error))
    }

    /// 写 L2 源的保护（**引擎侧第二道闸**；第一道是编辑器的语句闸门，第三道是只读账号）
    ///
    /// 为什么需要它：Oracle 的社区扩展**不支持 `READ_ONLY`**（真机原话 *does not accept option
    /// 'read_only' yet*），所以 `ATTACH` 之后 DuckDB 未拦写——不补这一道，联邦档里一句
    /// `INSERT INTO ora.…` 会真的落到源库（而界面上写着“本地跑的通道不能写源库”）。
    ///
    /// 判据取“**保守**”那一侧：写语句（判据与驱动层同一份 `returns_rows`）+ 提到该源的别名
    /// （或它就是主源——未限定名会落在它上面）就拒，不做语义分析（宁可多拒一句，不能放过去真写）。
    fn l2_write_refusal(&self, sql: &str) -> Option<String> {
        let Ok(mounted) = self.mounted.lock() else {
            return None; // 锁被污染：不在这里另报一个错（执行路径会给真实原因）
        };
        let primary = self.primary.lock().ok().and_then(|primary| primary.clone());
        write_refusal(&mounted, primary.as_deref(), sql)
    }

    /// 把错误文本里可能出现的源凭据抹掉（每个源一条规则；命中即换，没命中就原样）
    fn scrub_error(&self, error: CoreError) -> CoreError {
        let text = error.to_string();
        let mut scrubbed = text.clone();
        if let Ok(mounted) = self.mounted.lock() {
            for entry in mounted.iter() {
                scrubbed = scrub_credentials(&entry.source.connection_string, &scrubbed);
            }
        }
        if scrubbed == text {
            return error;
        }
        CoreError::common(shared::error::CommonError::General(scrubbed))
    }

    /// 重新挂载某个源：源库里**新建的表**要这样才看得见（数据本来就是实时的）
    ///
    /// 先 `USE memory` 再 `DETACH`：默认 catalog 指向该源时 DuckDB 拒绝卸载它。
    pub fn refresh(&self, alias: &str) -> Result<(), String> {
        let source = self
            .mounted
            .lock()
            .map_err(|_| "会话状态锁被污染".to_string())?
            .iter()
            .find(|entry| entry.alias() == alias)
            .map(|entry| entry.source.clone())
            .ok_or_else(|| format!("会话里没有源 {alias}"))?;

        let conn = self.lock().map_err(|error| error.to_string())?;
        let was_primary = self
            .primary
            .lock()
            .map(|primary| primary.as_deref() == Some(alias))
            .unwrap_or(false);

        conn.execute_batch(&format!("USE memory; DETACH {alias}"))
            .map_err(|error| scrub_error(&source.connection_string, format!("卸载 {alias} 失败：{error}")))?;
        // 重挂走与 `open` 同一条实现：L2 的 Secret 在这里重建（口令可能有变）
        let state = attach_source(&conn, &source);

        if let Ok(mut mounted) = self.mounted.lock()
            && let Some(entry) = mounted.iter_mut().find(|entry| entry.alias() == alias)
        {
            entry.state = state.clone();
        }
        // 重挂之后主源要 `USE` 回去（否则未限定名会解析到 memory 上）
        if was_primary && matches!(state, MountState::Ready { .. }) {
            use_catalog(&conn, alias).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// 切换主源（未限定名从此在它里面解析）
    pub fn set_primary(&self, alias: &str) -> Result<(), String> {
        let ready = {
            let mounted = self
                .mounted
                .lock()
                .map_err(|_| "会话状态锁被污染".to_string())?;
            let entry = mounted
                .iter()
                .find(|entry| entry.alias() == alias)
                .ok_or_else(|| format!("会话里没有源 {alias}"))?;
            entry.is_ready()
        };
        if !ready {
            return Err(format!("源 {alias} 当前不可用，不能设为主源"));
        }
        let conn = self.lock().map_err(|error| error.to_string())?;
        use_catalog(&conn, alias).map_err(|error| error.to_string())?;
        if let Ok(mut primary) = self.primary.lock() {
            *primary = Some(alias.to_string());
        }
        if let Ok(mut note) = self.primary_note.lock() {
            *note = None; // 用户显式选过了，回退说明就该收起来
        }
        Ok(())
    }

    /// 会话快照（**界面读这个**：纯内存，渲染路径可调）
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            primary: self.primary.lock().ok().and_then(|primary| primary.clone()),
            sources: self
                .mounted
                .lock()
                .map(|mounted| mounted.clone())
                .unwrap_or_default(),
            primary_note: self.primary_note.lock().ok().and_then(|note| note.clone()),
        }
    }

    /// 记一条“主源为什么不是你要的那个”的说明（界面上要说出来，不能悄悄换）
    ///
    /// 用在「换主源失败」这种路径上：[`Self::set_primary`] 拒了之后，快照里得留下原因，
    /// 否则用户看到的只是“怎么没换”。
    pub(crate) fn set_primary_note(&self, note: Option<String>) {
        if let Ok(mut current) = self.primary_note.lock() {
            *current = note;
        }
    }

    /// 中断这条会话上正在跑的语句（三态可读，与加速档同一语义）
    pub fn interrupt(&self) -> Result<bool, String> {
        if !self.running.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(false);
        }
        self.interrupt.interrupt();
        Ok(true)
    }

    /// 会话活了多久（诊断）
    pub fn mounted_for(&self) -> std::time::Duration {
        self.mounted_at
            .elapsed()
            .unwrap_or(std::time::Duration::ZERO)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoreError> {
        self.conn.lock().map_err(|error| {
            CoreError::common(shared::error::CommonError::General(format!(
                "联邦会话连接锁被污染：{error}"
            )))
        })
    }
}

/// 写 L2 源的判据（**纯函数**：不碰连接，测试直接喂清单）
///
/// 为什么这么粗：写语句没有 AST 可用，而这一步是**最后一道**能拦住真写的地方（Oracle 的
/// `ATTACH` 不接受 `READ_ONLY`，DuckDB 自己不拦）。所以宁可多拒一句（用户改用只读账号、
/// 或去源库执行），也不能漏过去——判错的方向只能是“拒”，不能是“放”。
fn write_refusal(mounted: &[MountedSource], primary: Option<&str>, sql: &str) -> Option<String> {
    if crate::driver::utils::returns_rows(sql) {
        return None;
    }
    for entry in mounted.iter() {
        // 支持 `READ_ONLY` 的源由 DuckDB 自己拒（判据只有一处，不在这里重复）
        if entry.source.kind.supports_read_only_attach() {
            continue;
        }
        let alias = entry.alias();
        if mentions_alias(sql, alias) || primary == Some(alias) {
            return Some(format!(
                "源 {alias} 不支持只读挂载（它的扫描器没有 READ_ONLY），对它的写被拦下了：\
                 请在源库上执行，或改用只读账号"
            ));
        }
    }
    None
}

/// 语句里有没有把某个源当限定名前缀（`ora.表` / `"ora".表` / `` `ora`.表 ``）
///
/// 只看字面，不解析：前面紧挨着的字符是标识符字符时，说明那只是个更长的名字（`myora.`），
/// 不算提到这个源。
fn mentions_alias(sql: &str, alias: &str) -> bool {
    let lowered = sql.to_ascii_lowercase();
    for needle in [
        format!("{alias}."),
        format!("\"{alias}\"."),
        format!("`{alias}`."),
    ] {
        let mut from = 0;
        while let Some(offset) = lowered[from..].find(&needle) {
            let start = from + offset;
            let boundary = sql[..start]
                .chars()
                .next_back()
                .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
            if boundary {
                return true;
            }
            from = start + 1;
        }
    }
    false
}

/// 开一条按产品纪律配置好的内存连接（扩展目录 / 内存闸 / 溢写口 / 不静默联网）
fn open_configured() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|error| format!("开联邦分析连接失败：{error}"))?;
    DuckDBManager::configure_connection(&conn)
        .map_err(|error| format!("配置联邦分析连接失败：{error}"))?;
    Ok(conn)
}

/// 装 + 加载一个扩展（失败原因里带上“离线怎么办”；L2 要从社区仓库装）
fn install_and_load(conn: &Connection, kind: AccelKind, extension: &str) -> Result<(), String> {
    conn.execute_batch(&install_sql(kind, extension))
        .map_err(|error| {
            format!(
                "联邦需要 DuckDB 扩展 {extension}，安装失败：{error}（首次需要能访问 DuckDB 扩展源；离线时请先把扩展放进 {}）",
                paths::extensions_dir().display()
            )
        })
}

/// 挂载前把凭据建成本连接的 Secret（L2 的唯一入口；L1 没有 Secret，直接过）
///
/// **会话级**（`CREATE OR REPLACE SECRET` 不带 `IN PERSISTENT`）：口令不落盘，会话没了就没了。
/// 幂等，所以 `open` 与 `refresh` 都走它（重挂时再建一次，不多存一份凭据）。
fn ensure_secret(conn: &Connection, source: &FederatedSource) -> Result<(), String> {
    let Some(secret) = &source.secret else {
        return Ok(());
    };
    conn.execute_batch(&secret.create_sql).map_err(|error| {
        scrub_error(
            &source.connection_string,
            format!("源 {}：建凭据（Secret）失败：{error}", source.alias),
        )
    })
}

/// 建凭据 + `ATTACH`（**挂载只有这一处实现**：`open` 与 `refresh` 共用，别在两处各挂一遍）
///
/// 失败不抛：状态就是结论（挂不上的源不该把整条会话带下去）。
fn attach_source(conn: &Connection, source: &FederatedSource) -> MountState {
    let outcome = ensure_secret(conn, source).and_then(|()| {
        conn.execute_batch(&source.attach_sql())
            .map_err(|error| attach_error(&source.alias, &error))
    });
    match outcome {
        Ok(()) => MountState::Ready {
            tables: count_tables(conn, &source.alias).unwrap_or(0),
        },
        // 错误文本脱敏：凭据随 `ATTACH` 串进 DuckDB，报错会把参数回显出来
        Err(reason) => MountState::Failed(scrub_error(&source.connection_string, reason)),
    }
}

/// 挂一个源（**失败不抛**：状态就是结论）
fn mount_one(conn: &Connection, source: &FederatedSource) -> MountedSource {
    MountedSource {
        source: source.clone(),
        state: attach_source(conn, source),
    }
}

/// 挂载 / 卸载失败的原话（点名源：跨源场景里“哪个源出问题”是最重要的信息）
fn attach_error(alias: &str, error: &duckdb::Error) -> String {
    format!("源 {alias}：{error}")
}

/// 错误文本脱敏（凭据随 `ATTACH` 串进 DuckDB，报错会把参数回显出来）
fn scrub_error(connection_string: &str, text: String) -> String {
    scrub_credentials(connection_string, &text)
}

/// 挂载时的表数量（纯展示；数不出来记 0，不因此把源判成失败）
fn count_tables(conn: &Connection, alias: &str) -> Result<usize, String> {
    conn.query_row(
        "SELECT count(*) FROM duckdb_tables() WHERE database_name = ?",
        [alias],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count as usize)
    .map_err(|error| error.to_string())
}

/// 切默认 catalog 到某个源
fn use_catalog(conn: &Connection, alias: &str) -> Result<(), String> {
    conn.execute_batch(&format!("USE {alias}"))
        .map_err(|error| format!("切主源到 {alias} 失败：{error}"))
}

/// 选主源：指定的可用就用它；否则回退到第一个可用源并**留下说明**
fn pick_primary(
    mounted: &[MountedSource],
    requested: Option<&str>,
) -> (Option<String>, Option<String>) {
    let ready: Vec<&str> = mounted
        .iter()
        .filter(|entry| entry.is_ready())
        .map(|entry| entry.alias())
        .collect();

    if let Some(want) = requested {
        if ready.contains(&want) {
            return (Some(want.to_string()), None);
        }
        let fallback = ready.first().map(|alias| (*alias).to_string());
        let note = match fallback.as_deref() {
            Some(alias) => Some(format!("主源 {want} 不可用，已改用 {alias}")),
            None => Some(format!("主源 {want} 不可用，当前没有可用源")),
        };
        return (fallback, note);
    }
    (ready.first().map(|alias| (*alias).to_string()), None)
}

// ===== 会话缓存（进程内，按“会话主人”索引） =====
//
// 与 [`super::super::accel`] 同一套思路，多出来的那一层是**源集合指纹**：加速档一条源
// 一个连接，源变了就是另一个 conn_id；联邦的源清单会变（用户改了参与的连接、别名重排），
// 所以“同一条会话”要靠指纹认：**指纹一致才复用**，不一致就重建（旧的连同它的本地临时
// 对象一起消失——那是源清单变更的代价，日志里会写一条）。

/// 会话主人是谁
///
/// 不是“主源”：主源可以在会话里换（[`FederatedSession::set_primary`]），但**会话的身份**
/// 是**当前文档绑定的连接**（同一个连接上的两份文档共享一条联邦会话）。
static SESSIONS: Lazy<Mutex<HashMap<String, CachedFederation>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 建会话的进程内互斥（**查指纹 → 建 → 登记**这一整段）
///
/// 不直接用 [`mount_lock`]：那个锁 [`FederatedSession::open`] 自己会拿，同一线程再拿一次
/// 就是死锁（`std::sync::Mutex` 不可重入）。这里只护缓存表，两个锁之间不存在环。
static SESSION_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

struct CachedFederation {
    /// 源集合指纹（只由源清单决定：主源切换不重建会话）
    fingerprint: String,
    session: Arc<FederatedSession>,
}

/// 源集合指纹（**纯函数**：源清单或连接串一变就换指纹）
///
/// 只含源本身（conn_id / 别名 / 种类 / 连接串），**不含主源**：换主源是同一条会话上的
/// `USE` 切换，不该把会话重建（那会把用户建的本地临时对象一起丢掉）。
pub fn source_fingerprint(sources: &[FederatedSource]) -> String {
    let mut parts: Vec<String> = sources
        .iter()
        .map(|source| {
            format!(
                "{}|{}|{}|{}",
                source.conn_id,
                source.alias,
                source.kind.label(),
                source.connection_string
            )
        })
        .collect();
    parts.sort();
    parts.join(";")
}

/// 取（必要时建立）一条联邦会话
///
/// **会做 I/O**（首次装扩展要联网一次、`ATTACH` 要连源库），属于**事件路径**（工作线程上
/// 的执行）；界面可用性看 [`snapshot_for`] 与宿主侧的门控。
///
/// 源清单为空直接拒：没有源就没有“联邦”这回事，与其给一条空会话跑出莫名其妙的结果，
/// 不如在入口把话说清楚。
pub fn ensure_session(
    owner: &str,
    sources: &[FederatedSource],
    requested_primary: Option<&str>,
) -> Result<Arc<FederatedSession>, String> {
    if sources.is_empty() {
        return Err(
            "联邦查询至少需要一个源：在连接设置里开启「DuckDB 本地加速」并连接它".to_string(),
        );
    }
    let fingerprint = source_fingerprint(sources);
    if let Some(session) = cached_for(owner, &fingerprint) {
        apply_requested_primary(&session, requested_primary);
        return Ok(session);
    }

    let _guard = SESSION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // 等锁期间可能已被别人建好了
    if let Some(session) = cached_for(owner, &fingerprint) {
        apply_requested_primary(&session, requested_primary);
        return Ok(session);
    }

    let session = Arc::new(FederatedSession::open(sources, requested_primary)?);
    if let Ok(mut sessions) = SESSIONS.lock() {
        if sessions.contains_key(owner) {
            tracing::info!(
                owner = %owner,
                "联邦源清单变了，重建会话（旧的本地临时对象随旧会话消失）"
            );
        }
        sessions.insert(
            owner.to_string(),
            CachedFederation {
                fingerprint,
                session: session.clone(),
            },
        );
    }
    let snapshot = session.snapshot();
    tracing::info!(
        owner = %owner,
        sources = snapshot.sources.len(),
        ready = snapshot.ready_count(),
        primary = snapshot.primary.as_deref().unwrap_or("-"),
        "联邦会话已就绪"
    );
    Ok(session)
}

/// 已建立的会话（**不做 I/O**；没有就是 `None`）
pub fn session_for(owner: &str) -> Option<Arc<FederatedSession>> {
    SESSIONS.lock().ok()?.get(owner).map(|cached| cached.session.clone())
}

/// 会话快照（**渲染路径可调**：纯内存；没有会话就是 `None`）
///
/// 注意 `None` 与“有空会话”不同：没有会话 = 还没挂过（源清单 UI 显示空态）。
pub fn snapshot_for(owner: &str) -> Option<SessionSnapshot> {
    session_for(owner).map(|session| session.snapshot())
}

/// 放掉某个主人的会话（测试 / 连接被删时用）
pub fn drop_session(owner: &str) -> bool {
    SESSIONS
        .lock()
        .map(|mut sessions| sessions.remove(owner).is_some())
        .unwrap_or(false)
}

/// 放掉全部联邦会话（测试用：进程内状态要能清干净）
pub fn drop_all() {
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.clear();
    }
}

/// 中断某个主人会话上正在跑的语句（没有会话 = `None`，与加速档同一语义）
pub fn cancel(owner: &str) -> Option<Result<bool, String>> {
    session_for(owner).map(|session| session.interrupt())
}

/// 切换主源（没有会话就是错误：**不替用户建一条**，那是执行路径的事）
pub fn set_primary(owner: &str, alias: &str) -> Result<(), String> {
    let session = session_for(owner).ok_or_else(|| "还没有联邦会话：先执行一次再切换主源".to_string())?;
    session.set_primary(alias)
}

/// 按源重挂（表清单刷新；没有会话就是错误，理由同上）
pub fn refresh_source(owner: &str, alias: &str) -> Result<(), String> {
    let session = session_for(owner)
        .ok_or_else(|| "还没有联邦会话：先执行一次再刷新源".to_string())?;
    session.refresh(alias)
}

/// 重挂会话里的所有源（界面上「刷新全部」）
///
/// 返回**逐源结果**：失败的源如实带原因（不吞、不合并成一句“部分失败”）。
pub fn refresh_all(owner: &str) -> Result<Vec<(String, Result<(), String>)>, String> {
    let session = session_for(owner)
        .ok_or_else(|| "还没有联邦会话：先执行一次再刷新源".to_string())?;
    let aliases: Vec<String> = session
        .snapshot()
        .sources
        .iter()
        .map(|entry| entry.alias().to_string())
        .collect();
    Ok(aliases
        .into_iter()
        .map(|alias| {
            let outcome = session.refresh(&alias);
            (alias, outcome)
        })
        .collect())
}

/// 缓存命中：主人与指纹都对上才算
fn cached_for(owner: &str, fingerprint: &str) -> Option<Arc<FederatedSession>> {
    let sessions = SESSIONS.lock().ok()?;
    let cached = sessions.get(owner)?;
    (cached.fingerprint == fingerprint).then(|| cached.session.clone())
}

/// 把“请求的主源”落到会话上（**不重建会话**）
///
/// 请求的主源不可用就保留现状，并把原因留在快照的 `primary_note` 里——不能让一次
/// “换主源”失败变成静默的“什么都没发生”。
fn apply_requested_primary(session: &FederatedSession, requested: Option<&str>) {
    let Some(want) = requested else {
        return;
    };
    if session.snapshot().primary.as_deref() == Some(want) {
        return;
    }
    if let Err(reason) = session.set_primary(want) {
        session.set_primary_note(Some(reason));
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{FederatedSession, write_refusal};
    use crate::duckdb::federation::registry::{FederatedSource, MountState, MountedSource};
    use std::sync::Arc;

    /// 造一个 DuckDB 文件源（**离线可跑**：duckdb 文件挂载不需要任何扩展）
    fn file_source(dir: &std::path::Path, name: &str, alias: &str, rows: i64) -> FederatedSource {
        let path = dir.join(format!("{name}.duckdb"));
        let conn = duckdb::Connection::open(&path).expect("建源文件");
        conn.execute_batch(&format!(
            "CREATE TABLE items AS SELECT i AS id, 'v' || i AS name FROM range({rows}) AS r(i)"
        ))
        .expect("建表");
        drop(conn);
        FederatedSource::new(name, alias, "duckdb", path.to_str().expect("路径")).expect("组装源")
    }

    fn workdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_fed_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    /// 两个源跨源 join：未限定名走主源，限定名走各自
    #[test]
    fn two_sources_join_across_with_a_primary() {
        let dir = workdir("join");
        let alpha = file_source(&dir, "alpha", "alpha", 3);
        let beta = file_source(&dir, "beta", "beta", 3);
        let session = FederatedSession::open(&[alpha, beta], Some("alpha")).expect("建会话");

        let snapshot = session.snapshot();
        assert_eq!(snapshot.ready_count(), 2, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("alpha"));
        assert!(snapshot.primary_note.is_none());

        // 未限定名走主源
        let result = session.run("SELECT count(*) AS n FROM items").expect("主源可查");
        assert_eq!(result.to_rows()[0][0].to_string(), "3");

        // 跨源 join 用限定名
        let joined = session
            .run("SELECT count(*) AS n FROM alpha.items a JOIN beta.items b ON a.id = b.id")
            .expect("跨源 join");
        assert_eq!(joined.to_rows()[0][0].to_string(), "3");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一个源挂不上：其余照常可用，失败的原因留在快照里
    #[test]
    fn a_broken_source_does_not_take_the_session_down() {
        let dir = workdir("broken");
        let good = file_source(&dir, "good", "good", 2);
        let missing = FederatedSource::new(
            "missing",
            "missing",
            "duckdb",
            dir.join("nope.duckdb").to_str().expect("路径"),
        )
        .expect("组装源");

        // 请求的主源正好是坏的那个 → 回退到可用的，并留下说明
        let session =
            FederatedSession::open(&[missing, good], Some("missing")).expect("会话仍该建起来");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.ready_count(), 1, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("good"), "回退到可用源");
        assert!(
            snapshot.primary_note.as_deref().unwrap_or("").contains("missing"),
            "回退要说出来：{:?}",
            snapshot.primary_note
        );
        // 失败源保留在清单里，带原话
        let failed = snapshot
            .sources
            .iter()
            .find(|entry| entry.alias() == "missing")
            .expect("失败源不该消失");
        assert!(matches!(failed.state, MountState::Failed(_)), "{failed:?}");

        let result = session.run("SELECT count(*) AS n FROM items").expect("可用源照查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 会话内重名直接拒（两个源一个名会静默指到其中一个）
    #[test]
    fn duplicate_aliases_are_refused_before_mounting() {
        let dir = workdir("dup");
        let a = file_source(&dir, "a", "same", 1);
        let b = file_source(&dir, "b", "same", 1);
        let error = FederatedSession::open(&[a, b], None).expect_err("该拒");
        assert!(error.contains("same"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 重挂某源后状态与表数不变、主源仍在它上面；切主源后未限定名跟着走
    ///
    /// 注：“源库新建的表重挂后可见”在**单进程测试里验不了**：DuckDB 文件同一进程只允许
    /// 一个连接（`File is already open in …`），会话挂着它就写不进去；那一条靠真机探针
    /// （MySQL / SQLite 源，见 `tests/duckdb_accel_probe.rs`）验。
    #[test]
    fn refresh_keeps_the_source_and_primary_switches_resolution() {
        let dir = workdir("refresh");
        let first = file_source(&dir, "first", "first", 1);
        let second = file_source(&dir, "second", "second", 2);
        let session =
            FederatedSession::open(&[first.clone(), second.clone()], Some("first")).expect("建会话");

        // 重挂：状态仍是可用、表数正确，主源没丢
        session.refresh("first").expect("重挂");
        let snapshot = session.snapshot();
        let entry = snapshot
            .sources
            .iter()
            .find(|entry| entry.alias() == "first")
            .expect("源还在");
        assert!(entry.is_ready(), "重挂后该是可用的：{entry:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("first"));
        let result = session.run("SELECT count(*) AS n FROM items").expect("主源照常");
        assert_eq!(result.to_rows()[0][0].to_string(), "1");

        // 切主源：未限定名解析到 second（items 行数从 1 变 2）
        session.set_primary("second").expect("切主源");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.primary.as_deref(), Some("second"));
        let result = session.run("SELECT count(*) AS n FROM items").expect("查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写源库对象被 DuckDB 自己拒（只读是引擎侧的第一道闸）
    #[test]
    fn writing_to_a_source_is_refused_by_the_engine() {
        let dir = workdir("readonly");
        let source = file_source(&dir, "ro", "ro", 1);
        let session = FederatedSession::open(&[source], None).expect("建会话");
        let error = session
            .run("INSERT INTO ro.items VALUES (99, 'x')")
            .expect_err("写源库该被拒");
        assert!(
            error.to_string().to_lowercase().contains("read-only")
                || error.to_string().to_lowercase().contains("read only"),
            "{error}"
        );
        // 本地临时对象照常允许（分析要用）
        session
            .run("CREATE TEMP TABLE scratch AS SELECT 1 AS n")
            .expect("本地临时表可以建");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写 L2 源被会话层拦（Oracle 没有 `READ_ONLY` 那道引擎闸）；读语句与 L1 源不受影响
    #[test]
    fn writing_to_an_l2_source_is_refused_by_the_session() {
        let oracle = FederatedSource::new("c1", "ora", "oracle", "oracle://u:p@h:1521/XEPDB1")
            .expect("L2 源（组装不连库）");
        let pg = FederatedSource::new(
            "c2",
            "wh",
            "postgres_native",
            "postgres_native://u:p@h:5432/w",
        )
        .expect("L1 源（组装不连库）");
        let mounted = vec![ready(oracle), ready(pg)];

        // 写 L2：限定名 / 带引号 / 它就是主源（未限定名落在它上面），三种都得拒
        for sql in [
            "INSERT INTO ora.T VALUES (1)",
            "UPDATE ora.T SET a = 1",
            "DELETE FROM \"ora\".T",
            "INSERT INTO T VALUES (1)",
        ] {
            let primary = if sql.contains("ora.") || sql.contains("\"ora\"") {
                Some("wh")
            } else {
                Some("ora")
            };
            assert!(write_refusal(&mounted, primary, sql).is_some(), "该拒：{sql}");
        }

        // 放行的一侧：别名长得像但不是一个名字、写 L1（由 READ_ONLY 拦）、本地对象、读语句
        for (primary, sql) in [
            (Some("wh"), "INSERT INTO myora.T VALUES (1)"),
            (Some("wh"), "INSERT INTO wh.t VALUES (1)"),
            (Some("wh"), "CREATE TEMP TABLE t AS SELECT 1"),
            (None, "INSERT INTO memory.t VALUES (1)"),
            (Some("wh"), "SELECT * FROM ora.T"),
            (Some("ora"), "WITH x AS (SELECT 1 AS n) SELECT n FROM x"),
        ] {
            assert!(write_refusal(&mounted, primary, sql).is_none(), "不该拦：{sql}");
        }
    }

    /// 造一个“已挂上”的条目（只给判定用，不连任何库）
    fn ready(source: FederatedSource) -> MountedSource {
        MountedSource {
            source,
            state: MountState::Ready { tables: 0 },
        }
    }

    /// 指纹只看源清单：主源换了不换指纹（换主源是同一条会话上的 `USE` 切换）
    #[test]
    fn the_fingerprint_follows_the_source_set_only() {
        let dir = workdir("fingerprint");
        let alpha = file_source(&dir, "alpha", "alpha", 1);
        let beta = file_source(&dir, "beta", "beta", 1);

        let one = super::source_fingerprint(&[alpha.clone()]);
        assert_eq!(one, super::source_fingerprint(&[alpha.clone()]), "同样输入同指纹");
        assert_ne!(one, super::source_fingerprint(&[alpha, beta]), "多一个源就要换指纹");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 源清单没变 → 复用同一条会话（换主源也不重建）；变了 → 重建
    #[test]
    fn the_session_is_reused_until_the_source_set_changes() {
        let dir = workdir("cache");
        let alpha = file_source(&dir, "alpha", "alpha", 1);
        let beta = file_source(&dir, "beta", "beta", 2);
        let gamma = file_source(&dir, "gamma", "gamma", 3);
        let owner = "C_fed_cache";

        let first =
            super::ensure_session(owner, &[alpha.clone(), beta.clone()], Some("alpha"))
                .expect("建会话");
        let count = first.run("SELECT count(*) AS n FROM items").expect("查主源");
        assert_eq!(count.to_rows()[0][0].to_string(), "1", "主源是 alpha");

        // 源集合没变：复用同一条会话，**换主源只是在会话上切 `USE`**
        first
            .run("CREATE TEMP TABLE scratch AS SELECT 7 AS n")
            .expect("建本地临时表");
        let switched =
            super::ensure_session(owner, &[alpha.clone(), beta.clone()], Some("beta"))
                .expect("复用会话");
        assert!(Arc::ptr_eq(&first, &switched), "换主源不该重建会话");
        assert_eq!(switched.snapshot().primary.as_deref(), Some("beta"));
        let count = switched.run("SELECT count(*) AS n FROM items").expect("查新主源");
        assert_eq!(count.to_rows()[0][0].to_string(), "2", "未限定名跟主源走");
        let scratch = switched
            .run("SELECT count(*) AS n FROM scratch")
            .expect("本地临时对象该留着");
        assert_eq!(scratch.to_rows()[0][0].to_string(), "1");

        // 源集合变了：重建（新会话用新的源清单）
        let rebuilt = super::ensure_session(owner, &[gamma], None).expect("重建会话");
        assert!(!Arc::ptr_eq(&switched, &rebuilt), "源清单变了就该重建");
        let snapshot = rebuilt.snapshot();
        assert_eq!(snapshot.sources.len(), 1, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("gamma"));
        let count = rebuilt.run("SELECT count(*) AS n FROM items").expect("查新源");
        assert_eq!(count.to_rows()[0][0].to_string(), "3");

        // 会话查询 / 注销（界面与中断路径用这两个）
        assert!(super::session_for(owner).is_some());
        assert!(super::snapshot_for(owner).is_some(), "快照是纯内存读");
        assert!(super::cancel(owner).is_some(), "没在跑也要能问一句");
        assert!(super::drop_session(owner));
        assert!(super::session_for(owner).is_none());
        assert!(super::snapshot_for(owner).is_none());
        assert!(super::cancel(owner).is_none(), "没有会话就没有可中断的");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 一个源都没有：入口就把话说清楚（不拿空会话跑出莫名其妙的结果）
    #[test]
    fn an_empty_source_list_is_refused_at_the_entrance() {
        let error = super::ensure_session("C_fed_empty", &[], None).expect_err("该拒");
        assert!(error.contains("至少需要一个源"), "{error}");
    }

    /// 刷新全部：逐源给结果（界面上要能逐行说清）
    #[test]
    fn refresh_all_reports_each_source() {
        let dir = workdir("refresh_all");
        let alpha = file_source(&dir, "alpha", "alpha", 1);
        let beta = file_source(&dir, "beta", "beta", 1);
        let owner = "C_fed_refresh_all";
        super::ensure_session(owner, &[alpha, beta], Some("alpha")).expect("建会话");

        let results = super::refresh_all(owner).expect("刷新全部");
        assert_eq!(results.len(), 2, "{results:?}");
        for (alias, outcome) in &results {
            assert!(outcome.is_ok(), "{alias} 该重挂成功：{outcome:?}");
        }
        // 没会话的主人：如实报“先执行一次”
        let missing = super::refresh_all("C_fed_no_session").expect_err("该提醒");
        assert!(missing.contains("先执行一次"), "{missing}");
        super::drop_session(owner);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

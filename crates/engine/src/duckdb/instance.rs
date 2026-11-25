//! 分析实例 · 会话层。
//!
//! **形态**（2026-11-26 用户裁决，替代"多实例各一条连接"）：
//!
//! - 一个进程内**只有一个分析实例**：一条内存库（`open_in_memory`），底座
//!   `analytics` / `rds_global` 与源 `ATTACH` 别名都挂在它上面，**实例级共享**；
//! - 每个 SQL 编辑器 = 这个实例上的**一条会话**（`Connection::try_clone()`，实测可用）：
//!   catalog 看得到，`TEMP` 表**会话级不可见**（DuckDB 语义，不是我们的限制）；
//! - 会话之间不通信：A 里写的**持久**表，B 里刷新后才出现；A 里的 `TEMP` 表，B 永远看不到；
//! - 需要跨会话共享的临时结果放 `rds_tmp` 这一层 schema（实例级），由
//!   [`AnalysisInstance::register_temp`] 登记，**只登记不静默删**，清理走
//!   [`AnalysisInstance::drop_temp`]（面板给手动入口）。
//!
//! 为什么不是每个编辑器一条内存库：那样 `tmp_q_` 与联邦源根本不在一个实例里，
//! `tmp_q_ JOIN mysql_src` 会直接报 Catalog Error（这是第 ⑪ 条遗留的真因）。
//!
//! **保护**（共享一个实例的代价，必须有）：
//! 1. 写语句走单写者车道 [`AnalysisInstance::write_lane`]，读并发不排队；
//! 2. 每句都有超时，超了调**本会话**的 `InterruptHandle`（不打断别人的语句）；
//! 3. 内存上限由 `DuckDBManager::configure_connection` 统一设（默认 2GB / 环境变量可覆盖）；
//! 4. 任何一句话失败只影响本会话，实例不跟着倒（错误不跨会话传播）。
//!
//! 这里的 API 只做「实例 / 会话 / 车道 / 登记」四件事；挂载源的 SQL 仍由
//! `federation::session` 提供，避免两处各写一套 `ATTACH`。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use duckdb::{Connection, InterruptHandle};

use super::accel::run_sql_on;
use super::manager::DuckDBManager;
use shared::error::CoreError;

/// 实例级共享临时结果所在的 schema（跨会话可见，手动清理）。
pub const SHARED_TEMP_SCHEMA: &str = "rds_tmp";

/// 单句默认超时（毫秒）。超时只中断**这一条会话**上的语句。
pub const DEFAULT_STATEMENT_TIMEOUT_MS: u64 = 30_000;

/// 不能拿来当源别名的名字（挂了它就盖住底座 / 系统 catalog）。
pub const RESERVED_ALIASES: &[&str] = &[
    "memory",
    "system",
    "temp",
    "main",
    "analytics",
    "rds_global",
    "rds_tmp",
    "information_schema",
    "pg_catalog",
];

/// 一条会话的对外摘要（**纯内存读**，界面可直接拿它渲染）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub label: String,
    /// 归属：文档 id / owner 之类的调用方口径，引擎不解释它
    pub owner: String,
    pub created_at_ms: u64,
    pub last_used_ms: u64,
    pub queries: u64,
    pub open: bool,
}

/// `rds_tmp` 里一件被登记过的临时结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempEntry {
    /// `rds_tmp` 下的表名
    pub name: String,
    /// 谁建的（会话 id 或调用方口径）
    pub created_by: String,
    /// 一句话说明（面板提示用，例如「结果集 3 行 · 12 列」）
    pub note: String,
    pub created_at_ms: u64,
    /// 最后被读的时间（只用于展示"多久没用了"，**不作为自动删的依据**）
    pub last_read_ms: u64,
}

/// 别名登记：一个实例里别名必须唯一，冲突后到者让位（`mysql_src_2`）。
#[derive(Debug, Default, Clone)]
struct MountRegistry {
    /// 实际别名 → 源连接 id
    aliases: HashMap<String, String>,
}

impl MountRegistry {
    /// 申请一个别名：`wanted` 被占了就加 `_2`、`_3`…
    fn reserve(&mut self, wanted: &str, conn_id: &str) -> Result<String, String> {
        let base = wanted.trim();
        if base.is_empty() {
            return Err("源别名不能为空".to_string());
        }
        if RESERVED_ALIASES.iter().any(|reserved| *reserved == base) {
            return Err(format!(
                "别名 {base} 是底座 / 系统 catalog 的名字，换一个（建议用连接名）"
            ));
        }
        if let Some(existing) = self.aliases.get(base) {
            if existing == conn_id {
                return Ok(base.to_string());
            }
        }
        let mut candidate = base.to_string();
        let mut index = 2;
        while self.aliases.contains_key(&candidate) {
            candidate = format!("{base}_{index}");
            index += 1;
            if index > 99 {
                return Err(format!("别名 {base} 的冲突太多（>98 个），换个名字"));
            }
        }
        self.aliases.insert(candidate.clone(), conn_id.to_string());
        Ok(candidate)
    }

    fn release(&mut self, alias: &str) {
        self.aliases.remove(alias);
    }
}

struct InstanceState {
    sessions: HashMap<String, Arc<AnalysisSession>>,
    /// owner（文档 / 源连接 / 通道）→ 会话 id：同一 owner 再要会话时复用同一条，
    /// 这样"同一份文档两次执行"不会每次都换一条 `TEMP` 世界
    owners: HashMap<String, String>,
    /// 源连接 id → (实际挂载别名, 源的命名口径)。冲突时别名可能是 `name_2`；
    /// 界面要按这个查表，限定名要按这个 kind 拼。
    conn_alias: HashMap<String, (String, crate::duckdb::accel::AccelKind)>,
    mounts: MountRegistry,
    temps: Vec<TempEntry>,
    next_session: u64,
}

/// 分析实例：进程内一份，持有控制连接与全部会话。
pub struct AnalysisInstance {
    /// 控制连接：建会话、挂载底座、`rds_tmp` 维护。**不承载用户查询**。
    control: Mutex<Connection>,
    write_lane: Mutex<()>,
    state: Mutex<InstanceState>,
    timeout: Duration,
    /// 内存库统一前缀，测试用它区分不同实例（生产恒为 `analysis`）
    tag: String,
}

impl std::fmt::Debug for AnalysisInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("AnalysisInstance")
            .field("tag", &self.tag)
            .field("sessions", &state.as_ref().map(|s| s.sessions.len()).unwrap_or(0))
            .field("temps", &state.as_ref().map(|s| s.temps.len()).unwrap_or(0))
            .finish()
    }
}

/// 一条会话：`try_clone` 出来的第二连接，catalog 共享、`TEMP` 独占。
pub struct AnalysisSession {
    id: String,
    label: String,
    owner: String,
    conn: Arc<Mutex<Connection>>,
    interrupt: Arc<InterruptHandle>,
    running: AtomicBool,
    closed: AtomicBool,
    created_at_ms: AtomicU64,
    last_used_ms: AtomicU64,
    queries: AtomicU64,
    /// 本会话最近一次挂载的结果（逐源状态：`Ready` / `Failed` 的原话都在这里）。
    ///
    /// 为什么要存：`ATTACH` 出去以后，某条源为什么不可用只有挂载那一步知道；
    /// 面板 / 全局库核对想报原话时，不该再挂一遍去找原因。
    mounted: Mutex<Vec<super::federation::registry::MountedSource>>,
    /// 弱引用：实例持有会话，会话反向取车道 / 登记册，不能成环
    instance: Weak<AnalysisInstance>,
}

impl std::fmt::Debug for AnalysisSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalysisSession")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("owner", &self.owner)
            .field("open", &!self.closed.load(Ordering::SeqCst))
            .finish()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// 某个别名是否已经挂在这个实例上（`ATTACH` 是实例级事实，查 `duckdb_databases()`）。
fn control_has_database(conn: &Connection, alias: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM duckdb_databases() WHERE database_name = ?",
        [alias],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

static INSTANCE: OnceLock<Mutex<Option<Arc<AnalysisInstance>>>> = OnceLock::new();

fn instance_slot() -> &'static Mutex<Option<Arc<AnalysisInstance>>> {
    INSTANCE.get_or_init(|| Mutex::new(None))
}

/// 取（必要时建立）进程内分析实例。
///
/// **会做 I/O**（首次建库、挂底座），属事件路径；渲染路径用
/// [`crate::duckdb::instance::current`] 取已有实例（不建）。
///
/// 槽里放的是 **强引用**：会话只持有 `Weak`，若这里也放 `Weak`，最后一个调用方
/// 一放手实例就没了、所有会话跟着失效（"进程单例"必须活到 `drop_instance`）。
pub fn instance() -> Result<Arc<AnalysisInstance>, String> {
    let slot = instance_slot();
    let mut guard = slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(instance) = guard.as_ref() {
        return Ok(instance.clone());
    }
    let instance = AnalysisInstance::open()?;
    *guard = Some(instance.clone());
    Ok(instance)
}

/// 已经建好的实例（**不做 I/O**；没有就是 `None`）。
pub fn current() -> Option<Arc<AnalysisInstance>> {
    instance_slot()
        .lock()
        .ok()?
        .as_ref()
        .map(|instance| instance.clone())
}

/// 放掉全局实例（测试 / 切项目重建时用：会话全关、连接释放）。
pub fn drop_instance() -> bool {
    let dropped = instance_slot()
        .lock()
        .map(|mut slot| slot.take().is_some())
        .unwrap_or(false);
    dropped
}

impl AnalysisInstance {
    /// 开实例（**只由 [`instance`] 与测试调用**）：内存库 + 产品纪律配置 + `rds_tmp`。
    pub fn open() -> Result<Arc<Self>, String> {
        Self::open_tagged("analysis", Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MS))
    }

    /// 测试用：不登记全局，超时与 tag 可指定（避免用例之间互相踩 `rds_tmp`）。
    pub fn open_tagged(tag: &str, timeout: Duration) -> Result<Arc<Self>, String> {
        let conn = Connection::open_in_memory()
            .map_err(|error| format!("开分析实例失败：{error}"))?;
        DuckDBManager::configure_connection(&conn)
            .map_err(|error| format!("配置分析实例失败：{error}"))?;
        conn.execute_batch(&format!("CREATE SCHEMA IF NOT EXISTS {SHARED_TEMP_SCHEMA}"))
            .map_err(|error| format!("建 {SHARED_TEMP_SCHEMA} 失败：{error}"))?;
        // 底座在**实例**上挂（`analytics` 读写 / `rds_global` 只读）：`ATTACH` 是实例级事实，
        // 挂一次所有会话都看得见；挂不上只让那一格缺席，不把实例带下去
        for (alias, outcome) in super::federation::session::attach_bases(&conn) {
            if let Err(reason) = outcome {
                tracing::warn!(alias = %alias, reason = %reason, "底座没挂上（这一格本轮不可用）");
            }
        }
        Ok(Arc::new(Self {
            control: Mutex::new(conn),
            write_lane: Mutex::new(()),
            state: Mutex::new(InstanceState {
                sessions: HashMap::new(),
                owners: HashMap::new(),
                conn_alias: HashMap::new(),
                mounts: MountRegistry::default(),
                temps: Vec::new(),
                next_session: 0,
            }),
            timeout,
            tag: tag.to_string(),
        }))
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 在控制连接上执行一句（底座挂载 / `rds_tmp` 维护用；不承载用户查询）。
    pub fn run_control(&self, sql: &str) -> Result<(), String> {
        let conn = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        conn.execute_batch(sql)
            .map_err(|error| format!("实例控制语句失败：{error}"))
    }

    /// 换项目时重挂底座：**先摘旧再挂新**，返回逐底座结果。
    ///
    /// 为什么必须摘：`analytics` 指向的是"当前项目"的分析库文件，切项目后旧的那份如果还挂着，
    /// 用户会对着上个项目的表查数据（比"重建慢一点"严重得多）。
    /// 本方法是**事件路径**（有 I/O）；纯内存读看 [`Self::sessions`] / [`Self::temps`]。
    pub fn refresh_bases(&self) -> Vec<(String, Result<(), String>)> {
        let conn = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bases = super::federation::session::bases();
        for base in &bases {
            if control_has_database(&conn, &base.alias) {
                // `DETACH` 之前先回默认 catalog：当前默认就是它时 DuckDB 拒绝摘
                let _ = conn.execute_batch(&format!("USE memory; DETACH {}", base.alias));
            }
        }
        super::federation::session::attach_bases(&conn)
    }

    /// 开一条会话：`try_clone` 出第二连接（catalog 共享、`TEMP` 独立）。
    ///
    /// `self: &Arc<Self>`：会话要反向拿实例（写车道 / 登记册），用弱引用避免成环。
    pub fn open_session(
        self: &Arc<Self>,
        owner: &str,
        label: &str,
    ) -> Result<Arc<AnalysisSession>, String> {
        if owner.trim().is_empty() {
            return Err("会话必须有归属（owner 不能为空）".to_string());
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.next_session += 1;
        let id = format!("s{}", state.next_session);
        let conn = {
            let control = self
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            control.try_clone().map_err(|error| {
                format!("分析实例不支持开第二条会话（try_clone 失败）：{error}")
            })?
        };
        let interrupt = conn.interrupt_handle();
        let stamp = now_ms();
        let session = Arc::new(AnalysisSession {
            id: id.clone(),
            label: label.to_string(),
            owner: owner.to_string(),
            conn: Arc::new(Mutex::new(conn)),
            interrupt,
            running: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            created_at_ms: AtomicU64::new(stamp),
            last_used_ms: AtomicU64::new(stamp),
            queries: AtomicU64::new(0),
            mounted: Mutex::new(Vec::new()),
            instance: Arc::downgrade(self),
        });
        state.sessions.insert(id.clone(), session.clone());
        tracing::info!(session = %id, owner = %owner, label = %label, tag = %self.tag, "分析会话已开");
        Ok(session)
    }

    /// 取一条已开的会话（不做 I/O）。
    pub fn session(&self, id: &str) -> Option<Arc<AnalysisSession>> {
        self.state.lock().ok()?.sessions.get(id).cloned()
    }

    /// 按 owner 取（必要时建立）会话：同一 owner 复用同一条，`TEMP` 世界不来回换。
    ///
    /// 典型 owner：文档 id（编辑器联邦 / 加速档用 `accel:<源连接 id>`）、`process`。
    /// 会话被关掉后再要，会开一条**新的**（旧会话的 `TEMP` 已随关闭消失，这是正确语义）。
    pub fn ensure_session(
        self: &Arc<Self>,
        owner: &str,
        label: &str,
    ) -> Result<Arc<AnalysisSession>, String> {
        if owner.trim().is_empty() {
            return Err("会话必须有归属（owner 不能为空）".to_string());
        }
        {
            let state = self
                .state
                .lock()
                .map_err(|_| "实例状态锁被污染".to_string())?;
            if let Some(id) = state.owners.get(owner) {
                if let Some(session) = state.sessions.get(id) {
                    if session.is_open() {
                        return Ok(session.clone());
                    }
                }
            }
        }
        let session = self.open_session(owner, label)?;
        if let Ok(mut state) = self.state.lock() {
            state.owners.insert(owner.to_string(), session.id().to_string());
        }
        Ok(session)
    }

    /// 所有会话摘要（**渲染路径可调**）。
    pub fn sessions(&self) -> Vec<SessionInfo> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        state.sessions.values().map(|s| s.info()).collect()
    }

    /// 关一条会话（再开就是新 id：`TEMP` 表随之消失，这是正确语义）。
    pub fn close_session(&self, id: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(session) = state.sessions.remove(id) else {
            return false;
        };
        // owner 映射要跟着销：否则下次 `ensure_session` 会拿到一条已关的会话再开新的却记旧 id
        let owners: Vec<String> = state
            .owners
            .iter()
            .filter(|(_, session_id)| session_id.as_str() == id)
            .map(|(owner, _)| owner.clone())
            .collect();
        for owner in owners {
            state.owners.remove(&owner);
        }
        session.close();
        true
    }

    /// 关全部会话（切项目 / 退出前）。`rds_tmp` 里的东西**不跟着消失**，仍可手动清理。
    pub fn close_all_sessions(&self) -> usize {
        let Ok(mut state) = self.state.lock() else {
            return 0;
        };
        let sessions: Vec<_> = state.sessions.drain().map(|(_, s)| s).collect();
        state.owners.clear();
        for session in &sessions {
            session.close();
        }
        sessions.len()
    }

    /// 为源连接申请实例级别名（唯一性由实例单点决定，后到者让位）。
    ///
    /// `kind` 一起记下来：面板要给出的限定名取决于源的命名口径（两段 / 三段），
    /// 而那个 kind 只有挂载方（内核）知道，事后没人能反推。
    pub fn reserve_alias(
        &self,
        wanted: &str,
        conn_id: &str,
        kind: crate::duckdb::accel::AccelKind,
    ) -> Result<String, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "实例状态锁被污染".to_string())?;
        // 同一个源重复挂 → 回原别名（幂等；`rds_src` 这类固定别名尤其要紧）
        if let Some((alias, _)) = state.conn_alias.get(conn_id) {
            return Ok(alias.clone());
        }
        let alias = state.mounts.reserve(wanted, conn_id)?;
        state
            .conn_alias
            .insert(conn_id.to_string(), (alias.clone(), kind));
        // 并发下可能刚有人用同一别名挂了 ATTACH：这里只登记不重复挂，
        // 真挂载由 federation::session / 调用方负责（按别名判重）。
        Ok(alias)
    }

    /// **进程面**会话（owner `process`）：导出、结果集临时表、洞察取样都落在这条上。
    ///
    /// 与文档会话共享 catalog（底座 + 源别名），`TEMP` / 临时表也彼此可见——这正是
    /// "取样的 `tmp_i_` 与源、与 `tmp_q_` 能一起 JOIN"的前提。
    pub fn process_session(self: &Arc<Self>) -> Result<Arc<AnalysisSession>, String> {
        self.ensure_session("process", "process")
    }

    /// 本地加速档的 owner 键（内核与工作台共用一份，别各自拼字符串）。
    pub fn accel_owner(conn_id: &str) -> String {
        format!("accel:{conn_id}")
    }

    /// 某源连接此刻的会话（没有就开一条）。
    ///
    /// 用途：载入 / 归档这类「在那个源上再跑一条 SQL」的活。catalog 是实例级共享的，
    /// 新会话照样看得见已挂载的源；**这里不负责挂载**——挂载是实例级事实，
    /// 没挂过先用 [`Self::alias_for_conn`] 判断，别在取会话时悄悄挂上去。
    pub fn session_for_conn(
        self: &Arc<Self>,
        conn_id: &str,
    ) -> Result<Arc<AnalysisSession>, String> {
        self.ensure_session(&Self::accel_owner(conn_id), "本地加速")
    }

    /// 中断某个源连接上的会话（界面「取消」按的是源，不是某条会话 id）。
    pub fn interrupt_conn(&self, conn_id: &str) -> Result<bool, String> {
        let session = {
            let state = self
                .state
                .lock()
                .map_err(|_| "实例状态锁被污染".to_string())?;
            let id = state
                .owners
                .get(&Self::accel_owner(conn_id))
                .ok_or_else(|| "这个源还没有会话（没跑过加速档查询）".to_string())?;
            state
                .sessions
                .get(id)
                .cloned()
                .ok_or_else(|| "这个源还没有会话（没跑过加速档查询）".to_string())?
        };
        Ok(session.interrupt())
    }

    /// 此刻挂在实例上的所有源连接：`(conn_id, 实际别名, 源的命名口径)`。
    ///
    /// 面板的「会话源」树根按它渲染——树根必须与 `duckdb_databases()` 里的 catalog 名
    /// 一致，所以给的是**实际别名**（冲突让位后的 `rds_src_2` 也算）。
    pub fn mounted_conns(&self) -> Vec<(String, String, crate::duckdb::accel::AccelKind)> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        state
            .conn_alias
            .iter()
            .map(|(conn_id, (alias, kind))| (conn_id.clone(), alias.clone(), kind.clone()))
            .collect()
    }

    /// 卸下某个源连接的挂载（面板右键「卸载」）：`DETACH` + 放别名 + 关它在实例上的会话。
    ///
    /// 只断挂载，不动连接本身；返回 `false` = 这个源本来就没挂（调用方据此提示）。
    pub fn unmount_conn(&self, conn_id: &str) -> Result<bool, String> {
        let Some(alias) = self.alias_for_conn(conn_id) else {
            return Ok(false);
        };
        let valid = !alias.is_empty()
            && alias
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        if !valid {
            return Err(format!("别名 {alias:?} 里有不允许的字符，拒绝拿去拼 SQL"));
        }
        {
            let conn = self
                .control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // 控制连接可能停在别的 catalog 上；`DETACH` 前先回 memory，
            // 否则"正被使用"会让卸下失败
            let _ = conn.execute_batch("USE memory");
            conn.execute_batch(&format!("DETACH {alias}"))
                .map_err(|error| format!("卸下 {alias} 失败：{error}"))?;
        }
        self.release_alias(&alias);
        // 会话一并关掉：源都不在了，留着它只会让面板看到一条空会话
        let session = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "实例状态锁被污染".to_string())?;
            let owner = Self::accel_owner(conn_id);
            state
                .owners
                .remove(&owner)
                .and_then(|id| state.sessions.remove(&id))
        };
        if let Some(session) = session {
            session.close();
        }
        Ok(true)
    }

    /// 某个源连接此刻挂在实例上的别名（**纯内存读**；没挂过就是 `None`）。
    ///
    /// 数据对象面板按它查表清单：挂载是实例级事实，不该再由"那个源自己有没有会话"回答。
    pub fn alias_for_conn(&self, conn_id: &str) -> Option<String> {
        self.state
            .lock()
            .ok()?
            .conn_alias
            .get(conn_id)
            .map(|(alias, _)| alias.clone())
    }

    /// 列出某个源连接挂载下的表 / 视图（**事件路径**：要查 DuckDB 元数据）。
    ///
    /// 用控制连接查——它不属于任何会话，不受某条会话的 `USE` / `TEMP` 影响；
    /// 挂载是实例级事实，所以这里看到的正是"实例此刻能查到什么"。
    /// 限定名按挂载时登记的 kind 算（两段 / 三段），与面板的「复制」一致。
    pub fn list_mounted_tables_for_conn(
        &self,
        conn_id: &str,
    ) -> Result<Vec<crate::duckdb::accel::MountedTable>, String> {
        let (alias, kind) = {
            let state = self
                .state
                .lock()
                .map_err(|_| "实例状态锁被污染".to_string())?;
            state
                .conn_alias
                .get(conn_id)
                .cloned()
                .ok_or_else(|| "这个源还没挂进分析实例".to_string())?
        };
        let conn = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::duckdb::accel::list_mounted_tables(&conn, &alias, kind)
    }

    /// 列出某个别名下的表 / 视图名（**事件路径**：要查 DuckDB 元数据）。
    ///
    /// 只给名字、不拼限定名：限定名的口径随源 kind 变，那种事归
    /// [`Self::list_mounted_tables_for_conn`]。这里留给「只想知道有没有表」的场景。
    pub fn list_tables_of_alias(&self, alias: &str) -> Result<Vec<String>, String> {
        let conn = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut statement = conn
            .prepare(
                "SELECT table_name FROM duckdb_tables() WHERE database_name = ? \
                 UNION ALL SELECT view_name FROM duckdb_views() WHERE database_name = ?",
            )
            .map_err(|error| format!("列不出 {alias} 的表：{error}"))?;
        let rows = statement
            .query_map([alias, alias], |row| row.get::<_, String>(0))
            .map_err(|error| format!("列不出 {alias} 的表：{error}"))?;
        let mut tables: Vec<String> = rows.filter_map(Result::ok).collect();
        tables.sort();
        Ok(tables)
    }

    /// 放掉别名（卸载源时调）。
    pub fn release_alias(&self, alias: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.mounts.release(alias);
            state.conn_alias.retain(|_, (value, _)| value != alias);
        }
    }

    /// 登记一件 `rds_tmp` 临时结果（**只登记，不删**）。
    pub fn register_temp(&self, name: &str, created_by: &str, note: &str) -> TempEntry {
        let stamp = now_ms();
        let entry = TempEntry {
            name: name.to_string(),
            created_by: created_by.to_string(),
            note: note.to_string(),
            created_at_ms: stamp,
            last_read_ms: stamp,
        };
        if let Ok(mut state) = self.state.lock() {
            state.temps.retain(|existing| existing.name != entry.name);
            state.temps.push(entry.clone());
        }
        entry
    }

    /// 已登记的临时结果（渲染路径可调；按创建时间倒序）。
    pub fn temps(&self) -> Vec<TempEntry> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        let mut temps = state.temps.clone();
        temps.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
        temps
    }

    /// 手动清理一件（用户点"清理"才走这里，**绝不自动删**）。
    ///
    /// 先删表再销登记：表已经不在也把登记销掉（返回 `true` 表示登记被清掉）。
    pub fn drop_temp(&self, name: &str) -> Result<bool, String> {
        if name.trim().is_empty() {
            return Err("临时结果名不能为空".to_string());
        }
        let quoted = name.replace('"', "\"\"");
        self.run_control(&format!(
            "DROP TABLE IF EXISTS {SHARED_TEMP_SCHEMA}.\"{quoted}\""
        ))?;
        let removed = self
            .state
            .lock()
            .map(|mut state| {
                let before = state.temps.len();
                state.temps.retain(|entry| entry.name != name);
                before != state.temps.len()
            })
            .unwrap_or(false);
        Ok(removed)
    }

    /// 清空 `rds_tmp` 里**所有已登记**的临时结果（用户点"全部清理"）。
    pub fn drop_all_temps(&self) -> Result<usize, String> {
        let names: Vec<String> = self.temps().into_iter().map(|entry| entry.name).collect();
        let mut dropped = 0;
        for name in names {
            if self.drop_temp(&name).unwrap_or(false) {
                dropped += 1;
            }
        }
        Ok(dropped)
    }

    /// 写语句的车道：拿它的语句之间串行，读语句不拿（并发不被写阻塞）。
    ///
    /// 锁被前一句的 panic 污染时**恢复它**（`into_inner`），别让整个实例从此不能写。
    fn write_lane(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write_lane
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl AnalysisSession {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn is_open(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }

    /// 底层连接的**共享句柄**（`Arc<Mutex<Connection>>`）。
    ///
    /// 给"进程单例"这类历史入口用：它们要的是 `lock()` 之后直接 `prepare` / `execute`，
    /// 不想改签名。注意走这里**绕开**写车道与超时——只给内部短语句（CTAS 临时表、TTL 清理）用；
    /// 用户 SQL 一律走 [`Self::run`]。
    pub fn connection_handle(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            label: self.label.clone(),
            owner: self.owner.clone(),
            created_at_ms: self.created_at_ms.load(Ordering::SeqCst),
            last_used_ms: self.last_used_ms.load(Ordering::SeqCst),
            queries: self.queries.load(Ordering::SeqCst),
            open: self.is_open(),
        }
    }

    /// 在**本条会话**上执行一句：写语句走实例写车道，超时只中断本会话。
    pub fn run(&self, sql: &str) -> Result<shared::models::QueryResult, CoreError> {
        let timeout = self
            .instance
            .upgrade()
            .map(|instance| instance.timeout())
            .unwrap_or(Duration::from_millis(DEFAULT_STATEMENT_TIMEOUT_MS));
        self.run_with_timeout(sql, timeout)
    }

    /// 带超时的执行（超时 = 中断本会话的语句，返回一句可读错误）。
    ///
    /// 读语句不进写车道：共享实例的好处是读能并发，别被写排队拖住。
    pub fn run_with_timeout(
        &self,
        sql: &str,
        timeout: Duration,
    ) -> Result<shared::models::QueryResult, CoreError> {
        if !self.is_open() {
            return Err(common_error("这条分析会话已经关了，重新开一条再执行"));
        }
        let is_write = !crate::driver::utils::returns_rows(sql);
        // `upgrade` 出来的 Arc 必须活在 guard 之前：guard 借着它（只在 Arc 存活期间有效）
        let instance = self.instance.upgrade();
        let _lane = if is_write {
            instance.as_ref().map(|instance| instance.write_lane())
        } else {
            None
        };

        let done = Arc::new(AtomicBool::new(false));
        let timed_out = Arc::new(AtomicBool::new(false));
        let watchdog = spawn_watchdog(
            timeout,
            done.clone(),
            timed_out.clone(),
            self.interrupt.clone(),
        );

        let outcome = {
            let conn = self
                .conn
                .lock()
                .map_err(|_| common_error("会话连接锁被污染（上一条语句崩了），重开一条会话"))?;
            run_sql_on(&conn, &self.running, sql)
        };

        done.store(true, Ordering::SeqCst);
        if let Some(handle) = watchdog {
            let _ = handle.join();
        }

        self.queries.fetch_add(1, Ordering::SeqCst);
        self.last_used_ms.store(now_ms(), Ordering::SeqCst);

        match outcome {
            Ok(result) => Ok(result),
            Err(error) => {
                if timed_out.load(Ordering::SeqCst) {
                    return Err(common_error(format!(
                        "语句超过 {} ms 被中断（本会话已恢复，可继续执行）",
                        timeout.as_millis()
                    )));
                }
                Err(error)
            }
        }
    }

    /// 中断本会话正在跑的语句（**只影响自己**）。返回"有没有语句可中断"。
    pub fn interrupt(&self) -> bool {
        if !self.running.load(Ordering::SeqCst) {
            return false;
        }
        self.interrupt.interrupt();
        true
    }

    /// 把一批源挂进**本会话**（`ATTACH` 出的 catalog 是实例级的，所有会话都能看见）。
    ///
    /// 为什么挂在会话连接上、不挂控制连接：L2（Oracle）的凭据是**会话级** `SECRET`，
    /// 与 `ATTACH` 必须落在同一个会话；挂在控制连接上，查询方会话里没有那个 Secret。
    ///
    /// 挂载实现只有一处（`federation::session::mount_into`），这里不另写一套。
    /// 单个源挂不上不抛（状态即结论）；只有「别名重复 / 扩展装不上」这类整批失败才返回 `Err`。
    pub fn mount_sources(
        &self,
        sources: &[super::federation::registry::FederatedSource],
    ) -> Result<Vec<super::federation::registry::MountedSource>, String> {
        if !self.is_open() {
            return Err("这条分析会话已经关了，重新开一条再挂源".to_string());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| "会话连接锁被污染（上一条语句崩了），重开一条会话".to_string())?;
        let mounted = super::federation::session::mount_into(&conn, sources)?;
        if let Ok(mut stored) = self.mounted.lock() {
            for entry in &mounted {
                stored.retain(|existing| existing.alias() != entry.alias());
                stored.push(entry.clone());
            }
        }
        Ok(mounted)
    }

    /// 本会话最近一次挂载的逐源状态（纯内存读，渲染路径可调）。
    pub fn mounted_sources(&self) -> Vec<super::federation::registry::MountedSource> {
        self.mounted
            .lock()
            .map(|stored| stored.clone())
            .unwrap_or_default()
    }

    /// 借本会话的连接跑一小段逻辑（**不接管执行**：只给需要直接 `execute_batch` 的调用方，
    /// 例如 `DETACH` / `CREATE VIEW` 这类必须拿到原始连接的活）。
    ///
    /// 走这里**不经过**写车道与超时（调用方自己是事件路径、语句都很短）；要执行用户 SQL
    /// 一律用 [`Self::run`] / [`Self::run_with_timeout`]。
    pub fn with_connection<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        if !self.is_open() {
            return Err("这条分析会话已经关了，重新开一条再执行".to_string());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| "会话连接锁被污染（上一条语句崩了），重开一条会话".to_string())?;
        f(&conn)
    }

    /// 切本会话的默认 catalog（联邦主源用）。
    ///
    /// 只接受标识符字符：别名来自 `MountRegistry` 与驱动的校验，但这里是拼 SQL 的地方，
    /// 宁可多一条白名单检查，也不让一个精心构造的别名变成第二条语句。
    pub fn use_catalog(&self, alias: &str) -> Result<(), String> {
        let valid = !alias.is_empty()
            && alias
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        if !valid {
            return Err(format!("别名 {alias:?} 里有不允许的字符，拒绝拿去拼 SQL"));
        }
        self.run_with_timeout(&format!("USE {alias}"), Duration::from_secs(5))
            .map(|_| ())
            .map_err(|error| format!("切主源到 {alias} 失败：{error}"))
    }

    fn close(&self) {
        // 先中断：关会话时正在跑的语句不该把界面卡住
        self.interrupt();
        self.closed.store(true, Ordering::SeqCst);
    }
}

fn spawn_watchdog(
    timeout: Duration,
    done: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
    interrupt: Arc<InterruptHandle>,
) -> Option<thread::JoinHandle<()>> {
    if timeout.is_zero() || timeout == Duration::from_secs(u64::MAX / 1000) {
        return None;
    }
    let started = Instant::now();
    Some(thread::spawn(move || loop {
        if done.load(Ordering::SeqCst) {
            return;
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            timed_out.store(true, Ordering::SeqCst);
            interrupt.interrupt();
            return;
        }
        let remaining = timeout - elapsed;
        thread::sleep(remaining.min(Duration::from_millis(20)));
    }))
}

fn common_error(message: impl Into<String>) -> CoreError {
    CoreError::common(shared::error::CommonError::General(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(tag: &str) -> Arc<AnalysisInstance> {
        AnalysisInstance::open_tagged(tag, Duration::from_millis(3000)).expect("开实例")
    }

    /// 会话 A 在 `main` 里建的**持久**表，会话 B 立刻看得见（catalog 实例级共享）。
    #[test]
    fn persistent_tables_are_shared_across_sessions() {
        let instance = instance("t_share");
        let left = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        let right = instance.open_session("doc-b", "编辑器 B").expect("开 B");

        left.run_with_timeout(
            "CREATE TABLE shared_orders(id INTEGER, amount INTEGER)",
            Duration::from_secs(3),
        )
        .expect("建表");
        left.run_with_timeout(
            "INSERT INTO shared_orders VALUES (1, 10), (2, 20)",
            Duration::from_secs(3),
        )
        .expect("插行");

        let result = right
            .run_with_timeout("SELECT id, amount FROM shared_orders ORDER BY id", Duration::from_secs(3))
            .expect("B 读 A 建的表");
        assert_eq!(result.total_rows, 2, "catalog 共享：B 应看到 A 的持久表");

        instance.close_all_sessions();
    }

    /// `TEMP` 表**会话级**：这是 DuckDB 语义，也正是"一个编辑器一条会话"的边界。
    #[test]
    fn temp_tables_stay_inside_one_session() {
        let instance = instance("t_temp");
        let left = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        let right = instance.open_session("doc-b", "编辑器 B").expect("开 B");

        left.run_with_timeout(
            "CREATE TEMP TABLE scratch_only_here AS SELECT 42 AS answer",
            Duration::from_secs(3),
        )
        .expect("建 TEMP");
        left.run_with_timeout("SELECT * FROM scratch_only_here", Duration::from_secs(3))
            .expect("自己看得见");

        let other = right.run_with_timeout("SELECT * FROM scratch_only_here", Duration::from_secs(3));
        let message = other.expect_err("另一个会话不该看到 TEMP 表").to_string();
        assert!(
            message.contains("scratch_only_here") || message.to_lowercase().contains("does not exist"),
            "应为 Catalog Error，实际：{message}"
        );
    }

    /// `rds_tmp` 是实例级共享层：跨会话可见，清理只在手动调用时发生。
    #[test]
    fn shared_temp_schema_is_visible_and_only_cleaned_manually() {
        let instance = instance("t_tmp");
        let left = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        let right = instance.open_session("doc-b", "编辑器 B").expect("开 B");

        left.run_with_timeout(
            "CREATE TABLE rds_tmp.result_set_1 AS SELECT * FROM range(3)",
            Duration::from_secs(3),
        )
        .expect("写共享临时");
        instance.register_temp("result_set_1", left.id(), "结果集 1 · 3 行");

        let seen = right
            .run_with_timeout("SELECT * FROM rds_tmp.result_set_1", Duration::from_secs(3))
            .expect("B 读共享临时");
        assert_eq!(seen.total_rows, 3, "rds_tmp 跨会话可见");

        assert_eq!(instance.temps().len(), 1, "只登记一件");
        assert!(instance.drop_temp("result_set_1").expect("清理"), "手动清理应成功");
        assert!(instance.temps().is_empty(), "登记应销掉");
        assert!(
            right
                .run_with_timeout("SELECT count(*) FROM rds_tmp.result_set_1", Duration::from_secs(3))
                .is_err(),
            "清理后两个会话都读不到"
        );
    }

    /// 关会话 = TEMP 跟着消失（再开新会话是新世界），别的会话不受影响。
    #[test]
    fn closing_a_session_drops_its_temp_tables_and_spares_others() {
        let instance = instance("t_close");
        let survivor = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        let doomed = instance.open_session("doc-b", "编辑器 B").expect("开 B");

        survivor
            .run_with_timeout("CREATE TEMP TABLE keep_me AS SELECT 1 AS v", Duration::from_secs(3))
            .expect("建 TEMP");
        doomed
            .run_with_timeout("CREATE TEMP TABLE gone_soon AS SELECT 1 AS v", Duration::from_secs(3))
            .expect("建 TEMP");

        assert!(instance.close_session(doomed.id()), "关 B");
        assert_eq!(instance.sessions().len(), 1, "只剩 A");
        assert!(instance.session(doomed.id()).is_none(), "B 取不到");
        survivor
            .run_with_timeout("SELECT * FROM keep_me", Duration::from_secs(3))
            .expect("A 的 TEMP 不跟着消失");
    }

    /// 别名冲突后到者让位，且底座名字不许被占；连接 → 别名映射要跟着走。
    #[test]
    fn alias_conflicts_are_resolved_by_the_instance_single_point() {
        let instance = instance("t_alias");
        use crate::duckdb::accel::AccelKind;
        assert_eq!(
            instance
                .reserve_alias("mysql_src", "conn-1", AccelKind::MySql)
                .expect("首个"),
            "mysql_src"
        );
        assert_eq!(
            instance
                .reserve_alias("mysql_src", "conn-2", AccelKind::MySql)
                .expect("第二个"),
            "mysql_src_2"
        );
        assert_eq!(
            instance
                .reserve_alias("mysql_src", "conn-1", AccelKind::MySql)
                .expect("同一源重挂"),
            "mysql_src"
        );
        assert!(
            instance
                .reserve_alias("analytics", "conn-3", AccelKind::DuckDb)
                .is_err(),
            "底座名要拒"
        );

        // 面板按连接 id 查挂载事实：映射必须与 reserve 的结果一致
        assert_eq!(instance.alias_for_conn("conn-1").as_deref(), Some("mysql_src"));
        assert_eq!(instance.alias_for_conn("conn-2").as_deref(), Some("mysql_src_2"));
        assert_eq!(instance.alias_for_conn("conn-9"), None);

        instance.release_alias("mysql_src");
        assert_eq!(instance.alias_for_conn("conn-1"), None, "放掉别名要同时清映射");
        assert_eq!(
            instance
                .reserve_alias("mysql_src", "conn-4", AccelKind::MySql)
                .expect("放掉后可用"),
            "mysql_src"
        );
    }

    /// 超时会中断本会话的语句并给出可读错误，之后这条会话还能继续用。
    #[test]
    fn timeout_interrupts_the_statement_then_session_stays_usable() {
        let instance = instance("t_timeout");
        let session = instance.open_session("doc-a", "编辑器 A").expect("开 A");

        // 用一句**真的跑不完**的连表聚合（`range(1e5) × range(1e5)` = 10^10 行）
        let outcome = session.run_with_timeout(
            "SELECT count(*) FROM range(0, 100000) a(x), range(0, 100000) b(y)",
            Duration::from_millis(30),
        );
        let error = outcome.expect_err("应超时中断").to_string();
        assert!(error.contains("被中断"), "错误应说明中断，实际：{error}");

        let after = session
            .run_with_timeout("SELECT 1 AS ok", Duration::from_secs(3))
            .expect("中断后仍可用");
        assert_eq!(after.total_rows, 1);
    }

    /// 挂源入口：整批失败（别名重复）不留半挂状态，会话照常可用。
    #[test]
    fn mounting_rejects_duplicate_aliases_and_leaves_the_session_usable() {
        use super::super::federation::registry::FederatedSource;
        let instance = instance("t_mount");
        let session = instance.open_session("doc-a", "编辑器 A").expect("开 A");

        let make = |alias: &str| {
            FederatedSource::new("conn-1", alias, "sqlite", "C:/data/probe.db")
                .expect("造源（不连库，只验别名判定）")
        };
        let outcome = session.mount_sources(&[make("dup"), make("dup")]);
        let error = outcome.expect_err("别名重复应整批拒").to_string();
        assert!(error.contains("出现了两次"), "错误要说清重复，实际：{error}");

        session
            .run_with_timeout("SELECT 1 AS ok", Duration::from_secs(3))
            .expect("拒绝后会话仍可用");
    }

    /// 写语句走车道、读语句不排队：两句写串行执行（不会互相踩连接）。
    #[test]
    fn writes_are_serialized_on_the_lane_while_reads_run_on_the_session() {
        let instance = instance("t_lane");
        let session = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        session
            .run_with_timeout("CREATE TABLE lane_orders(id INTEGER)", Duration::from_secs(3))
            .expect("建表");

        let left = Arc::clone(&session);
        let right = Arc::clone(&session);
        let first = thread::spawn(move || {
            left.run_with_timeout("INSERT INTO lane_orders VALUES (1)", Duration::from_secs(3))
        });
        let second = thread::spawn(move || {
            right.run_with_timeout("INSERT INTO lane_orders VALUES (2)", Duration::from_secs(3))
        });
        first.join().expect("线程").expect("第一句写");
        second.join().expect("线程").expect("第二句写");

        let count = session
            .run_with_timeout("SELECT * FROM lane_orders", Duration::from_secs(3))
            .expect("读");
        assert_eq!(count.total_rows, 2);
    }
}

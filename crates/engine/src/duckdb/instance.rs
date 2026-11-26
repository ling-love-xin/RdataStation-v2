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
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use duckdb::{Connection, InterruptHandle};

use super::accel::run_sql_on;
use super::manager::DuckDBManager;
use super::temp_table::temp_schema::{ensure_shared_schema, SHARED_MAIN_PATH, SHARED_TEMP_PATH};
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

    /// 这个别名此刻归谁（纯内存读；没人占就是 `None`）
    fn owner_of(&self, alias: &str) -> Option<&String> {
        self.aliases.get(alias)
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
    /// 被用户"卸掉"的底座别名（**只是遮蔽，不 DETACH**）。
    ///
    /// `ATTACH` 是实例级事实，摘掉会让别的会话当场报 catalog 不存在；所以"卸载底座"
    /// 落成这张名单：面板不再列它、以及它里面的表。边界要说清——**显式限定名
    /// `analytics.x` 仍可达**（会话级遮蔽挡不住裸限定名，DuckDB 侧也没有这个开关）。
    hidden_bases: Vec<String>,
    next_session: u64,
}

/// 分析实例：进程内一份，持有控制连接与全部会话。
pub struct AnalysisInstance {
    /// 控制连接：建会话、挂载底座、`rds_tmp` 维护。**不承载用户查询**。
    control: Mutex<Connection>,
    write_lane: Mutex<()>,
    state: Mutex<InstanceState>,
    /// 每句默认超时（毫秒；**0 = 不限时**）。可运行时改（设置页「语句超时」→ 这里），
    /// 所以是原子量而不是普通字段：会话 `run` 每次都现读。
    timeout_ms: AtomicU64,
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
    /// 最近一次 `USE` 切过去的主源别名（纯展示：面板要在源行上区分「主源 / 已挂载」）。
    ///
    /// 存这里而不是每次现查 `current_database()`：面板是渲染路径，不能为一行小标跑 SQL。
    primary: Mutex<Option<String>>,
    /// 最近一次「要的主源没切成」的原因（界面要能说出来，不能悄悄换）
    ///
    /// 与 [`Self::primary`] 同构：真值放在会话上，联邦门面 / 数据对象面板 / 源清单浮层
    /// 读的都是这一份，不再各自持一份副本。
    primary_note: Mutex<Option<String>>,
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

/// 别名白名单：拼进 SQL 前只认标识符字符（与 `MountRegistry` 的登记口径一致）。
///
/// 抽成一处是因为 `USE` / `SET search_path` / `DETACH` 三处都在拼别名，
/// 各写一份迟早漏掉一处。
fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

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
        // 第 3b 项：控制连接也要认 `rds_tmp` 裸名（照看临时表、诊断、`drop_temp` 都走它）。
        // 落点与 `search_path` 都写**全限定**（`memory.rds_tmp`）：裸 schema 名会跟着会话的
        // `USE` 漂到只读源 catalog 上，2.0.1 当场报 "No catalog + schema named …"。
        ensure_shared_schema(&conn).map_err(|error| format!("配置分析实例失败：{error}"))?;
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
                hidden_bases: Vec::new(),
                next_session: 0,
            }),
            timeout_ms: AtomicU64::new(timeout.as_millis().max(0) as u64),
            tag: tag.to_string(),
        }))
    }

    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// 每句默认超时（`0` = 不限时；看门狗对零时长直接不启）。
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.load(Ordering::SeqCst))
    }

    /// 改每句默认超时（毫秒）。**只影响之后开始的语句**：正在跑的那句按它启动时的值走。
    pub fn set_timeout_ms(&self, ms: u64) {
        self.timeout_ms.store(ms, Ordering::SeqCst);
        tracing::info!(timeout_ms = ms, "分析实例语句超时已更新");
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

    /// 会话视角下"卸掉"一个底座：**只记遮蔽，不 DETACH**。
    ///
    /// 为什么不 DETACH：`ATTACH` 是实例级事实，摘掉会让别的会话当场报 catalog 不存在
    /// （底座是"一直踩着的地基"）。遮蔽挡住的是**面板视图**：底座行与它里面的表不再列出。
    /// 边界（必须说清，不装完整）：显式限定名 `analytics.表` **仍然可达**——会话级遮蔽
    /// 挡不住裸限定名，DuckDB 侧也没有这个开关。
    ///
    /// 只认 [`federation::session::bases`] 里的别名；其余别名（尤其源）一律拒：
    /// 源的卸载有它自己那条链（`unmount_conn` / `DETACH`），两条不能混。
    pub fn hide_base(&self, alias: &str) -> Result<(), String> {
        if !super::federation::session::bases()
            .iter()
            .any(|base| base.alias == alias)
        {
            return Err(format!("{alias} 不是执行面底座，不能按底座卸载"));
        }
        let Ok(mut state) = self.state.lock() else {
            return Err("分析实例状态锁不可用".to_string());
        };
        if !state.hidden_bases.iter().any(|hidden| hidden == alias) {
            state.hidden_bases.push(alias.to_string());
        }
        Ok(())
    }

    /// 把"卸掉"的底座挂回（只清遮蔽；`ATTACH` 本来就没动过）。
    pub fn show_base(&self, alias: &str) -> Result<(), String> {
        let Ok(mut state) = self.state.lock() else {
            return Err("分析实例状态锁不可用".to_string());
        };
        state.hidden_bases.retain(|hidden| hidden != alias);
        Ok(())
    }

    /// 当前被遮蔽的底座别名（面板据此不列底座行、也不列它里面的表）。
    pub fn hidden_bases(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|state| state.hidden_bases.clone())
            .unwrap_or_default()
    }

    /// 这个底座此刻在面板/会话视角里可见吗（`false` = 用户卸掉了）。
    pub fn base_visible(&self, alias: &str) -> bool {
        !self
            .hidden_bases()
            .iter()
            .any(|hidden| hidden == alias)
    }

    /// 确保一台库以**底座**身份挂在实例上（`ATTACH ... READ_ONLY`）。
    ///
    /// 为什么单独开一个入口：`analytics` / `rds_global` 是保留别名，源别名册
    /// （[`Self::reserve_alias`]）一律拒——那道闸门是为了不让**源连接**盖住底座；
    /// 但引擎自己挂底座这条路径本来就不该走别名册（底座不是源）。
    ///
    /// 幂等：已挂着**同一份文件**就算成；挂着别的文件（切 `RDS_HOME`）就摘了重挂。
    pub fn ensure_base(&self, alias: &str, path: &Path) -> Result<(), String> {
        let conn = self
            .control
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if control_has_database(&conn, alias) {
            let attached = conn
                .query_row(
                    "SELECT path FROM duckdb_databases() WHERE database_name = ?",
                    [alias],
                    |row| row.get::<_, Option<String>>(0),
                )
                .ok()
                .flatten();
            let wanted = path.to_string_lossy();
            if attached.as_deref() == Some(wanted.as_ref()) {
                return Ok(());
            }
            // `DETACH` 之前先回默认 catalog：当前默认就是它时 DuckDB 拒绝摘
            conn.execute_batch(&format!("USE memory; DETACH {alias}"))
                .map_err(|error| format!("底座 {alias} 摘不掉（可能正被写入方持有）：{error}"))?;
        }
        let literal = path.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("ATTACH '{literal}' AS {alias} (READ_ONLY)"))
            .map_err(|error| format!("底座 {alias} 挂不上（可能正被写入方持有）：{error}"))?;
        Ok(())
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
        // 第 3b 项：每条会话都认 `rds_tmp` 裸名。克隆虽继承控制连接的设置，
        // 但这里是「临时表能不能被看见」的唯一前提，显式设一次（幂等）。
        // 全限定写法（`memory.rds_tmp`）：会话随后会 `USE <源>`（加速 / 联邦），
        // 裸 schema 名在那之后解析不到只读源上的临时区（实测 2.0.1 报 No catalog + schema）。
        ensure_shared_schema(&conn)
            .map_err(|error| format!("会话 {id} 设 {SHARED_TEMP_SCHEMA} 落点失败：{error}"))?;
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
            primary: Mutex::new(None),
            primary_note: Mutex::new(None),
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

    /// 按 owner 取一条**已经开着**的会话（纯内存读）。
    ///
    /// 与 [`Self::ensure_session`] 的区别是这里**不新建**：面板「刷新源 / 切主源」、
    /// 载入 / 归档 拿不到会话时应该明说“会话已经不在了”，而不是悄悄开一条空会话，
    /// 让后续 SQL 报「表不见了」。
    pub fn session_for_owner(&self, owner: &str) -> Option<Arc<AnalysisSession>> {
        let state = self.state.lock().ok()?;
        let id = state.owners.get(owner)?;
        state.sessions.get(id).cloned()
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

    /// 一条会话是不是「分析会话」（联邦 / 全局分析那些挂着源、会进数据对象源段的会话）。
    ///
    /// 判据是**挂着源**，不是 `label`：`ensure_session` 对同一 owner 是 1:1 复用——先开
    /// 全局分析库建下的那条会话（label「全局分析」）之后会被联邦执行复用，label 不会改。
    /// 把 label 当角色判据，「先开全局分析库、再跑联邦查询」这条常见路径上的源就会在
    /// 面板里整段消失（2026-11-26 合入 main 的 `871b9325` 之后露出来的）。
    ///
    /// 排除两类 owner：`process`（导出 / 取样 / 结果集的进程面会话）与
    /// `accel:<连接 id>`（本地加速档：挂着 `rds_src`，但加速档不占源段，只在连接行上写「已挂载」）。
    pub fn is_analysis_session(info: &SessionInfo, session: &AnalysisSession) -> bool {
        if !session.is_open() || info.owner == "process" || info.owner.starts_with("accel:") {
            return false;
        }
        !session.mounted_sources().is_empty()
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

    /// 这个别名此刻归哪个源连接（纯内存读；没人占就是 `None`）。
    ///
    /// 用途：联邦挂载前**先看归属**（第 2g 项）——`ATTACH` 是实例级事实，
    /// 同一个别名被两个源看中时，先挂的那个才是库里的真身；后到的必须报失败，
    /// 不能"看到别名已存在就算自己挂上了"（那样查询会静默指到别人的库）。
    pub fn alias_owner(&self, alias: &str) -> Option<String> {
        self.state
            .lock()
            .ok()?
            .mounts
            .owner_of(alias)
            .cloned()
    }

    /// **进程面**会话（owner `process`）：导出、结果集临时表、洞察取样都落在这条上。    ///
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
    ///
    /// 2026-11-27：卸完还要**收拾悬空会话**——凡是以它为「主源」的会话，`USE <别名>` 指向的
    /// catalog 已经没了，之后任何未限定名的查询都会报 catalog 不存在。这里逐个重建默认路径
    /// （[`AnalysisSession::reset_search_path`] 发现主源不在会清记录并退回 `memory`），
    /// 并把该别名从各会话的挂载记录里摘掉（面板「会话源」跟着少一行）。
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
        // 其余会话：摘掉这个别名（主源 → 退回 memory）
        for info in self.sessions() {
            let Some(session) = self.session(&info.id) else {
                continue;
            };
            if !session.is_open() {
                continue;
            }
            session.forget_mounted(&alias);
            if session.primary_alias().as_deref() == Some(alias.as_str()) {
                if let Err(reason) = session.reset_search_path() {
                    tracing::warn!(
                        alias = %alias,
                        session = %session.id(),
                        reason = %reason,
                        "卸载后会话默认路径没能重建（该会话后续查询可能落到 memory）"
                    );
                }
            }
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
    ///
    /// 两处都删：`rds_tmp.<名>`（新口径的共享临时区）与 `<名>`（留在 `memory.main` 的旧口径表）。
    /// 为什么容这两处：结果集 / 洞察样本是历史上建在 `memory.main` 的，名字（含 `tmp_q_` 前缀）
    /// 已经被洞察的引用模板按裸标识符吃进 SQL，搬 schema 会连带改一片模板；所以这一刀先
    /// 做到"登记 + 手工清得掉"，两处都认。
    pub fn drop_temp(&self, name: &str) -> Result<bool, String> {
        if name.trim().is_empty() {
            return Err("临时结果名不能为空".to_string());
        }
        let bare = name
            .strip_prefix(&format!("{SHARED_TEMP_PATH}."))
            .or_else(|| name.strip_prefix(&format!("{SHARED_TEMP_SCHEMA}.")))
            .unwrap_or(name);
        let quoted = bare.replace('"', "\"\"");
        // 钉死 `memory.rds_tmp`：控制连接哪天被 `USE` 过也不至于删到源上的同名表
        self.run_control(&format!(
            "DROP TABLE IF EXISTS {SHARED_TEMP_PATH}.\"{quoted}\""
        ))?;
        // 旧口径（`memory.main`）的同一张表：不在就静默跳过，删成功与否都继续走销登记
        let _ = self.run_control(&format!("DROP TABLE IF EXISTS \"{quoted}\""));
        let _ = super::DuckDBManager::temp_table_manager().unregister(bare);
        let removed = self
            .state
            .lock()
            .map(|mut state| {
                let before = state.temps.len();
                state.temps.retain(|entry| entry.name != name && entry.name != bare);
                before != state.temps.len()
            })
            .unwrap_or(false);
        Ok(removed)
    }

    /// 收掉某个 owner 名下的临时结果（关文档 / 关模块时调）。
    ///
    /// 名单是**两处并集**：实例登记册（用户看得见、点得掉的那一份）与临时表管理器
    /// （TTL / 上限用的那一份）。控制连接重开过时实例册可能先空、管理器那份还在，
    /// 只查一处会漏收。
    ///
    /// 单张删除复用 [`Self::drop_temp`]（同一套落点 + 销登记），失败不阻断其余表，
    /// 收完回第一条错。
    pub fn drop_temps_of(&self, owner: &str) -> Result<usize, String> {
        let mut names: Vec<String> = self
            .temps()
            .into_iter()
            .filter(|entry| entry.created_by == owner)
            .map(|entry| entry.name)
            .collect();
        for name in super::DuckDBManager::temp_table_manager().list_by_owner(owner) {
            if !names.iter().any(|existing| existing == &name) {
                names.push(name);
            }
        }
        let mut dropped = 0;
        let mut first_error: Option<String> = None;
        for name in names {
            match self.drop_temp(&name) {
                Ok(true) => dropped += 1,
                Ok(false) => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(dropped),
        }
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
                // 用户点「取消」也走同一条中断：DuckDB 回的是 INTERRUPT 原文，
                // 面板上要能一眼看出「这是被中断，不是 SQL 写错」——所以换成人话再交出去。
                let text = error.to_string();
                let lowered = text.to_lowercase();
                if lowered.contains("interrupt") || lowered.contains("cancelled") || lowered.contains("canceled") {
                    return Err(common_error(format!(
                        "查询已被中断（本会话已恢复，可继续执行）：{text}"
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
        use super::federation::registry::{MountState, MountedSource};

        if !self.is_open() {
            return Err("这条分析会话已经关了，重新开一条再挂源".to_string());
        }
        // 第 2g 项：先按实例的别名登记核对归属，再决定挂哪些。
        // 别名被**别的源**占着 → 这个源判 Failed（原话说明），绝不"看到已存在就算挂上"；
        // 名额归自己（同 conn_id 重复挂）→ 幂等，照常挂（`attach_source` 会认已挂）。
        let instance = self.instance.upgrade();
        let mut granted: Vec<super::federation::registry::FederatedSource> = Vec::new();
        let mut refusals: Vec<(String, String)> = Vec::new();
        for source in sources {
            let owner = instance
                .as_ref()
                .and_then(|instance| instance.alias_owner(&source.alias));
            match owner {
                Some(held) if held != source.conn_id => refusals.push((
                    source.alias.clone(),
                    format!(
                        "源 {}：别名 {} 已被另一个源（连接 {held}）占用，换个别名再挂；\
                         这个源本批不参与查询（别名是实例级的，不会替你改名）",
                        source.alias, source.alias
                    ),
                )),
                _ => granted.push(source.clone()),
            }
        }
        // 自己的源登记进别名表（`conn_id → 别名`）：之后冲突判定、面板查挂载都靠它。
        if let Some(instance) = instance.as_ref() {
            for source in &granted {
                instance.reserve_alias(&source.alias, &source.conn_id, source.kind)?;
            }
        }
        let mounted = if granted.is_empty() {
            Vec::new()
        } else {
            let conn = self.conn.lock().map_err(|_| {
                "会话连接锁被污染（上一条语句崩了），重开一条会话".to_string()
            })?;
            super::federation::session::mount_into(&conn, &granted)?
        };
        // 按原顺序拼回去：被拒的源以 Failed 出现（界面上一眼看到是"谁被占了"）
        let mut ordered = Vec::with_capacity(sources.len());
        let mut mounted_iter = mounted.into_iter();
        for source in sources {
            if let Some((_, reason)) = refusals.iter().find(|(alias, _)| alias == &source.alias) {
                ordered.push(MountedSource {
                    source: source.clone(),
                    state: MountState::Failed(reason.clone()),
                });
            } else if let Some(next) = mounted_iter.next() {
                ordered.push(next);
            }
        }
        if let Ok(mut stored) = self.mounted.lock() {
            for entry in &ordered {
                stored.retain(|existing| existing.alias() != entry.alias());
                stored.push(entry.clone());
            }
        }
        Ok(ordered)
    }

    /// 本会话最近一次挂载的逐源状态（纯内存读，渲染路径可调）。
    pub fn mounted_sources(&self) -> Vec<super::federation::registry::MountedSource> {
        self.mounted
            .lock()
            .map(|stored| stored.clone())
            .unwrap_or_default()
    }

    /// 把本会话的挂载**记录**对齐到给定别名集合（**只动记录，不 `DETACH`**）。
    ///
    /// 用途：一条文档从联邦切回本地加速（或反过来）时，会话按 owner 复用，
    /// 旧通道挂的源会一直留在 `mounted_sources()` 里，面板的「会话源」就同时显示
    /// 两套通道的源。`ATTACH` 是实例级事实（可能还有别的会话在用），所以这里只清
    /// 本会话的视图，源段的「已挂载」仍由实例事实回答——用户没点卸载就不替它卸。
    ///
    /// `aliases` 是**本通道真正要用**的那些别名；按别名比对（会保留冲突让位后的实际名）。
    pub fn retain_mounted(&self, aliases: &[String]) {
        if let Ok(mut stored) = self.mounted.lock() {
            stored.retain(|entry| aliases.iter().any(|alias| alias == entry.alias()));
        }
    }

    /// 把某一条挂载从本会话的记录里摘掉（源段「卸载」时对**所有**会话调一次）。
    ///
    /// 与 `retain_mounted` 的分工：那个按"本通道要哪几个"整体对齐，这个只摘一个名字；
    /// 两者都**不动实例上的 `ATTACH`**——真 `DETACH` 只由 [`Self::unmount_conn`] 做一次。
    pub fn forget_mounted(&self, alias: &str) {
        if let Ok(mut stored) = self.mounted.lock() {
            stored.retain(|entry| entry.alias() != alias);
        }
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

    /// 切本会话的默认 catalog（联邦主源 / 本地加速用），并把共享临时区**钉回** `search_path`。
    ///
    /// 为什么必须一起设：`USE <源>` 会把 `search_path` 重置成 `<源>.main`，`rds_tmp` 里的
    /// 临时结果集当场"消失"（2.0.1 实测：此时裸 schema 名报 `No catalog + schema named "rds_tmp"`）。
    /// 这里写全限定 `memory.rds_tmp`，主源与临时表都能按裸名解析，底座（`analytics` /
    /// `rds_global`）则**不在默认路径里**——本地加速因此看不见底座（显式限定仍可达）。
    ///
    /// 只接受标识符字符：别名来自 `MountRegistry` 与驱动的校验，但这里是拼 SQL 的地方，
    /// 宁可多一条白名单检查，也不让一个精心构造的别名变成第二条语句。
    pub fn use_catalog(&self, alias: &str) -> Result<(), String> {
        if !valid_alias(alias) {
            return Err(format!("别名 {alias:?} 里有不允许的字符，拒绝拿去拼 SQL"));
        }
        // 两条语句必须同一次 `execute_batch`：`USE` 成功而 `SET` 没跟上，会话就停在
        // "主源切了、临时表却看不见"的半截状态。
        let sql = format!("USE {alias}; SET search_path = '{alias}.main,{SHARED_TEMP_PATH}'");
        let outcome = self.with_connection(|conn| {
            conn.execute_batch(&sql)
                .map_err(|error| error.to_string())
        });
        match outcome {
            Ok(()) => {
                // 切成功才记：失败时主源没变，记下来会让面板把小标写错
                if let Ok(mut primary) = self.primary.lock() {
                    *primary = Some(alias.to_string());
                }
                if let Ok(mut note) = self.primary_note.lock() {
                    *note = None; // 用户显式选过了，回退说明就该收起来
                }
                Ok(())
            }
            Err(error) => {
                let reason = format!("切主源到 {alias} 失败：{error}");
                // 失败也要说出来：否则用户只看到“怎么没换”
                if let Ok(mut note) = self.primary_note.lock() {
                    *note = Some(reason.clone());
                }
                Err(reason)
            }
        }
    }

    /// 按会话**当前**的主源重建 `search_path`（`USE memory; DETACH …` 之后必调）。
    ///
    /// 重挂 / 卸源这类动作先把会话拽回 `memory`，`search_path` 也跟着变成 `memory.main`；
    /// 不重建就会出现"主源还在、临时表却解析不到"。主源已经不在（被卸掉）时退到 `memory`
    /// 并把原因写在 `primary_note` 上，不让会话卡在一个不存在的 catalog 上。
    pub fn reset_search_path(&self) -> Result<(), String> {
        match self.primary_alias().as_deref() {
            Some(alias) if valid_alias(alias) => {
                let sql = format!("SET search_path = '{alias}.main,{SHARED_TEMP_PATH}'");
                let outcome = self.with_connection(|conn| {
                    conn.execute_batch(&sql)
                        .map_err(|error| error.to_string())
                });
                if outcome.is_ok() {
                    return Ok(());
                }
                // 主源已被卸（catalog 不在了）：清记录、留说明，再退到 memory
                if let Ok(mut slot) = self.primary.lock() {
                    *slot = None;
                }
                if let Ok(mut note) = self.primary_note.lock() {
                    *note = Some(format!(
                        "主源 {alias} 已不在，默认目录回退到 memory：{}",
                        outcome.err().unwrap_or_default()
                    ));
                }
                self.reset_search_path()
            }
            _ => {
                let sql = format!("SET search_path = '{SHARED_MAIN_PATH},{SHARED_TEMP_PATH}'");
                self.with_connection(|conn| {
                    conn.execute_batch(&sql)
                        .map_err(|error| format!("设 search_path 失败：{error}"))
                })
            }
        }
    }

    /// 本会话当前的主源别名（纯内存读；没切过就是 `None`）。
    pub fn primary_alias(&self) -> Option<String> {
        self.primary.lock().ok().and_then(|slot| slot.clone())
    }

    /// 本会话「主源为什么不是你要的那个」的说明（纯内存读）
    pub fn primary_note(&self) -> Option<String> {
        self.primary_note.lock().ok().and_then(|slot| slot.clone())
    }

    /// 记一条主源回退说明（`None` = 收起来）
    pub(crate) fn set_primary_note(&self, note: Option<String>) {
        if let Ok(mut slot) = self.primary_note.lock() {
            *slot = note;
        }
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

    /// 切主源（本地加速 / 联邦都要 `USE <源>`）之后，共享临时表仍按裸名可见。
    ///
    /// 2.0.1 实测：`USE src` 会把 `search_path` 重置成 `src.main`，此时
    /// `SET search_path = 'main,rds_tmp'` 直接报 `No catalog + schema named "rds_tmp" found`。
    /// 所以 `use_catalog` 必须写全限定 `memory.rds_tmp`——这条用例就是那件事的回归。
    #[test]
    fn switching_catalog_keeps_shared_temps_visible() {
        let instance = instance("t_switch_path");
        let session = instance.open_session("doc-a", "编辑器").expect("开会话");
        session
            .run_with_timeout(
                "CREATE TABLE rds_tmp.tmp_q_keep AS SELECT 7 AS n",
                Duration::from_secs(3),
            )
            .expect("建共享临时");

        // 测试里用可写内存库代替只读源文件：要验的只是 catalog 切换与 search_path
        instance
            .run_control("ATTACH ':memory:' AS src_a")
            .expect("挂测试源");
        session.use_catalog("src_a").expect("切主源");

        let seen = session
            .run_with_timeout("SELECT n FROM tmp_q_keep", Duration::from_secs(3))
            .expect("切源后临时表应仍按裸名可见");
        assert_eq!(seen.to_rows()[0][0].to_string(), "7");

        let path = session
            .run_with_timeout(
                "SELECT value FROM duckdb_settings() WHERE name = 'search_path'",
                Duration::from_secs(3),
            )
            .expect("读 search_path");
        let path = path.to_rows()[0][0].to_string();
        assert_eq!(
            path,
            format!("src_a.main,{SHARED_TEMP_PATH}"),
            "默认路径只能有主源与共享临时区"
        );
        // 底座口径：默认路径里没有 `analytics` / `rds_global`
        // —— 本地加速因此看不见底座，显式限定（`analytics.…`）仍可达。
        assert!(!path.contains("analytics") && !path.contains("rds_global"));
        instance.close_all_sessions();
    }

    /// 换通道只换**会话视图**：`retain_mounted` 清掉上一通道的源记录，实例上的挂载不动。
    #[test]
    fn retain_mounted_only_trims_the_session_view() {
        use super::super::federation::registry::FederatedSource;

        let dir = std::env::temp_dir().join(format!("rds_retain_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("src.duckdb");
        Connection::open(&path)
            .expect("建源")
            .execute_batch("CREATE TABLE t AS SELECT 1 AS n")
            .expect("建表");

        let instance = instance("t_retain");
        let session = instance.open_session("doc-a", "编辑器").expect("开会话");
        let source = FederatedSource::new(
            "conn-a",
            "fed_a",
            "duckdb",
            path.to_str().expect("路径"),
        )
        .expect("造源");
        session.mount_sources(&[source]).expect("挂源");
        assert_eq!(session.mounted_sources().len(), 1);

        session.retain_mounted(&["rds_src".to_string()]);
        assert!(
            session.mounted_sources().is_empty(),
            "非本通道的源记录应退场，否则面板会把两套通道的源一起列出来"
        );
        assert!(
            instance.alias_owner("fed_a").is_some(),
            "实例上的别名归属不动：源段的卸载是另一颗两态按钮"
        );
        instance.close_all_sessions();
    }

    /// 源段卸载之后，用它当主源的会话不能悬空：默认路径退回 memory，挂载记录里少这一行。
    #[test]
    fn unmounting_a_primary_source_falls_back_to_memory() {
        use super::super::federation::registry::FederatedSource;

        let dir = std::env::temp_dir().join(format!("rds_unmount_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("src.duckdb");
        Connection::open(&path)
            .expect("建源")
            .execute_batch("CREATE TABLE t AS SELECT 1 AS n")
            .expect("建表");

        let instance = instance("t_unmount");
        let session = instance.open_session("doc-a", "编辑器").expect("开会话");
        let source = FederatedSource::new(
            "conn-a",
            "fed_a",
            "duckdb",
            path.to_str().expect("路径"),
        )
        .expect("造源");
        session.mount_sources(&[source]).expect("挂源");
        session.use_catalog("fed_a").expect("设主源");
        assert_eq!(session.primary_alias().as_deref(), Some("fed_a"));

        assert!(instance.unmount_conn("conn-a").expect("卸载"));
        assert_eq!(session.primary_alias(), None, "主源记录该清掉");
        assert!(
            session.mounted_sources().is_empty(),
            "挂载记录里不该再留着 fed_a"
        );
        session
            .run_with_timeout("SELECT 1 AS ok", Duration::from_secs(3))
            .expect("卸载后会话仍可用（退回 memory）");
        let path_after = session
            .run_with_timeout(
                "SELECT value FROM duckdb_settings() WHERE name = 'search_path'",
                Duration::from_secs(3),
            )
            .expect("读 search_path")
            .to_rows()[0][0]
            .to_string();
        assert_eq!(
            path_after,
            format!("{SHARED_MAIN_PATH},{SHARED_TEMP_PATH}"),
            "默认路径该退回 memory，且临时区仍在"
        );
        instance.close_all_sessions();
    }

    /// 手动清理要认**两种落点**：`rds_tmp.<名>`（新口径）与 `memory.main.<名>`（历史口径）。
    ///
    /// 为什么容两处：结果集 / 洞察样本的名字（含 `tmp_q_` 前缀）已经被洞察的引用模板按
    /// 裸标识符吃进 SQL，整体搬 schema 会连带改一片模板；这一版先做到「登记 + 手工清得掉」。
    #[test]
    fn manual_cleanup_drops_either_schema_and_unregisters() {
        let instance = instance("t_drop_locations");
        let session = instance.open_session("doc-a", "编辑器 A").expect("开会话");

        // ① 历史口径：裸名建表（默认 catalog/schema = memory.main）
        session
            .run_with_timeout("CREATE TABLE tmp_q_legacy (id INTEGER)", Duration::from_secs(3))
            .expect("建历史口径表");
        instance.register_temp("tmp_q_legacy", "用例", "历史口径");
        assert!(
            session
                .run_with_timeout("SELECT count(*) FROM tmp_q_legacy", Duration::from_secs(3))
                .is_ok(),
            "建在前该读得到"
        );
        assert!(instance.drop_temp("tmp_q_legacy").expect("清理"), "登记该销掉");
        assert!(
            session
                .run_with_timeout("SELECT count(*) FROM tmp_q_legacy", Duration::from_secs(3))
                .is_err(),
            "清理后表该没了"
        );

        // ② 新口径：`rds_tmp.<名>`；登记时带不带 schema 前缀都要能清
        session
            .run_with_timeout(
                "CREATE TABLE rds_tmp.tmp_i_sample_1 AS SELECT 1 AS a",
                Duration::from_secs(3),
            )
            .expect("建共享临时");
        instance.register_temp("tmp_i_sample_1", "用例", "新口径");
        assert!(instance.drop_temp("rds_tmp.tmp_i_sample_1").expect("带前缀清理"), "该清掉");
        assert!(
            session
                .run_with_timeout("SELECT count(*) FROM rds_tmp.tmp_i_sample_1", Duration::from_secs(3))
                .is_err(),
            "清理后 rds_tmp 里也不该有"
        );
        assert!(instance.temps().is_empty(), "两件都应销掉登记");
    }

    /// 按 owner 收批：只动这个 owner 的表，别人的表与登记都不碰。
    ///
    /// 这是「关文档收掉这份工作的结果集」的唯一凭据——A 建的表不能因为关 B 就没了，
    /// 也不能因为 A 关了就把 B 一起带走。
    #[test]
    fn dropping_temps_of_one_owner_spares_the_others() {
        let instance = instance("t_drop_owner");
        let session = instance.open_session("doc-a", "编辑器 A").expect("开会话");
        session
            .run_with_timeout(
                "CREATE TABLE rds_tmp.tmp_q_a AS SELECT 1 AS v; \
                 CREATE TABLE rds_tmp.tmp_q_b AS SELECT 2 AS v",
                Duration::from_secs(3),
            )
            .expect("建两张表");
        instance.register_temp("tmp_q_a", "doc-a", "A 的结果集");
        instance.register_temp("tmp_q_b", "doc-b", "B 的结果集");

        assert_eq!(instance.drop_temps_of("doc-a").expect("收 A 一批"), 1);
        assert!(
            session
                .run_with_timeout("SELECT count(*) FROM rds_tmp.tmp_q_a", Duration::from_secs(3))
                .is_err(),
            "A 的表该没了"
        );
        assert!(
            session
                .run_with_timeout("SELECT count(*) FROM rds_tmp.tmp_q_b", Duration::from_secs(3))
                .is_ok(),
            "B 的表不该被牵连"
        );
        assert_eq!(instance.temps().len(), 1, "只剩 B 的登记");
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

    /// 超时值可运行时改（设置页「语句超时」走这条），并且**会话 `run` 每次都现读**。
    #[test]
    fn the_default_timeout_is_read_at_runtime() {
        let instance = instance("t_timeout_runtime");
        let session = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        assert_eq!(instance.timeout(), Duration::from_millis(3000), "用例实例的初始值");

        // 改到 30ms：无超时的 `run` 也该被掐掉（说明 run 没有缓存旧值）
        instance.set_timeout_ms(30);
        let error = session
            .run("SELECT count(*) FROM range(0, 100000) a(x), range(0, 100000) b(y)")
            .expect_err("该被 30ms 掐掉")
            .to_string();
        assert!(error.contains("被中断"), "{error}");

        // 关掉超时（0 = 不限时）：同一句前缀的规模缩小后能正常跑完
        instance.set_timeout_ms(0);
        assert_eq!(instance.timeout(), Duration::ZERO);
        let ok = session
            .run("SELECT count(*) AS n FROM range(0, 1000)")
            .expect("不限时该跑完");
        assert_eq!(ok.total_rows, 1);
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

    /// 第 2g 项：同一个别名被两个源看中时，**后到的判失败**，不静默指到先挂的库。
    #[test]
    fn an_alias_held_by_another_source_is_refused_not_hijacked() {
        use super::super::federation::registry::{FederatedSource, MountState};

        let dir = std::env::temp_dir().join(format!("rds_alias_owner_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        let first_path = dir.join("first.duckdb");
        let second_path = dir.join("second.duckdb");
        for (path, rows) in [(&first_path, 3), (&second_path, 5)] {
            let conn = Connection::open(path).expect("建源文件");
            conn.execute_batch(&format!(
                "CREATE TABLE items AS SELECT i AS id FROM range({rows}) AS r(i)"
            ))
            .expect("建表");
        }

        let instance = instance("t_alias_owner");
        let session_a = instance.open_session("doc-a", "编辑器 A").expect("开 A");
        let session_b = instance.open_session("doc-b", "编辑器 B").expect("开 B");

        let source_a = FederatedSource::new(
            "conn-a",
            "shared",
            "duckdb",
            first_path.to_str().expect("路径"),
        )
        .expect("造源 A");
        let mounted_a = session_a.mount_sources(&[source_a]).expect("挂 A");
        assert!(mounted_a[0].is_ready(), "第一个源该挂上：{:?}", mounted_a[0].state);

        let source_b = FederatedSource::new(
            "conn-b",
            "shared",
            "duckdb",
            second_path.to_str().expect("路径"),
        )
        .expect("造源 B");
        let mounted_b = session_b.mount_sources(&[source_b]).expect("挂 B（不该整批报错）");
        match &mounted_b[0].state {
            MountState::Failed(reason) => {
                assert!(reason.contains("已被另一个源"), "要说清被谁占了：{reason}");
                assert!(reason.contains("conn-a"), "要点名占位的源：{reason}");
            }
            other => panic!("后到的源应判失败，实际：{other:?}"),
        }

        // 别名仍指向 A 的库（3 行），而不是 B 的 5 行——没有被静默劫持
        let count = session_b
            .run_with_timeout("SELECT count(*) AS n FROM shared.items", Duration::from_secs(3))
            .expect("查别名");
        assert_eq!(count.to_rows()[0][0].to_string(), "3");

        let _ = std::fs::remove_dir_all(&dir);
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

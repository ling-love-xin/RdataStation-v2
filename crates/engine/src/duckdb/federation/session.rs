//! 联邦会话：把多个源挂到**分析实例的一条会话**上（见 `docs/architecture/federation/`）
//!
//! ## 与 `accel` 的关系（先说清楚，避免又长出一套）
//!
//! `accel` 是“一条源一条连接”（整库 `ATTACH … (READ_ONLY)` + `USE`）；联邦是它的**推广**：
//! 同一条会话挂**多条**源、每条一个别名，`USE` 指向**主源**。所以这里直接复用 accel 的：
//!
//! 1. [`mount_lock`]（`INSTALL` / `LOAD` 是进程级资源，并发建会话会撞文件）；
//! 2. 写保护双保险（`ATTACH … (READ_ONLY)` 让 DuckDB 自己拒 + 编辑器侧提交前拒）；
//!    L2 没有第一道，改用会话层判定（见 [`FederatedSession::l2_write_refusal`]）。
//!
//! 2026-11-26 起本层**不再自建内存库**：执行落在 `AnalysisInstance` 的一条会话上
//! （`docs/architecture/database/analysis-instance-sessions.md`）。本层保留的是联邦语义
//! （挂载清单 / 主源 / 快照 / 写拒绝 / 错误脱敏）。
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
//! 2. **源清单变了会重建会话**（先关实例上那条旧会话 → `TEMP` 一起消失，再开新会话）——
//!    这是“改了参与源就当新会话”的代价，比让旧表清单继续骗人强。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use duckdb::Connection;
use once_cell::sync::Lazy;

use shared::error::CoreError;
use shared::models::QueryResult;

use super::super::accel::{
    AccelKind, MountedTable, list_mounted_tables, mount_lock, scrub_credentials,
};
use super::super::instance::AnalysisSession;
use super::super::manager::DuckDBManager;
use super::registry::{FederatedSource, MountState, MountedSource, SessionSnapshot};

/// 一条联邦会话：**分析实例上的一条会话** + 一批只读挂载的记账
///
/// 2026-11-26 起它不再自己开内存库：执行落在 `AnalysisInstance` 的会话上
/// （`docs/architecture/database/analysis-instance-sessions.md`）。这层保留的是
/// **联邦语义**（挂载清单 / 主源 / 快照 / 写拒绝 / 错误脱敏），不是"一台独立的库"。
pub struct FederatedSession {
    /// 实例会话（catalog 与底座由实例共享，`TEMP` 仍属于这条会话）
    session: Arc<AnalysisSession>,
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
    /// 在**实例的一条会话**上建联邦并逐个挂载（挂不上的记原因，不中断）
    ///
    /// 会做 I/O（首次装扩展要联网一次、`ATTACH` 要连源库），属于**事件路径**；
    /// 界面可用性看 [`Self::snapshot`]（纯内存读）。
    ///
    /// `session` 必须是本条 owner 的实例会话；底座已由实例挂好（`ATTACH` 是实例级事实），
    /// 这里只管源。
    pub fn open(
        session: Arc<AnalysisSession>,
        sources: &[FederatedSource],
        requested_primary: Option<&str>,
    ) -> Result<Self, String> {
        // 源挂载只有一处实现（重名拦截也在里面）
        let mounted = session.mount_sources(sources)?;

        let (primary, primary_note) = pick_primary(&mounted, requested_primary);
        if let Some(alias) = primary.as_deref() {
            session.use_catalog(alias)?;
        }

        Ok(Self {
            session,
            mounted: Mutex::new(mounted),
            primary: Mutex::new(primary),
            primary_note: Mutex::new(primary_note),
            mounted_at: std::time::SystemTime::now(),
        })
    }

    /// 这条联邦会话落在实例的哪条会话上（关会话 / 诊断用）
    pub fn analysis_session(&self) -> &Arc<AnalysisSession> {
        &self.session
    }

    /// 在本地库上执行一句（读语句出 Arrow 批，写语句出影响行数）
    ///
    /// 源库对象的写在引擎侧就被 `READ_ONLY` 拒（这里不做第二套判定：判据只有一处）——
    /// **除了 L2 源**：Oracle 的扩展不支持 `READ_ONLY`，对它的写只能由会话层拦（[`Self::l2_write_refusal`]）。
    /// 错误文本先脱敏：源连接串是带凭据的，驱动报错可能把它回显出来。
    pub fn run(&self, sql: &str) -> Result<QueryResult, CoreError> {
        if let Some(reason) = self.l2_write_refusal(sql) {
            return Err(CoreError::common(shared::error::CommonError::General(
                reason,
            )));
        }
        let err = self.session.run(sql).map_err(|error| self.scrub_error(error))?;
        Ok(err)
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

        let was_primary = self
            .primary
            .lock()
            .map(|primary| primary.as_deref() == Some(alias))
            .unwrap_or(false);

        let state = self.session.with_connection(|conn| {
            conn.execute_batch(&format!("USE memory; DETACH {alias}"))
                .map_err(|error| {
                    scrub_error(
                        &source.connection_string,
                        format!("卸载 {alias} 失败：{error}"),
                    )
                })?;
            // 重挂走与 `open` 同一条实现：L2 的 Secret 在这里重建（口令可能有变）
            let state = attach_source(conn, &source);
            // 重挂之后主源要 `USE` 回去（否则未限定名会解析到 memory 上）
            if was_primary && matches!(state, MountState::Ready { .. }) {
                use_catalog(conn, alias)?;
            }
            Ok(state)
        })?;

        if let Ok(mut mounted) = self.mounted.lock()
            && let Some(entry) = mounted.iter_mut().find(|entry| entry.alias() == alias)
        {
            entry.state = state;
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
        self.session
            .use_catalog(alias)
            .map_err(|error| error.to_string())?;
        if let Ok(mut primary) = self.primary.lock() {
            *primary = Some(alias.to_string());
        }
        if let Ok(mut note) = self.primary_note.lock() {
            *note = None; // 用户显式选过了，回退说明就该收起来
        }
        Ok(())
    }

    /// 把一个**外部文件**挂成会话里的视图（数据对象第 ① 类里最便宜的那一档）
    ///
    /// 建视图不物化：每次查询重读文件，文件改了结果跟着变——这才是“加载进 DuckDB 实例”
    /// 的本意，而不是偷偷拷一份（需要拷的只有临时表那一档，见 P4）。
    pub fn add_file_view(&self, path: &str, alias: &str) -> Result<(), String> {
        let sql = file_view_sql(path, alias)?;
        self.session.with_connection(|conn| {
            conn.execute_batch(&sql)
                .map_err(|error| format!("加载文件失败：{error}"))
        })
    }

    /// 会话已经开着，再挂一个源（数据对象面板「加源」里选连接的出口）
    ///
    /// 与 [`Self::open`] 同一套挂载口径（`mount_one` + 重名拦截）。挂载失败**照样
    /// 留一行**并带原因——界面要能说清“哪个源没挂上”，而不是悄悄吞掉。
    pub fn add_source(&self, source: FederatedSource) -> Result<(), String> {
        let already = self
            .mounted
            .lock()
            .map(|store| store.iter().any(|seen| seen.alias() == source.alias))
            .unwrap_or(false);
        if already {
            return Err(format!("源 {} 已经在会话里了", source.alias));
        }

        let mounted = self
            .session
            .with_connection(|conn| Ok(mount_one(conn, &source)))?;
        let failure = match &mounted.state {
            MountState::Failed(reason) => Some(reason.clone()),
            _ => None,
        };
        if let Ok(mut store) = self.mounted.lock() {
            store.push(mounted);
        }
        match failure {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    /// 列出某个挂载源里的表 / 视图（数据对象面板「源行展开」的数据）
    ///
    /// 会查这条会话的连接（**事件路径**，不是渲染路径）。别名不在会话里、或那个源
    /// 挂载失败时如实报错——面板叶子行要的是清单，不是一句“算了吧”。
    pub fn list_tables(&self, alias: &str) -> Result<Vec<MountedTable>, String> {
        let (ready, kind) = {
            let mounted = self
                .mounted
                .lock()
                .map_err(|_| "会话状态锁被污染".to_string())?;
            let entry = mounted
                .iter()
                .find(|entry| entry.alias() == alias)
                .ok_or_else(|| format!("会话里没有源 {alias}"))?;
            (entry.is_ready(), entry.source.kind)
        };
        if !ready {
            return Err(format!("源 {alias} 当前不可用，列不出表清单"));
        }
        self.session
            .with_connection(|conn| list_mounted_tables(conn, alias, kind))
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
        Ok(self.session.interrupt())
    }

    /// 会话活了多久（诊断）
    pub fn mounted_for(&self) -> std::time::Duration {
        self.mounted_at
            .elapsed()
            .unwrap_or(std::time::Duration::ZERO)
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

/// 执行面底座（联邦 / 分析共用）：一条固定的只读 catalog，不是"用户加的源"。
///
/// 为什么底座不是源：分析库和全局库是**执行面自带的**两个库（项目持久表 / `res_` 结果集
/// 落在 `analytics`，全局表落在 `rds_global`）。用户口径（2026-11-25）：
/// 「全局分析库 + 归档文件必须能进联邦，联邦和分析本质上没有差别」。
/// 所以它们跟着会话走：每次 open 挂、缺文件就跳过（还没建库不是错误），
/// 不参与源段的登记/三态，也不给卸载入口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseMount {
    /// SQL 里的 catalog 名（固定，不让入口自己起名）
    pub alias: String,
    /// 库文件绝对路径
    pub path: String,
}

/// 项目分析库在会话 SQL 里的固定别名（与引擎别处对 `analytics` 的叫法一致）
pub const PROJECT_BASE_ALIAS: &str = "analytics";

/// 全局分析库在会话 SQL 里的固定别名：与 [`crate::analysis::global::GLOBAL_ALIAS`] 同一常量
/// （两处各起一个名，`rds_global` 与 `analytics` 就会在 SQL 里出现两套写法）
pub const GLOBAL_BASE_ALIAS: &str = crate::analysis::global::GLOBAL_ALIAS;

/// 进程级底座清单（由宿主在项目开关时设置；引擎不猜路径——那是宿主的事）
static BASES: Lazy<Mutex<Vec<BaseMount>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// 设置执行面底座。宿主在「打开项目 / 关闭项目」时调它。
///
/// 只改配置，**不动已建会话**：下一次 [`ensure_session`] 会因指纹变化重建会话
/// （与「源清单变了就重建」同一口径——切项目后旧的 `analytics` 一定不能留着）。
pub fn set_bases(bases: Vec<BaseMount>) {
    let mut current = BASES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *current = bases;
}

/// 当前底座清单（只读快照；数据对象面板据此出「底座」行——面板不猜路径、不查盘）
pub fn bases() -> Vec<BaseMount> {
    BASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// 当前底座指纹（顺序无关）：并进会话指纹，切项目就重建会话
pub fn bases_fingerprint() -> String {
    let bases = BASES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut parts: Vec<String> = bases
        .iter()
        .map(|base| format!("{}|{}", base.alias, base.path))
        .collect();
    parts.sort();
    parts.join(";")
}

/// 把底座只读挂进会话（**缺文件跳过**：项目刚建、全局库还没初始化都不是错误）
///
/// 公开给分析实例用：实例是"一台分析引擎"，底座必须在实例上，而不是某一条会话上
/// （`ATTACH` 本身就是实例级事实，挂在哪条会话上都对所有会话可见）。
pub(crate) fn attach_bases(conn: &Connection) -> Vec<(String, Result<(), String>)> {
    let bases = BASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let mut out = Vec::with_capacity(bases.len());
    for base in bases {
        let path = std::path::Path::new(&base.path);
        if !path.is_file() {
            out.push((base.alias, Err("库文件还没建".to_string())));
            continue;
        }
        let literal = base.path.replace('\'', "''");
        let alias = base.alias.clone();
        let sql = format!("ATTACH '{literal}' AS {alias} (READ_ONLY)");
        let outcome = conn
            .execute_batch(&sql)
            .map_err(|error| format!("底座 {alias} 挂不上（可能正被写入方持有）：{error}"));
        out.push((base.alias, outcome));
    }
    out
}

// 2026-11-26：`open_configured` 随"联邦自建内存库"一起删了——现在连接由分析实例给
// （`AnalysisInstance::open_tagged` 里调 `DuckDBManager::configure_connection`）。

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

/// 这个别名是否已经挂在这个**实例**上（catalog 是实例级的，不是会话级的）。
///
/// 为什么必须查：实例化之后第二次 `ATTACH` 同一别名会报 "already attached"，
/// 而"同一份文档再执行一次"、"第二个会话挂同一个源"都是正常路径。
fn is_attached(conn: &Connection, alias: &str) -> bool {
    conn.query_row(
        "SELECT count(*) FROM duckdb_databases() WHERE database_name = ?",
        [alias],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// 建凭据 + `ATTACH`（**挂载只有这一处实现**：`open` 与 `refresh` 共用，别在两处各挂一遍）
///
/// **幂等**：别名已在实例上挂着就直接算成功（数一下表）。`ATTACH` 是实例级事实，
/// 重复挂只会报 "already attached"；把它当失败会让"再执行一次"变成假报错。
///
/// 失败不抛：状态就是结论（挂不上的源不该把整条会话带下去）。
fn attach_source(conn: &Connection, source: &FederatedSource) -> MountState {
    if is_attached(conn, &source.alias) {
        return MountState::Ready {
            tables: count_tables(conn, &source.alias).unwrap_or(0),
        };
    }
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

/// 把一批源挂进**已有连接**（挂载的唯一实现：`FederatedSession::open` 与分析实例的
/// 会话都调它，别在两处各挂一遍）。
///
/// - 同一批里别名重复 → 直接拒（两个源叫一个名会静默指到其中一个，那是最难查的一类错）；
/// - 扩展按**种类**去重后 `ensure`（`ensure` 本身幂等，这里只省重复的登记与日志）；
/// - 整段串行（`INSTALL` 写扩展目录、`LOAD` 是连接级，都要独占）；
/// - 单个源挂不上**不抛**：状态就是结论（`open` 与实例会话都按状态展示）。
pub(crate) fn mount_into(
    conn: &Connection,
    sources: &[FederatedSource],
) -> Result<Vec<MountedSource>, String> {
    let mut seen: Vec<String> = Vec::new();
    for source in sources {
        if seen.iter().any(|alias| alias == &source.alias) {
            return Err(format!("别名 {} 在这次的源里出现了两次", source.alias));
        }
        seen.push(source.alias.clone());
    }

    let _mount = mount_lock();
    let mut ensured: HashSet<AccelKind> = HashSet::new();
    for source in sources {
        if ensured.insert(source.kind) {
            crate::duckdb::accel::ensure_extension(conn, source.kind)?;
        }
    }
    Ok(sources
        .iter()
        .map(|source| mount_one(conn, source))
        .collect())
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
/// 脚本 / 文档类扩展名：**它们不是数据**（铁律 G 第 31 条）。
///
/// 与「DuckDB 读不了」分开说的原因：读不了是能力问题、会随扩展变得可读；
/// 这些文件从来就不是数据源，说成「读不了」等于承诺了一个不该有的能力。
fn is_non_data_extension(ext: &str) -> bool {
    matches!(
        ext,
        "sql"
            | "py"
            | "rs"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "md"
            | "markdown"
            | "toml"
            | "yaml"
            | "yml"
            | "sh"
            | "bash"
            | "ps1"
            | "bat"
            | "cmd"
            | "ini"
            | "cfg"
            | "conf"
            | "html"
            | "css"
    )
}

/// 外部文件 → 建视图的 SQL（按扩展名挑 DuckDB 的读取函数）
///
/// 只认 DuckDB 能直接读的那几类。excel 要 `excel` 扩展（本机未实测装得上），
/// 所以这里先把“读不了”的情况变成一句可读原因，而不是等执行时报 SQL 错误。
pub fn file_view_sql(path: &str, alias: &str) -> Result<String, String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let reader = match ext.as_str() {
        "csv" | "tsv" | "txt" => "read_csv_auto",
        "parquet" => "read_parquet",
        "json" | "jsonl" | "ndjson" => "read_json_auto",
        "xlsx" | "xls" => "read_excel",
        other => {
            // 铁律 G 第 31 条：脚本 / 文档类文件要写「这类文件不是数据」，
            // 不能表述成「能力缺失 / 读不了」——那不是引擎的短板，是它本来就不是数据
            if is_non_data_extension(other) {
                return Err(format!(
                    "「.{other}」这类文件不是数据（脚本 / 文档文件不参与分析）：\
                     数据文件请用 csv / parquet / json / excel"
                ));
            }
            return Err(format!(
                "DuckDB 读不了 .{other}：只支持 csv / parquet / json / excel"
            ));
        }
    };
    let quoted_path = path.replace('\'', "''");
    let quoted_alias = alias.replace('"', "\"\"");
    Ok(format!(
        "CREATE OR REPLACE VIEW \"{quoted_alias}\" AS SELECT * FROM {reader}('{quoted_path}')"
    ))
}

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

/// 会话指纹 = 源清单 + 底座清单。
///
/// 底座必须参与：切项目后 `analytics` 指向另一个库文件，旧会话里的那份**必须**消失，
/// 否则用户会对着上个项目的表查数据（那是比"重建慢一点"严重得多的错）。
pub fn session_fingerprint(sources: &[FederatedSource]) -> String {
    format!("{}#bases:{}", source_fingerprint(sources), bases_fingerprint())
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
    let fingerprint = session_fingerprint(sources);
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

    // 源清单变了：先把**实例上的那条旧会话关掉**（`TEMP` 随旧会话消失，正是重建的本意），
    // 再用同一个 owner 开一条新的（`ensure_session` 会给出新 id 并更新 owner 映射）。
    if let Ok(sessions) = SESSIONS.lock() {
        if let Some(cached) = sessions.get(owner) {
            if let Ok(instance) = crate::duckdb::instance::instance() {
                instance.close_session(cached.session.analysis_session().id());
            }
        }
    }

    let instance = crate::duckdb::instance::instance()?;
    let analysis_session = instance.ensure_session(&format!("fed:{owner}"), "联邦")?;
    let session = Arc::new(FederatedSession::open(
        analysis_session,
        sources,
        requested_primary,
    )?);
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
    SESSIONS
        .lock()
        .ok()?
        .get(owner)
        .map(|cached| cached.session.clone())
}

/// 会话快照（**渲染路径可调**：纯内存；没有会话就是 `None`）
///
/// 注意 `None` 与“有空会话”不同：没有会话 = 还没挂过（源清单 UI 显示空态）。
pub fn snapshot_for(owner: &str) -> Option<SessionSnapshot> {
    session_for(owner).map(|session| session.snapshot())
}

/// 放掉某个主人的会话（测试 / 连接被删时用）
///
/// 要连**实例上的那条会话**一起关：不然会话表里留一条空壳，`TEMP` 与 owner 映射也无主。
pub fn drop_session(owner: &str) -> bool {
    let cached = SESSIONS
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(owner));
    let Some(cached) = cached else {
        return false;
    };
    if let Ok(instance) = crate::duckdb::instance::instance() {
        instance.close_session(cached.session.analysis_session().id());
    }
    true
}

/// 放掉全部联邦会话（测试用：进程内状态要能清干净）
pub fn drop_all() {
    let sessions: Vec<_> = SESSIONS
        .lock()
        .map(|mut sessions| sessions.drain().map(|(_, cached)| cached).collect())
        .unwrap_or_default();
    if let Ok(instance) = crate::duckdb::instance::instance() {
        for cached in sessions {
            instance.close_session(cached.session.analysis_session().id());
        }
    }
}

/// 中断某个主人会话上正在跑的语句（没有会话 = `None`，与加速档同一语义）
pub fn cancel(owner: &str) -> Option<Result<bool, String>> {
    session_for(owner).map(|session| session.interrupt())
}

/// 切换主源（没有会话就是错误：**不替用户建一条**，那是执行路径的事）
pub fn set_primary(owner: &str, alias: &str) -> Result<(), String> {
    let session =
        session_for(owner).ok_or_else(|| "还没有联邦会话：先执行一次再切换主源".to_string())?;
    session.set_primary(alias)
}

/// 按源重挂（表清单刷新；没有会话就是错误，理由同上）
pub fn refresh_source(owner: &str, alias: &str) -> Result<(), String> {
    let session =
        session_for(owner).ok_or_else(|| "还没有联邦会话：先执行一次再刷新源".to_string())?;
    session.refresh(alias)
}

/// 重挂会话里的所有源（界面上「刷新全部」）
///
/// 返回**逐源结果**：失败的源如实带原因（不吞、不合并成一句“部分失败”）。
/// 把外部文件加载进某个联邦会话（面板「加源」的文件分支走这里）
pub fn add_file_view(owner: &str, path: &str, alias: &str) -> Result<(), String> {
    let session = session_for(owner)
        .ok_or_else(|| "还没有联邦会话：先切到联邦执行一次查询".to_string())?;
    session.add_file_view(path, alias)
}

pub fn refresh_all(owner: &str) -> Result<Vec<(String, Result<(), String>)>, String> {
    let session =
        session_for(owner).ok_or_else(|| "还没有联邦会话：先执行一次再刷新源".to_string())?;
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

/// 列出某条会话里某个源的表 / 视图（数据对象面板「源行展开」用）
///
/// 面板只知道 `(owner, alias)`（它从 [`owners`] 的快照里拿到的就是这两样），
/// 这里替它找会话；没有会话如实报错——**不替用户建一条**，那是执行路径的事。
pub fn list_tables_for(owner: &str, alias: &str) -> Result<Vec<MountedTable>, String> {
    let session =
        session_for(owner).ok_or_else(|| "还没有联邦会话：先执行一次再展开源".to_string())?;
    session.list_tables(alias)
}

/// **列出当前进程所有存活会话**（owner + 快照），按 owner 排序。
///
/// 给「会话面板」用：面板不知道 owner 是谁也能把会话看全（`snapshot_for` 要先知道 owner，
/// 而 owner 是宿主侧的文档/面板身份，UI 侧不该反过来猜它）。快照是**一次性拷贝**：
/// 面板拿着它渲染，不持有会话句柄，也就挡不住会话被重建。
pub fn owners() -> Vec<(String, SessionSnapshot)> {
    let Ok(sessions) = SESSIONS.lock() else {
        return Vec::new();
    };
    let mut out: Vec<(String, SessionSnapshot)> = sessions
        .iter()
        .map(|(owner, cached)| (owner.clone(), cached.session.snapshot()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
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
    use super::{
        BaseMount, FederatedSession, GLOBAL_BASE_ALIAS, PROJECT_BASE_ALIAS, file_view_sql,
        session_fingerprint, set_bases, write_refusal,
    };
    use crate::duckdb::federation::registry::{FederatedSource, MountState, MountedSource};
    use std::sync::Arc;

    /// 测试用：每条用例一个**独立分析实例**（避免共用 catalog 互相踩），owner 固定 `doc`。
    ///
    /// 底座在 `open_tagged` 时挂上，所以调它的用例必须先把 `set_bases(...)` 设好。
    fn try_open_for_test(
        tag: &str,
        sources: &[FederatedSource],
        primary: Option<&str>,
    ) -> Result<Arc<FederatedSession>, String> {
        let instance = crate::duckdb::instance::AnalysisInstance::open_tagged(
            tag,
            std::time::Duration::from_secs(10),
        )?;
        let session = instance.open_session("doc", "测试")?;
        Ok(Arc::new(FederatedSession::open(
            session, sources, primary,
        )?))
    }

    fn open_for_test(
        tag: &str,
        sources: &[FederatedSource],
        primary: Option<&str>,
    ) -> Arc<FederatedSession> {
        try_open_for_test(tag, sources, primary).expect("建联邦会话")
    }

    /// 铁律 G 第 31 条：脚本 / 文档类文件只能说「不是数据」，不能说「读不了」。
    /// 未知格式（如 `.arrow2`）仍按能力表述，因为那才是真的缺读取器。
    #[test]
    fn non_data_files_are_named_as_non_data_not_as_a_missing_reader() {
        let script = file_view_sql("resources/analysis.sql", "f_analysis").unwrap_err();
        assert!(
            script.contains("不是数据"),
            "脚本文件要说清不是数据，实际：{script}"
        );
        assert!(!script.contains("读不了"), "不许说成能力缺失：{script}");

        let doc = file_view_sql("resources/notes.md", "f_notes").unwrap_err();
        assert!(doc.contains("不是数据"), "{doc}");

        // 真缺读取器的格式保持原口径（这不是「不是数据」，是引擎现在不认）
        let unknown = file_view_sql("resources/sample.arrow2", "f_sample").unwrap_err();
        assert!(unknown.contains("读不了"), "{unknown}");
    }

    /// 会话开着也能再加源：面板「加源」就是这条路（不是重开会话，那会丢已挂的源）
    #[test]
    fn a_session_takes_one_more_source_after_it_is_open() {
        let dir = workdir("add_source");
        let first = file_source(&dir, "a.csv", "src_a", 2);
        let second = file_source(&dir, "b.csv", "src_b", 3);
        let session = open_for_test("add_source", &[first], None);
        session.add_source(second).expect("追加源");
        let snapshot = session.snapshot();
        assert_eq!(snapshot.sources.len(), 2, "{snapshot:?}");
        // 重名再挂要被挡下（与 open 同口径）
        let again = file_source(&dir, "c.csv", "src_b", 1);
        assert!(session.add_source(again).is_err(), "重名应被拒");
    }

    /// 数据对象面板「源行展开」：列出挂载源里的表，限定名粘进 SQL 要真能查
    #[test]
    fn listing_tables_of_a_mounted_source_gives_copiable_names() {
        let dir = workdir("list_tables");
        let source = file_source(&dir, "alpha", "alpha", 2);
        let session = open_for_test("single_source", &[source], None);

        let tables = session.list_tables("alpha").expect("列源里的表");
        let items = tables.iter().find(|t| t.name == "items").expect("有 items 表");
        assert!(!items.is_view);
        // duckdb 文件源是三段名（两段名只有 L2 源，见 AccelKind::two_segment_names）
        assert_eq!(items.schema, "main");
        assert!(items.qualified.contains("alpha"), "{}", items.qualified);
        let result = session
            .run(&format!("SELECT count(*) AS n FROM {}", items.qualified))
            .expect("限定名可查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        // 别名不在会话里：如实报错（界面据此说「为什么展不开」）
        assert!(session.list_tables("nope").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_csv_file_becomes_a_view_on_the_session() {
        let dir = workdir("file_view");
        let path = dir.join("orders.csv");
        std::fs::write(&path, "id,name\n1,a\n2,b\n").expect("写 csv");

        let session = open_for_test("empty", &[], None);
        session
            .add_file_view(path.to_str().expect("路径"), "orders_file")
            .expect("文件加载成视图");
        let ok = session.run("SELECT count(*) FROM orders_file").is_ok();
        assert!(ok, "视图应能查");
    }

    #[test]
    fn the_reader_follows_the_extension() {
        assert!(file_view_sql("/tmp/a.csv", "f").expect("csv").contains("read_csv_auto"));
        assert!(file_view_sql("/tmp/a.parquet", "f").expect("parquet").contains("read_parquet"));
        assert!(file_view_sql("/tmp/a.json", "f").expect("json").contains("read_json_auto"));
        assert!(file_view_sql("/tmp/a.xlsx", "f").expect("excel").contains("read_excel"));
    }

    #[test]
    fn an_unreadable_extension_says_why_instead_of_failing_at_query_time() {
        let error = file_view_sql("/tmp/a.docx", "f").expect_err("docx 读不了");
        assert!(error.contains("docx"), "{error}");
    }

    #[test]
    fn paths_with_quotes_are_escaped() {
        let sql = file_view_sql("/tmp/it's.csv", "f").expect("csv");
        assert!(sql.contains("it''s.csv"), "{sql}");
    }

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

    /// 底座（项目 / 全局分析库）跟着会话走：一条源都没有时，SQL 里也能直接引用它们。
    ///
    /// 用户口径（2026-11-25）：联邦与分析本质相同，`analytics` / `rds_global` 是执行面
    /// 自带的两个库，必须能被联邦 SQL 看见；底座清单变了（切项目）会话要重建。
    #[test]
    fn bases_are_mounted_with_the_session_and_fingerprinted() {
        static BASE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = BASE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let dir = workdir("bases");
        let project_db = dir.join("analytics.duckdb");
        {
            let conn = duckdb::Connection::open(&project_db).expect("建项目分析库");
            conn.execute_batch("CREATE TABLE res_demo AS SELECT 1 AS n")
                .expect("建表");
        }
        let global_db = dir.join("global_analytics.duckdb");
        {
            let conn = duckdb::Connection::open(&global_db).expect("建全局分析库");
            conn.execute_batch("CREATE TABLE G_month AS SELECT 2 AS n")
                .expect("建表");
        }
        let bases = vec![
            BaseMount {
                alias: PROJECT_BASE_ALIAS.to_string(),
                path: project_db.to_str().expect("路径").to_string(),
            },
            BaseMount {
                alias: GLOBAL_BASE_ALIAS.to_string(),
                path: global_db.to_str().expect("路径").to_string(),
            },
        ];
        set_bases(bases.clone());
        // 指纹里必须有底座：否则切项目后旧会话还挂着上个项目的表
        let with_bases = session_fingerprint(&[]);
        set_bases(Vec::new());
        let without_bases = session_fingerprint(&[]);
        assert_ne!(with_bases, without_bases, "底座变了指纹必须变");
        set_bases(bases);

        let session = open_for_test("bases_only", &[], None);
        let project = session
            .run("SELECT count(*) AS n FROM analytics.main.res_demo")
            .expect("项目分析库应可查");
        assert_eq!(project.to_rows()[0][0].to_string(), "1");
        let global = session
            .run("SELECT count(*) AS n FROM rds_global.main.G_month")
            .expect("全局分析库应可查");
        assert_eq!(global.to_rows()[0][0].to_string(), "1");

        set_bases(Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!with_bases.is_empty());
        assert!(!without_bases.is_empty());
    }

    /// 两个源跨源 join：未限定名走主源，限定名走各自
    #[test]
    fn two_sources_join_across_with_a_primary() {
        let dir = workdir("join");
        let alpha = file_source(&dir, "alpha", "alpha", 3);
        let beta = file_source(&dir, "beta", "beta", 3);
        let session = open_for_test("join", &[alpha, beta], Some("alpha"));

        let snapshot = session.snapshot();
        assert_eq!(snapshot.ready_count(), 2, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("alpha"));
        assert!(snapshot.primary_note.is_none());

        // 未限定名走主源
        let result = session
            .run("SELECT count(*) AS n FROM items")
            .expect("主源可查");
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
        let session = open_for_test("primary_fallback", &[missing, good], Some("missing"));
        let snapshot = session.snapshot();
        assert_eq!(snapshot.ready_count(), 1, "{snapshot:?}");
        assert_eq!(snapshot.primary.as_deref(), Some("good"), "回退到可用源");
        assert!(
            snapshot
                .primary_note
                .as_deref()
                .unwrap_or("")
                .contains("missing"),
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

        let result = session
            .run("SELECT count(*) AS n FROM items")
            .expect("可用源照查");
        assert_eq!(result.to_rows()[0][0].to_string(), "2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 会话内重名直接拒（两个源一个名会静默指到其中一个）
    #[test]
    fn duplicate_aliases_are_refused_before_mounting() {
        let dir = workdir("dup");
        let a = file_source(&dir, "a", "same", 1);
        let b = file_source(&dir, "b", "same", 1);
        let error = try_open_for_test("dup", &[a, b], None).expect_err("该拒");
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
        let session = open_for_test("refresh", &[first.clone(), second.clone()], Some("first"));

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
        let result = session
            .run("SELECT count(*) AS n FROM items")
            .expect("主源照常");
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
        let session = open_for_test("single_source", &[source], None);
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
            assert!(
                write_refusal(&mounted, primary, sql).is_some(),
                "该拒：{sql}"
            );
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
            assert!(
                write_refusal(&mounted, primary, sql).is_none(),
                "不该拦：{sql}"
            );
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

        let one = super::source_fingerprint(std::slice::from_ref(&alpha));
        assert_eq!(
            one,
            super::source_fingerprint(std::slice::from_ref(&alpha)),
            "同样输入同指纹"
        );
        assert_ne!(
            one,
            super::source_fingerprint(&[alpha, beta]),
            "多一个源就要换指纹"
        );
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

        let first = super::ensure_session(owner, &[alpha.clone(), beta.clone()], Some("alpha"))
            .expect("建会话");
        let count = first
            .run("SELECT count(*) AS n FROM items")
            .expect("查主源");
        assert_eq!(count.to_rows()[0][0].to_string(), "1", "主源是 alpha");

        // 源集合没变：复用同一条会话，**换主源只是在会话上切 `USE`**
        first
            .run("CREATE TEMP TABLE scratch AS SELECT 7 AS n")
            .expect("建本地临时表");
        let switched = super::ensure_session(owner, &[alpha.clone(), beta.clone()], Some("beta"))
            .expect("复用会话");
        assert!(Arc::ptr_eq(&first, &switched), "换主源不该重建会话");
        assert_eq!(switched.snapshot().primary.as_deref(), Some("beta"));
        let count = switched
            .run("SELECT count(*) AS n FROM items")
            .expect("查新主源");
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
        let count = rebuilt
            .run("SELECT count(*) AS n FROM items")
            .expect("查新源");
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

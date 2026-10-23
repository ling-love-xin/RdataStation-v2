//! 数据源导航后台任务（加载 / 预热 / 预取）。
//!
//! 为什么需要：元数据内省会走数据库驱动（依赖 tokio），在 UI 线程上 `block_on`
//! 会冻结界面。本模块用**一个分发线程 + 每任务独立工作线程**处理任务，视图只提交
//! 任务、轮询进度（原子量）并取回结果（结果队列），不参与阻塞。
//!
//! 每任务独立线程的原因：同步阻塞（rusqlite / 驱动查询）卡住时只挂起该线程，
//! 不拖累其他任务。**代价是并发无上限**——多节点同时展开会并发 acquire 撞满连接池
//! （真机：3 个 schema 展开 → 15 个 folder 任务抢 2 条池连接 → 排队 30s 超时）。
//! 因此 `LoadChildren` 执行前过全局信号量（[`LOAD_CONCURRENCY`]），执行并发受限，
//! 超出的任务在许可处排队（loading 状态保持，UI 显示「加载中」，不丢任务）。
//!
//! 任务类型：
//! - `LoadChildren`：导航树懒加载 / 分页加载（主线程 render 不再做 I/O）；
//! - `SearchIndex`：搜索框的跨连接索引搜索（命中行回填到导航树列表最前，`NavRow::SearchHit`）；
//! - `LoadProperties`：属性面板对象加载；
//! - `GenerateDml`：右键「生成 INSERT/UPDATE/DELETE」（先取列，再拼模板）；
//! - `TestConnection`：右键「测试连接」（独立会话探测，不注册连接池）；
//! - `Warm`（C1 预热，方案 C）：内省 catalogs / schemas 并写入 L2，可取消、有进度；
//! - `PrefetchColumns`（C2 邻接预取）：预取指定表 / 视图的列写入 L2，失败静默。
//!
//! 缓存写入由 `NavigatorService` 的 cache-aside 完成；本模块不直接碰缓存。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use crate::model::{NavNodeKind, NavPath, ObjectRef, PropertyRef};
use crate::nav_host::ConnectionProbe;
use property::ObjectProperties;
use crate::sql_gen::DmlKind;
use engine::persistence::FtsSearchResult;

/// 导航加载任务的全局并发上限。
///
/// 对齐 DBeaver/DataGrip 的**串行内省**（用户决策 2026-10-08：除缓存设计外全参考
/// DBeaver）：内省查询逐个执行，天然不撞连接资源、不超建服务端连接。
/// 值为 1 = 严格串行（展开多个节点时逐个加载，每次 100~300ms，用户感知可接受；
/// 此前的并行 + 池的组合在真机上反复出现 30s 超时 / pool timed out）。
const LOAD_CONCURRENCY: usize = 1;
static LOAD_SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

fn load_semaphore() -> &'static tokio::sync::Semaphore {
    LOAD_SEM.get_or_init(|| tokio::sync::Semaphore::new(LOAD_CONCURRENCY))
}

/// 列预取独立并发上限（2026-10-08 卡顿根治，app_run70 铁证）。
///
/// 背景：`PrefetchColumns` 原先**与导航共用 LOAD_CONCURRENCY(=1) 信号量**——表展开
/// 后提交的列预取（每表 5s 超时 × N 表）把唯一信号量占住 10s+，用户紧接着点其他
/// schema 的 `LoadChildren` 只能 `acquire` 阻塞排队——真机：crm schema 入队 6.26s、
/// logistics 入队 7.27s 才开始执行（任务本身 0ms，卡顿感全来自信号量排队）。
/// 修复：列预取走**独立信号量**，绝不与导航内省互抢；顺带让列预取并发 2（两个
/// schema 的表同时预取，互不阻塞）。
const PREFETCH_CONCURRENCY: usize = 2;
static PREFETCH_SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

fn prefetch_semaphore() -> &'static tokio::sync::Semaphore {
    PREFETCH_SEM.get_or_init(|| tokio::sync::Semaphore::new(PREFETCH_CONCURRENCY))
}

/// 预取目标（catalog / schema / 表或视图名）。
#[derive(Clone, Debug)]
pub struct ColumnTarget {
    pub catalog: String,
    pub schema: String,
    pub table: String,
}

/// 导航子节点加载结果（回传主线程）。
pub struct LoadResult {
    pub key: String,
    pub conn_id: String,
    pub project_root: Option<String>,
    pub path: NavPath,
    /// 本块结果的起始下标（0 = 首屏；>0 = 「加载更多」追加页）。
    pub offset: usize,
    /// 这是一次「定位」的结果：值为目标在**全量**里的绝对位次。
    ///
    /// 与「加载更多」的区别必须靠它区分（两者 `offset` 都 > 0）：
    /// 加载更多是**追加**语义（并到已有行后面），定位是**换窗**语义
    /// （当前只展示这一窗，界顶那行说清「已定位到第 N 条」并可回到开头）。
    pub jumped_to: Option<usize>,
    pub result: Result<crate::navigator_service::NavPage, String>,
}

/// 属性加载结果（回传主线程）。
pub struct PropsResult {
    pub key: String,
    pub result: Result<ObjectProperties, String>,
}

/// 生成 SQL 结果（回传主线程）。
pub struct SqlGenResult {
    pub key: String,
    pub result: Result<String, String>,
}

/// 连接测试结果（回传主线程）。
pub struct TestConnResult {
    pub conn_id: String,
    pub name: String,
    pub result: Result<String, String>,
}

/// 导航建连结果（回传主线程）。
///
/// 由宿主（`workbench::services::nav_runtime`）在**后台线程**完成建连后提交：
/// 不可达主机（mysql / PG 等）的建连含超时重试，绝不能在 UI 主线程同步等待；
/// 视图侧经 [`drain_connect_results`] 轮询取走并回填状态（成功 → 预热；失败 → 提示 + 回滚）。
pub struct ConnectResult {
    pub conn_id: String,
    pub ok: bool,
    pub message: String,
}

/// 单个类别文件夹一次最多预取的表数量（避免大 schema 触发长时间后台 IO）。
pub const PREFETCH_BATCH: usize = 20;

/// 一页导航对象数。
///
/// **为什么直接用 UI 的首批渲染常量**：分页的两侧必须同尺寸——取回一页、渲染窗口
/// 就长一页；若各用各的常量，要么取回 200 条只能显示一半，要么“加载更多”永远按不完。
pub const PAGE_SIZE: usize = workbench_shell::ui::NAV_FOLDER_PAGE_SIZE;

/// 单个导航任务的**内省超时**。
///
/// 为什么必须有：worker 是**单线程串行**，一条永不返回的内省（服务端锁等待 / 半开的 TCP /
/// 被长事务挡住的 `pg_attribute`）会让后面所有展开排队等它 —— 界面看着就是「卡住」，
/// 而 worker 其实活着（与 panic 那一路不同）。超时只结束**等待**：sqlx 的查询无法取消，
/// 那条连接要等服务端返回才回到池里，但队列能继续走、界面能拿到一条可读的错误。
///
/// 取值：与 sqlx 池默认的 `acquire_timeout`（30 s）同量级。正常内省都在毫秒级
/// （真机：PG 61 列 13-48 ms、MySQL 51 列 43 ms），只有真出事才会撞上。
pub const JOB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 超时错误的文案（界面直接显示这一句）。
fn timeout_error(what: &str) -> String {
    format!(
        "{what}超时（{} 秒未返回）：可以重试；若反复超时，检查服务端是否有长事务 / 锁等待",
        JOB_TIMEOUT.as_secs()
    )
}

/// 单个连接在一次搜索里最多返回多少条命中（跨连接累加前先各自封顶）。
pub const SEARCH_LIMIT_PER_CONN: usize = 50;

/// 一次搜索的总命中上限（多连接时防止“30 个连接 × 50 条”把命中列表与渲染拖垮）。
pub const SEARCH_MAX_HITS: usize = 300;

/// 一次索引搜索的并行度：目标连接数按此分块，每块一条 scoped 线程直开临时缓存连接。
///
/// 经验值（2026-10-14 拍板）：8 块既能把 30+ 连接的搜索摊到多核，又不至于为每次搜索
/// 开几十条线程（每条都要建连 + 跑 PRAGMA + 迁移校验，线程数不等于加速）。
pub const SEARCH_PARALLEL: usize = 8;

/// 一次索引搜索的目标连接（宿主可见且通过 facet 筛选的连接）。
#[derive(Clone, Debug)]
pub struct SearchTarget {
    pub conn_id: String,
    /// 连接显示名（命中行要显示“是哪个连接”）。
    pub label: String,
    /// 驱动 id（命中行要开属性面板，需要它标注驱动）。
    pub driver: String,
}

/// 搜索档：名称（`metadata_index` 中缀）与内容（FTS5，注释 / 数据类型）。
///
/// 两档不是同一种结果形态：名称档给短名字列表，内容档要让用户看到**为什么命中**
/// （`snippet` 带命中标记），因此 `#` 前缀单独一档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchKind {
    /// 名称档：`metadata_index` + LIKE 中缀（不受长度限制）。
    Name,
    /// 内容档：`metadata_fts`（trigram，**至少 3 个字符**，不足必无命中）。
    FullText,
}

/// 一条搜索命中（已展开成视图可直接渲染的形状）。
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub conn_id: String,
    pub conn_label: String,
    pub driver: String,
    /// 类别：`table` / `view` / `schema` / `column` / `routine`。
    pub object_type: String,
    pub object_name: String,
    /// 列命中时的所属表名。
    pub parent_name: Option<String>,
    pub catalog: Option<String>,
    pub schema: Option<String>,
    /// 内容档命中片段（带 `<mark>` 标记）；名称档为 `None`。
    pub snippet: Option<String>,
}

impl SearchHit {
    /// 命中对象的**身份**：`{连接}/{catalog}/{schema}/{父对象}/{名字}`（`ObjectRef::key` 口径）。
    ///
    /// 为什么要单独一处：导航结果行的业务键（`NavRow::key`，虚拟列表里那一行的唯一身份 /
    /// 键盘漫游锚点）与元素 id（GPUI 要求同层唯一）都要它，两边各拼一份必有一份会漂移。
    /// key 与树节点同构，于是「结果区那一行」与「树上那一行」说的是同一个对象。
    ///
    /// 索引里出现**不可寻址**的类别串时退化为「连接-类别-名字」：这类命中渲染得出来
    /// （见 `nav_object_type_label` 的兜底），只是定位不了——身份仍要唯一，不能返回空串。
    pub(crate) fn key(&self) -> String {
        let object = ObjectRef::from_index_hit(
            &self.conn_id,
            &self.object_type,
            &self.object_name,
            self.parent_name.as_deref(),
            self.catalog.as_deref(),
            self.schema.as_deref(),
        );
        match object {
            Some(object) => object.key(),
            None => format!("{}-{}-{}", self.conn_id, self.object_type, self.object_name),
        }
    }
}

/// 一次搜索的完整结果（一个批次涵盖全部目标连接）。
pub struct SearchResult {
    /// 本批次的消费方（诊断用；分发改由槽位承担，见 [`SearchConsumer`]）。
    pub consumer: SearchConsumer,
    /// 发起批次序号：视图侧据此丢弃**过期批次**（新一批已发出，旧批迟到覆盖）。
    pub seq: u64,
    /// 本次搜索词；回填方据此丢弃**过期批次**（用户已经把词改了）。
    pub query: String,
    /// 实际搜了的连接数（有缓存的那些；无缓存的连接不建文件、直接跳过）。
    pub searched: usize,
    pub hits: Vec<SearchHit>,
}

/// 索引搜索的**消费方**：同一条后台通道被多个入口用（导航面板的树顶结果行、
/// Quick Open 浮层、元数据搜索面板），结果必须按消费方分发——`drain` 是「取走」语义，
/// 不分流就是谁先取谁得，另一个入口会永远拿到空结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchConsumer {
    /// 数据源导航面板（结果行在导航列表最前）。
    Navigator,
    /// Quick Open 浮层。
    QuickOpen,
    /// 元数据搜索面板（中央「搜索: query」tab，2026-10-14 P0）。
    Panel,
}

impl SearchConsumer {
    /// 槽位数量（`search_results` / `pending_search` 两张表共用）。
    const COUNT: usize = 3;

    /// 内部槽位下标。
    fn ix(self) -> usize {
        match self {
            SearchConsumer::Navigator => 0,
            SearchConsumer::QuickOpen => 1,
            SearchConsumer::Panel => 2,
        }
    }
}

enum Job {
    LoadChildren {
        conn_id: String,
        project_root: Option<String>,
        key: String,
        path: NavPath,
        fresh: bool,
        /// 分页起始下标（0 = 首屏；>0 = 「加载更多」）。
        offset: usize,
        /// 本页上限（[`PAGE_SIZE`]）。
        limit: usize,
        /// 「定位到某个对象」：按名字先算位次，再把 `offset` 改为它所在的那一页。
        /// `None` = 普通分页（首屏 / 加载更多）。
        object: Option<String>,
    },
    LoadProperties {
        key: String,
        property: PropertyRef,
        conn_label: String,
        /// 驱动 id / 显示名（属性面板「驱动」行、fallback 用）
        driver: String,
        /// 数据库类型 id（`drivers.type_id`，如 `postgresql`）——**属性源分派键**
        type_id: Option<String>,
        /// 数据库类型的**展示文字**：目录名（`data_source_types.name`）优先，取不到时回退类型 id
        /// （`drivers.type_id`）；属性面板「数据库类型」行用。
        db_type: Option<String>,
        /// 「数据」页签分页抓取参数（`None` = 不取数据）。
        data: Option<crate::table_data::TableDataRequest>,
    },
    /// 搜索框的跨连接索引搜索（名称档中缀 / 内容档 FTS5）。
    SearchIndex {
        consumer: SearchConsumer,
        kind: SearchKind,
        query: String,
        /// 发起批次序号：与视图侧 `search_seq` 配对，回填时丢弃过期批次
        /// （同词重跑 / toggle 重排时，`query` 判据区分不了先后，seq 才能）。
        seq: u64,
        /// 搜索框「区分大小写（`Aa`）」：名称档在 LIKE 候选上做 Rust 侧 `contains` 精筛
        /// （SQLite `LIKE` 对 ASCII 不敏感，候选是超集）。
        case_sensitive: bool,
        /// 搜索框「正则（`.*`）」：名称档在 LIKE 候选上做正则精筛
        /// （LIKE 中缀候选先取，正则再过滤；锚定 / 转义类模式受候选集约束，见
        /// [`search_targets`] 注）。
        regex: bool,
        project_root: Option<String>,
        targets: Vec<SearchTarget>,
    },
    Warm {
        conn_id: String,
        project_root: Option<String>,
    },
    /// 编辑器补全的实时内省回退（§710）：把整条连接的表 / 视图 / 列按 cache-aside
    /// 写进 L2。与 [`Job::Warm`] 同策略（让位导航内省），但**不占预热进度**——
    /// 补全没做全时只给关键字与函数，进度条不该为它转。
    WarmCompletion {
        conn_id: String,
        project_root: Option<String>,
    },
    PrefetchColumns {
        conn_id: String,
        project_root: Option<String>,
        targets: Vec<ColumnTarget>,
    },
    /// 右键「生成 INSERT/UPDATE/DELETE」：取列后拼模板。
    GenerateDml {
        key: String,
        conn_id: String,
        project_root: Option<String>,
        catalog: String,
        schema: String,
        table: String,
        /// 已去重的限定名（用于模板注释与语句）。
        qualified: String,
        kind: DmlKind,
    },
    /// 只给测试用：投一个**必定 panic** 的任务，验证 worker 会把它隔离掉
    /// （以前一次 panic 会让整个导航加载永久卡死）。
    #[cfg(test)]
    PanicForTest {
        key: String,
        conn_id: String,
        project_root: Option<String>,
        path: NavPath,
    },
    /// 右键「测试连接」：独立会话探测，结果回传主线程。
    ///
    /// `probe` 由视图从宿主取（[`ConnectionProbe`]）：它是**函数指针**，故可随任务跨线程；
    /// 服务单例与桥接运行时由宿主侧函数自取，不跟着任务跑。
    TestConnection {
        conn_id: String,
        project_root: Option<String>,
        name: String,
        probe: ConnectionProbe,
    },
}

/// 共享状态：任务队列 + 进度 + 结果队列。
struct Shared {
    /// 发件端。**包一层 `Mutex`**：worker 线程一旦退出（panic / 异常返回），
    /// 通道就废了 —— 那时要换一条新通道并把发件端替换掉（见 [`restart_worker`]）。
    tx: Mutex<Sender<Job>>,
    /// worker 是否活着。投递前先看它：不活就先重启。
    worker_alive: AtomicBool,
    // 预热进度 / 取消
    warm_active: AtomicBool,
    warm_done: AtomicUsize,
    warm_total: AtomicUsize,
    warm_cancel: AtomicBool,
    // 未完成的加载（含排队与执行中）
    pending_loads: AtomicUsize,
    pending_props: AtomicUsize,
    pending_sql: AtomicUsize,
    pending_test: AtomicUsize,
    pending_connect: AtomicUsize,
    pending_search: [AtomicUsize; SearchConsumer::COUNT],
    load_results: Mutex<Vec<LoadResult>>,
    props_results: Mutex<Vec<PropsResult>>,
    sql_results: Mutex<Vec<SqlGenResult>>,
    test_results: Mutex<Vec<TestConnResult>>,
    connect_results: Mutex<Vec<ConnectResult>>,
    search_results: [Mutex<Vec<SearchResult>>; SearchConsumer::COUNT],
}

impl Shared {
    /// 把一批搜索结果交给对应消费方（worker 与单测共用，保证「分发」只有一处）。
    fn push_search_result(&self, consumer: SearchConsumer, result: SearchResult) {
        lock(&self.search_results[consumer.ix()]).push(result);
    }

    /// 取走某消费方已完成的搜索结果。
    fn take_search_results(&self, consumer: SearchConsumer) -> Vec<SearchResult> {
        std::mem::take(&mut *lock(&self.search_results[consumer.ix()]))
    }
}

static JOBS: OnceLock<Shared> = OnceLock::new();

fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        spawn_worker(rx);
        Shared {
            tx: Mutex::new(tx),
            worker_alive: AtomicBool::new(true),
            warm_active: AtomicBool::new(false),
            warm_done: AtomicUsize::new(0),
            warm_total: AtomicUsize::new(0),
            warm_cancel: AtomicBool::new(false),
            pending_loads: AtomicUsize::new(0),
            pending_props: AtomicUsize::new(0),
            pending_sql: AtomicUsize::new(0),
            pending_test: AtomicUsize::new(0),
            pending_connect: AtomicUsize::new(0),
            pending_search: std::array::from_fn(|_| AtomicUsize::new(0)),
            load_results: Mutex::new(Vec::new()),
            props_results: Mutex::new(Vec::new()),
            sql_results: Mutex::new(Vec::new()),
            test_results: Mutex::new(Vec::new()),
            connect_results: Mutex::new(Vec::new()),
            search_results: std::array::from_fn(|_| Mutex::new(Vec::new())),
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn service(project_root: Option<String>) -> crate::navigator_service::NavigatorService {
    crate::navigator_service::NavigatorService::with_context(
        engine::get_connection_manager().clone(),
        project_root,
        false,
    )
}

/// 起一个 worker 线程（`Shared` 初始化与「重启」共用）。
///
/// 线程退出时（正常返回 / panic）由 `Drop` 把存活标志落下 —— 下一次投递会把它重启。
fn spawn_worker(rx: mpsc::Receiver<Job>) {
    let spawned = std::thread::Builder::new()
        .name("rds-nav-jobs".to_string())
        // 驱动内省在 debug 下递归较深，给足栈避免溢出。
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            struct AliveGuard;
            impl Drop for AliveGuard {
                fn drop(&mut self) {
                    shared().worker_alive.store(false, Ordering::SeqCst);
                }
            }
            let _guard = AliveGuard;
            worker(rx);
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "导航后台线程起不来：导航将只能读到缓存");
        shared().worker_alive.store(false, Ordering::SeqCst);
    }
}

/// 换一条通道并把 worker 重新起起来（投递失败时走这里）。
fn restart_worker() {
    let s = shared();
    let (tx, rx) = mpsc::channel::<Job>();
    *lock(&s.tx) = tx;
    s.worker_alive.store(true, Ordering::SeqCst);
    spawn_worker(rx);
    tracing::warn!("导航后台线程已重启（上一个线程已退出）");
}

/// 提交一个任务（所有 `enqueue_*` 的统一出口）。
///
/// 三件事保证「投出去的任务一定有回音」：
/// 1. worker 不活 → 先重启；
/// 2. 投递失败（通道已断）→ 重启后**重投一次**；
/// 3. 还失败 → 按任务的凭据回填一条**可见的错误**并配平计数 ——
///    绝不留下一个永远「加载中」的节点。
fn submit(job: Job) {
    let s = shared();
    let mut job = job;
    for attempt in 0..2 {
        let tx = lock(&s.tx).clone();
        match tx.send(job) {
            Ok(()) => return,
            Err(back) => {
                job = back.0;
                if attempt == 0 {
                    restart_worker();
                }
            }
        }
    }
    let reason = "导航后台线程不可用（已重启仍投递失败）".to_string();
    tracing::error!(reason = %reason, "导航任务投递失败");
    if let Some(ticket) = job.ticket() {
        report_failure(ticket, &reason);
    }
    if let Some(counter) = job.counter() {
        counter.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 一个任务的**失败凭据**。
///
/// 为什么要在执行前取出来：`run_job` 会把 `Job` 的所有权吃进去，panic 之后就再也拿不回来 ——
/// 没有它，一次 panic 会让那个节点永远停在「加载中」（本仓真机踩到过：一个坏表把整棵树的
/// 加载卡死）。
enum Ticket {
    Load {
        key: String,
        conn_id: String,
        project_root: Option<String>,
        path: NavPath,
    },
    Props {
        key: String,
    },
    Sql {
        key: String,
    },
    Test {
        conn_id: String,
        name: String,
    },
    Search {
        consumer: SearchConsumer,
        query: String,
        /// 发起批次序号：失败收尾也要带它，让视图侧只收**当前批次**的「搜索中」收尾。
        seq: u64,
    },
    Warm,
}

impl Job {
    /// 这个任务回填结果需要的信息（`None` = 无结果可回填，如预取）。
    fn ticket(&self) -> Option<Ticket> {
        match self {
            Job::LoadChildren {
                conn_id,
                project_root,
                key,
                path,
                ..
            } => Some(Ticket::Load {
                key: key.clone(),
                conn_id: conn_id.clone(),
                project_root: project_root.clone(),
                path: path.clone(),
            }),
            Job::LoadProperties { key, .. } => Some(Ticket::Props { key: key.clone() }),
            Job::GenerateDml { key, .. } => Some(Ticket::Sql { key: key.clone() }),
            Job::TestConnection { conn_id, name, .. } => Some(Ticket::Test {
                conn_id: conn_id.clone(),
                name: name.clone(),
            }),
            Job::SearchIndex {
                consumer,
                query,
                seq,
                ..
            } => Some(Ticket::Search {
                consumer: *consumer,
                query: query.clone(),
                seq: *seq,
            }),
            Job::Warm { .. } => Some(Ticket::Warm),
            Job::WarmCompletion { .. } => None,
            Job::PrefetchColumns { .. } => None,
            #[cfg(test)]
            Job::PanicForTest {
                key,
                conn_id,
                project_root,
                path,
            } => Some(Ticket::Load {
                key: key.clone(),
                conn_id: conn_id.clone(),
                project_root: project_root.clone(),
                path: path.clone(),
            }),
        }
    }

    /// 这个任务占着哪个「未完成」计数（`None` = 不计数的任务）。
    ///
    /// **减一只在 worker 层做**（见 [`worker`]）：无论任务成功、失败还是 panic 都配平 ——
    /// 之前各分支自己减，panic 那条路一断，`has_pending_*` 就永远是 true（界面永远「加载中」）。
    fn counter(&self) -> Option<&'static AtomicUsize> {
        match self {
            Job::LoadChildren { .. } => Some(&shared().pending_loads),
            Job::LoadProperties { .. } => Some(&shared().pending_props),
            Job::GenerateDml { .. } => Some(&shared().pending_sql),
            Job::TestConnection { .. } => Some(&shared().pending_test),
            Job::SearchIndex { consumer, .. } => Some(&shared().pending_search[consumer.ix()]),
            Job::Warm { .. } | Job::PrefetchColumns { .. } | Job::WarmCompletion { .. } => None,
            #[cfg(test)]
            Job::PanicForTest { .. } => Some(&shared().pending_loads),
        }
    }
}

/// 按凭据回填一条**可见的错误**（界面据此结束「加载中」并显示原因）。
fn report_failure(ticket: Ticket, reason: &str) {
    let s = shared();
    match ticket {
        Ticket::Load {
            key,
            conn_id,
            project_root,
            path,
        } => {
            lock(&s.load_results).push(LoadResult {
                key,
                conn_id,
                project_root,
                path,
                offset: 0,
                jumped_to: None,
                result: Err(reason.to_string()),
            });
        }
        Ticket::Props { key } => {
            lock(&s.props_results).push(PropsResult {
                key,
                result: Err(reason.to_string()),
            });
        }
        Ticket::Sql { key } => {
            lock(&s.sql_results).push(SqlGenResult {
                key,
                result: Err(reason.to_string()),
            });
        }
        Ticket::Test { conn_id, name } => {
            lock(&s.test_results).push(TestConnResult {
                conn_id,
                name,
                result: Err(reason.to_string()),
            });
        }
        Ticket::Search {
            consumer,
            query,
            seq,
        } => {
            // 空命中 + `searched = 0`：视图侧据此把「搜索中…」收尾（与空目标同口径）。
            // seq 必须带上：过期批次的失败收尾同样要能被序号守卫丢弃，否则「搜索中…」
            // 会被旧批次提前收掉（新批次还在跑时标题就停了）。
            s.push_search_result(
                consumer,
                SearchResult {
                    consumer,
                    query,
                    seq,
                    searched: 0,
                    hits: Vec::new(),
                },
            );
        }
        Ticket::Warm => {
            s.warm_active.store(false, Ordering::SeqCst);
        }
    }
}

/// panic 载荷 → 一行可读文字。
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "（未知 panic 载荷）".to_string()
}

fn worker(rx: mpsc::Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        // 每任务独立线程：同步阻塞（rusqlite / 驱动查询）卡住时只挂起该线程，
        // 分发线程立即接下一任务——tokio `timeout` 对同步阻塞不生效（实测：一个
        // 卡住的查询会让整个 worker 排队、树全卡，JOB_TIMEOUT 兜不住）。
        // 并发由 [`LOAD_CONCURRENCY`] 信号量在 `run_job` 内限制（LoadChildren）。
        std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Runtime::new() else {
                return;
            };
            // 执行前先取凭据与计数：`run_job` 之后 `job` 就没了
            let ticket = job.ticket();
            let counter = job.counter();
            // **隔离**：一个坏表 / 一段坏驱动代码只毁它自己这一次任务。
            // 之前没有这层，一次 panic 会让 worker 线程直接死掉：之后所有展开都不再有回音
            // （界面永远「加载中」），且挂起的计数也让进度条停在半路。
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&rt, job)));
            if let Err(payload) = outcome {
                let reason = format!("导航任务内部错误（已隔离）：{}", panic_text(&*payload));
                tracing::error!(reason = %reason, "导航后台任务 panic");
                if let Some(ticket) = ticket {
                    report_failure(ticket, &reason);
                }
            }
            if let Some(counter) = counter {
                counter.fetch_sub(1, Ordering::SeqCst);
            }
        });
    }
}

/// 执行一个任务（结果回填在这里；**计数不在这里减**，见 [`Job::counter`]）。
fn run_job(rt: &tokio::runtime::Runtime, job: Job) {
    match job {
        Job::LoadChildren {
            conn_id,
            project_root,
            key,
            path,
            fresh,
            offset,
            limit,
            object,
        } => {
            // 限流：同一时刻最多 LOAD_CONCURRENCY 个加载任务执行（其余在信号量处排队，
            // loading 状态保持，UI 显示「加载中」）。不排队的话，多节点同时展开会
            // 并发 acquire 撞满连接池（真机：15 个 folder 任务抢 2 条池连接，30s 超时）。
            let _permit = rt.block_on(async { load_semaphore().acquire().await });
            // 竞态守卫（2026-10-08 真机）：点击连接 → `ensure_connected_for_browse`
            // 乐观标记 connected → 渲染驱动立即入队 Connection/Schema 层任务 → 建连线程
            // （50-100ms）尚未注册连接 → 任务在连接未注册时执行 → CONN_NOT_FOUND 红字
            // 闪现（随后 apply_connect_results 自动重试，但体验是「刚点就红」）。
            // 在途建连期间短等归零再执行；超过上限则照常执行（建连失败时红字是正确提示）。
            const CONNECT_WAIT_STEP_MS: u64 = 10;
            const CONNECT_WAIT_TICKS: u32 = 50; // 最多等 500ms
            let mut waited_ticks = 0u32;
            while has_pending_connect() && waited_ticks < CONNECT_WAIT_TICKS {
                rt.block_on(async {
                    tokio::time::sleep(std::time::Duration::from_millis(CONNECT_WAIT_STEP_MS)).await
                });
                waited_ticks += 1;
            }
            if waited_ticks > 0 {
                tracing::info!(
                    key = %key,
                    waited_ms = u64::from(waited_ticks) * CONNECT_WAIT_STEP_MS,
                    "导航任务等待在途建连完成"
                );
            }
            tracing::info!(conn_id = %conn_id, key = %key, path = ?path, "导航任务开始执行");
            let t_exec = std::time::Instant::now();
            let svc = crate::navigator_service::NavigatorService::with_context(
                engine::get_connection_manager().clone(),
                project_root.clone(),
                fresh,
            );
            // 定位：位次 → 页起点。算不出位次就**如实**沿用原 offset 并让 `jumped_to` 为 None
            // （界面那时会说「索引里没有它」，绝不假装定位成功）。
            let (offset, jumped_to) = match object.as_deref() {
                Some(name) => match svc.object_position(&conn_id, &path, name) {
                    Some(pos) => {
                        let page = limit.max(1);
                        (pos - (pos % page), Some(pos))
                    }
                    None => (offset, None),
                },
                None => (offset, None),
            };
            let outcome = rt.block_on(async {
                tokio::time::timeout(
                    JOB_TIMEOUT,
                    svc.load_children_page(&conn_id, &path, offset, limit),
                )
                .await
            });
            let result = match outcome {
                Ok(page) => page.map_err(|e| e.to_string()),
                Err(_) => Err(timeout_error("加载子节点")),
            };
            tracing::info!(key = %key, elapsed_ms = t_exec.elapsed().as_millis(), ok = result.is_ok(), "导航任务完成");
            // DBeaver `loadChildren` 对齐（2026-10-09）：schema 展开即后台全量内省。
            // 用户在 schema 展开瞬间看到静态类别文件夹（0ms）；预热完成后点
            // Tables / Views / … 缓存命中 0ms——不再为每次点击付建连 + SQL 等待。
            // 独立线程 + 独立 Runtime：预热任务不随本 worker 线程退出被 abort；
            // 与导航查询并发时按连接锁串行，安全。
            if let NavPath::Schema { catalog, schema } = &path {
                let mgr = engine::get_connection_manager().clone();
                let pr = project_root.clone();
                let cid = conn_id.to_string();
                let cat = catalog.clone();
                let sch = schema.clone();
                let fresh_flag = fresh;
                std::thread::spawn(move || {
                    let Ok(rt) = tokio::runtime::Runtime::new() else {
                        return;
                    };
                    rt.block_on(async move {
                        let svc = crate::navigator_service::NavigatorService::with_context(
                            mgr, pr, fresh_flag,
                        );
                        svc.warm_schema(&cid, &cat, &sch).await;
                    });
                });
            }
            lock(&shared().load_results).push(LoadResult {
                key,
                conn_id,
                project_root,
                path,
                offset,
                jumped_to,
                result,
            });
        }
        Job::LoadProperties {
            key,
            property,
            conn_label,
            driver,
            type_id,
            db_type,
            data,
        } => {
            // 属性面板详情也是元数据查询：与导航内省共用同一串行信号量
            // （DBeaver 的 Metadata 连接语义——所有元数据查询走唯一通道，不并发占池。
            // 真机：详情卡并行 acquire 会把池占满，内省任务 5s acquire 超时）。
            let _permit = rt.block_on(async { load_semaphore().acquire().await });
            let svc = service(None);
            let outcome = rt.block_on(async {
                tokio::time::timeout(
                    JOB_TIMEOUT,
                    svc.load_properties(
                        &property,
                        &conn_label,
                        &driver,
                        type_id.as_deref(),
                        db_type.as_deref(),
                        data.as_ref(),
                    ),
                )
                .await
            });
            let result = match outcome {
                Ok(props) => props.map_err(|e| e.to_string()),
                Err(_) => Err(timeout_error("加载对象属性")),
            };
            lock(&shared().props_results).push(PropsResult { key, result });
        }
        Job::Warm {
            conn_id,
            project_root,
        } => {
            // 预热也是元数据内省：与导航共用串行信号量但**让位**（不阻塞排队）。
            // 内省忙时放弃本批（下次展开时 cache-aside 会补写 L2），不让后台线程堆积。
            let Ok(_permit) = load_semaphore().try_acquire() else {
                tracing::debug!(conn_id = %conn_id, "预热让位导航内省（跳过本批）");
                return;
            };
            let state = shared();
            state.warm_active.store(true, Ordering::SeqCst);
            state.warm_cancel.store(false, Ordering::SeqCst);
            state.warm_done.store(0, Ordering::SeqCst);
            state.warm_total.store(0, Ordering::SeqCst);
            let svc = service(project_root);
            let _ = rt.block_on(svc.warm_schemas(
                &conn_id,
                |total| state.warm_total.store(total, Ordering::SeqCst),
                || state.warm_cancel.load(Ordering::SeqCst),
                |done| state.warm_done.store(done, Ordering::SeqCst),
            ));
            state.warm_active.store(false, Ordering::SeqCst);
        }
        Job::WarmCompletion {
            conn_id,
            project_root,
        } => {
            // 补全内省也是元数据内省：与导航共用串行信号量但**让位**（不阻塞排队）。
            // 内省忙时放弃本批（补全下回按键会再试），不让后台线程堆积、不抢源库。
            let Ok(_permit) = load_semaphore().try_acquire() else {
                tracing::debug!(conn_id = %conn_id, "补全内省让位导航（跳过本批）");
                return;
            };
            let svc = service(project_root);
            let _ = rt.block_on(svc.warm_completion_metadata(&conn_id));
        }
        Job::PrefetchColumns {
            conn_id,
            project_root,
            targets,
        } => {
            let svc = service(project_root);
            let tuples: Vec<(String, String, String)> = targets
                .into_iter()
                .map(|t| (t.catalog, t.schema, t.table))
                .collect();
            // 预取与内省**互不抢信号量**（2026-10-08 卡顿根治）：导航用 LOAD_SEM
            // 严格串行（DBeaver 语义），列预取用独立 PREFETCH_SEM（并发 2）——表展开
            // 后的列预取不再把唯一信号量占住 10s+ 而阻塞用户点别的 schema。
            // 仍 `try_acquire`（尽力而为）：预取信号量也忙时放弃本批（下次展开再预取）。
            let Ok(_permit) = prefetch_semaphore().try_acquire() else {
                tracing::debug!(conn_id = %conn_id, "列预取让位（跳过本批）");
                return;
            };
            // 返回成功条数；失败条目在服务内按表记 DEBUG 痕（见 `prefetch_columns`）。
            // 预取是**尽力而为**：超时只记一条 warn（它不该占住队列）。
            let prefetched = rt.block_on(async {
                tokio::time::timeout(JOB_TIMEOUT, svc.prefetch_columns(&conn_id, &tuples)).await
            });
            if prefetched.is_err() {
                tracing::warn!(
                    conn_id = %conn_id,
                    tables = tuples.len(),
                    "列预取超时（已放弃这一批；不影响导航本身的加载）"
                );
            }
        }
        Job::GenerateDml {
            key,
            conn_id,
            project_root,
            catalog,
            schema,
            table,
            qualified,
            kind,
        } => {
            // 取列也是元数据查询：与导航内省共用同一串行信号量（对齐 DBeaver
            // Metadata 连接语义——元数据查询走唯一通道，不并发占池）。
            let _permit = rt.block_on(async { load_semaphore().acquire().await });
            let svc = service(project_root);
            let result = rt.block_on(async {
                // 列走导航同一套 cache-aside（命中 L2 不发查询）；取列那次 I/O 包超时。
                let nodes = tokio::time::timeout(
                    JOB_TIMEOUT,
                    svc.load_children(
                        &conn_id,
                        &NavPath::Table {
                            catalog,
                            schema,
                            table,
                        },
                    ),
                )
                .await
                .map_err(|_| timeout_error("生成 SQL 时取列"))?
                .map_err(|e| e.to_string())?;
                let columns: Vec<crate::sql_gen::DmlColumn> = nodes
                    .iter()
                    .filter_map(|n| match &n.kind {
                        NavNodeKind::Column { primary, .. } => Some(crate::sql_gen::DmlColumn {
                            name: n.name.clone(),
                            primary: *primary,
                        }),
                        _ => None,
                    })
                    .collect();
                Ok(crate::sql_gen::dml_template(&qualified, &columns, kind))
            });
            lock(&shared().sql_results).push(SqlGenResult { key, result });
        }
        Job::SearchIndex {
            consumer,
            kind,
            query,
            seq,
            case_sensitive,
            regex,
            project_root,
            targets,
        } => {
            // 并行搜索（2026-10-14 P1）：目标连接按 `SEARCH_PARALLEL` 分块，每块一条
            // scoped 线程**直开临时缓存连接**（`NavCache::open_direct`，不走池——搜索目标
            // 是全部可见连接，池会让从未浏览过的连接也永久注册并预热，白占句柄）。
            // 线程只读各自连接自己的缓存文件（不同缓存文件 = 不同 SQLite 库），无共享可变
            // 状态，也无需抢 `LOAD_SEM` / `PREFETCH_SEM`。
            //
            // 排序：合并按「chunk 序 + target 序」保持确定性（并行只提速，不重排结果）；
            // 每目标限额固定 `SEARCH_LIMIT_PER_CONN`，全局 300 截断放在合并后统一做——
            // 尾部连接因此更公平（旧实现一边搜一边按剩余名额限流，越靠后的连接越吃亏）。
            // 块线程 panic（join 返回 Err）按空块处理：不毁整次搜索，只少这一块的命中。
            let chunk_size = targets.len().div_ceil(SEARCH_PARALLEL).max(1);
            let mut searched = 0usize;
            let mut hits = Vec::new();
            std::thread::scope(|s| {
                let handles: Vec<_> = targets
                    .chunks(chunk_size)
                    .map(|chunk| {
                        // 每块克隆查询词与项目根（至多 SEARCH_PARALLEL 份，开销可忽略）；
                        // `kind` / `case_sensitive` / `regex` 是 Copy 直接带上。scoped 线程
                        // 在 `scope` 结束前必先 join，克隆的是 String 值本身，不借 `chunk`
                        // 之外的任何栈变量。
                        let query = query.clone();
                        let project_root = project_root.clone();
                        s.spawn(move || {
                            search_targets(
                                kind,
                                &query,
                                case_sensitive,
                                regex,
                                project_root.as_deref(),
                                chunk,
                            )
                        })
                    })
                    .collect();
                // 按 chunk 序收集（join 失败 = 该块 panic → 空块，不 panic 整任务）。
                for handle in handles {
                    match handle.join() {
                        Ok((s, h)) => {
                            searched += s;
                            hits.extend(h);
                        }
                        Err(_) => {
                            tracing::error!(
                                "并行索引搜索块执行失败（该块无结果，本次搜索继续）"
                            );
                        }
                    }
                }
            });
            hits.truncate(SEARCH_MAX_HITS);
            shared().push_search_result(
                consumer,
                SearchResult {
                    consumer,
                    seq,
                    query,
                    searched,
                    hits,
                },
            );
        }
        Job::TestConnection {
            conn_id,
            project_root,
            name,
            probe,
        } => {
            // 走宿主给的探测入口，与右键「连接」同一套 URL / 网络档案规则。
            let result = probe(&conn_id, project_root.as_deref());
            lock(&shared().test_results).push(TestConnResult {
                conn_id,
                name,
                result,
            });
        }
        #[cfg(test)]
        Job::PanicForTest { .. } => panic!("测试用：故意 panic，验证 worker 会把这次任务隔离掉"),
    }
}

/// schema.object 两段拆分（DBeaver TreeFilter 口径，见竞品分析 §5.3-A④）：
/// 恰好一个点且两边非空才拆；`a.b.c` / `a.` / `.b` 不拆（整串中缀）。
///
/// 返回 `(归属段, 名称段)`：归属段优先匹配 `schema`，其次 `catalog`
/// （MySQL 无 schema 层，`库.表` 是用户直觉，两处都要认）。
fn nav_split_two_segment(query: &str) -> (Option<&str>, Option<&str>) {
    if query.matches('.').count() != 1 {
        return (None, None);
    }
    match query.split_once('.') {
        Some((a, b)) if !a.is_empty() && !b.is_empty() => (Some(a), Some(b)),
        _ => (None, None),
    }
}

/// 搜索一个连接块（单条 scoped 线程内执行；纯函数，便于确定性单测）。
///
/// 返回 `(搜了的连接数, 命中)`：无落盘缓存的连接直接跳过（搜索不建文件）；
/// 每连接命中封顶 [`SEARCH_LIMIT_PER_CONN`]（全局 300 截断由调用方在合并后统一做）。
///
/// `case_sensitive` / `regex`（搜索框 `Aa` / `.*` 开关）：名称档在 **LIKE 中缀候选**上
/// 再做 Rust 侧精筛——SQLite `LIKE` 对 ASCII 不区分大小写，候选是超集，精筛不会漏；
/// 正则同理叠加在候选集上（LIKE 已要求整词中缀命中，故 `^ord` 这类**锚定**模式与
/// `.` 通配任意字符的语义**不完整**——这是「中缀 + 精筛」架构的已知边界，面板 UI 已注明）。
/// 正则编译失败（非法模式）时退化为不精筛（日志留痕），避免一次手误让整个搜索报废。
///
/// `schema.object` 两段：名称段走 LIKE 中缀，归属段在候选上按 `schema` / `catalog`
/// 精筛（`Aa` / `.*` 精筛同样只作用于名称段——归属段是系统事实，不做模糊）。
fn search_targets(
    kind: SearchKind,
    query: &str,
    case_sensitive: bool,
    regex: bool,
    project_root: Option<&str>,
    targets: &[SearchTarget],
) -> (usize, Vec<SearchHit>) {
    let mut searched = 0usize;
    let mut hits = Vec::new();
    for target in targets {
        // 没落盘缓存的连接直接跳过：搜索不建文件（见 `cache::cache_file_exists`）。
        if !crate::cache::cache_file_exists(&target.conn_id, project_root) {
            continue;
        }
        let Some(cache) = crate::cache::NavCache::open_direct(&target.conn_id, project_root) else {
            continue;
        };
        searched += 1;
        match kind {
            SearchKind::Name => {
                let (segment_schema, segment_name) = nav_split_two_segment(query);
                // 两段时名称段单独做 LIKE 中缀（整串含 `.` 在对象名里永远中缀不中）。
                let needle = segment_name.unwrap_or(query);
                for hit in cache.search_index(needle, SEARCH_LIMIT_PER_CONN) {
                    hits.push(SearchHit {
                        conn_id: target.conn_id.clone(),
                        conn_label: target.label.clone(),
                        driver: target.driver.clone(),
                        object_type: hit.object_type,
                        object_name: hit.object_name,
                        parent_name: hit.parent_name,
                        catalog: hit.catalog_name,
                        schema: hit.schema_name,
                        snippet: None,
                    });
                }
                // 归属段精筛：schema 或 catalog 任一精确命中即可（MySQL `库.表`）。
                if let Some(schema) = segment_schema {
                    hits.retain(|h| {
                        h.schema.as_deref() == Some(schema)
                            || h.catalog.as_deref() == Some(schema)
                    });
                }
                // 「Aa」区分大小写：`contains` 精筛（Rust 字符串比较区分大小写）。
                if case_sensitive {
                    hits.retain(|h| h.object_name.contains(needle));
                }
                // 「.*」正则：正则精筛（`(?i)` 前缀与「Aa」关联动；编译失败退化为不筛）。
                if regex {
                    let pattern = if case_sensitive {
                        needle.to_string()
                    } else {
                        format!("(?i){needle}")
                    };
                    match regex::Regex::new(&pattern) {
                        Ok(re) => hits.retain(|h| re.is_match(&h.object_name)),
                        Err(e) => {
                            tracing::warn!(
                                pattern = %pattern,
                                error = %e,
                                "导航搜索正则编译失败，本轮退化为中缀匹配"
                            );
                        }
                    }
                }
            }
            SearchKind::FullText => {
                for hit in cache.search_fts(query, SEARCH_LIMIT_PER_CONN) {
                    hits.push(fts_hit_to_search_hit(target, hit));
                }
            }
        }
    }
    (searched, hits)
}

/// 提交导航树懒加载（首屏）。
pub fn enqueue_load(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    fresh: bool,
) {
    enqueue_load_page(conn_id, project_root, key, path, fresh, 0, PAGE_SIZE);
}

/// 提交导航树分页加载（`offset` = 已加载条数，即可「加载更多」）。
#[allow(clippy::too_many_arguments)]
pub fn enqueue_load_page(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    fresh: bool,
    offset: usize,
    limit: usize,
) {
    shared().pending_loads.fetch_add(1, Ordering::SeqCst);
    tracing::info!(conn_id = %conn_id, key = %key, fresh, "导航任务入队");
    submit(Job::LoadChildren {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        key: key.to_string(),
        path,
        fresh,
        offset,
        limit,
        object: None,
    });
}

/// 提交「定位到某个对象」的页加载。
///
/// 与 [`enqueue_load_page`] 分开命名（而不是加个 `Option` 参数）：调用点的意图不同——
/// 一个是在当前窗口往后翻，另一个是**换窗跳到目标**；混在一个签名里最容易接错。
pub fn enqueue_locate_page(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    object: &str,
    limit: usize,
) {
    shared().pending_loads.fetch_add(1, Ordering::SeqCst);
    submit(Job::LoadChildren {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        key: key.to_string(),
        path,
        fresh: false,
        offset: 0,
        limit,
        object: Some(object.to_string()),
    });
}

/// 提交属性加载。
///
/// `driver` = 驱动 id / 显示名（fallback「驱动」行）；`type_id` = 数据库类型 id
/// （`drivers.type_id`）——**属性源分派键**。
pub fn enqueue_properties(
    key: &str,
    property: PropertyRef,
    conn_label: &str,
    driver: &str,
    type_id: Option<&str>,
    db_type: Option<&str>,
    data: Option<crate::table_data::TableDataRequest>,
) {
    shared().pending_props.fetch_add(1, Ordering::SeqCst);
    submit(Job::LoadProperties {
        key: key.to_string(),
        property,
        conn_label: conn_label.to_string(),
        driver: driver.to_string(),
        type_id: type_id.map(str::to_string),
        db_type: db_type.map(str::to_string),
        data,
    });
}

/// 连接成功后提交预热（C1）。重复连接会重新排队。
pub fn warm_after_connect(conn_id: &str, project_root: Option<&str>) {
    submit(Job::Warm {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
    });
}

/// 编辑器补全的实时内省回退（§710）：提交「整条连接的表 / 视图 / 列 cache-aside
/// 内省」到导航队列。与导航内省共享信号量并让位；完成前补全只给关键字与函数
/// （如实，不假装有元数据），写进 L2 后下一次按键就有候选。
pub fn enqueue_warm_completion(conn_id: &str, project_root: Option<&str>) {
    submit(Job::WarmCompletion {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
    });
}

/// 提交列预取（C2）。空列表不排队。
pub fn prefetch_columns(conn_id: &str, project_root: Option<&str>, targets: Vec<ColumnTarget>) {
    if targets.is_empty() {
        return;
    }
    submit(Job::PrefetchColumns {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        targets,
    });
}

/// 提交「生成 INSERT/UPDATE/DELETE」（右键，表 / 视图）。
#[allow(clippy::too_many_arguments)]
pub fn enqueue_generate_dml(
    key: &str,
    conn_id: &str,
    project_root: Option<&str>,
    catalog: &str,
    schema: &str,
    table: &str,
    qualified: &str,
    kind: DmlKind,
) {
    shared().pending_sql.fetch_add(1, Ordering::SeqCst);
    submit(Job::GenerateDml {
        key: key.to_string(),
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        catalog: catalog.to_string(),
        schema: schema.to_string(),
        table: table.to_string(),
        qualified: qualified.to_string(),
        kind,
    });
}

/// 提交「测试连接」（右键）。
/// 提交连接测试（独立会话探测）。
///
/// `probe` 由视图从宿主取（`NavHost::connection_probe`）；见 [`ConnectionProbe`] 的跨线程约定。
pub fn enqueue_test_connection(
    conn_id: &str,
    project_root: Option<&str>,
    name: &str,
    probe: ConnectionProbe,
) {
    shared().pending_test.fetch_add(1, Ordering::SeqCst);
    submit(Job::TestConnection {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        name: name.to_string(),
        probe,
    });
}

/// 是否仍有未完成的树加载（排队或执行中）。
pub fn has_pending_loads() -> bool {
    shared().pending_loads.load(Ordering::SeqCst) > 0
}

/// 是否仍有未完成的属性加载。
pub fn has_pending_props() -> bool {
    shared().pending_props.load(Ordering::SeqCst) > 0
}

/// 是否仍有未完成的「生成 SQL」。
pub fn has_pending_sql() -> bool {
    shared().pending_sql.load(Ordering::SeqCst) > 0
}

/// 是否仍有未完成的「测试连接」。
pub fn has_pending_test() -> bool {
    shared().pending_test.load(Ordering::SeqCst) > 0
}

/// FTS 命中 → 统一命中形状（纯映射，便于单测）。
///
/// 两处不对称要当心：
/// - FTS 行的 `schema_name` / `parent_name` 是**空串**（不是 NULL），转成 `Option` 时要过滤；
/// - FTS 表里没有 catalog（名称档靠 JOIN `schemata` 拿），所以内容档的 `catalog` 为 `None`。
fn fts_hit_to_search_hit(target: &SearchTarget, hit: FtsSearchResult) -> SearchHit {
    let non_empty = |s: String| if s.is_empty() { None } else { Some(s) };
    SearchHit {
        conn_id: target.conn_id.clone(),
        conn_label: target.label.clone(),
        driver: target.driver.clone(),
        object_type: hit.search_type,
        object_name: hit.object_name,
        parent_name: non_empty(hit.parent_name),
        catalog: None,
        schema: non_empty(hit.schema_name),
        snippet: Some(hit.snippet),
    }
}

/// 某消费方是否仍有未完成的索引搜索。
pub fn has_pending_search(consumer: SearchConsumer) -> bool {
    shared().pending_search[consumer.ix()].load(Ordering::SeqCst) > 0
}

/// 直接投递一批搜索命中（`SearchConsumer::QuickOpen` 的定位窗口测试用）。
///
/// **为什么开这个小口**：`⌥↵ 在树中定位` 的窗口级测试需要一行**真元数据命中**；
/// 而真命中的前置是「连接 + 冷启动内省 + 索引重建」——在窗口测试里不现实。
/// 伪造队列里的那一批，比伪造整条搜索链路或整棵导航树诚实得多。
/// 生产路径上只有 worker 会调 `push_search_result`。
pub fn push_search_results_for_test(consumer: SearchConsumer, result: SearchResult) {
    shared().push_search_result(consumer, result);
}

/// 提交跨连接索引搜索（按消费方分槽）。
///
/// 空目标（无可见连接）也走一趟：回传一个 `searched = 0` 的空结果，
/// 让视图侧能把「搜索中…」收尾（否则结果区标题会一直停在那一句）。
pub fn enqueue_search(
    consumer: SearchConsumer,
    seq: u64,
    kind: SearchKind,
    query: &str,
    case_sensitive: bool,
    regex: bool,
    project_root: Option<&str>,
    targets: Vec<SearchTarget>,
) {
    shared().pending_search[consumer.ix()].fetch_add(1, Ordering::SeqCst);
    submit(Job::SearchIndex {
        consumer,
        kind,
        query: query.to_string(),
        seq,
        case_sensitive,
        regex,
        project_root: project_root.map(|s| s.to_string()),
        targets,
    });
}

/// 取走已完成的树加载结果。
pub fn drain_load_results() -> Vec<LoadResult> {
    std::mem::take(&mut *lock(&shared().load_results))
}

/// 取走已完成的属性加载结果。
pub fn drain_props_results() -> Vec<PropsResult> {
    std::mem::take(&mut *lock(&shared().props_results))
}

/// 取走已完成的「生成 SQL」结果。
pub fn drain_sql_results() -> Vec<SqlGenResult> {
    std::mem::take(&mut *lock(&shared().sql_results))
}

/// 取走已完成的「测试连接」结果。
pub fn drain_test_results() -> Vec<TestConnResult> {
    std::mem::take(&mut *lock(&shared().test_results))
}

/// 取走已完成的「导航建连」结果。
pub fn drain_connect_results() -> Vec<ConnectResult> {
    std::mem::take(&mut *lock(&shared().connect_results))
}

/// 标记一次后台建连已开始（workbench 调度入口调用）：让导航轮询泵在有连接在途时
/// 保持存活，连接结果（可能 30s 后才到）才能被回填，而不是 idle 提前退出。
pub fn connect_pending_inc() {
    shared().pending_connect.fetch_add(1, Ordering::SeqCst);
}

/// 后台建连线程结束（无论成败）时调用，与 [`connect_pending_inc`] 成对。
pub fn connect_pending_dec() {
    shared().pending_connect.fetch_sub(1, Ordering::SeqCst);
}

/// 是否有后台建连在途。
pub fn has_pending_connect() -> bool {
    shared().pending_connect.load(Ordering::SeqCst) > 0
}

/// 宿主（workbench）在后台线程完成建连后提交结果（成功 / 失败都回传）。
///
/// 视图侧经 [`ensure_nav_pump`](crate::nav_view::actions::NavigatorActions::ensure_nav_pump)
/// 轮询取走：成功 → 标记已连接并触发预热；失败 → 回滚乐观标记并提示。
pub fn submit_connect_outcome(conn_id: &str, ok: bool, message: &str) {
    lock(&shared().connect_results).push(ConnectResult {
        conn_id: conn_id.to_string(),
        ok,
        message: message.to_string(),
    });
}

/// 取走某消费方已完成的索引搜索结果（另一个消费方的批次不受影响）。
pub fn drain_search_results(consumer: SearchConsumer) -> Vec<SearchResult> {
    shared().take_search_results(consumer)
}

/// 当前是否有预热任务在跑。
pub fn warm_active() -> bool {
    shared().warm_active.load(Ordering::SeqCst)
}

/// 预热进度 `(完成, 总数)`。
pub fn warm_progress() -> (usize, usize) {
    let s = shared();
    (
        s.warm_done.load(Ordering::SeqCst),
        s.warm_total.load(Ordering::SeqCst),
    )
}

/// 请求取消当前预热（在 catalog 之间生效）。
pub fn cancel_warm() {
    shared().warm_cancel.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用 `Shared`：不起 worker（不发 job），只验「分发 / 取走」两件事。
    fn test_shared() -> Shared {
        let (tx, _rx) = mpsc::channel::<Job>();
        Shared {
            tx: Mutex::new(tx),
            worker_alive: AtomicBool::new(true),
            warm_active: AtomicBool::new(false),
            warm_done: AtomicUsize::new(0),
            warm_total: AtomicUsize::new(0),
            warm_cancel: AtomicBool::new(false),
            pending_loads: AtomicUsize::new(0),
            pending_props: AtomicUsize::new(0),
            pending_sql: AtomicUsize::new(0),
            pending_test: AtomicUsize::new(0),
            pending_connect: AtomicUsize::new(0),
            pending_search: std::array::from_fn(|_| AtomicUsize::new(0)),
            load_results: Mutex::new(Vec::new()),
            props_results: Mutex::new(Vec::new()),
            sql_results: Mutex::new(Vec::new()),
            test_results: Mutex::new(Vec::new()),
            connect_results: Mutex::new(Vec::new()),
            search_results: std::array::from_fn(|_| Mutex::new(Vec::new())),
        }
    }

    fn result(consumer: SearchConsumer, query: &str) -> SearchResult {
        SearchResult {
            consumer,
            seq: 0,
            query: query.to_string(),
            searched: 1,
            hits: Vec::new(),
        }
    }

    /// 三个消费方的槽互不可见：取走自己的批次不会动到对方（本仓曾经的“互抢”就是这里错的）。
    #[test]
    fn search_results_are_partitioned_by_consumer() {
        let shared = test_shared();
        shared.push_search_result(
            SearchConsumer::Navigator,
            result(SearchConsumer::Navigator, "nav"),
        );
        shared.push_search_result(
            SearchConsumer::QuickOpen,
            result(SearchConsumer::QuickOpen, "qo"),
        );
        shared.push_search_result(
            SearchConsumer::Panel,
            result(SearchConsumer::Panel, "panel"),
        );

        let quick_open = shared.take_search_results(SearchConsumer::QuickOpen);
        assert_eq!(quick_open.len(), 1);
        assert_eq!(quick_open[0].query, "qo");

        // Quick Open 取走自己的批次后，导航 / 面板的批次仍在各自槽里
        let nav = shared.take_search_results(SearchConsumer::Navigator);
        assert_eq!(nav.len(), 1);
        assert_eq!(nav[0].query, "nav");
        let panel = shared.take_search_results(SearchConsumer::Panel);
        assert_eq!(panel.len(), 1);
        assert_eq!(panel[0].query, "panel");

        // `take` 是取走语义：再取为空
        assert!(
            shared
                .take_search_results(SearchConsumer::QuickOpen)
                .is_empty()
        );
        assert!(
            shared
                .take_search_results(SearchConsumer::Navigator)
                .is_empty()
        );
        assert!(
            shared
                .take_search_results(SearchConsumer::Panel)
                .is_empty()
        );
    }

    /// 内容档映射：空串转 `None`、catalog 缺位、snippet 带上。
    #[test]
    fn fts_hit_maps_empty_strings_and_keeps_snippet() {
        let target = SearchTarget {
            conn_id: "P_conn".to_string(),
            label: "营销分析".to_string(),
            driver: "postgres".to_string(),
        };
        let mapped = fts_hit_to_search_hit(
            &target,
            FtsSearchResult {
                search_type: "column".to_string(),
                schema_name: "public".to_string(),
                object_name: "channel_code".to_string(),
                parent_name: String::new(), // FTS 行里的“无父对象”是空串
                snippet: "…下单<mark>渠道</mark>…".to_string(),
            },
        );
        assert_eq!(mapped.conn_id, "P_conn");
        assert_eq!(mapped.object_type, "column");
        assert_eq!(mapped.object_name, "channel_code");
        assert_eq!(mapped.parent_name, None, "空串不能当父对象名");
        assert_eq!(mapped.schema.as_deref(), Some("public"));
        assert_eq!(mapped.catalog, None, "FTS 表里没有 catalog");
        assert!(
            mapped
                .snippet
                .as_deref()
                .unwrap_or_default()
                .contains("<mark>"),
            "内容档要把 snippet 带出去（UI 靠它显示“为什么命中”）"
        );
    }

    /// 计数归零是**异步**的：worker 先回填结果、后 `fetch_sub`（见 [`worker`]），且
    /// 并发套件下兄弟测试的在途任务也会让全局计数暂时非零。结果 drain 到手后
    /// 还要给计数一个归零窗口再断言，否则 15s 轮询结束的最后一个微窗口会偶发误报
    /// （2026-10-15 并发负载下实测 flake 一次；隔离复跑 5/5 通过）。
    fn settle_pending_loads(max_wait: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + max_wait;
        while std::time::Instant::now() < deadline {
            if !has_pending_loads() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        !has_pending_loads()
    }

    /// **一次 panic 不再拖垮整条导航加载队列**（真机踩到的形态：一个坏表 → worker 线程死掉
    /// → 之后所有展开都没有回音，界面永远「加载中」，且挂起计数让进度条停在半路）。
    ///
    /// 这条用例走**真实全局 worker**（`enqueue_*` 的生产路径）：先投一个必定 panic 的任务，
    /// 再投一个正常任务（连接不存在 → 回填 Err）。两个都要有回音，且计数要归零。
    #[test]
    fn worker_isolates_panics_and_recovers_from_a_dead_channel() {
        let path = NavPath::Connection;
        submit(Job::PanicForTest {
            key: "panic-test".to_string(),
            conn_id: "P_no_such_conn".to_string(),
            project_root: None,
            path: path.clone(),
        });
        shared().pending_loads.fetch_add(1, Ordering::SeqCst);
        // 贴着生产入口再投一个正常任务
        enqueue_load("P_no_such_conn", None, "after-panic", path, false);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut results = Vec::new();
        while std::time::Instant::now() < deadline {
            results.extend(drain_load_results());
            if results.len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let keys: Vec<&str> = results.iter().map(|r| r.key.as_str()).collect();
        assert!(
            keys.contains(&"panic-test") && keys.contains(&"after-panic"),
            "两个任务都要有回音（panic 的那个回错误、后面的正常跑完）：{keys:?}"
        );
        for r in &results {
            assert!(r.result.is_err(), "两者都该是错误结果：{}", r.key);
        }
        assert!(
            settle_pending_loads(std::time::Duration::from_secs(5)),
            "计数必须配平：panic 那条路曾经漏减，界面会永远「加载中」"
        );

        // ---- 自愈：把发件端换到一条**接收端已丢弃**的通道（模拟 worker 线程已退出），
        // 下一次投递应当自动换通道 + 重启 worker，任务照样有回音。
        let (dead_tx, dead_rx) = mpsc::channel::<Job>();
        drop(dead_rx);
        *lock(&shared().tx) = dead_tx;
        shared().worker_alive.store(false, Ordering::SeqCst);

        enqueue_load(
            "P_no_such_conn",
            None,
            "after-restart",
            NavPath::Connection,
            false,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut recovered = Vec::new();
        while std::time::Instant::now() < deadline {
            recovered.extend(drain_load_results());
            if !recovered.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let recovered_keys: Vec<String> = recovered.iter().map(|r| r.key.clone()).collect();
        assert!(
            recovered.iter().any(|r| r.key == "after-restart"),
            "worker 死掉后任务要能自愈（换通道 + 重启）：{recovered_keys:?}"
        );
        assert!(
            settle_pending_loads(std::time::Duration::from_secs(5)),
            "自愈那条路同样要配平计数"
        );
    }

    /// 槽位下标两两不同且落在 `COUNT` 内（`ix()` 与 `COUNT` 的契约）。
    #[test]
    fn consumer_slots_are_distinct() {
        assert_ne!(
            SearchConsumer::Navigator.ix(),
            SearchConsumer::QuickOpen.ix()
        );
        assert_ne!(
            SearchConsumer::Navigator.ix(),
            SearchConsumer::Panel.ix()
        );
        assert_ne!(
            SearchConsumer::QuickOpen.ix(),
            SearchConsumer::Panel.ix()
        );
        assert!(SearchConsumer::Navigator.ix() < SearchConsumer::COUNT);
        assert!(SearchConsumer::QuickOpen.ix() < SearchConsumer::COUNT);
        assert!(SearchConsumer::Panel.ix() < SearchConsumer::COUNT);
    }

    fn search_temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_navsearch_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// 播种一个连接缓存：schema + 一张表 + 一列，重建索引（此后 `search_index` 可命中）。
    fn seed_cache(conn_id: &str, root: &std::path::Path, table: &str) {
        use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};
        let mut cache = crate::cache::NavCache::open(conn_id, Some(&root.to_string_lossy())).expect("打开缓存");
        cache.put_schemas("main", &["public".to_string()]);
        cache.mark_schema_complete("main", "public");
        let sid = cache.schema_id("main", "public").expect("schema_id");
        cache.put_objects(sid, &[NodeInfo::new(table, SchemaObjectKind::Table)]);
        cache.put_columns(
            sid,
            table,
            &[ColumnDetail {
                name: "order_id".to_string(),
                data_type: "INTEGER".to_string(),
                nullable: false,
                is_primary_key: true,
                is_foreign_key: false,
                default_value: None,
                comment: None,
                is_identity: false,
                identity_generation: None,
                collation: None,
                extra: Default::default(),
                references: None,
                ordinal: 0,
            }],
        );
        cache.rebuild_index("main", "public");
    }

    fn target(conn_id: &str) -> SearchTarget {
        SearchTarget {
            conn_id: conn_id.to_string(),
            label: conn_id.to_string(),
            driver: "postgres".to_string(),
        }
    }

    /// schema.object 两段拆分：恰好一个点且两边非空才拆；多段 / 空段 / 无点不拆。
    #[test]
    fn split_two_segment_only_on_single_dot() {
        use super::nav_split_two_segment;
        assert_eq!(nav_split_two_segment("public.orders"), (Some("public"), Some("orders")));
        assert_eq!(nav_split_two_segment("orders"), (None, None));
        assert_eq!(nav_split_two_segment("a.b.c"), (None, None));
        assert_eq!(nav_split_two_segment("a."), (None, None));
        assert_eq!(nav_split_two_segment(".b"), (None, None));
    }

    /// 两段搜索：名称段 LIKE 中缀 + 归属段精筛（schema/catalog 任一命中）。
    /// seed_cache 把表播在 `public` schema 下：`public.orders` 命中、`other.orders` 不中。
    #[test]
    fn two_segment_search_filters_by_schema() {
        let root = search_temp_root("twoseg");
        let root_s = root.to_string_lossy().to_string();
        seed_cache("P_two_0", &root, "orders");
        let targets = [target("P_two_0")];

        let (_, hits) = search_targets(
            SearchKind::Name,
            "public.orders",
            false,
            false,
            Some(&root_s),
            &targets,
        );
        assert!(
            hits.iter().any(|h| h.object_name == "orders"),
            "public.orders 应命中 public 下的 orders：{hits:?}"
        );

        let (_, hits) = search_targets(
            SearchKind::Name,
            "other.orders",
            false,
            false,
            Some(&root_s),
            &targets,
        );
        assert!(
            hits.is_empty(),
            "other.orders 不得命中 public 下的 orders：{hits:?}"
        );

        // 单段词不受影响（回归）：`orders` 仍中缀命中。
        let (_, hits) = search_targets(
            SearchKind::Name,
            "orders",
            false,
            false,
            Some(&root_s),
            &targets,
        );
        assert!(hits.iter().any(|h| h.object_name == "orders"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 纯函数 `search_targets`：保序（target 序）、跳过无缓存连接、每连接命中封顶。
    #[test]
    fn search_targets_keeps_target_order_and_skips_missing_cache() {
        let root = search_temp_root("chunk");
        let root_s = root.to_string_lossy().to_string();
        // 只播前两个连接的缓存；第三个不建（模拟从未内省过的连接）。
        seed_cache("P_chunk_a", &root, "orders_a");
        seed_cache("P_chunk_b", &root, "orders_b");
        let targets = [target("P_chunk_a"), target("P_chunk_b"), target("P_chunk_c")];

        let (searched, hits) = search_targets(
            SearchKind::Name,
            "order",
            false,
            false,
            Some(&root_s),
            &targets,
        );
        assert_eq!(searched, 2, "无缓存的连接不计入 searched");
        // 保序：按 target 序（a 的命中在前，b 的在后）
        let conns: Vec<&str> = hits.iter().map(|h| h.conn_id.as_str()).collect();
        let a_first = conns.iter().position(|&c| c == "P_chunk_a").unwrap();
        let b_first = conns.iter().position(|&c| c == "P_chunk_b").unwrap();
        assert!(a_first < b_first, "块内保序：{conns:?}");
        assert!(
            hits.iter().all(|h| h.conn_id != "P_chunk_c"),
            "无缓存连接不得有命中"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 搜索开关精筛（`Aa` 大小写 / `.*` 正则）：LIKE 中缀候选是超集，精筛只收不减；
    /// 正则编译失败退化为不筛（不 panic）。
    ///
    /// 注意正则模式 = 搜索词本身：LIKE 已要求该词**字面**中缀命中，故 `^` 锚定、
    /// `.` 通配、字符类这类「LIKE 字面不匹配」的模式天然无候选（架构边界，见
    /// [`search_targets`]）；正则开关在本架构里的实际能力是大小写联动（`(?i)` 前缀）
    /// 与非法模式退化。本测试按此真实行为断言。
    #[test]
    fn search_targets_applies_case_and_regex_flags() {
        let root = search_temp_root("flags");
        let root_s = root.to_string_lossy().to_string();
        seed_cache("P_f_a", &root, "Orders");
        seed_cache("P_f_b", &root, "orders");
        seed_cache("P_f_c", &root, "orders[");
        let targets = [
            target("P_f_a"),
            target("P_f_b"),
            target("P_f_c"),
        ];
        let names = |hits: &[SearchHit]| -> std::collections::BTreeSet<String> {
            hits.iter()
                .map(|h| h.object_name.clone())
                .collect::<std::collections::BTreeSet<String>>()
        };

        // 默认（LIKE 不敏感）：三种大小写/字形都命中（列 order_id 不含 "orders" 字面，不中）。
        let (_, hits) =
            search_targets(SearchKind::Name, "orders", false, false, Some(&root_s), &targets);
        assert_eq!(
            names(&hits),
            ["Orders", "orders", "orders["]
                .into_iter()
                .map(String::from)
                .collect(),
            "默认全中"
        );

        // `Aa`：只留精确小写（`contains` 是中缀，`orders[` 含 `orders` 也在列）。
        let (_, hits) =
            search_targets(SearchKind::Name, "orders", true, false, Some(&root_s), &targets);
        assert_eq!(
            names(&hits),
            ["orders", "orders["].into_iter().map(String::from).collect(),
            "区分大小写只留小写"
        );

        // 正则（忽略大小写）：pattern = "(?i)orders"，`(?i)` 生效——`Orders` 保留
        //（若没 (?i) 会被正则大小写敏感排除）；中缀无锚定，`orders[` 也在。
        let (_, hits) =
            search_targets(SearchKind::Name, "orders", false, true, Some(&root_s), &targets);
        assert_eq!(
            names(&hits),
            ["Orders", "orders", "orders["]
                .into_iter()
                .map(String::from)
                .collect(),
            "正则 (?i) 生效：Orders 保留"
        );

        // 正则 + 大小写：pattern = "orders"（无 (?i)）→ `Orders` 被正则敏感排除，
        // `orders[` 仍中（无锚定）。
        let (_, hits) =
            search_targets(SearchKind::Name, "orders", true, true, Some(&root_s), &targets);
        assert_eq!(
            names(&hits),
            ["orders", "orders["].into_iter().map(String::from).collect(),
            "正则 + 大小写：排除 Orders"
        );

        // 非法正则：退化为不筛（候选 `orders[` 保留，不 panic）。
        let (_, hits) =
            search_targets(SearchKind::Name, "orders[", false, true, Some(&root_s), &targets);
        assert_eq!(
            names(&hits),
            ["orders["].into_iter().map(String::from).collect(),
            "非法正则退化为不筛"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 并行路径走真实 worker：分块并行后按「chunk 序 + target 序」合并，`searched` 全计，
    /// 全局 300 截断在合并后统一生效。
    #[test]
    fn parallel_search_merges_in_order_and_truncates_globally() {
        let root = search_temp_root("parallel");
        let root_s = root.to_string_lossy().to_string();
        let conn_ids = ["P_par_0", "P_par_1", "P_par_2", "P_par_3", "P_par_4", "P_par_5"];
        for (i, conn_id) in conn_ids.iter().enumerate() {
            seed_cache(conn_id, &root, &format!("orders_{i}"));
        }
        let targets: Vec<SearchTarget> = conn_ids.iter().map(|c| target(c)).collect();

        enqueue_search(
            SearchConsumer::Panel,
            0,
            SearchKind::Name,
            "order",
            false,
            false,
            Some(&root_s),
            targets,
        );
        // 轮询等 worker 回填（与 `worker_isolates_panics…` 同一 deadline 模式）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut results = Vec::new();
        while std::time::Instant::now() < deadline {
            results.extend(drain_search_results(SearchConsumer::Panel));
            if !results.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!results.is_empty(), "并行搜索应有回音");
        let batch = &results[0];
        assert_eq!(batch.searched, 6, "全部有缓存的连接都应被搜到");
        // 保序：6 张表按 target 序（orders_0..orders_5）出现。
        // 注意每连接还命中列 `order_id`（也是中缀命中），所以整列表是
        // [orders_0, order_id, orders_1, order_id, …]——只断言表命中的相对顺序。
        let table_names: Vec<&str> = batch
            .hits
            .iter()
            .filter(|h| h.object_name.starts_with("orders_"))
            .map(|h| h.object_name.as_str())
            .collect();
        let expected: Vec<String> = (0..6).map(|i| format!("orders_{i}")).collect();
        assert_eq!(table_names, expected, "并行只提速不重排结果");
        // 同一连接内：表命中的所属连接正确
        assert!(
            batch.hits.iter().all(|h| h.object_name == "order_id" || h.conn_id.starts_with("P_par_")),
            "命中所属连接应与 target 一致"
        );
        assert!(
            batch.hits.len() <= SEARCH_MAX_HITS,
            "全局命中数不得超过 SEARCH_MAX_HITS"
        );
        assert!(!has_pending_search(SearchConsumer::Panel), "计数必须配平");
        let _ = std::fs::remove_dir_all(&root);
    }
}

//! 后台任务**调度与执行**：全局任务队列（优先级堆 + 分发线程 + 每任务工作线程）、
//! 信号量限流、执行体（[`run_job`]）与对外接口（enqueue / drain / has_pending / warm）。
//! 数据模型见 [`model`]，搜索命中模型见 [`hits`]。

use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};

use crate::model::{NavNodeKind, NavPath, PropertyRef};
use crate::nav_host::ConnectionProbe;
use crate::sql_gen::DmlKind;
use engine::persistence::NameMatchMode;

use super::hits::{
    search_targets, SearchConsumer, SearchKind,
    SearchResult, SearchTarget, SEARCH_MAX_HITS, SEARCH_PARALLEL,
};
use super::model::{
    ColumnTarget, ConnectResult, LoadResult, PropsResult, RowEstimateResult, SqlGenResult,
    TestConnResult, JOB_TIMEOUT, PAGE_SIZE,
    parse_name_value_pairs, timeout_error,
};

/// 每连接元数据任务并发上限 = 1：同连接内省 / 属性 / 取列严格串行（驱动单连接 Mutex
/// 语义保持），不并发占池。信号量只代表「同连接任务串行」，不持有连接句柄 / 配置、
/// 无会话状态；条目随连接断开 / 删除回收（见 [`drop_conn_sem`]）。
const PER_CONN_CONCURRENCY: usize = 1;

/// 全局软帽：跨连接并发上界（B-2-A 起步 K=3，真机 V3 数据复测后定稿；不取消软帽）。
/// 在 per-conn 信号量之外套一层，只挡排队、不改变 per-conn 互斥；防止连接数多时
/// 任务线程 / 独立 Runtime 数失控。
const LOAD_GLOBAL_CAP: usize = 3;

/// 结果回填泵间隔（D8，对齐 DBeaver `DBNModel` 事件批量合并窗口 100ms 口径）：
/// 一轮内 drain 全部六类结果后单次重绘，避免每类结果各自触发一次状态写入与
/// 布局测量。可配置口径 = 常量集中一处，本批不接 settings 面板。
pub(crate) const NAV_PUMP_INTERVAL_MS: u64 = 100;
static GLOBAL_LOAD_SEM: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

/// 预热起步延迟（S2-③，zqlz `schema_service.rs:49` 同值 3s）。
///
/// 连接成功后的整库预热 / 补全内省**延迟 3s 再入队**：让用户连接后的首次浏览查询
/// 先走（它才是关键路径），预热再进场与内省竞争信号量。延迟用独立短线程实现——
/// 不占 worker 主循环、不堵优先级队列（S2-① 的插队不受影响）。
///
/// 规模阈值（zqlz `BULK_COLUMN_TABLE_LIMIT=1500`）已评估**无需引入**：zqlz 用它切换
/// 「bulk 全量扫描 → 逐表 paced」两条取数路径；我们的 `warm_schemas` 本就是逐 schema
/// 遍历（无 unfiltered 全量扫描），结构上已规避该问题，加了阈值反而多一次预查询。
const WARM_START_DELAY: std::time::Duration = std::time::Duration::from_secs(3);

pub(super) fn global_load_semaphore() -> Arc<tokio::sync::Semaphore> {
    GLOBAL_LOAD_SEM
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(LOAD_GLOBAL_CAP)))
        .clone()
}

/// 列预取独立并发上限（2026-10-08 卡顿根治，app_run70 铁证）。
///
/// 背景：`PrefetchColumns` 原先**与导航共用旧全局串行信号量（容量 1）**——表展开
/// 后提交的列预取（每表 5s 超时 × N 表）把唯一信号量占住 10s+，用户紧接着点其他
/// schema 的 `LoadChildren` 只能 `acquire` 阻塞排队——真机：crm schema 入队 6.26s、
/// logistics 入队 7.27s 才开始执行（任务本身 0ms，卡顿感全来自信号量排队）。
/// 修复：列预取走**独立信号量**，绝不与导航内省互抢；顺带让列预取并发 2（两个
/// schema 的表同时预取，互不阻塞）。
const PREFETCH_CONCURRENCY: usize = 2;
static PREFETCH_SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

pub(super) fn prefetch_semaphore() -> &'static tokio::sync::Semaphore {
    PREFETCH_SEM.get_or_init(|| tokio::sync::Semaphore::new(PREFETCH_CONCURRENCY))
}

/// 取（必要时新建）所属连接的信号量。
///
/// 同一 `conn_id` 复用同一 `Arc`；断开 / 删除经 [`drop_conn_sem`] 移除条目后，
/// 重连经 `entry` 新建全新信号量，不复用旧条目残留令牌。
pub(crate) fn per_conn_semaphore(conn_id: &str) -> Arc<tokio::sync::Semaphore> {
    let mut map = lock(&shared().conn_sems);
    map.entry(conn_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(PER_CONN_CONCURRENCY)))
        .clone()
}

/// 断开 / 删除连接后回收信号量条目：`Arc` 计数归零即释放；在途任务持有的克隆不受影响。
/// 两条生命周期路径（disconnect / delete_connection）都调本函数，幂等：重复调用无害。
pub fn drop_conn_sem(conn_id: &str) {
    lock(&shared().conn_sems).remove(conn_id);
}

/// 阻塞族元数据任务取锁（`LoadChildren` / `LoadProperties` / `GenerateDml`）：
/// 先取所属连接信号量（同连接串行），再取全局软帽（跨连接上界）。
///
/// 取锁层级固定为 per-conn → 全局、单向获取，不存在反向加锁，无死锁；先占 per-conn
/// 再抢全局，避免排队任务占住全局名额饿死其他连接。两个许可保活到任务结束，退出作用域
/// 即依次归还。
pub(super) fn acquire_meta_permit(
    rt: &tokio::runtime::Runtime,
    conn_id: &str,
) -> (
    tokio::sync::OwnedSemaphorePermit,
    tokio::sync::OwnedSemaphorePermit,
) {
    let per_conn = per_conn_semaphore(conn_id);
    rt.block_on(async move {
        let per = per_conn.acquire_owned().await.expect("per-conn 信号量不会关闭");
        let global = global_load_semaphore()
            .acquire_owned()
            .await
            .expect("全局软帽不会关闭");
        (per, global)
    })
}

/// 让位族任务取锁（`Warm` / `WarmCompletion`）：所属连接忙即跳过本批，不排队、
/// 不套全局软帽（B-2-C：维持现状 `try_acquire` 口径，不引入新等待）。
/// 返回持锁许可（`None` = 跳过）；许可保活到任务结束。
pub(super) fn try_acquire_warm_permit(conn_id: &str) -> Option<tokio::sync::OwnedSemaphorePermit> {
    per_conn_semaphore(conn_id).try_acquire_owned().ok()
}

/// 预取目标（catalog / schema / 表或视图名）。
pub(crate) enum Job {
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
        /// S3-① 可见库白名单快照（入队时从宿主取）：`None` = 默认模式隐藏系统库 /
        /// 系统 schema；`Some` = 白名单模式。与任务一起快照——执行时连接配置可能
        /// 已变（白名单本身在 settings，不随连接配置走，但保持一致的成本极低）。
        visible_databases: Option<std::collections::BTreeSet<String>>,
        /// P1 导航设置快照（2026-10-25）：连接级「显示系统对象」开关。
        /// `true` = 默认模式下也不隐藏系统 catalog / schema（与白名单模式正交）。
        show_system_objects: bool,
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
        /// 名称档匹配方式（D11）：前缀 / 包含；默认前缀（对齐 DBeaver starts-with）。
        name_mode: NameMatchMode,
        /// 搜索框「注释」开关：开启后把 FTS 注释候选并入名称档（按族档案 comments 集合
        /// 过滤类型；开关关闭时不并入）。
        include_comments: bool,
        /// 搜索框「定义」开关：本批内省链路尚未采集定义，开关默认关闭、不产生定义候选；
        /// 字段先随任务透传，采集链路接通后再启用并入。
        include_definitions: bool,
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
    /// S3-④ 行估计后台补填：对某 schema 的已加载表名单做**目录级**统计查询
    /// （PG `pg_class.reltuples` / MySQL `TABLE_ROWS`），回填行尾「≈ N」徽标。
    ///
    /// 与 Warm 同调度（per-conn 让位、不套全局软帽）：同连接元数据通道忙即跳过本批
    /// （下次展开 / 刷新会再触发），不抢导航内省、不堆积后台线程。
    /// 大 schema 由视图按 PREFETCH_BATCH 分批入队；`seq` 供回填端丢弃过期批次。
    RowEstimate {
        conn_id: String,
        project_root: Option<String>,
        catalog: String,
        schema: String,
        driver: String,
        seq: u64,
    },
}

/// 共享状态：任务队列 + 进度 + 结果队列。
pub(crate) struct Shared {
    /// 发件端。**包一层 `Mutex`**：worker 线程一旦退出（panic / 异常返回），
    /// 通道就废了 —— 那时要换一条新通道并把发件端替换掉（见 [`restart_worker`]）。
    /// 消息是带优先级的 [`QueuedJob`]（worker 侧堆调度，S2-①）。
    pub(crate) tx: Mutex<Sender<QueuedJob>>,
    /// worker 是否活着。投递前先看它：不活就先重启。
    pub(crate) worker_alive: AtomicBool,
    // 预热进度 / 取消
    pub(crate) warm_active: AtomicBool,
    pub(crate) warm_done: AtomicUsize,
    pub(crate) warm_total: AtomicUsize,
    pub(crate) warm_cancel: AtomicBool,
    // 未完成的加载（含排队与执行中）
    pub(crate) pending_loads: AtomicUsize,
    pub(crate) pending_props: AtomicUsize,
    pub(crate) pending_sql: AtomicUsize,
    pub(crate) pending_test: AtomicUsize,
    pub(crate) pending_connect: AtomicUsize,
    pub(crate) pending_search: [AtomicUsize; SearchConsumer::COUNT],
    /// S3-④ 行估计（后台补填）未完成计数：`has_pending_row_estimates` 供空闲判定。
    pub(crate) pending_row_estimates: AtomicUsize,
    pub(crate) load_results: Mutex<Vec<LoadResult>>,
    pub(crate) props_results: Mutex<Vec<PropsResult>>,
    pub(crate) sql_results: Mutex<Vec<SqlGenResult>>,
    pub(crate) test_results: Mutex<Vec<TestConnResult>>,
    pub(crate) connect_results: Mutex<Vec<ConnectResult>>,
    pub(crate) search_results: [Mutex<Vec<SearchResult>>; SearchConsumer::COUNT],
    /// S3-④ 行估计结果（回传主线程；`estimates` 为空 = 失败 / 超时 / 驱动不支持，
    /// 视图侧据此收掉「…」占位）。
    pub(crate) row_estimates: Mutex<Vec<RowEstimateResult>>,
    /// 每连接信号量表：key = `conn_id`，value = 容量 1 的 per-conn 信号量。
    /// 挂本结构（不另开 `OnceLock` 静态全局）；条目由 [`drop_conn_sem`] 回收。
    pub(crate) conn_sems: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl Shared {
    /// 把一批搜索结果交给对应消费方（worker 与单测共用，保证「分发」只有一处）。
    pub(crate) fn push_search_result(&self, consumer: SearchConsumer, result: SearchResult) {
        lock(&self.search_results[consumer.ix()]).push(result);
    }

    /// 取走某消费方已完成的搜索结果。
    pub(crate) fn take_search_results(&self, consumer: SearchConsumer) -> Vec<SearchResult> {
        std::mem::take(&mut *lock(&self.search_results[consumer.ix()]))
    }
}

static JOBS: OnceLock<Shared> = OnceLock::new();

pub(crate) fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<QueuedJob>();
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
            pending_row_estimates: AtomicUsize::new(0),
            load_results: Mutex::new(Vec::new()),
            props_results: Mutex::new(Vec::new()),
            sql_results: Mutex::new(Vec::new()),
            test_results: Mutex::new(Vec::new()),
            connect_results: Mutex::new(Vec::new()),
            search_results: std::array::from_fn(|_| Mutex::new(Vec::new())),
            row_estimates: Mutex::new(Vec::new()),
            conn_sems: Mutex::new(HashMap::new()),
        }
    })
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(super) fn service(project_root: Option<String>) -> crate::navigator_service::NavigatorService {
    crate::navigator_service::NavigatorService::with_context(
        engine::get_connection_manager().clone(),
        project_root,
        false,
    )
}

/// 可见库管理弹框（S3-①b）的候选清单：`catalog → schemas` 两级全量（不过滤系统对象）。
///
/// 同步 `block_on` 查询（对齐 `run_job` 的桥接运行时模式）：弹框打开是**模态事件**，
/// 短暂阻塞可接受；有 L1/L2 缓存时瞬时返回，首次（连接未展开过）会内省一次。
/// 返回空 = 查询失败（提示由调用方展示，弹框显示空列表而不是卡死）。
pub fn picker_catalog_schemas(
    conn_id: &str,
    project_root: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    let svc = service(project_root.map(|s| s.to_string()));
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    let outcome = rt.block_on(async {
        let catalogs = tokio::time::timeout(JOB_TIMEOUT, svc.list_all_catalogs(conn_id)).await;
        let catalogs = match catalogs {
            Ok(Ok(names)) => names,
            _ => return Vec::new(),
        };
        let mut out = Vec::with_capacity(catalogs.len());
        for catalog in catalogs {
            let schemas =
                tokio::time::timeout(JOB_TIMEOUT, svc.list_all_schemas(conn_id, &catalog)).await;
            let schemas = match schemas {
                Ok(Ok(names)) => names,
                _ => Vec::new(),
            };
            out.push((catalog, schemas));
        }
        out
    });
    outcome
}

/// 起一个 worker 线程（`Shared` 初始化与「重启」共用）。
///
/// 线程退出时（正常返回 / panic）由 `Drop` 把存活标志落下 —— 下一次投递会把它重启。
pub(super) fn spawn_worker(rx: mpsc::Receiver<QueuedJob>) {
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
pub(super) fn restart_worker() {
    let s = shared();
    let (tx, rx) = mpsc::channel::<QueuedJob>();
    *lock(&s.tx) = tx;
    s.worker_alive.store(true, Ordering::SeqCst);
    spawn_worker(rx);
    tracing::warn!("导航后台线程已重启（上一个线程已退出）");
}

/// 任务优先级（S2-① 竞品差异化：9 家竞品都没有优先级，我们第一个做）。
///
/// 用户主动动作（展开 / 重试 / 加载更多 / 搜索）必须插到后台任务前面——
/// 否则恢复展开态 / 定位链 / 预取的一大批低优任务会把用户手点的响应堵在
/// `per-conn` / 全局软帽信号量后面（FIFO 先到先执行，正是遗留 #5 的症状）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    /// 用户主动动作：先执行。
    User,
    /// 后台恢复 / 定位 / 刷新 / 预取：让位。
    Background,
}

/// 排队中的任务（带优先级与全局序号）。
///
/// `seq` 保证**同优先级内部仍按入队序执行**（FIFO 公平）：高优插队不破坏低优彼此的顺序。
/// `Ord` 取 `priority` 升序（User < Background → User 先出）、同优先 `seq` 升序。
pub(crate) struct QueuedJob {
    pub(crate) priority: JobPriority,
    pub(crate) seq: u64,
    pub(crate) job: Job,
}

impl Ord for QueuedJob {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap 是 max-heap（pop 出最大者）：让「更优先、seq 更小」的任务**更大**，
        // 堆顶就是下一个该执行的。priority 升序（User < Background）经 `other` 侧反转后，
        // User 的 QueuedJob 恒大于 Background 的。
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for QueuedJob {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for QueuedJob {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}

impl Eq for QueuedJob {}

/// 全局单调递增序号：同优先级任务按入队先后执行（公平性）。
static NEXT_JOB_SEQ: AtomicU64 = AtomicU64::new(0);

/// 提交一个任务（所有 `enqueue_*` 的统一出口）。
///
/// 三件事保证「投出去的任务一定有回音」：
/// 1. worker 不活 → 先重启；
/// 2. 投递失败（通道已断）→ 重启后**重投一次**；
/// 3. 还失败 → 按任务的凭据回填一条**可见的错误**并配平计数 ——
///    绝不留下一个永远「加载中」的节点。
pub(crate) fn submit(job: Job, priority: JobPriority) {
    let s = shared();
    let seq = NEXT_JOB_SEQ.fetch_add(1, Ordering::SeqCst);
    let mut queued = QueuedJob { priority, seq, job };
    for attempt in 0..2 {
        let tx = lock(&s.tx).clone();
        match tx.send(queued) {
            Ok(()) => return,
            Err(back) => {
                queued = back.0;
                if attempt == 0 {
                    restart_worker();
                }
            }
        }
    }
    let job = queued.job;
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
pub(super) enum Ticket {
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
    /// S3-④ 行估计：失败 / panic 也要回一条空结果，否则视图的「…」占位收不掉。
    RowEstimate {
        conn_id: String,
        catalog: String,
        schema: String,
        seq: u64,
    },
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
            Job::RowEstimate {
                conn_id,
                catalog,
                schema,
                seq,
                ..
            } => Some(Ticket::RowEstimate {
                conn_id: conn_id.clone(),
                catalog: catalog.clone(),
                schema: schema.clone(),
                seq: *seq,
            }),
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
            Job::RowEstimate { .. } => Some(&shared().pending_row_estimates),
            #[cfg(test)]
            Job::PanicForTest { .. } => Some(&shared().pending_loads),
        }
    }
}

/// 按凭据回填一条**可见的错误**（界面据此结束「加载中」并显示原因）。
pub(super) fn report_failure(ticket: Ticket, reason: &str) {
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
                conn_fingerprint: None,
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
        Ticket::RowEstimate {
            conn_id,
            catalog,
            schema,
            seq,
        } => {
            // 失败收尾 = 空结果：视图据 `(conn_id, schema, seq)` 清掉「…」占位
            // （估计失败不该留红字，徽标消失即最安静的失败）。
            lock(&s.row_estimates).push(RowEstimateResult {
                conn_id,
                catalog,
                schema,
                seq,
                estimates: Vec::new(),
                sizes: Vec::new(),
            });
        }
    }
}

/// panic 载荷 → 一行可读文字。
pub(super) fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "（未知 panic 载荷）".to_string()
}

pub(crate) fn worker(rx: mpsc::Receiver<QueuedJob>) {
    // 优先级调度（S2-①）：通道进来的任务先进二叉堆，每次取**最高优**执行；
    // 同优先级按入队序（`QueuedJob.seq`）保持 FIFO 公平。高优任务（用户手点）到了
    // 就能插到低优（恢复展开 / 定位 / 刷新）前面——per-conn / 全局软帽信号量
    // 的 FCFS 跟随 spawn 顺序，所以**重排 spawn 顺序即重排执行顺序**。
    let mut heap: BinaryHeap<QueuedJob> = BinaryHeap::new();
    loop {
        let queued = if let Some(top) = heap.pop() {
            // 顺手把通道里已排队的全收进堆：下一轮 pop 时优先级全局可见
            // （否则高优任务在通道里干等低优先执行）。
            while let Ok(j) = rx.try_recv() {
                heap.push(j);
            }
            top
        } else {
            match rx.recv() {
                Ok(j) => j,
                Err(_) => break,
            }
        };
        let job = queued.job;
        // 每任务独立线程：同步阻塞（rusqlite / 驱动查询）卡住时只挂起该线程，
        // 分发线程立即接下一任务——tokio `timeout` 对同步阻塞不生效（实测：一个
        // 卡住的查询会让整个 worker 排队、树全卡，JOB_TIMEOUT 兜不住）。
        // 并发由 per-conn 信号量（同连接串行）+ 全局软帽（跨连接上界）在 `run_job`
        // 内限制（阻塞族元数据任务）。
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
pub(super) fn run_job(rt: &tokio::runtime::Runtime, job: Job) {
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
            visible_databases,
            show_system_objects,
        } => {
            // 调度（B-2-1 per-conn 分片）：先取所属连接信号量（同连接内省串行，保持驱动
            // 单连接 Mutex 语义），再取全局软帽（跨连接并发上界 K=3）；两层都忙时在此
            // 排队，loading 状态保持、UI 显示「加载中」，不丢任务。
            let (_per_conn, _global) = acquire_meta_permit(rt, &conn_id);
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
                    svc.load_children_page(
                        &conn_id,
                        &path,
                        offset,
                        limit,
                        visible_databases.as_ref(),
                        show_system_objects,
                    ),
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
            // S2-② 执行时指纹快照：任务在队列里排队 / 执行期间用户可能改配置，
            // 回填时比对（见 `apply_load_results` 的 stale guard）——配置已变就丢弃，
            // 不让旧配置下的元数据覆盖新配置的树。
            let conn_fingerprint = engine::persistence::identity_fingerprint(&conn_id);
            lock(&shared().load_results).push(LoadResult {
                key,
                conn_id,
                project_root,
                path,
                offset,
                jumped_to,
                conn_fingerprint,
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
            // 属性面板详情也是元数据查询：与导航内省共用所属连接信号量（B-2-B，保持
            // 「同连接元数据通道唯一」语义，避免属性查询与内省抢同一条单连接），再套
            // 全局软帽。
            let conn_id = property.conn_id.clone();
            let (_per_conn, _global) = acquire_meta_permit(rt, &conn_id);
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
            // 预热也是元数据内省：取所属连接信号量但**让位**（B-2-C 忙即跳，不阻塞
            // 排队、不套全局软帽）。同连接内省忙时放弃本批（下次展开时 cache-aside 会
            // 补写 L2），不让后台线程堆积。
            let Some(_permit) = try_acquire_warm_permit(&conn_id) else {
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
            // 补全内省也是元数据内省：取所属连接信号量但**让位**（B-2-C 忙即跳，不阻塞
            // 排队、不套全局软帽）。同连接忙时放弃本批（补全下回按键会再试），不让后台
            // 线程堆积、不抢源库。
            let Some(_permit) = try_acquire_warm_permit(&conn_id) else {
                tracing::debug!(conn_id = %conn_id, "补全内省让位导航（跳过本批）");
                return;
            };
            let svc = service(project_root);
            let _ = rt.block_on(svc.warm_completion_metadata(&conn_id));
        }
        Job::RowEstimate {
            conn_id,
            project_root,
            catalog,
            schema,
            driver,
            seq,
        } => {
            // 与 Warm 同调度（per-conn 让位，不套全局软帽）：同连接元数据通道忙即跳过
            // 本批——估计是「名单先上屏」之后的补填，不该反过来堵住导航内省。
            // 跳过也回空结果（视图清「…」占位），只是不执行查询。
            let Some(_permit) = try_acquire_warm_permit(&conn_id) else {
                tracing::debug!(conn_id = %conn_id, schema = %schema, "行估计让位导航内省（跳过本批）");
                lock(&shared().row_estimates).push(RowEstimateResult {
                    conn_id,
                    catalog,
                    schema,
                    seq,
                    estimates: Vec::new(),
                    sizes: Vec::new(),
                });
                return;
            };
            // 行估计 / 大小两个查询独立：方言各自 None 兜底，互不牵连；两者都不支持才
            // 直接空结果收尾（入队侧已判一次，这里是兜底——不发真 COUNT / 大小查询）。
            let dialect = crate::nav::dialect_of(&driver);
            let est_sql = dialect.row_estimate_sql(&schema);
            let size_sql = dialect.table_size_sql(&schema);
            if est_sql.is_none() && size_sql.is_none() {
                lock(&shared().row_estimates).push(RowEstimateResult {
                    conn_id,
                    catalog,
                    schema,
                    seq,
                    estimates: Vec::new(),
                    sizes: Vec::new(),
                });
                return;
            }
            let svc = crate::metadata_service::MetadataService::new(
                engine::get_connection_manager().clone(),
            );
            // 行估计：Some 才发查询；失败 / 超时 → 空列表（不显 0、不报红字）。
            let estimates = match est_sql {
                Some(sql) => {
                    let outcome = rt.block_on(async {
                        tokio::time::timeout(JOB_TIMEOUT, svc.execute_query(&conn_id, &catalog, &sql))
                            .await
                    });
                    match outcome {
                        Ok(Ok(result)) => parse_name_value_pairs(result.rows),
                        _ => Vec::new(),
                    }
                }
                None => Vec::new(),
            };
            // D12 表大小：与行估计同一批次、同一让位通道，独立查询独立解析。
            let sizes = match size_sql {
                Some(sql) => {
                    let outcome = rt.block_on(async {
                        tokio::time::timeout(JOB_TIMEOUT, svc.execute_query(&conn_id, &catalog, &sql))
                            .await
                    });
                    match outcome {
                        Ok(Ok(result)) => parse_name_value_pairs(result.rows),
                        _ => Vec::new(),
                    }
                }
                None => Vec::new(),
            };
            // S3-④-C（拍板回写 L2）：估计值 / 大小落缓存表，重开连接复用；刷新元数据时
            // 由新批次覆盖。失败静默（尽力而为——内存回填已成功，L2 只是跨会话复用通道）。
            if !estimates.is_empty() {
                if let Some(mut cache) =
                    crate::cache::NavCache::open_direct(&conn_id, project_root.as_deref())
                {
                    cache.update_row_estimates(&catalog, &schema, &estimates);
                }
            }
            if !sizes.is_empty() {
                if let Some(mut cache) =
                    crate::cache::NavCache::open_direct(&conn_id, project_root.as_deref())
                {
                    cache.update_table_sizes(&catalog, &schema, &sizes);
                }
            }
            lock(&shared().row_estimates).push(RowEstimateResult {
                conn_id,
                catalog,
                schema,
                seq,
                estimates,
                sizes,
            });
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
            // 预取与内省**互不抢信号量**（2026-10-08 卡顿根治）：导航用 per-conn
            // 信号量串行 + 全局软帽，列预取用独立 PREFETCH_SEM（并发 2）——表展开
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
            // 取列也是元数据查询：先取所属连接信号量（同连接串行），再套全局软帽
            // （对齐 DBeaver Metadata 连接语义——元数据查询走唯一通道，不并发占池）。
            let (_per_conn, _global) = acquire_meta_permit(rt, &conn_id);
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
                        // 列层无系统对象，不过滤（S3-① 只作用于 catalog / schema 层）。
                        None,
                        // 同上：列层无系统对象，开关不影响（P1，2026-10-25）。
                        false,
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
            name_mode,
            include_comments,
            include_definitions,
            project_root,
            targets,
        } => {
            // 并行搜索（2026-10-14 P1）：目标连接按 `SEARCH_PARALLEL` 分块，每块一条
            // scoped 线程**直开临时缓存连接**（`NavCache::open_direct`，不走池——搜索目标
            // 是全部可见连接，池会让从未浏览过的连接也永久注册并预热，白占句柄）。
            // 线程只读各自连接自己的缓存文件（不同缓存文件 = 不同 SQLite 库），无共享可变
            // 状态，也无需抢 per-conn 信号量 / 全局软帽 / `PREFETCH_SEM`。
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
                        // `kind` / `case_sensitive` / `regex` / `name_mode` / 注释 / 定义开关
                        // 都是 Copy 直接带上。scoped 线程在 `scope` 结束前必先 join，克隆的是
                        // String 值本身，不借 `chunk` 之外的任何栈变量。
                        let query = query.clone();
                        let project_root = project_root.clone();
                        s.spawn(move || {
                            search_targets(
                                kind,
                                &query,
                                case_sensitive,
                                regex,
                                name_mode,
                                include_comments,
                                include_definitions,
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
pub fn nav_result_is_stale(
    conn_exists: bool,
    conn_connected: bool,
    fp_at_issue: Option<&str>,
    fp_now: Option<&str>,
) -> bool {
    !conn_exists || !conn_connected || (fp_at_issue.is_some() && fp_at_issue != fp_now)
}

/// 提交导航树懒加载（首屏 / 恢复展开态）。
///
/// `visible_databases`（S3-①）：入队时的可见库白名单快照（`None` = 默认隐藏系统库）。
/// `show_system_objects`（P1，2026-10-25）：连接级「显示系统对象」开关快照。
#[allow(clippy::too_many_arguments)]
pub fn enqueue_load(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    fresh: bool,
    priority: JobPriority,
    visible_databases: Option<std::collections::BTreeSet<String>>,
    show_system_objects: bool,
) {
    enqueue_load_page(
        conn_id,
        project_root,
        key,
        path,
        fresh,
        0,
        PAGE_SIZE,
        priority,
        visible_databases,
        show_system_objects,
    );
}

/// 提交导航树分页加载（`offset` = 已加载条数，即可「加载更多」）。
///
/// `priority`：用户手点（展开 / 加载更多）传 [`JobPriority::User`]，
/// 后台恢复（展开态恢复 / 刷新 / 定位链）传 [`JobPriority::Background`]——
/// 高优任务在 worker 侧插到低优前面（见 [`worker`] 的堆调度）。
/// `visible_databases`（S3-①）：入队时的可见库白名单快照。
/// `show_system_objects`（P1，2026-10-25）：连接级「显示系统对象」开关快照。
#[allow(clippy::too_many_arguments)]
pub fn enqueue_load_page(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    fresh: bool,
    offset: usize,
    limit: usize,
    priority: JobPriority,
    visible_databases: Option<std::collections::BTreeSet<String>>,
    show_system_objects: bool,
) {
    shared().pending_loads.fetch_add(1, Ordering::SeqCst);
    tracing::info!(conn_id = %conn_id, key = %key, fresh, priority = ?priority, "导航任务入队");
    submit(
        Job::LoadChildren {
            conn_id: conn_id.to_string(),
            project_root: project_root.map(|s| s.to_string()),
            key: key.to_string(),
            path,
            fresh,
            offset,
            limit,
            object: None,
            visible_databases,
            show_system_objects,
        },
        priority,
    );
}

/// 提交「定位到某个对象」的页加载。
///
/// 与 [`enqueue_load_page`] 分开命名（而不是加个 `Option` 参数）：调用点的意图不同——
/// 一个是在当前窗口往后翻，另一个是**换窗跳到目标**；混在一个签名里最容易接错。
/// 定位链是后台路径（用户的目标是最终定位，中间层展开不该插到手点前面）。
/// `visible_databases`（S3-①）：入队时的可见库白名单快照。
pub fn enqueue_locate_page(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    object: &str,
    limit: usize,
    visible_databases: Option<std::collections::BTreeSet<String>>,
    show_system_objects: bool,
) {
    shared().pending_loads.fetch_add(1, Ordering::SeqCst);
    submit(
        Job::LoadChildren {
            conn_id: conn_id.to_string(),
            project_root: project_root.map(|s| s.to_string()),
            key: key.to_string(),
            path,
            fresh: false,
            offset: 0,
            limit,
            object: Some(object.to_string()),
            visible_databases,
            show_system_objects,
        },
        JobPriority::Background,
    );
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
    submit(
        Job::LoadProperties {
            key: key.to_string(),
            property,
            conn_label: conn_label.to_string(),
            driver: driver.to_string(),
            type_id: type_id.map(str::to_string),
            db_type: db_type.map(str::to_string),
            data,
        },
        JobPriority::User,
    );
}

/// 连接成功后提交预热（C1）。重复连接会重新排队。
///
/// S2-③ 起步延迟：延迟 [`WARM_START_DELAY`] 再入队，让用户首次浏览查询先走。
/// 独立短线程 sleep——不占 worker 主循环（worker 只跑已入队任务）、不堵 UI 线程。
pub fn warm_after_connect(conn_id: &str, project_root: Option<&str>) {
    let conn_id = conn_id.to_string();
    let project_root = project_root.map(|s| s.to_string());
    std::thread::spawn(move || {
        std::thread::sleep(WARM_START_DELAY);
        submit(
            Job::Warm {
                conn_id,
                project_root,
            },
            JobPriority::Background,
        );
    });
}

/// 编辑器补全的实时内省回退（§710）：提交「整条连接的表 / 视图 / 列 cache-aside
/// 内省」到导航队列。与导航内省共享信号量并让位；完成前补全只给关键字与函数
/// （如实，不假装有元数据），写进 L2 后下一次按键就有候选。
///
/// S2-③ 起步延迟与 [`warm_after_connect`] 同理：3s 后再入队，不抢用户编辑关键路径。
pub fn enqueue_warm_completion(conn_id: &str, project_root: Option<&str>) {
    let conn_id = conn_id.to_string();
    let project_root = project_root.map(|s| s.to_string());
    std::thread::spawn(move || {
        std::thread::sleep(WARM_START_DELAY);
        submit(
            Job::WarmCompletion {
                conn_id,
                project_root,
            },
            JobPriority::Background,
        );
    });
}

/// 提交列预取（C2）。空列表不排队。
pub fn prefetch_columns(conn_id: &str, project_root: Option<&str>, targets: Vec<ColumnTarget>) {
    if targets.is_empty() {
        return;
    }
    submit(
        Job::PrefetchColumns {
            conn_id: conn_id.to_string(),
            project_root: project_root.map(|s| s.to_string()),
            targets,
        },
        JobPriority::Background,
    );
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
    submit(
        Job::GenerateDml {
            key: key.to_string(),
            conn_id: conn_id.to_string(),
            project_root: project_root.map(|s| s.to_string()),
            catalog: catalog.to_string(),
            schema: schema.to_string(),
            table: table.to_string(),
            qualified: qualified.to_string(),
            kind,
        },
        JobPriority::User,
    );
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
    submit(
        Job::TestConnection {
            conn_id: conn_id.to_string(),
            project_root: project_root.map(|s| s.to_string()),
            name: name.to_string(),
            probe,
        },
        JobPriority::User,
    );
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

/// 是否仍有未完成的 S3-④ 行估计（后台补填，不阻塞任何交互；供空闲判定与测试）。
pub fn has_pending_row_estimates() -> bool {
    shared().pending_row_estimates.load(Ordering::SeqCst) > 0
}

/// S3-④ 提交某 schema 的行估计 + D12 表大小补填（后台补填，优先级 = 后台）。
///
/// 行估计与大小两个方言 SQL **都**为 `None` 时**不入队**（SQL 生成即判定）：视图侧也
/// 判了一次，这里是兜底——避免不支持驱动白排一批让位任务；仅其一支持时本批仍入队，
/// 任务内各自 None 兜底。
pub fn enqueue_row_estimate(
    conn_id: &str,
    project_root: Option<&str>,
    catalog: &str,
    schema: &str,
    driver: &str,
    seq: u64,
) {
    let dialect = crate::nav::dialect_of(driver);
    if dialect.row_estimate_sql(schema).is_none() && dialect.table_size_sql(schema).is_none() {
        return;
    }
    shared().pending_row_estimates.fetch_add(1, Ordering::SeqCst);
    submit(
        Job::RowEstimate {
            conn_id: conn_id.to_string(),
            project_root: project_root.map(|s| s.to_string()),
            catalog: catalog.to_string(),
            schema: schema.to_string(),
            driver: driver.to_string(),
            seq,
        },
        JobPriority::Background,
    );
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
    name_mode: NameMatchMode,
    include_comments: bool,
    include_definitions: bool,
    project_root: Option<&str>,
    targets: Vec<SearchTarget>,
) {
    shared().pending_search[consumer.ix()].fetch_add(1, Ordering::SeqCst);
    submit(
        Job::SearchIndex {
            consumer,
            kind,
            query: query.to_string(),
            seq,
            case_sensitive,
            regex,
            name_mode,
            include_comments,
            include_definitions,
            project_root: project_root.map(|s| s.to_string()),
            targets,
        },
        // 搜索是用户敲出来的：高优先（插到恢复展开/预取前面，结果尽快上屏）。
        JobPriority::User,
    );
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

/// 取走已完成的 S3-④ 行估计结果。
pub fn drain_row_estimates() -> Vec<RowEstimateResult> {
    std::mem::take(&mut *lock(&shared().row_estimates))
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

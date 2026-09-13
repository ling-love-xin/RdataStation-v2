//! 数据源导航后台任务（加载 / 预热 / 预取）。
//!
//! 为什么需要：元数据内省会走数据库驱动（依赖 tokio），在 UI 线程上 `block_on`
//! 会冻结界面。本模块用**单一工作线程 + tokio 运行时**串行执行任务，视图只提交
//! 任务、轮询进度（原子量）并取回结果（结果队列），不参与阻塞。
//!
//! 任务类型：
//! - `LoadChildren`：导航树懒加载（主线程 render 不再做 I/O）；
//! - `LoadProperties`：属性面板对象加载；
//! - `Warm`（C1 预热，方案 C）：内省 catalogs / schemas 并写入 L2，可取消、有进度；
//! - `PrefetchColumns`（C2 邻接预取）：预取指定表 / 视图的列写入 L2，失败静默。
//!
//! 缓存写入由 `NavigatorService` 的 cache-aside 完成；本模块不直接碰缓存。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use database::model::{NavNode, NavPath, PropertyRef};
use database::property_panel::ObjectProperties;

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
    pub result: Result<Vec<NavNode>, String>,
}

/// 属性加载结果（回传主线程）。
pub struct PropsResult {
    pub key: String,
    pub result: Result<ObjectProperties, String>,
}

/// 单个类别文件夹一次最多预取的表数量（避免大 schema 触发长时间后台 IO）。
pub const PREFETCH_BATCH: usize = 20;

enum Job {
    LoadChildren {
        conn_id: String,
        project_root: Option<String>,
        key: String,
        path: NavPath,
        fresh: bool,
    },
    LoadProperties {
        key: String,
        property: PropertyRef,
        conn_label: String,
        driver: String,
        /// 数据库类型（`drivers.type_id`）；属性面板「数据库类型」行用。
        db_type: Option<String>,
    },
    Warm {
        conn_id: String,
        project_root: Option<String>,
    },
    PrefetchColumns {
        conn_id: String,
        project_root: Option<String>,
        targets: Vec<ColumnTarget>,
    },
}

/// 共享状态：任务队列 + 进度 + 结果队列。
struct Shared {
    tx: Sender<Job>,
    // 预热进度 / 取消
    warm_active: AtomicBool,
    warm_done: AtomicUsize,
    warm_total: AtomicUsize,
    warm_cancel: AtomicBool,
    // 未完成的加载（含排队与执行中）
    pending_loads: AtomicUsize,
    pending_props: AtomicUsize,
    load_results: Mutex<Vec<LoadResult>>,
    props_results: Mutex<Vec<PropsResult>>,
}

static JOBS: OnceLock<Shared> = OnceLock::new();

fn shared() -> &'static Shared {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rds-nav-jobs".to_string())
            // 驱动内省在 debug 下递归较深，给足栈避免溢出。
            .stack_size(16 * 1024 * 1024)
            .spawn(move || worker(rx))
            .expect("failed to spawn nav jobs worker");
        Shared {
            tx,
            warm_active: AtomicBool::new(false),
            warm_done: AtomicUsize::new(0),
            warm_total: AtomicUsize::new(0),
            warm_cancel: AtomicBool::new(false),
            pending_loads: AtomicUsize::new(0),
            pending_props: AtomicUsize::new(0),
            load_results: Mutex::new(Vec::new()),
            props_results: Mutex::new(Vec::new()),
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn service(project_root: Option<String>) -> database::navigator_service::NavigatorService {
    database::navigator_service::NavigatorService::with_context(
        engine::get_connection_manager().clone(),
        project_root,
        false,
    )
}

fn worker(rx: mpsc::Receiver<Job>) {
    // tokio 运行时随工作线程存活；数据库驱动依赖它。
    let Ok(rt) = tokio::runtime::Runtime::new() else {
        return;
    };
    while let Ok(job) = rx.recv() {
        match job {
            Job::LoadChildren {
                conn_id,
                project_root,
                key,
                path,
                fresh,
            } => {
                let svc = database::navigator_service::NavigatorService::with_context(
                    engine::get_connection_manager().clone(),
                    project_root.clone(),
                    fresh,
                );
                let result = rt
                    .block_on(svc.load_children(&conn_id, &path))
                    .map_err(|e| e.to_string());
                lock(&shared().load_results).push(LoadResult {
                    key,
                    conn_id,
                    project_root,
                    path,
                    result,
                });
                shared().pending_loads.fetch_sub(1, Ordering::SeqCst);
            }
            Job::LoadProperties {
                key,
                property,
                conn_label,
                driver,
                db_type,
            } => {
                let svc = service(None);
                let result = rt
                    .block_on(svc.load_properties(
                        &property,
                        &conn_label,
                        &driver,
                        db_type.as_deref(),
                    ))
                    .map_err(|e| e.to_string());
                lock(&shared().props_results).push(PropsResult { key, result });
                shared().pending_props.fetch_sub(1, Ordering::SeqCst);
            }
            Job::Warm {
                conn_id,
                project_root,
            } => {
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
                let _ = rt.block_on(svc.prefetch_columns(&conn_id, &tuples));
            }
        }
    }
}

/// 提交导航树懒加载。
pub fn enqueue_load(
    conn_id: &str,
    project_root: Option<&str>,
    key: &str,
    path: NavPath,
    fresh: bool,
) {
    shared().pending_loads.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::LoadChildren {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        key: key.to_string(),
        path,
        fresh,
    });
}

/// 提交属性加载。
pub fn enqueue_properties(
    key: &str,
    property: PropertyRef,
    conn_label: &str,
    driver: &str,
    db_type: Option<&str>,
) {
    shared().pending_props.fetch_add(1, Ordering::SeqCst);
    let _ = shared().tx.send(Job::LoadProperties {
        key: key.to_string(),
        property,
        conn_label: conn_label.to_string(),
        driver: driver.to_string(),
        db_type: db_type.map(str::to_string),
    });
}

/// 连接成功后提交预热（C1）。重复连接会重新排队。
pub fn warm_after_connect(conn_id: &str, project_root: Option<&str>) {
    let _ = shared().tx.send(Job::Warm {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
    });
}

/// 提交列预取（C2）。空列表不排队。
pub fn prefetch_columns(conn_id: &str, project_root: Option<&str>, targets: Vec<ColumnTarget>) {
    if targets.is_empty() {
        return;
    }
    let _ = shared().tx.send(Job::PrefetchColumns {
        conn_id: conn_id.to_string(),
        project_root: project_root.map(|s| s.to_string()),
        targets,
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

/// 取走已完成的树加载结果。
pub fn drain_load_results() -> Vec<LoadResult> {
    std::mem::take(&mut *lock(&shared().load_results))
}

/// 取走已完成的属性加载结果。
pub fn drain_props_results() -> Vec<PropsResult> {
    std::mem::take(&mut *lock(&shared().props_results))
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

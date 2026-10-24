//! B-2-0 并发前置探针（立项方案 §3.3，见
//! `docs/architecture/database/database-nav-b-tier-proposal.md`）。
//!
//! ## 目的
//!
//! 在 per-connection 信号量调度改造（B-2-1）之前，先直连驱动层验证 V1–V5 闸门场景：
//!
//! - V1 两条已连接各自内省并发，观测墙钟是否 ≈ 较慢单连接水平、服务端连接数 ≤ 实例数；
//! - V2 同一连接第二个内省任务排队，观测等待分布与死锁；
//! - V3 两连接 + 列预取 + 批量查询同时在途，观测 Runtime / 在途任务上界；
//! - V4 服务端长事务 / 行锁等待下两条连接内省，观测互不拖垮与超时可观测；
//! - V5 不可达端点发起任务，观测错误如实返回、不挂死。
//!
//! ## 隔离边界（方案 §3.3.1，不得越界）
//!
//! - 只直连驱动层 [`Database`] / [`MetadataBrowser`]，不接 `nav_jobs` 队列、不接
//!   `NavHost` / 视图层、不接 `ConnectionManager`；
//! - 场景由本探针手工编排，不改动任何生产代码路径；
//! - 不新增 dev-dependency，复用 engine 既有驱动 trait 与 tokio。
//!
//! ## 环境变量（与仓内其余真机测试套件同规矩：缺变量即跳过该类端点）
//!
//! - `RDS_TEST_PG_URL`：PostgreSQL 连接串，例 `postgres://user:pw@host:5432/db`；
//! - `RDS_TEST_MYSQL_URL`：MySQL 连接串，例 `mysql://user:pw@host:3306/db`；
//! - `RDS_TEST_SQLITE_PATH`：SQLite 文件路径，缺省用系统临时目录探针文件；
//! - `RDS_TEST_DUCKDB_PATH`：DuckDB 文件路径，缺省用系统临时目录探针文件。
//!
//! Oracle / MSSQL / JDBC / wasm 本期排除（决策 B-2-D）。
//!
//! ## 运行与归档
//!
//! ```powershell
//! cargo run -p rds-engine --bin probe_b2 -j 2
//! ```
//!
//! 原始日志由调用方重定向到 `.rds/tmp/`（不入库）；验证报告成文 `docs/analysis/`；
//! 闸门报告归档后本 bin 即删除，不长期滞留（方案 §3.3.1）。
//!
//! ## 基线形态对照
//!
//! 现状生产任务每个都 `std::thread::spawn` + `Runtime::new()`（`nav_jobs.rs:812-815`），
//! 本探针复刻同一形态以观测 Runtime 数量上界；V3 数据是 B-2-1 全局软帽取值的依据。

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use rds_engine::driver::native::duckdb::DuckDbDatabase;
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
use rds_engine::driver::native::postgres::PostgresDatabase;
use rds_engine::driver::native::sqlite::SqliteDatabase;
use rds_engine::driver::{Database, MetadataBrowser, Transaction, DynDatabase};
use rds_engine::{common_err, CommonError, CoreError};

/// 任务超时，照抄现状 `nav_jobs.rs:150` 的 `JOB_TIMEOUT = 30s`。
const JOB_TIMEOUT: Duration = Duration::from_secs(30);

/// V2 同连接排队重复轮次：等待分布 min / median / max 需要样本量。
const V2_ROUNDS: usize = 5;

/// 探针端点类别（决定内省序列、服务端连接数观测 SQL 与 V4 适用性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointKind {
    Postgres,
    Mysql,
    Sqlite,
    Duckdb,
}

impl EndpointKind {
    /// 是否服务端库：V4 行锁等待与服务端连接数观测只对服务端库成立。
    fn is_server(self) -> bool {
        matches!(self, EndpointKind::Postgres | EndpointKind::Mysql)
    }
}

/// 一个已连接的探针端点：驱动实例经类型擦除为 [`DynDatabase`]，统一走 trait 方法。
struct ProbeEndpoint {
    kind: EndpointKind,
    /// 显示标签（含序号，避免两个同类型端点同名）。
    label: String,
    /// 构造地址（URL 或本地路径），V4 需要同地址另建第二实例观测跨连接互不拖垮。
    address: String,
    db: DynDatabase,
}

/// 内省 / 查询任务的统一摘要：节点计数 + 自由说明（库名、表名等观测口径备注）。
#[derive(Default, Clone, Debug)]
struct TaskOutput {
    nodes: usize,
    note: String,
}

/// 任务执行结局：区分成功、业务错误、超时、Runtime 初始化失败。
enum Outcome {
    Ok(TaskOutput),
    Err(CoreError),
    Timeout,
    RtInitFail(String),
}

/// 单个任务的采集记录（场景、标签、墙钟、结局、摘要）。
struct TaskRecord {
    scene: String,
    label: String,
    wall_ms: u64,
    outcome: Outcome,
}

impl TaskRecord {
    fn from_outcome(scene: String, label: String, elapsed: Duration, outcome: Outcome) -> Self {
        Self {
            scene,
            label,
            wall_ms: elapsed.as_millis() as u64,
            outcome,
        }
    }

    fn rt_init_fail(scene: String, label: String, reason: String) -> Self {
        Self {
            scene,
            label,
            wall_ms: 0,
            outcome: Outcome::RtInitFail(reason),
        }
    }

    /// 按结局归类错误档位（连接 / 查询驱动 / 超时 / 通用 / Runtime 失败）。
    fn error_kind(&self) -> Option<&'static str> {
        match &self.outcome {
            Outcome::Ok(_) => None,
            Outcome::Timeout => Some("超时"),
            Outcome::RtInitFail(_) => Some("Runtime 初始化失败"),
            Outcome::Err(e) => Some(classify_error(e)),
        }
    }

    /// 单行打印到标准输出，报告成文时直接摘录。
    fn print(&self) {
        match &self.outcome {
            Outcome::Ok(out) => println!(
                "[{}] {}：{}ms 成功（nodes={}，{}）",
                self.scene, self.label, self.wall_ms, out.nodes, out.note
            ),
            Outcome::Timeout => println!(
                "[{}] {}：{}ms 超时（超过 {}s 未返回）",
                self.scene,
                self.label,
                self.wall_ms,
                JOB_TIMEOUT.as_secs()
            ),
            Outcome::RtInitFail(reason) => {
                println!("[{}] {}：Runtime 初始化失败：{reason}", self.scene, self.label)
            }
            Outcome::Err(e) => println!(
                "[{}] {}：{}ms 失败（{}）：{e}",
                self.scene,
                self.label,
                self.wall_ms,
                self.error_kind().unwrap_or("未知")
            ),
        }
    }
}

/// 跨任务共享的运行时指标：Runtime 创建次数、在途任务数与峰值。
struct ProbeMetrics {
    rt_count: AtomicUsize,
    in_flight: AtomicUsize,
    in_flight_peak: AtomicUsize,
}

impl Default for ProbeMetrics {
    fn default() -> Self {
        Self {
            rt_count: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            in_flight_peak: AtomicUsize::new(0),
        }
    }
}

/// 错误归类：与方案 §3.3.1「超时与错误分类」口径一致，分连接 / 查询驱动 / 超时 / 通用。
fn classify_error(e: &CoreError) -> &'static str {
    match e {
        CoreError::Connection(_) => "连接类",
        CoreError::Database(_) => "查询/驱动类",
        CoreError::Common(CommonError::Timeout { .. }) => "超时",
        CoreError::Common(_) => "通用类",
        CoreError::Storage(_) => "存储类",
        CoreError::Cache(_) => "缓存类",
        CoreError::Plugin(_) => "插件类",
    }
}

/// 取环境变量，空串视为未设置。
fn env_url(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

/// 本地库缺省探针文件：放在系统临时目录，不污染产品数据目录。
fn default_local_path(kind: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("rds_probe_b2_{kind}.db"));
    path.to_string_lossy().into_owned()
}

/// 中位数（u64 样本），V2 等待分布报告用。
fn median_ms(mut samples: Vec<u64>) -> f64 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    samples.sort_unstable();
    if n % 2 == 1 {
        samples[n / 2] as f64
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) as f64 / 2.0
    }
}

/// 按类别构造驱动实例（同步构造的本地库在 async 函数内直接返回）。
async fn build_endpoint(kind: EndpointKind, label: String, address: &str) -> Result<ProbeEndpoint, CoreError> {
    let db: DynDatabase = match kind {
        EndpointKind::Postgres => Arc::new(PostgresDatabase::new(address).await?),
        EndpointKind::Mysql => Arc::new(MySqlNativeDatabase::new(address).await?),
        EndpointKind::Sqlite => Arc::new(SqliteDatabase::new(address)?),
        EndpointKind::Duckdb => Arc::new(DuckDbDatabase::new(address)?),
    };
    Ok(ProbeEndpoint {
        kind,
        label,
        address: address.to_string(),
        db,
    })
}

/// 一次 `LoadChildren` 内省序列：catalogs → schema → tables，返回节点计数与路径备注。
///
/// 无 schema 层的驱动（MySQL / SQLite / DuckDB）schema 参数回退为 catalog 名，
/// 与生产 `metadata_service.rs:126,159` 同一口径。
async fn load_children(db: &dyn Database) -> Result<TaskOutput, CoreError> {
    let browser: &dyn MetadataBrowser = db
        .as_metadata_browser()
        .ok_or_else(|| common_err("驱动未实现 MetadataBrowser"))?;

    let catalogs = browser.get_catalogs().await?;
    let catalog = catalogs
        .first()
        .map(|n| n.name.clone())
        .unwrap_or_default();

    let schema = if browser.has_schema_level() {
        let schemas = browser.get_schemas(&catalog).await?;
        schemas
            .first()
            .map(|n| n.name.clone())
            .unwrap_or_else(|| catalog.clone())
    } else {
        catalog.clone()
    };

    let tables = browser.get_tables(&catalog, &schema).await?;
    Ok(TaskOutput {
        nodes: tables.len(),
        note: format!("catalog={catalog} schema={schema}"),
    })
}

/// 一次列预取（`LoadProperties` 对应动作）：取首表详情，返回列数。
///
/// 无表时返回空摘要，不算失败——V3 只观测混合在途调度，不依赖表量。
async fn load_first_table_detail(db: &dyn Database) -> Result<TaskOutput, CoreError> {
    let browser: &dyn MetadataBrowser = db
        .as_metadata_browser()
        .ok_or_else(|| common_err("驱动未实现 MetadataBrowser"))?;

    let catalogs = browser.get_catalogs().await?;
    let catalog = catalogs
        .first()
        .map(|n| n.name.clone())
        .unwrap_or_default();
    let schema = if browser.has_schema_level() {
        let schemas = browser.get_schemas(&catalog).await?;
        schemas
            .first()
            .map(|n| n.name.clone())
            .unwrap_or_else(|| catalog.clone())
    } else {
        catalog.clone()
    };
    let tables = browser.get_tables(&catalog, &schema).await?;
    let Some(table) = tables.first() else {
        return Ok(TaskOutput {
            nodes: 0,
            note: "无表可预取".to_string(),
        });
    };
    let detail = browser
        .get_table_detail(&catalog, &schema, &table.name)
        .await?;
    Ok(TaskOutput {
        nodes: detail.columns.len(),
        note: format!("table={}", table.name),
    })
}

/// 服务端当前连接数（best-effort：本地库不适用；服务端查询失败返回 None 并在报告标注）。
///
/// PG `pg_stat_activity` 按当前库过滤；MySQL 读 `information_schema.PROCESSLIST`。
async fn server_conn_count(db: &dyn Database, kind: EndpointKind) -> Option<u32> {
    let sql = match kind {
        EndpointKind::Postgres => {
            "SELECT count(*) AS n FROM pg_stat_activity WHERE datname = current_database()"
        }
        EndpointKind::Mysql => "SELECT COUNT(*) AS n FROM information_schema.PROCESSLIST",
        EndpointKind::Sqlite | EndpointKind::Duckdb => return None,
    };
    let row = db.query(sql).await.ok()?.rows.into_iter().next()?;
    row.first().and_then(|v| v.as_int()).map(|n| n as u32)
}

/// 复刻现状「每任务独立线程 + 独立 Runtime」基线形态（`nav_jobs.rs:812-815`），
/// 在途计数与 Runtime 计数由 [`ProbeMetrics`] 汇总；任务体套 `JOB_TIMEOUT` 兜底。
fn spawn_probe_job<F>(
    scene: &str,
    label: &str,
    fut: F,
    metrics: Arc<ProbeMetrics>,
) -> mpsc::Receiver<TaskRecord>
where
    F: Future<Output = Result<TaskOutput, CoreError>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let scene = scene.to_string();
    let label = label.to_string();

    metrics.rt_count.fetch_add(1, Ordering::SeqCst);
    let in_flight = metrics.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    // 在途峰值：CAS 循环更新，避免多线程并发下漏记。
    let mut peak = metrics.in_flight_peak.load(Ordering::SeqCst);
    while in_flight > peak {
        match metrics.in_flight_peak.compare_exchange(
            peak,
            in_flight,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break,
            Err(current) => peak = current,
        }
    }

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(TaskRecord::rt_init_fail(scene, label, e.to_string()));
                metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
                return;
            }
        };
        let t0 = Instant::now();
        let outcome = rt.block_on(async {
            match tokio::time::timeout(JOB_TIMEOUT, fut).await {
                Ok(Ok(out)) => Outcome::Ok(out),
                Ok(Err(e)) => Outcome::Err(e),
                Err(_) => Outcome::Timeout,
            }
        });
        let record = TaskRecord::from_outcome(scene, label, t0.elapsed(), outcome);
        metrics.in_flight.fetch_sub(1, Ordering::SeqCst);
        let _ = tx.send(record);
    });

    rx
}

/// V1：两条已连接各自 `LoadChildren` 并发。
///
/// 判据（方案 §3.3.2）：总墙钟 ≈ 较慢者单连接基线（不存在全局锁导致二次串行）；
/// 服务端连接数 ≤ 实例数。基线先串行各跑一次取较慢者。
async fn v1_two_connections(
    endpoints: &[ProbeEndpoint],
    metrics: Arc<ProbeMetrics>,
) {
    println!("\n===== V1 两连接并发内省 =====");
    if endpoints.len() < 2 {
        println!("可用端点不足 2 个，V1 跨连接并发跳过（当前 {} 个）", endpoints.len());
        return;
    }
    let (a, b) = (&endpoints[0], &endpoints[1]);

    // 基线：各自串行跑一次。
    let baseline_a = {
        let db = a.db.clone();
        let rx = spawn_probe_job("V1-基线", &a.label, async move { load_children(&*db).await }, metrics.clone());
        rx.recv().expect("基线任务通道不应断开")
    };
    let baseline_b = {
        let db = b.db.clone();
        let rx = spawn_probe_job("V1-基线", &b.label, async move { load_children(&*db).await }, metrics.clone());
        rx.recv().expect("基线任务通道不应断开")
    };
    baseline_a.print();
    baseline_b.print();
    let slowest_baseline = baseline_a.wall_ms.max(baseline_b.wall_ms);

    // 服务端连接数：并发前采样。
    let count_before_a = server_conn_count(&*a.db, a.kind).await;
    let count_before_b = server_conn_count(&*b.db, b.kind).await;

    // 并发：两任务同时在途。
    let wall_start = Instant::now();
    let rx_a = {
        let db = a.db.clone();
        spawn_probe_job("V1-并发", &a.label, async move { load_children(&*db).await }, metrics.clone())
    };
    let rx_b = {
        let db = b.db.clone();
        spawn_probe_job("V1-并发", &b.label, async move { load_children(&*db).await }, metrics.clone())
    };
    let record_a = rx_a.recv().expect("并发任务通道不应断开");
    let record_b = rx_b.recv().expect("并发任务通道不应断开");
    let wall_total = wall_start.elapsed().as_millis() as u64;
    record_a.print();
    record_b.print();

    let count_after_a = server_conn_count(&*a.db, a.kind).await;
    let count_after_b = server_conn_count(&*b.db, b.kind).await;

    let speedup = if wall_total > 0 {
        slowest_baseline as f64 / wall_total as f64
    } else {
        0.0
    };
    println!(
        "V1 观测：较慢基线 {slowest_baseline}ms，并发总墙钟 {wall_total}ms，加速比 ×{speedup:.2}（判据：总墙钟 ≈ 较慢基线，而非 2×）"
    );
    println!(
        "V1 服务端连接数：{} before={:?} after={:?}；{} before={:?} after={:?}（本地库 N/A；判据：连接数 ≤ 实例数）",
        a.label, count_before_a, count_after_a, b.label, count_before_b, count_after_b
    );
}

/// V2：同一连接第二个 `LoadChildren` 排队。
///
/// 判据（方案 §3.3.2）：在驱动 Mutex 上等待不超过 `JOB_TIMEOUT`、不死锁、不饿死。
/// 口径注记：驱动内部 Mutex 等待不可直接观测，排队段以「第二条完成墙钟 − 第一条完成墙钟」
/// 近似；报告成文时据此口径标注，不把近似值当精确锁等待。
async fn v2_same_connection_queueing(endpoints: &[ProbeEndpoint], metrics: Arc<ProbeMetrics>) {
    println!("\n===== V2 同连接排队 =====");
    for endpoint in endpoints {
        println!("--- {}（同一实例，{V2_ROUNDS} 轮）---", endpoint.label);
        let mut waits: Vec<u64> = Vec::new();
        for round in 1..=V2_ROUNDS {
            let rx_first = {
                let db = endpoint.db.clone();
                spawn_probe_job(
                    "V2-排队",
                    &format!("{}·轮{round}·第一条", endpoint.label),
                    async move { load_children(&*db).await },
                    metrics.clone(),
                )
            };
            let rx_second = {
                let db = endpoint.db.clone();
                spawn_probe_job(
                    "V2-排队",
                    &format!("{}·轮{round}·第二条", endpoint.label),
                    async move { load_children(&*db).await },
                    metrics.clone(),
                )
            };
            let first = rx_first.recv().expect("排队任务通道不应断开");
            let second = rx_second.recv().expect("排队任务通道不应断开");
            first.print();
            second.print();
            // 串行排队段近似：第二条在第一条释放锁后才拿到连接，完成差即排队等待量级。
            let wait = second.wall_ms.saturating_sub(first.wall_ms);
            waits.push(wait);
        }
        let min = waits.iter().copied().min().unwrap_or(0);
        let max = waits.iter().copied().max().unwrap_or(0);
        let median = median_ms(waits);
        println!(
            "V2 观测（{}）：等待 min={min}ms median={median:.1}ms max={max}ms；判据：max < {}s、全部返回即未死锁",
            endpoint.label,
            JOB_TIMEOUT.as_secs()
        );
    }
}

/// V3：两条连接 + 一列预取 + 一批查询同时在途。
///
/// 判据（方案 §3.3.2）：线程 / Runtime 数有上界，不出现无限 `Runtime::new()` 堆积。
/// 本场景记录 `rt_count` 增量与在途峰值；现状每任务一 Runtime 的基线形态见
/// `nav_jobs.rs:812-815`，B-2-1 全局软帽取值以本场景真机数据为准。
async fn v3_mixed_in_flight(endpoints: &[ProbeEndpoint], metrics: Arc<ProbeMetrics>) {
    println!("\n===== V3 混合在途（内省 + 列预取 + 批量查询） =====");
    if endpoints.is_empty() {
        println!("无可用端点，V3 跳过");
        return;
    }
    let rt_before = metrics.rt_count.load(Ordering::SeqCst);

    let mut receivers: Vec<mpsc::Receiver<TaskRecord>> = Vec::new();
    // 两条连接内省。
    for endpoint in endpoints.iter().take(2) {
        let db = endpoint.db.clone();
        receivers.push(spawn_probe_job(
            "V3-混合",
            &format!("{}·内省", endpoint.label),
            async move { load_children(&*db).await },
            metrics.clone(),
        ));
    }
    // 一条列预取（取第一个端点）。
    let detail_db = endpoints[0].db.clone();
    receivers.push(spawn_probe_job(
        "V3-混合",
        &format!("{}·列预取", endpoints[0].label),
        async move { load_first_table_detail(&*detail_db).await },
        metrics.clone(),
    ));
    // 三条批量 SELECT（模拟索引搜索 / 轻量查询在途）：按端点轮询分散，多端点时
    // 不全部挤在同一连接上被驱动 Mutex 串行，避免压低在途峰值观测值。
    for i in 1..=3 {
        let endpoint = &endpoints[(i - 1) % endpoints.len()];
        let db = endpoint.db.clone();
        receivers.push(spawn_probe_job(
            "V3-混合",
            &format!("{}·SELECT#{i}", endpoint.label),
            async move {
                db.query("SELECT 1")
                    .await
                    .map(|r| TaskOutput { nodes: r.rows.len(), note: "SELECT 1".to_string() })
            },
            metrics.clone(),
        ));
    }

    for rx in receivers {
        rx.recv().expect("V3 任务通道不应断开").print();
    }

    let rt_after = metrics.rt_count.load(Ordering::SeqCst);
    let peak = metrics.in_flight_peak.load(Ordering::SeqCst);
    println!(
        "V3 观测：本场景 Runtime 数 {}（累计 {}），在途任务峰值 {peak}；判据：Runtime / 在途数有上界、不无限堆积",
        rt_after - rt_before,
        rt_after
    );
}

/// V4：服务端长事务 / 行锁等待下两条连接内省，互不拖垮。
///
/// 判据（方案 §3.3.2）：各自超时可观测、互不拖垮；超时后连接懒重建路径可用
/// （懒重建属生产 `factory.rs:147-148` 路径，本探针只验证错误返回，不复现重建）。
/// 本地库（SQLite / DuckDB）无服务端行锁语义，本场景标注 N/A。
async fn v4_lock_wait(endpoints: &[ProbeEndpoint], metrics: Arc<ProbeMetrics>) {
    println!("\n===== V4 锁等待下双连接内省 =====");
    let server: Vec<&ProbeEndpoint> = endpoints.iter().filter(|e| e.kind.is_server()).collect();
    if server.is_empty() {
        println!("无服务端端点（PG/MySQL），V4 跳过；SQLite/DuckDB 本地文件无服务端行锁语义");
        return;
    }

    for endpoint in server {
        println!("--- {} 持锁事务 + 第二连接等待 ---", endpoint.label);

        // 探针表：事务外建好并提交，事务内持 `FOR UPDATE` 行锁。
        if let Err(e) = endpoint
            .db
            .query("DROP TABLE IF EXISTS rds_probe_b2_lock")
            .await
        {
            println!("V4 清理旧探针表失败（跳过本端点）：{e}");
            continue;
        }
        if let Err(e) = endpoint
            .db
            .query("CREATE TABLE rds_probe_b2_lock (id INT)")
            .await
        {
            println!("V4 建探针表失败（跳过本端点）：{e}");
            continue;
        }
        if let Err(e) = endpoint
            .db
            .query("INSERT INTO rds_probe_b2_lock (id) VALUES (1)")
            .await
        {
            println!("V4 插探针行失败（跳过本端点）：{e}");
            let _ = endpoint
                .db
                .query("DROP TABLE IF EXISTS rds_probe_b2_lock")
                .await;
            continue;
        }

        // 连接 B：同地址另建独立物理连接，观测跨连接互不拖垮。
        let second = match build_endpoint(
            endpoint.kind,
            format!("{}·B", endpoint.label),
            &endpoint.address,
        )
        .await
        {
            Ok(second) => second,
            Err(e) => {
                println!("V4 第二实例构造失败（跳过本端点）：{e}");
                let _ = endpoint
                    .db
                    .query("DROP TABLE IF EXISTS rds_probe_b2_lock")
                    .await;
                continue;
            }
        };

        // 连接 A：先在主流程内开启事务并拿到行锁，确认持锁后再启动 B 任务——
        // 否则 B 的 `FOR UPDATE` 可能先抢到锁，观测口径颠倒。
        let mut tx = match endpoint.db.begin_transaction().await {
            Ok(tx) => tx,
            Err(e) => {
                println!("V4 开启事务失败：{e}");
                let _ = endpoint
                    .db
                    .query("DROP TABLE IF EXISTS rds_probe_b2_lock")
                    .await;
                continue;
            }
        };
        if let Err(e) = tx
            .query("SELECT id FROM rds_probe_b2_lock WHERE id = 1 FOR UPDATE")
            .await
        {
            println!("V4 A 持锁查询失败（跳过本端点）：{e}");
            let _ = tx.rollback().await;
            let _ = endpoint
                .db
                .query("DROP TABLE IF EXISTS rds_probe_b2_lock")
                .await;
            continue;
        }
        let tx_holder: Arc<tokio::sync::Mutex<Option<Box<dyn Transaction>>>> =
            Arc::new(tokio::sync::Mutex::new(Some(tx)));

        // B 内省：A 持锁期间跑内省，观测是否被业务行锁拖垮（内省查 catalog 元数据，
        // 正常不应等待业务行锁；若墙钟异常拉长即说明锁等待发生传播）。
        let introspect_db = second.db.clone();
        let rx_introspect = spawn_probe_job(
            "V4-锁等待",
            &format!("{}·内省（持锁期间）", second.label),
            async move { load_children(&*introspect_db).await },
            metrics.clone(),
        );
        rx_introspect
            .recv()
            .expect("V4 内省任务通道不应断开")
            .print();

        // B 锁等待：同表 `FOR UPDATE` 等待 A 持有的行锁，30s 超时兜底，观测超时可观测性。
        let wait_db = second.db.clone();
        let rx_wait = spawn_probe_job(
            "V4-锁等待",
            &format!("{}·锁等待查询", second.label),
            async move {
                wait_db
                    .query("SELECT id FROM rds_probe_b2_lock WHERE id = 1 FOR UPDATE")
                    .await
                    .map(|r| TaskOutput { nodes: r.rows.len(), note: "FOR UPDATE".to_string() })
            },
            metrics.clone(),
        );
        rx_wait
            .recv()
            .expect("V4 锁等待任务通道不应断开")
            .print();

        // 收尾：结束 A 持锁事务、回滚、删探针表；B 实例随作用域丢弃。
        {
            let mut guard = tx_holder.lock().await;
            if let Some(mut tx) = guard.take() {
                let _ = tx.rollback().await;
            }
        }
        let _ = endpoint
            .db
            .query("DROP TABLE IF EXISTS rds_probe_b2_lock")
            .await;
        drop(second);
        println!("V4 收尾：探针表已清理");
    }
}

/// V5：不可达端点发起任务，观测错误如实返回、不挂死。
///
/// 口径注记：TCP 半开（空闲连接被网络层静默丢弃）本机不可稳定复现，本场景只覆盖
/// 「构造即不可达」；半开留待生产观察（方案 §3.3.2 V5、`factory.rs:147-148` 懒重建）。
async fn v5_unreachable(metrics: Arc<ProbeMetrics>) {
    println!("\n===== V5 不可达端点错误返回 =====");
    // 两个端口均为本机未监听端口，构造阶段即应连接失败；JOB_TIMEOUT 兜底防挂死。
    let cases: Vec<(EndpointKind, &str, &str)> = vec![
        (
            EndpointKind::Postgres,
            "PG·不可达",
            "postgres://postgres:wrongpw@127.0.0.1:59999/postgres",
        ),
        (
            EndpointKind::Mysql,
            "MySQL·不可达",
            "mysql://root:root@127.0.0.1:59998/mysql",
        ),
    ];
    for (kind, label, url) in cases {
        let rx = spawn_probe_job(
            "V5-断连",
            label,
            async move {
                let db = build_endpoint(kind, label.to_string(), url).await?;
                load_children(&*db.db).await
            },
            metrics.clone(),
        );
        rx.recv().expect("V5 任务通道不应断开").print();
    }
    println!("V5 口径注记：TCP 半开未覆盖（本机不可稳定复现）；懒重建路径属生产 factory 逻辑，本探针不复现");
}

/// 编排 V1–V5；端点按环境变量依次构造，缺变量跳过对应类别。
async fn run_probe(metrics: Arc<ProbeMetrics>) {
    println!("RdataStation B-2-0 并发前置探针（JOB_TIMEOUT = {}s）", JOB_TIMEOUT.as_secs());

    let mut endpoints: Vec<ProbeEndpoint> = Vec::new();

    if let Some(url) = env_url("RDS_TEST_PG_URL") {
        match build_endpoint(EndpointKind::Postgres, "PG·A".to_string(), &url).await {
            Ok(ep) => endpoints.push(ep),
            Err(e) => println!("PG 端点构造失败（跳过）：{e}"),
        }
    } else {
        println!("⏭️ 未设 RDS_TEST_PG_URL，PG 场景跳过");
    }

    if let Some(url) = env_url("RDS_TEST_MYSQL_URL") {
        match build_endpoint(EndpointKind::Mysql, "MySQL·A".to_string(), &url).await {
            Ok(ep) => endpoints.push(ep),
            Err(e) => println!("MySQL 端点构造失败（跳过）：{e}"),
        }
    } else {
        println!("⏭️ 未设 RDS_TEST_MYSQL_URL，MySQL 场景跳过");
    }

    let sqlite_path = env_url("RDS_TEST_SQLITE_PATH").unwrap_or_else(|| default_local_path("sqlite"));
    match build_endpoint(EndpointKind::Sqlite, "SQLite·A".to_string(), &sqlite_path).await {
        Ok(ep) => endpoints.push(ep),
        Err(e) => println!("SQLite 端点构造失败（跳过）：{e}"),
    }

    let duckdb_path = env_url("RDS_TEST_DUCKDB_PATH").unwrap_or_else(|| default_local_path("duckdb"));
    match build_endpoint(EndpointKind::Duckdb, "DuckDB·A".to_string(), &duckdb_path).await {
        Ok(ep) => endpoints.push(ep),
        Err(e) => println!("DuckDB 端点构造失败（跳过）：{e}"),
    }

    println!("已构造端点：{}", endpoints.iter().map(|e| e.label.as_str()).collect::<Vec<_>>().join("、"));

    v1_two_connections(&endpoints, metrics.clone()).await;
    v2_same_connection_queueing(&endpoints, metrics.clone()).await;
    v3_mixed_in_flight(&endpoints, metrics.clone()).await;
    v4_lock_wait(&endpoints, metrics.clone()).await;
    v5_unreachable(metrics.clone()).await;

    println!(
        "\n===== 汇总：Runtime 累计 {} 个，在途任务峰值 {} =====",
        metrics.rt_count.load(Ordering::SeqCst),
        metrics.in_flight_peak.load(Ordering::SeqCst)
    );
}

fn main() {
    // 外层编排 Runtime：场景内每个任务另起独立 Runtime（复刻现状基线形态）。
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("探针外层 Runtime 初始化失败");
    rt.block_on(run_probe(Arc::new(ProbeMetrics::default())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rds_engine::ConnectionError;

    #[test]
    fn median_of_odd_and_even_samples() {
        assert_eq!(median_ms(vec![1, 3, 2]), 2.0);
        assert_eq!(median_ms(vec![4, 1, 3, 2]), 2.5);
        assert_eq!(median_ms(Vec::new()), 0.0);
    }

    #[test]
    fn classify_error_buckets_connection_and_query() {
        let conn = CoreError::Connection(ConnectionError::NotFound("x".to_string()));
        assert_eq!(classify_error(&conn), "连接类");
        let common = CoreError::Common(CommonError::Timeout {
            operation: "op".to_string(),
            duration_ms: 1000,
        });
        assert_eq!(classify_error(&common), "超时");
    }
}

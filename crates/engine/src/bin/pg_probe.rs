// 临时诊断探针8：验证「PG 连接跨 tokio Runtime 复用挂起」假设。
// 探针7 单 Runtime 跨任务复用全成功；app 的 nav_jobs 在 worker 线程自建 Runtime 执行，
// scoped 连接跨任务（跨 Runtime）复用——若此处挂起，即 app_run68 connect_ms=2176/2647 的根因。
// 结构：Runtime A 建连接并存缓存 → Runtime B 拿同一连接 ping + 查询。
use shared::crypto::decrypt_password;
use sqlx::ConnectOptions as _;
use std::sync::Arc;
use std::time::Duration;

type SharedConn = Arc<tokio::sync::Mutex<Option<sqlx::postgres::PgConnection>>>;

fn build_url() -> String {
    let enc = "Id+lAU8XUPNlbCwWCLOB8d12lcLSJdNwFt8gbGmLDOaRYwg0xX4=";
    let pw = decrypt_password(enc).unwrap();
    format!("postgres://postgres:{pw}@192.168.3.138:5432/warehouse_supply_chain?sslmode=disable")
}

/// 模拟一次 nav_job（在指定 Runtime 内执行）：拿锁 → ping → SQL → drop（连接留缓存跨 Runtime 复用）。
async fn job(shared: &SharedConn, label: &str) {
    let t0 = std::time::Instant::now();
    let mut guard = shared.lock().await;
    if guard.is_none() {
        let mut opts: sqlx::postgres::PgConnectOptions = build_url().parse().unwrap();
        opts = opts.options([("plan_cache_mode", "force_custom_plan")]);
        *guard = Some(opts.connect().await.expect("建连失败"));
        println!("[{label}] 首次建连");
    }
    let tp = std::time::Instant::now();
    let ping_ok = tokio::time::timeout(Duration::from_secs(3), async {
        sqlx::query("SELECT 1").fetch_all(guard.as_mut().unwrap()).await
    }).await.map(|r| r.is_ok()).unwrap_or(false);
    println!("[{label}] ping: {}ms ok={ping_ok}", tp.elapsed().as_millis());
    if !ping_ok {
        return;
    }
    let ts = std::time::Instant::now();
    let r = tokio::time::timeout(Duration::from_secs(5), async {
        sqlx::query("SELECT c.relname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m','f') ORDER BY c.relname")
            .bind("biz_421").fetch_all(guard.as_mut().unwrap()).await
    }).await;
    match r {
        Ok(Ok(rows)) => println!("[{label}] SQL: {}ms rows={}", ts.elapsed().as_millis(), rows.len()),
        Ok(Err(e)) => println!("[{label}] SQL 失败: {e}"),
        Err(_) => println!("[{label}] SQL 超时(5s)"),
    }
    println!("[{label}] 任务总耗时 {}ms", t0.elapsed().as_millis());
    drop(guard);
}

fn main() {
    let shared: SharedConn = Arc::new(tokio::sync::Mutex::new(None));
    // 5 个独立 Runtime（模拟 app 多个 worker 线程各建一个 Runtime）
    let runtimes: Vec<tokio::runtime::Runtime> = (0..5)
        .map(|_| tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap())
        .collect();

    for (i, rt) in runtimes.iter().enumerate() {
        println!("\n===== 任务 {i}（Runtime {i}） =====");
        rt.block_on(job(&shared, &format!("任务{i}")));
        std::thread::sleep(Duration::from_millis(200));
    }
}

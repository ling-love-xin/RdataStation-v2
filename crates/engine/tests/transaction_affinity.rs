//! P0.2 探针：**事务会话亲和**（真实端点；未设置环境变量则跳过）
//!
//! ## 为什么需要这个探针
//!
//! 事务要求「`BEGIN` 与后续语句落在**同一个物理连接**上」。若驱动层每次查询都从连接池取
//! 一条新的物理连接，事务就会"看起来能用、实际无效"（`COMMIT`/`ROLLBACK` 作用在别的连接上）。
//! 这决定了事务功能是否需要引入 per-session 独占连接。
//!
//! ## 判据：临时表
//!
//! 临时表**只属于创建它的那条连接**，因此：
//! - 后续语句能看到它 → 会话亲和成立；
//! - 看不到（或建表就失败）→ 会话不亲和（事务功能必须换实现）。
//! 再用 `ROLLBACK` 后计数归零，验证事务语义真实生效。
//!
//! **本探针不改动用户数据**：只建临时表、只往临时表插一行、最后回滚。
//!
//! ## 运行方式
//!
//! ```sh
//! # PowerShell（凭据只从环境变量读，不写进仓库）
//! $env:RDS_TEST_MYSQL_URL="mysql://root:root@192.168.3.138:3306/mysql"
//! $env:RDS_TEST_PG_URL="postgres://postgres:postgresql@192.168.3.138:5432/postgres"
//! $env:RDS_TEST_SQLITE_PATH="D:\FossilT\T.fossil"
//! $env:RDS_TEST_DUCKDB_PATH="D:\data\123"
//! cargo test -p rds-engine --test transaction_affinity -j 2 -- --nocapture --test-threads=1
//! ```
//!
//! 未设置的目标自动跳过（不失败），便于无端点环境通过。

use std::sync::Arc;

// 集成测试是**独立 crate**：本包库目标名是 `rds_engine`（包名 `rds-engine`）。
// 其它 crate 里的 `engine::` 是它们 Cargo.toml 的依赖别名（`engine.workspace = true`），在本测试里不成立。
use rds_engine::connection_manager::ConnectionManager;
use rds_engine::services::sql_service::{SqlExecuteOptions, SqlExecuteResult, SqlService};
use rds_engine::{AutoDriverRegistrar, DriverConnectionConfig};

/// 执行选项：关缓存（避免缓存掩盖真实执行）、关历史（探针不污染用户历史）
fn options() -> SqlExecuteOptions {
    SqlExecuteOptions {
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15_000),
        use_cache: false,
    }
}

/// 取结果集首行首列（COUNT 这类单值结果）
fn first_cell(result: &SqlExecuteResult) -> Option<i64> {
    result
        .result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|value| value.as_int())
}

async fn probe(driver: &str, url_override: Option<String>, file_path: Option<String>) {
    let manager = Arc::new(ConnectionManager::new());

    let mut config = DriverConnectionConfig::new(driver);
    config.name = Some(format!("P0.2 探针（{driver}）"));
    config.url_override = url_override;
    config.file_path = file_path;

    let (conn_id, _db) = match manager.create_connection_with_registry(config).await {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("❌ {driver}：建连失败 —— {err}");
            return;
        }
    };
    eprintln!("➡️  {driver}：已连接（conn_id = {conn_id}）");

    let service = SqlService::new(manager.clone());

    // 1) 临时表（只属于创建它的连接）
    if let Err(err) = service
        .execute(
            Some(conn_id.clone()),
            "CREATE TEMPORARY TABLE rds_probe (id INT)",
            options(),
        )
        .await
    {
        eprintln!("❌ {driver}：建临时表失败 —— {err}");
        eprintln!("   （该驱动若不支持 TEMPORARY，可把语句改成 TEMP 再试）");
        manager.remove_connection(&conn_id).await;
        return;
    }

    // 2) BEGIN → INSERT → 计数
    let mut affinity_ok = false;
    for sql in [
        "BEGIN",
        "INSERT INTO rds_probe (id) VALUES (1)",
        "SELECT COUNT(*) AS n FROM rds_probe",
    ] {
        match service.execute(Some(conn_id.clone()), sql, options()).await {
            Ok(result) => {
                if sql.starts_with("SELECT") {
                    let n = first_cell(&result);
                    eprintln!("   · 事务内计数 = {n:?}（期望 1）");
                    match n {
                        Some(1) => {
                            affinity_ok = true;
                            eprintln!("   ✅ 临时表在事务内可见 → **会话亲和成立**");
                        }
                        Some(other) => eprintln!(
                            "   ❌ 计数 = {other} → 临时表不可见 → **会话不亲和**（事务需 per-session 独占连接）"
                        ),
                        None => eprintln!(
                            "   ⚠️ 驱动未填充 rows（total_rows = {}）→ 无法自动判定，需改读 Arrow batches",
                            result.result.total_rows
                        ),
                    }
                }
            }
            Err(err) => {
                eprintln!("❌ {driver}：`{sql}` 失败 —— {err}");
                break;
            }
        }
    }

    // 3) 回滚后再计数
    if affinity_ok {
        match service
            .execute(Some(conn_id.clone()), "ROLLBACK", options())
            .await
        {
            Ok(_) => match service
                .execute(
                    Some(conn_id.clone()),
                    "SELECT COUNT(*) AS n FROM rds_probe",
                    options(),
                )
                .await
            {
                Ok(result) => match first_cell(&result) {
                    Some(0) => eprintln!("   ✅ ROLLBACK 生效（回滚后计数 0）→ **事务语义真实可用**"),
                    Some(other) => {
                        eprintln!("   ❌ ROLLBACK 后计数 = {other}（期望 0）→ 事务语义不可信")
                    }
                    None => eprintln!("   ⚠️ 回滚后计数不可读（驱动未填充 rows）"),
                },
                Err(err) => eprintln!("❌ {driver}：回滚后计数失败 —— {err}"),
            },
            Err(err) => eprintln!("❌ {driver}：ROLLBACK 失败 —— {err}"),
        }
    }

    manager.remove_connection(&conn_id).await;
    eprintln!("—— {driver} 探针结束\n");
}

/// 文件型库（SQLite / DuckDB）走路径，网络型库走 URL
fn run(driver: &str, env_var: &str) {
    let Some(value) = std::env::var(env_var)
        .ok()
        .filter(|v| !v.trim().is_empty())
    else {
        eprintln!("⏭️  跳过 {driver}：未设置 {env_var}");
        return;
    };

    // 驱动注册是全局一次性动作（应用启动时也会做）
    AutoDriverRegistrar::register_builtin_drivers();

    let is_file_db = matches!(driver, "sqlite" | "duckdb");
    let (url_override, file_path) = if is_file_db {
        (None, Some(value))
    } else {
        (Some(value), None)
    };

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(probe(driver, url_override, file_path));
}

#[test]
fn mysql_transaction_session_affinity() {
    run("mysql", "RDS_TEST_MYSQL_URL");
}

#[test]
fn postgres_transaction_session_affinity() {
    run("postgres", "RDS_TEST_PG_URL");
}

#[test]
fn sqlite_transaction_session_affinity() {
    run("sqlite", "RDS_TEST_SQLITE_PATH");
}

#[test]
fn duckdb_transaction_session_affinity() {
    run("duckdb", "RDS_TEST_DUCKDB_PATH");
}

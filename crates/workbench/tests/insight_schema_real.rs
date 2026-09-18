//! 结构洞察的**真机**验证（M8：导航右键「结构洞察」的取数侧）。
//!
//! 链路：`InsightService::schema_report_view(conn_id, database, schema)`
//!   → `SchemaAnalyzer::analyze`（**走驱动元数据接口**：`MetadataBrowser` / `Database::list_*`）
//!   → 真驱动 → `SchemaReportView`。
//!
//! 为什么强调「走驱动元数据接口」：洞察**不再自己写 `information_schema` 方言 SQL**
//! （MySQL 用 `table_schema`、PG 用 catalog + schema、SQLite 走 `PRAGMA table_info`、
//! DuckDB 走 `duckdb_*`——这些差异已经在各自驱动里）。本文件的判据因此变成
//! 「与导航树同源」：结构洞察的边界就是**能被导航树看见的对象**。
//!
//! 环境变量（未设的库自动跳过，不算失败）与 `editor_exec_real.rs` 同一套：
//!
//! ```text
//! $env:RDS_TEST_MYSQL_URL="mysql://root:root@192.168.3.138:3306/mysql"
//! $env:RDS_TEST_PG_URL="postgres://postgres:postgresql@192.168.3.138:5432/postgres"
//! $env:RDS_TEST_SQLITE_PATH="D:\FossilT\T.fossil"
//! $env:RDS_TEST_DUCKDB_PATH="D:\data\123"
//! ```
//!
//! **sh / bash 下一律加单引号**（`D:\…` 的反斜杠会被吃掉 → 驱动在工作目录建空库 → 假通过）。
//!
//! 四个库各自的验证重点：
//! - **MySQL**：库名即 schema（`has_schema_level` = false，导航侧把 catalog 当 schema 传）；
//!   主键角标来自驱动算好的 `is_primary_key`（不再读 MySQL 私有的 `COLUMN_KEY`）
//! - **PostgreSQL**：真实 schema 层（`public`），有独立的（库, schema）校验
//! - **SQLite / DuckDB**：单库（导航树用 `main`）；SQLite 此前被「没有 information_schema」
//!   挡掉，改走驱动接口后**可用**——这正是本轮改动的收获之一
//! - **名写错了必须报错**（而不是给一张「零张表」的干净报告）
//!
//! 真库上会建两张 `rds_probe_schema_*` 探针表（`status` 一表 INT 一表 VARCHAR，
//! 用来验证「类型不一致」能检出），跑完**清理**（DROP）。

use engine::services::sql_service::{SqlExecuteOptions, SqlService};
use engine::{AutoDriverRegistrar, DriverConnectionConfig};

use insight::{InsightService, SchemaSection};

/// 探针表前缀（真库建表：前缀清晰 + 跑完清理）
const PREFIX: &str = "rds_probe_schema_";

struct Target {
    driver: &'static str,
    env: &'static str,
    /// 文件型库走 `file_path`
    file: bool,
    /// 导航树口径的（库, schema）。
    ///
    /// `None` = 向库自己要（MySQL 的 `DATABASE()`、PG 的 `current_database()` /
    /// `current_schema()`）；`Some` = 固定值：单库驱动（SQLite / DuckDB）跳过 Schema 层，
    /// 导航树把 catalog（`main`）当 schema 传下去——洞察侧接的就是那对值。
    nav_target: Option<(&'static str, &'static str)>,
    /// 该驱动有独立的 schema 层吗（PG = true）——决定要不要验「schema 写错」这一负例
    schema_level: bool,
}

const TARGETS: [Target; 4] = [
    Target {
        driver: "mysql",
        env: "RDS_TEST_MYSQL_URL",
        file: false,
        nav_target: None,
        schema_level: false,
    },
    Target {
        driver: "postgres",
        env: "RDS_TEST_PG_URL",
        file: false,
        nav_target: None,
        schema_level: true,
    },
    Target {
        driver: "sqlite",
        env: "RDS_TEST_SQLITE_PATH",
        file: true,
        nav_target: Some(("main", "main")),
        schema_level: false,
    },
    Target {
        driver: "duckdb",
        env: "RDS_TEST_DUCKDB_PATH",
        file: true,
        nav_target: Some(("main", "main")),
        schema_level: false,
    },
];

fn opts() -> SqlExecuteOptions {
    SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
    }
}

async fn connect(
    manager: &std::sync::Arc<engine::connection_manager::ConnectionManager>,
    target: &Target,
    value: &str,
) -> Option<String> {
    let mut config = DriverConnectionConfig::new(target.driver);
    config.name = Some(format!("M8 结构洞察探针（{}）", target.driver));
    if target.file {
        config.file_path = Some(value.to_string());
    } else {
        config.url_override = Some(value.to_string());
    }
    match manager.create_connection_with_registry(config).await {
        Ok((conn_id, _db)) => Some(conn_id),
        Err(error) => {
            eprintln!("❌ {}：建连失败 —— {error}", target.driver);
            None
        }
    }
}

/// 跑一句（建表 / 清理用）
async fn exec(conn_id: &str, sql: &str) -> Result<(), String> {
    let service = SqlService::new(engine::connection_manager::get_connection_manager().clone());
    service
        .execute(Some(conn_id.to_string()), sql, opts())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 取一个标量（当前库 / schema）
///
/// 取值走 `to_rows()`（由 Arrow `batches` 派生）而**不是 `rows` 字段**：native 驱动只填
/// `batches`，`rows` / `total_rows` 字段保持默认空值（架构 §12 #21）——读它会永远看到 0 行。
async fn scalar(conn_id: &str, sql: &str) -> Option<String> {
    let service = SqlService::new(engine::connection_manager::get_connection_manager().clone());
    let result = service
        .execute(Some(conn_id.to_string()), sql, opts())
        .await
        .ok()?;
    let rows = result.result.to_rows();
    eprintln!(
        "🔎 `{sql}` → columns={:?} batches={} 行={}",
        result.result.columns,
        result.result.batches.len(),
        rows.len()
    );
    rows.first()
        .and_then(|row| row.first())
        // `Value` 是引擎自己的枚举（不是 serde_json）：`Display` 对文本原样输出
        .map(|value| value.to_string())
        .filter(|text| !text.is_empty() && text != "NULL")
}

#[test]
fn schema_insight_runs_on_every_configured_database() {
    AutoDriverRegistrar::register_builtin_drivers();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let manager = engine::connection_manager::get_connection_manager().clone();

    let mut checked = 0;
    let mut failures: Vec<String> = Vec::new();

    for target in TARGETS {
        let target_started = std::time::Instant::now();
        let Ok(value) = std::env::var(target.env) else {
            eprintln!("⏭️  {}：未设 {}，跳过", target.driver, target.env);
            continue;
        };
        checked += 1;

        let Some(conn_id) = runtime.block_on(async {
            let started = std::time::Instant::now();
            let id = connect(&manager, &target, &value).await;
            eprintln!("⏱️  {}：建连耗时 {:.1?}", target.driver, started.elapsed());
            id
        }) else {
            failures.push(format!("{}：建连失败", target.driver));
            continue;
        };

        // 建两张探针表：`status` 一表 INT 一表 VARCHAR → 类型不一致应被检出
        let t1 = format!("{PREFIX}orders");
        let t2 = format!("{PREFIX}users");
        for drop_sql in [&t1, &t2]
            .iter()
            .map(|t| format!("DROP TABLE IF EXISTS {t}"))
        {
            let _ = runtime.block_on(exec(&conn_id, &drop_sql));
        }
        let ddl = [
            format!("CREATE TABLE {t1} (id INT, status INT, amount DECIMAL(10,2))"),
            format!("CREATE TABLE {t2} (id INT, status VARCHAR(20), nickname VARCHAR(30))"),
        ];
        let mut created = true;
        for sql in &ddl {
            if let Err(error) = runtime.block_on(exec(&conn_id, sql)) {
                failures.push(format!("{}：建探针表失败 —— {error}", target.driver));
                created = false;
                break;
            }
        }
        if !created {
            continue;
        }

        // 导航树口径的（库, schema）：单库驱动用固定 `main`，其余向库自己要
        let (database, schema) = match target.nav_target {
            Some(pair) => (pair.0.to_string(), pair.1.to_string()),
            None => {
                let is_mysql = target.driver.starts_with("mysql");
                let db_sql = if is_mysql {
                    "SELECT DATABASE()"
                } else {
                    "SELECT current_database()"
                };
                let database = runtime
                    .block_on(scalar(&conn_id, db_sql))
                    .unwrap_or_default();
                // MySQL 没有独立 Schema 层：导航树把 catalog 当 schema 传
                let schema = if is_mysql {
                    database.clone()
                } else {
                    runtime
                        .block_on(scalar(&conn_id, "SELECT current_schema()"))
                        .unwrap_or_default()
                };
                (database, schema)
            }
        };
        assert!(
            !database.is_empty(),
            "{}：读不到当前库名（nav_target = {:?}）",
            target.driver,
            target.nav_target
        );

        // 结构报告（真机）
        let started = std::time::Instant::now();
        let outcome = InsightService::schema_report_view(conn_id.clone(), &database, &schema);
        let elapsed = started.elapsed();
        let view = match outcome {
            Ok(view) => view,
            Err(error) => {
                failures.push(format!("{}：结构报告失败 —— {error}", target.driver));
                for t in [&t1, &t2] {
                    let _ = runtime.block_on(exec(&conn_id, &format!("DROP TABLE IF EXISTS {t}")));
                }
                continue;
            }
        };

        let mismatch = view
            .group(SchemaSection::TypeMismatches)
            .map(|group| group.rows.iter().any(|row| row.title.contains("status")))
            .unwrap_or(false);
        eprintln!(
            "✅ {}：库={database} schema={schema} → 标题={:?} 表 {} 列 {} 需关注 {} 项（类型不一致含 status = {mismatch}，耗时 {:.1?}）",
            target.driver,
            view.schema_name,
            view.table_count,
            view.total_columns,
            view.issue_count(),
            elapsed
        );

        assert!(
            !view.schema_name.is_empty(),
            "{}：报告标题不该为空",
            target.driver
        );
        assert!(
            view.table_count >= 2,
            "{}：报告应看见探针表（table_count={}，库={database} schema={schema}）",
            target.driver,
            view.table_count
        );
        assert!(
            view.total_columns >= 6,
            "{}：两张表至少 6 列（total_columns={}）",
            target.driver,
            view.total_columns
        );
        assert!(
            mismatch,
            "{}：`status` 一表 INT 一表 VARCHAR，应检出类型不一致（分组：{:?}）",
            target.driver,
            view.group(SchemaSection::TypeMismatches)
                .map(|group| group.rows.iter().map(|row| row.title.clone()).collect::<Vec<_>>())
        );

        // 负例：**名写错了要报错**，而不是给一张「零张表」的干净报告
        let bogus_db = InsightService::schema_report_view(
            conn_id.clone(),
            "rds_no_such_database_xyz",
            &schema,
        );
        match bogus_db {
            Ok(view) => failures.push(format!(
                "{}：库名不存在却返回了报告（{} 表）——零张表不能冒充结论",
                target.driver, view.table_count
            )),
            Err(error) => {
                let text = error.to_string();
                if !text.contains("rds_no_such_database_xyz") {
                    failures.push(format!(
                        "{}：报错要把写错的名字带上，实际：{text}",
                        target.driver
                    ));
                } else {
                    eprintln!("✅ {}：库名写错 → 可读回执", target.driver);
                }
            }
        }
        if target.schema_level {
            let bogus_schema = InsightService::schema_report_view(
                conn_id.clone(),
                &database,
                "rds_no_such_schema_xyz",
            );
            match bogus_schema {
                Ok(view) => failures.push(format!(
                    "{}：schema 不存在却返回了报告（{} 表）",
                    target.driver, view.table_count
                )),
                Err(error) => {
                    let text = error.to_string();
                    if !text.contains("rds_no_such_schema_xyz") {
                        failures.push(format!(
                            "{}：schema 报错要把名字带上，实际：{text}",
                            target.driver
                        ));
                    } else {
                        eprintln!("✅ {}：schema 写错 → 可读回执", target.driver);
                    }
                }
            }
        }

        // 清理（真库不留探针表）
        let cleanup_started = std::time::Instant::now();
        for t in [&t1, &t2] {
            if let Err(error) = runtime.block_on(exec(&conn_id, &format!("DROP TABLE IF EXISTS {t}"))) {
                eprintln!("⚠️  {}：清理 {t} 失败 —— {error}", target.driver);
            }
        }
        eprintln!(
            "⏱️  {}：本目标合计 {:.1?}（清理 {:.1?}）",
            target.driver,
            target_started.elapsed(),
            cleanup_started.elapsed()
        );
    }

    if checked == 0 {
        eprintln!("⏭️  四个环境变量都没设：本用例无真机可验（不算失败）");
        return;
    }
    assert!(
        failures.is_empty(),
        "真机结构洞察失败：\n{}",
        failures.join("\n")
    );
}

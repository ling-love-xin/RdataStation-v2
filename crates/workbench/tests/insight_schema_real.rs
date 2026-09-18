//! 结构洞察的**真机**验证（M8 收口：导航右键「结构洞察」的取数侧）。
//!
//! 链路：`InsightService::schema_report_view(conn_id, database, schema)`
//!   → `SchemaAnalyzer::analyze`（按连接的 `db_type` 选 `information_schema` 方言）
//!   → 真驱动 → `SchemaReportView`。
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
//! 四个库各自的验证重点（这正是这条路径此前没跑通的原因）：
//! - **MySQL**：`information_schema.tables` **没有 `table_catalog`**（拿它过滤会报列不存在）、
//!   库名在 `table_schema`；`column_key` 是 MySQL 专有列（能工作）
//! - **PostgreSQL / DuckDB**：**没有 `column_key`**（要 `'' AS column_key`）；
//!   `table_catalog` = 库名、`table_schema` = schema（双条件过滤）
//! - **SQLite**：没有 `information_schema` → 必须给**可读回执**，不能报「零张表」
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
    /// 没有 `information_schema`：走「可读回执」断言
    unsupported: bool,
}

const TARGETS: [Target; 4] = [
    Target {
        driver: "mysql",
        env: "RDS_TEST_MYSQL_URL",
        file: false,
        unsupported: false,
    },
    Target {
        driver: "postgres",
        env: "RDS_TEST_PG_URL",
        file: false,
        unsupported: false,
    },
    Target {
        driver: "sqlite",
        env: "RDS_TEST_SQLITE_PATH",
        file: true,
        unsupported: true,
    },
    Target {
        driver: "duckdb",
        env: "RDS_TEST_DUCKDB_PATH",
        file: true,
        unsupported: false,
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
        let Ok(value) = std::env::var(target.env) else {
            eprintln!("⏭️  {}：未设 {}，跳过", target.driver, target.env);
            continue;
        };
        checked += 1;

        let Some(conn_id) = runtime.block_on(connect(&manager, &target, &value)) else {
            failures.push(format!("{}：建连失败", target.driver));
            continue;
        };

        if target.unsupported {
            // SQLite：没有 information_schema，必须**明确回绝**（不是空报告）
            match InsightService::schema_report_view(conn_id, "", "") {
                Ok(view) => failures.push(format!(
                    "{}：该方言没有 information_schema，却返回了报告（{} 表）",
                    target.driver, view.table_count
                )),
                Err(error) => {
                    let text = error.to_string();
                    assert!(
                        text.contains("information_schema") || text.contains("不支持"),
                        "{}：回执要可读（说明为什么不支持），实际：{text}",
                        target.driver
                    );
                    eprintln!("✅ {}：按预期明确回绝 —— {text}", target.driver);
                }
            }
            continue;
        }

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

        // 当前库 / schema：向库自己要（不解析 URL——猜错了报告就查错地方）
        let is_mysql = target.driver.starts_with("mysql");
        // 探针：分清「驱动对所有查询都空」与「只是 DATABASE() 取不到」
        runtime.block_on(scalar(&conn_id, "SELECT 1 AS n"));
        runtime.block_on(scalar(
            &conn_id,
            "SELECT COUNT(*) AS c FROM information_schema.tables",
        ));
        let db_sql = if is_mysql {
            "SELECT DATABASE()"
        } else {
            "SELECT current_database()"
        };
        let sch_sql = if is_mysql {
            "SELECT DATABASE()"
        } else {
            "SELECT current_schema()"
        };
        let database = runtime
            .block_on(scalar(&conn_id, db_sql))
            .unwrap_or_default();
        let schema = runtime
            .block_on(scalar(&conn_id, sch_sql))
            .unwrap_or_default();
        assert!(
            !database.is_empty(),
            "{}：读不到当前库名（{db_sql}）",
            target.driver
        );

        // 结构报告（真机）
        let outcome = InsightService::schema_report_view(conn_id.clone(), &database, &schema);
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
            "✅ {}：schema_name={:?} 表 {} 列 {} 需关注 {} 项（类型不一致含 status = {mismatch}）",
            target.driver,
            view.schema_name,
            view.table_count,
            view.total_columns,
            view.issue_count()
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

        // 清理（真库不留探针表）
        for t in [&t1, &t2] {
            if let Err(error) = runtime.block_on(exec(&conn_id, &format!("DROP TABLE IF EXISTS {t}"))) {
                eprintln!("⚠️  {}：清理 {t} 失败 —— {error}", target.driver);
            }
        }
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

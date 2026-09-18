//! 源取样的**真机**验证（M8 收口：导航树 / 存档「查看洞察」那一类入口的取数侧）。
//!
//! 链路：`SampleSource { conn_id, sql, label }` → 洞察侧包一层 `LIMIT 500` →
//! 源库执行 → `tmp_i_*` 分析临时表（登记）→ 列画像 / 表探查。
//!
//! 为什么单独一个真机用例：这条路径要**过 Rust**（源库结果 → 打型 → 灌 DuckDB），
//! 而「结果怎么从引擎取出来」曾经照 v1 的 JSON 形态读（`json["batches"][0]["rows"]`）——
//! v2 的契约序列化不含 `batches`，于是样本恒空（真机踩过）。DuckDB 内存那一支
//! （文件类数据源，`conn_id: None`）走 `CREATE TABLE … AS` 不过 Rust，遮不住这个洞。
//!
//! 环境变量（未设的库自动跳过，不算失败）与 `editor_exec_real.rs` 同一套：
//!
//! ```text
//! $env:RDS_TEST_MYSQL_URL="mysql://root:root@192.168.3.138:3306/mysql"
//! $env:RDS_TEST_PG_URL="postgres://postgres:postgresql@192.168.3.138:5432/postgres"
//! $env:RDS_TEST_SQLITE_PATH="D:\FossilT\T.fossil"
//! $env:RDS_TEST_DUCKDB_PATH="D:\data\123"
//! $env:RDS_TEST_ORACLE_URL="oracle://devuser:***@192.168.3.138:1521/XEPDB1"
//! ```
//!
//! **sh / bash 下一律加单引号**（`D:\…` 的反斜杠会被吃掉 → 驱动在工作目录建空库 → 假通过）。
//!
//! 真库上会建 `rds_probe_source_*` 探针表（3 行，含一个 NULL），跑完清理（DROP）。
//!
//! 第二个用例是**扩展提供的源**：Oracle 经 community 扩展 `oracle_scanner`（表函数）读进来，
//! 同样能取样并出列画像 / 表探查——它是 D59 边界「凡 DuckDB 能分析的资源都能洞察」的实证。
//! 那条需要 `RDS_TEST_ORACLE_URL`，并会在 Oracle 侧建自己的 `RDS_PROBE_SOURCE_ORA`（跑完 DROP）。

use engine::services::sql_service::{SqlExecuteOptions, SqlService};
use engine::{AutoDriverRegistrar, DriverConnectionConfig};

use insight::{ColumnKind, InsightService, SampleSource};

/// 探针表前缀（真库建表：前缀清晰 + 跑完清理）
const PREFIX: &str = "rds_probe_source_";

struct Target {
    driver: &'static str,
    env: &'static str,
    /// 文件型库走 `file_path`
    file: bool,
}

const TARGETS: [Target; 4] = [
    Target {
        driver: "mysql",
        env: "RDS_TEST_MYSQL_URL",
        file: false,
    },
    Target {
        driver: "postgres",
        env: "RDS_TEST_PG_URL",
        file: false,
    },
    Target {
        driver: "sqlite",
        env: "RDS_TEST_SQLITE_PATH",
        file: true,
    },
    Target {
        driver: "duckdb",
        env: "RDS_TEST_DUCKDB_PATH",
        file: true,
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
    config.name = Some(format!("M8 源取样探针（{}）", target.driver));
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

/// 跑一句（建表 / 插数 / 清理用）
async fn exec(conn_id: &str, sql: &str) -> Result<(), String> {
    let service = SqlService::new(engine::connection_manager::get_connection_manager().clone());
    service
        .execute(Some(conn_id.to_string()), sql, opts())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[test]
fn source_sampling_reaches_a_column_profile_on_every_configured_database() {
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

        // 3 行：`amount` 无空值、`note` 有一个 NULL（NULL 率也要能算出来）
        let table = format!("{PREFIX}{}", target.driver);
        let _ = runtime.block_on(exec(&conn_id, &format!("DROP TABLE IF EXISTS {table}")));
        let ddl = format!("CREATE TABLE {table} (id INT, amount INT, note VARCHAR(20))");
        if let Err(error) = runtime.block_on(exec(&conn_id, &ddl)) {
            failures.push(format!("{}：建探针表失败 —— {error}", target.driver));
            continue;
        }
        let insert = format!(
            "INSERT INTO {table} (id, amount, note) VALUES (1, 10, 'a'), (2, 20, NULL), (3, 30, 'c')"
        );
        if let Err(error) = runtime.block_on(exec(&conn_id, &insert)) {
            failures.push(format!("{}：插探针数据失败 —— {error}", target.driver));
            let _ = runtime.block_on(exec(&conn_id, &format!("DROP TABLE IF EXISTS {table}")));
            continue;
        }

        // 源取样 → 列画像（`amount`：无空值）
        let source = SampleSource::new(
            conn_id.clone(),
            format!("SELECT * FROM {table}"),
            format!("probe.{table}"),
        );
        let amount = InsightService::profile_source_column(None, &source, "amount");
        match amount {
            Ok((temp_table, profile)) => {
                eprintln!(
                    "✅ {}：样本表 {temp_table} · amount 计数={} 空值={} 类型={:?}",
                    target.driver, profile.total_count, profile.null_count, profile.kind
                );
                if !temp_table.starts_with("tmp_i_") {
                    failures.push(format!(
                        "{}：样本表要带洞察前缀（TTL / 上限 / 按来源清理才认它），实际 {temp_table}",
                        target.driver
                    ));
                }
                if profile.total_count != 3 || profile.null_count != 0 {
                    failures.push(format!(
                        "{}：amount 应为 3 行 0 空值，实际 {} 行 {} 空值（样本没落到临时表？）",
                        target.driver, profile.total_count, profile.null_count
                    ));
                }
                if profile.kind != ColumnKind::Numeric {
                    failures.push(format!(
                        "{}：amount 是 INT，应判数值列，实际 {:?}",
                        target.driver, profile.kind
                    ));
                }
            }
            Err(error) => failures.push(format!("{}：源列画像失败 —— {error}", target.driver)),
        }

        // 有 NULL 的列：空值率要真算出来（不是「零行」的假象）
        match InsightService::profile_source_column(None, &source, "note") {
            Ok((_, profile)) => {
                if profile.total_count != 3 || profile.null_count != 1 {
                    failures.push(format!(
                        "{}：note 应为 3 行 1 空值，实际 {} 行 {} 空值",
                        target.driver, profile.total_count, profile.null_count
                    ));
                }
            }
            Err(error) => failures.push(format!("{}：note 画像失败 —— {error}", target.driver)),
        }

        // 表探查：同一份样本上逐列内省
        match InsightService::profile_source_table(&source, &table) {
            Ok((_, view)) => {
                eprintln!(
                    "✅ {}：表探查 {} 行 {} 列",
                    target.driver,
                    view.row_count,
                    view.columns.len()
                );
                if view.row_count != 3 || view.columns.len() != 3 {
                    failures.push(format!(
                        "{}：表探查应为 3 行 3 列，实际 {} 行 {} 列",
                        target.driver,
                        view.row_count,
                        view.columns.len()
                    ));
                }
            }
            Err(error) => failures.push(format!("{}：源表探查失败 —— {error}", target.driver)),
        }

        // 清理（真库不留探针表）
        if let Err(error) = runtime.block_on(exec(&conn_id, &format!("DROP TABLE IF EXISTS {table}"))) {
            eprintln!("⚠️  {}：清理 {table} 失败 —— {error}", target.driver);
        }
    }

    if checked == 0 {
        eprintln!("⏭️  四个环境变量都没设：本用例无真机可验（不算失败）");
        return;
    }
    assert!(
        failures.is_empty(),
        "真机源取样失败：\n{}",
        failures.join("\n")
    );
}

// ==================== 扩展提供的源（Oracle via `oracle_scanner`）====================

/// Oracle 连接串的几段（只解析本用例需要的形状）
struct OracleParts {
    host: String,
    port: u16,
    user: String,
    password: String,
    service: String,
}

fn parse_oracle_url(url: &str) -> OracleParts {
    let rest = url.split("://").nth(1).expect("URL 要有 scheme");
    let (cred, host_part) = rest.split_once('@').expect("URL 要带凭据");
    let (user, password) = cred.split_once(':').expect("凭据要有 user:pass");
    let (host_port, service) = host_part.split_once('/').expect("URL 要有 service name");
    let (host, port) = host_port.split_once(':').expect("要有端口");
    OracleParts {
        host: host.to_string(),
        port: port.parse().expect("端口是数字"),
        user: user.to_string(),
        password: password.to_string(),
        service: service.to_string(),
    }
}

/// 扩展提供的源同样能洞察：Oracle（community 扩展 `oracle_scanner`）→ 取样 → 列画像 / 表探查。
///
/// 实证的是 D59 的边界原话——**凡 DuckDB 能分析的资源都能洞察**：洞察侧只认「一段能在它
/// 自己的内存库里跑的只读 SQL」，至于数据怎么进来的（文件读取器 / `ATTACH` / community
/// 扩展的表函数）是调用方的事。这里特意选了最像「扩展」的一条：Oracle 表函数。
///
/// 只在设了 `RDS_TEST_ORACLE_URL` 时跑（未设自动跳过）；凭据只从环境变量读、只建**会话级**
/// Secret（不落盘）。会在 Oracle 建自己的探针表并在跑完 DROP（不碰用户对象）。
#[test]
fn extension_provided_source_is_analyzable() {
    let Ok(url) = std::env::var("RDS_TEST_ORACLE_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_ORACLE_URL，跳过扩展源用例");
        return;
    };
    let parts = parse_oracle_url(&url);
    const SECRET: &str = "rds_insight_ora";
    const TABLE: &str = "RDS_PROBE_SOURCE_ORA";

    // 1) 在**洞察自己的内存库**上装扩展 + 建会话级 Secret。
    //    扩展目录固定到仓库 scratch（与 `engine/tests/oracle_probe.rs` 同一处）：
    //    跑过一次就有缓存，也不碰产品目录。
    let ext = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/duckdb-ext-scratch");
    std::fs::create_dir_all(&ext).expect("建扩展目录");
    {
        let duckdb = insight::insight_engine::get_or_create_duckdb().expect("内存库");
        let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
        conn.execute_batch(&format!(
            "SET extension_directory = '{}';\
             SET autoinstall_known_extensions = false;\
             SET autoload_known_extensions = true;",
            ext.to_string_lossy().replace('\\', "/")
        ))
        .expect("设置扩展目录");
        conn.execute_batch("INSTALL oracle_scanner FROM community; LOAD oracle_scanner")
            .expect("装 / 载 oracle_scanner（需要网络；失败先看扩展仓库是否可达）");
        conn.execute_batch(&format!(
            "CREATE OR REPLACE SECRET {SECRET} (TYPE ORACLE, HOST '{}', PORT {}, \
             USER '{}', PASSWORD '{}', SERVICE_NAME '{}')",
            parts.host, parts.port, parts.user, parts.password, parts.service
        ))
        .expect("建 Oracle Secret");
    }

    // 在洞察的内存库上跑一句（DDL 在 Oracle 侧要经 `DBMS_UTILITY.EXEC_DDL_STATEMENT`）
    let on_duckdb = |batch: String| {
        let duckdb = insight::insight_engine::get_or_create_duckdb().expect("内存库");
        let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
        conn.execute_batch(&batch)
    };
    let ddl = |sql: &str| -> String {
        format!(
            "SELECT * FROM oracle_call_auto('{SECRET}', 'DBMS_UTILITY.EXEC_DDL_STATEMENT', ['{sql}'])"
        )
    };

    // 2) 在 Oracle 侧建探针表（先试探着删一次：上次失败可能留下过）
    let _ = on_duckdb(ddl(&format!("DROP TABLE {TABLE}")));
    on_duckdb(ddl(&format!(
        "CREATE TABLE {TABLE} (id NUMBER(10), amount NUMBER(10,2), note VARCHAR2(20))"
    )))
    .expect("建 Oracle 探针表");
    for (id, amount, note) in [(1, "10.50", "a"), (2, "20.25", "b"), (3, "30.75", "c")] {
        on_duckdb(format!(
            "SELECT * FROM oracle_execute('{SECRET}', \
             'INSERT INTO {TABLE} VALUES ({id}, {amount}, ''{note}'')')"
        ))
        .expect("插 Oracle 探针行");
    }

    // 3) 洞察只看到「一段能在内存库里跑的 SQL」——扩展读进来的数据与其他源同路
    let source = SampleSource::on_duckdb(
        format!("SELECT * FROM oracle_query('{SECRET}', 'SELECT id, amount, note FROM {TABLE}')"),
        format!("oracle.{TABLE}"),
    );

    let outcome = InsightService::profile_source_column(None, &source, "amount");
    // 凭据 / 网络问题要看得到原因，不要变成一个光秃秃的 panic
    let (temp_table, view) = match outcome {
        Ok(pair) => pair,
        Err(error) => panic!("Oracle 列画像失败（扩展源）：{error}"),
    };
    eprintln!(
        "✅ oracle(扩展)：样本表 {temp_table} · amount 计数={} 空值={} 类型={:?}",
        view.total_count, view.null_count, view.kind
    );
    assert!(
        temp_table.starts_with("tmp_i_"),
        "样本表要带洞察前缀，实际 {temp_table}"
    );
    assert_eq!(view.total_count, 3, "三行应全部落到样本表");
    assert_eq!(view.null_count, 0);
    assert_eq!(view.kind, ColumnKind::Numeric, "NUMBER(10,2) 应判数值列");

    let (_, table_view) =
        InsightService::profile_source_table(&source, TABLE).expect("Oracle 表探查");
    assert_eq!(table_view.row_count, 3);
    assert_eq!(table_view.columns.len(), 3, "id / amount / note 三列");

    // 4) 清理（Oracle 侧不留探针表）
    if let Err(error) = on_duckdb(ddl(&format!("DROP TABLE {TABLE}"))) {
        eprintln!("⚠️  清理 Oracle 探针表失败：{error}");
    }
}

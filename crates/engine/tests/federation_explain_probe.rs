//! 联邦计划形态探针（真机）：钉住 2026-11-27 实测的 `EXPLAIN (FORMAT JSON)` 两种形态。
//!
//! 跑法（凭据只从环境变量读；未设就跳过，不算失败）：
//! ```text
//! $env:RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql'
//! $env:RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres'
//! cargo test -p rds-engine -j 1 --test federation_explain_probe -- --nocapture --test-threads=1
//! ```
//!
//! **为什么要有这一条**：刀36 把执行计划接进了结果区，刀37 的真机取证发现 MySQL 的外部源
//! 查询**只出** `MYSQL_QUERY`（整句下推），而不是 `MYSQL_SCAN`——刀29 的判定因此漏了 MySQL。
//! 这条用例把当时的形态钉成可复算的断言：DuckDB 扩展或驱动形态一变，这里立刻红，
//! 不用等用户看到一张空白的计划表才发现。
//!
//! 钉住四件事：
//! 1. `EXPLAIN (FORMAT JSON)` 对 ATTACH 的外部源**可用**，回执是 `explain_key` / `explain_value` 两列；
//! 2. MySQL 的任意外部源查询 → 单节点 `MYSQL_QUERY`，`whole_statement = true`、
//!    结论「整句下推 · 源端执行」；
//! 3. PG 的简单查询 → `POSTGRES_SCAN`，带 `Table` 与 `Projections`（`projections_pushed = true`）；
//!    带本地 join 时退化成 `POSTGRES_QUERY`（整句下推）；
//! 4. 反例：本地 `SEQ_SCAN` 不算外部源（`has_full_table_scan` 的语义只归本地）。

// 注意 crate 名：集成测试是独立 crate，本包的库目标叫 `rds_engine`。

use rds_engine::duckdb::ExplainAnalyzer;
use rds_engine::duckdb::federation::registry::FederatedSource;
use rds_engine::duckdb::federation::session::FederatedSession;
use rds_engine::duckdb::instance::AnalysisInstance;

/// 打开一条带真源的联邦会话；`instance` 必须与 `session` 一起活到用例结束
fn open_session(
    sources: Vec<FederatedSource>,
    main_alias: &str,
) -> (std::sync::Arc<AnalysisInstance>, FederatedSession) {
    let tag = format!("fed_explain_probe_{}", std::process::id());
    let instance = AnalysisInstance::open_tagged(&tag, std::time::Duration::from_secs(30))
        .expect("开分析实例");
    let analysis_session = instance.open_session(&tag, "计划形态探针").expect("开会话");
    let session =
        FederatedSession::open(analysis_session, &sources, Some(main_alias)).expect("建联邦会话");
    (instance, session)
}

/// 源里第一张用户表（跳过系统 schema）
fn first_table(session: &FederatedSession, alias: &str) -> Option<(String, String)> {
    let sql = format!(
        "SELECT schema_name, table_name FROM duckdb_tables() \
         WHERE database_name = '{alias}' AND schema_name NOT IN ('information_schema', 'pg_catalog') \
         LIMIT 1"
    );
    let rows = session.run(&sql).ok()?.to_rows();
    rows.first()
        .map(|row| (row[0].to_string(), row[1].to_string()))
}

fn first_column(session: &FederatedSession, schema: &str, table: &str) -> Option<String> {
    let sql = format!(
        "SELECT column_name FROM duckdb_columns() \
         WHERE schema_name = '{schema}' AND table_name = '{table}' LIMIT 1"
    );
    session
        .run(&sql)
        .ok()?
        .to_rows()
        .first()
        .map(|row| row[0].to_string())
}

/// 跑一句 `EXPLAIN (FORMAT JSON)`，断言回执是两列，返回计划正文
fn explain_payload(session: &FederatedSession, sql: &str) -> String {
    let result = session
        .run(sql)
        .unwrap_or_else(|reason| panic!("{sql} 跑不通：{reason}"));
    assert_eq!(
        result.columns,
        vec!["explain_key".to_string(), "explain_value".to_string()],
        "EXPLAIN (FORMAT JSON) 的回执必须是两列：{sql}"
    );
    let rows = result.to_rows();
    assert_eq!(rows.len(), 1, "一次 EXPLAIN 只回一行：{sql}");
    let payload = rows[0][1].to_string();
    assert!(
        payload.trim_start().starts_with('['),
        "计划正文应是 JSON 数组，实际开头：{:.60}",
        payload
    );
    payload
}

#[test]
fn mysql_federated_plan_is_a_whole_statement_query() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过（真机形态未验证）");
        return;
    };
    let source =
        FederatedSource::new("mysql", "mysql_src", "mysql_native", &url).expect("组装 MySQL 源");
    let (_instance, session) = open_session(vec![source], "mysql_src");

    let Some((schema, table)) = first_table(&session, "mysql_src") else {
        panic!("MySQL 源里没有可用的表");
    };
    let qualified = format!("mysql_src.{schema}.{table}");
    let column = first_column(&session, &schema, &table).expect("拿不到列名");

    // 两种形态都试：不投影具体列 / 带谓词——2026-11-27 实测两者都是同一个 MYSQL_QUERY 节点
    for sql_body in [
        format!("SELECT * FROM {qualified} LIMIT 5"),
        format!("SELECT {column} FROM {qualified} WHERE {column} IS NOT NULL LIMIT 5"),
    ] {
        let payload = explain_payload(&session, &format!("EXPLAIN (FORMAT JSON) {sql_body}"));
        let plan = ExplainAnalyzer::parse_json_plan(&payload)
            .unwrap_or_else(|reason| panic!("计划解析失败（{sql_body}）：{reason}"));

        assert_eq!(
            plan.operator, "MYSQL_QUERY",
            "MySQL 的外部源查询应整句下推成 MYSQL_QUERY（{sql_body}）"
        );
        let facts = ExplainAnalyzer::collect_source_scans(&plan);
        assert_eq!(facts.len(), 1, "整句下推节点必须被认成外部源（{sql_body}）");
        assert!(
            facts[0].whole_statement,
            "MYSQL_QUERY 属整句下推（{sql_body}）"
        );
        assert_eq!(
            facts[0].summary(),
            "整句下推 · 源端执行",
            "整句下推不能报「投影未推」（{sql_body}）"
        );
    }
}

#[test]
fn postgres_federated_plan_degrades_from_scan_to_query() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过（真机形态未验证）");
        return;
    };
    let source =
        FederatedSource::new("postgres", "pg_src", "postgres_native", &url).expect("组装 PG 源");
    let (_instance, session) = open_session(vec![source], "pg_src");

    let Some((schema, table)) = first_table(&session, "pg_src") else {
        panic!("PG 源里没有可用的表");
    };
    let qualified = format!("pg_src.{schema}.{table}");
    let column = first_column(&session, &schema, &table).expect("拿不到列名");

    // 1) 简单查询 → POSTGRES_SCAN，带表名与投影
    let payload = explain_payload(
        &session,
        &format!("EXPLAIN (FORMAT JSON) SELECT {column} FROM {qualified} LIMIT 5"),
    );
    let plan = ExplainAnalyzer::parse_json_plan(&payload)
        .unwrap_or_else(|reason| panic!("解析失败：{reason}"));
    assert_eq!(
        plan.operator, "POSTGRES_SCAN",
        "PG 的简单外部源查询应是扫描形态，实际 {}",
        plan.operator
    );
    let facts = ExplainAnalyzer::collect_source_scans(&plan);
    assert_eq!(facts.len(), 1);
    assert!(!facts[0].whole_statement, "POSTGRES_SCAN 不是整句下推");
    assert_eq!(
        facts[0].table.as_deref(),
        Some(table.as_str()),
        "扫描形态应带表名"
    );
    assert!(facts[0].projections_pushed, "只查一列时投影应下推");

    // 2) 带本地 join → POSTGRES_QUERY（整句下推），此时表名与下推细节都看不到
    let joined = format!(
        "EXPLAIN (FORMAT JSON) SELECT p.{column} FROM {qualified} p \
         JOIN (SELECT 1 AS k) l ON p.{column} = l.k LIMIT 5"
    );
    let payload = explain_payload(&session, &joined);
    let plan = ExplainAnalyzer::parse_json_plan(&payload)
        .unwrap_or_else(|reason| panic!("解析失败（join）：{reason}"));
    assert_eq!(
        plan.operator, "POSTGRES_QUERY",
        "PG 带本地 join 时应退化成整句下推，实际 {}",
        plan.operator
    );
    let facts = ExplainAnalyzer::collect_source_scans(&plan);
    assert_eq!(facts.len(), 1, "POSTGRES_QUERY 也要被认成外部源");
    assert!(facts[0].whole_statement);
    assert_eq!(facts[0].table, None, "整句下推拿不到表名");
    assert_eq!(facts[0].summary(), "整句下推 · 源端执行");
}

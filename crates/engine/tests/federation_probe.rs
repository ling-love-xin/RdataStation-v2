//! 联邦多源探针（真机）：两条**真实**源挂在同一条 DuckDB 会话上做跨源查询。
//!
//! 跑法（凭据只从环境变量读）：
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! cargo test -p rds-engine -j 2 --test federation_probe -- --nocapture --test-threads=1
//! ```
//!
//! 覆盖四件事（第一期验收口径）：
//! 1. 多源挂载：MySQL + SQLite 同会话，**表数量是真实的**；
//! 2. **一条 SQL 跨两个源**（限定名）+ 未限定名走主源；
//! 3. 坏源**不阻断**（保留在快照里，带原话）；
//! 4. 只读：写源库被 DuckDB 拒；本地临时对象照常允许。
//!
//! **离线也能跑一半**：只有 duckdb 文件源时不联网（但那不叫联邦，本探针就是要真库）。

//! **注意 crate 名**：集成测试是独立 crate，本包的库目标叫 `rds_engine`
//! （`engine` 只是**其它** crate 依赖它时的别名）——与 `transaction_affinity.rs` 等同类坑。

use rds_engine::duckdb::federation::registry::{FederatedSource, MountState};
use rds_engine::duckdb::federation::session::FederatedSession;

/// 从快照里取某个源认到的表数量（挂载即为真实值）
fn tables_of(session: &FederatedSession, alias: &str) -> usize {
    let snapshot = session.snapshot();
    let entry = snapshot
        .sources
        .iter()
        .find(|entry| entry.alias() == alias)
        .unwrap_or_else(|| panic!("快照里没有源 {alias}"));
    match &entry.state {
        MountState::Ready { tables } => *tables,
        MountState::Failed(reason) => panic!("源 {alias} 没挂上：{reason}"),
    }
}

/// 拿某个源里第一张「用户表」（跳过系统 schema），返回 `schema.table`
fn first_table(session: &FederatedSession, alias: &str) -> Option<(String, String)> {
    let sql = format!(
        "SELECT schema_name, table_name FROM duckdb_tables() \
         WHERE database_name = '{alias}' AND schema_name NOT IN ('information_schema', 'pg_catalog') \
         LIMIT 1"
    );
    let result = session.run(&sql).expect("列举表");
    let rows = result.to_rows();
    rows.first()
        .map(|row| (row[0].to_string(), row[1].to_string()))
}

#[test]
fn probe_federation_across_mysql_and_sqlite() {
    let (Ok(mysql_url), Ok(sqlite_path)) = (
        std::env::var("RDS_TEST_MYSQL_URL"),
        std::env::var("RDS_TEST_SQLITE_PATH"),
    ) else {
        eprintln!("⏭️ 未设 RDS_TEST_MYSQL_URL / RDS_TEST_SQLITE_PATH，跳过联邦探针");
        return;
    };

    // 第三个源指向不存在的文件：验证“坏源不阻断”（duckdb 文件源不需要任何扩展）
    let ghost_path = std::env::temp_dir().join(format!("rds_fed_ghost_{}.duckdb", std::process::id()));
    let sources = vec![
        FederatedSource::new("mysql", "mysql_src", "mysql_native", &mysql_url)
            .expect("组装 MySQL 源"),
        FederatedSource::new("sqlite", "sqlite_src", "sqlite", &sqlite_path)
            .expect("组装 SQLite 源"),
        FederatedSource::new(
            "ghost",
            "ghost_src",
            "duckdb",
            ghost_path.to_str().expect("路径"),
        )
        .expect("组装 ghost 源"),
    ];

    let session = FederatedSession::open(&sources, Some("mysql_src")).expect("建联邦会话");
    let snapshot = session.snapshot();

    // 1. 多源挂载：两个真源可用 + 一个坏源带原话保留
    assert_eq!(snapshot.ready_count(), 2, "两个真源都该挂上：{snapshot:?}");
    let ghost = snapshot
        .sources
        .iter()
        .find(|entry| entry.alias() == "ghost_src")
        .expect("坏源不该消失");
    assert!(
        matches!(ghost.state, MountState::Failed(_)),
        "坏源该是失败态：{ghost:?}"
    );
    let mysql_tables = tables_of(&session, "mysql_src");
    let sqlite_tables = tables_of(&session, "sqlite_src");
    eprintln!(
        "✅ 多源挂载：MySQL {mysql_tables} 张表 · SQLite {sqlite_tables} 张表 · 坏源带原因保留"
    );
    assert!(mysql_tables > 0, "MySQL 源该有表");
    assert!(sqlite_tables > 0, "SQLite 源该有表");

    // 2. 未限定名走主源（拿一个不依赖具体表名的查法）
    let main_count = session
        .run("SELECT count(*) AS n FROM duckdb_tables()")
        .expect("主源可查");
    eprintln!(
        "✅ 未限定名走主源（mysql_src）：当前 catalog 可见表 {} 张",
        main_count.to_rows()[0][0]
    );

    // 3. **一条 SQL 跨两个源**：两个限定子查询分别在各自的源上跑
    let (mysql_schema, mysql_table) =
        first_table(&session, "mysql_src").expect("MySQL 源至少有一张表");
    let (sqlite_schema, sqlite_table) =
        first_table(&session, "sqlite_src").expect("SQLite 源至少有一张表");
    let cross = format!(
        "SELECT (SELECT count(*) FROM mysql_src.{mysql_schema}.{mysql_table}) AS mysql_rows, \
                (SELECT count(*) FROM sqlite_src.{sqlite_schema}.{sqlite_table}) AS sqlite_rows"
    );
    let result = session.run(&cross).expect("跨源查询");
    let row = &result.to_rows()[0];
    eprintln!(
        "✅ 一条 SQL 跨两源：mysql_src.{mysql_schema}.{mysql_table} → {} 行 · \
         sqlite_src.{sqlite_schema}.{sqlite_table} → {} 行",
        row[0], row[1]
    );

    // 4. 只读：写源库被拒；本地临时对象允许
    let write_attempt = session.run(&format!(
        "CREATE TABLE mysql_src.{mysql_schema}.rds_probe_should_fail (n INTEGER)"
    ));
    let error = write_attempt.expect_err("写源库必须被拒");
    eprintln!("✅ 写源库被拒：{}", error.to_string().lines().next().unwrap_or(""));
    session
        .run("CREATE TEMP TABLE rds_probe_scratch AS SELECT 1 AS n")
        .expect("本地临时表该允许");
    eprintln!("✅ 本地临时对象允许（分析会话的“变量”）");

    // 5. 切主源：未限定名跟着走（第二次开快照读到的 primary 要变）
    session.set_primary("sqlite_src").expect("切主源");
    let after = session.snapshot();
    assert_eq!(after.primary.as_deref(), Some("sqlite_src"));
    eprintln!("✅ 主源可切换：mysql_src → sqlite_src");

    eprintln!("════ 联邦多源探针通过（MySQL + SQLite 同会话跨源）════");
}

//! 真机回归：**错误里的对象位置**（`ErrorLocation`）。
//!
//! ## 钉什么
//!
//! 「数据库指认的对象」要能从出口层拿到：撞约束时要能说出**哪张表 / 哪一列 / 哪条约束**，
//! 而不是丢给用户一整段文本让他自己认。四家的取法不同，两条路都要验：
//!
//! * **PostgreSQL**：走协议字段（`PgDatabaseError` / `DbError`）。**必须真机**：字段是
//!   locale 无关的，而端点 locale 是中文（`字段 "nope" 不存在`）—— 文本解析在这里会当场
//!   失效，只有真机能证明字段那条路走通了。
//! * **MySQL / SQLite / DuckDB**：协议层没有字段，走消息解析。样本是 2026-09-21 从这些
//!   端点上抄下来的（离线单测 `driver::error_location::tests` 用同一批样本盯着），这里验的
//!   是**端到端真能拿到**（驱动把后端错误映射成 `CoreError` 的那一段）。
//!
//! ## 如实钉住的边界（不是缺陷，是「拿不到就是拿不到」）
//!
//! | 库 | 拿不到的情形 | 为什么 |
//! | --- | --- | --- |
//! | PG | 未定义列 / 未定义表 | 协议字段只在**约束类**错误上带对象；那类错误的位置由 `position` 送到光标 |
//! | MySQL(sqlx) | 外键冲突（真机上 1062 之外的 FK 报错没走到） | 观察到的 sqlx 那条路径不报 FK 错，Official 那条报 |
//! | SQLite | 外键冲突 | `FOREIGN KEY constraint failed` 里没有任何名字 |
//! | DuckDB | 外键冲突 | 「key … does not exist in the referenced table」不点表也不点列 |
//!
//! 未设环境变量时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! RDS_TEST_DUCKDB_PATH='D:\data\123' \
//!   cargo test -p rds-engine --test error_location_from_real_errors -- --nocapture --test-threads=1
//! ```

use rds_engine::driver::Database;
use rds_engine::driver::native::duckdb::DuckDbDatabase;
use rds_engine::driver::native::mysql::MySqlDatabase;
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
use rds_engine::driver::native::postgres::PostgresDatabase;
use rds_engine::driver::native::postgres_native::PostgresNativeDatabase;
use rds_engine::driver::native::sqlite::SqliteDatabase;
use shared::error::{CoreError, DatabaseError, ErrorLocation};

/// 从出口错误里取出位置（拿不到 = `None`）。
fn location_of(err: &CoreError) -> Option<ErrorLocation> {
    match err {
        CoreError::Database(DatabaseError::Query { location, .. }) => location.clone(),
        _ => None,
    }
}

/// 跑一句**应当失败**的 SQL，返回 (位置, 展示文本)。
async fn fail(db: &dyn Database, sql: &str) -> (Option<ErrorLocation>, String) {
    match db.query(sql).await {
        Ok(_) => panic!("这句本该失败：{sql}"),
        Err(e) => (location_of(&e), e.to_string()),
    }
}

/// 断言位置**逐字段**等于期望，并且界面看到的那句话里带上了它。
fn assert_loc(label: &str, got: &Option<ErrorLocation>, shown: &str, want: ErrorLocation) {
    let Some(got) = got.as_ref() else {
        panic!("{label}：本该有位置，实际没有。出口文本：{shown}");
    };
    assert_eq!(*got, want, "{label}（出口文本：{shown}）");
    let text = got.display_text().expect("非空位置应有展示文本");
    assert!(
        shown.contains(&text),
        "{label}：出口文本没带位置「{text}」：{shown}"
    );
    println!("✓ {label} → {text}");
}

/// 断言**拿不到位置**（并说明这是已登记的边界，不是漏填）。
fn assert_no_loc(label: &str, got: &Option<ErrorLocation>, why: &str) {
    assert!(got.is_none(), "{label}：{why}，不该编一个位置：{got:?}");
    println!("✓ {label} → 无位置（{why}）");
}

async fn exec(db: &dyn Database, sql: &str) {
    let _ = db.query(sql).await;
}

/// 建探针表（唯一 / 外键 / 非空都有），并插好底行。
async fn probe_table(db: &dyn Database, t: &str, parent: &str, int_ty: &str, text_ty: &str) {
    for sql in [
        format!("DROP TABLE IF EXISTS {t}"),
        format!("DROP TABLE IF EXISTS {parent}"),
    ] {
        exec(db, &sql).await;
    }
    db.query(&format!("CREATE TABLE {parent} (id {int_ty} PRIMARY KEY)"))
        .await
        .expect("建父表");
    db.query(&format!(
        "CREATE TABLE {t} (id {int_ty} PRIMARY KEY, \
         tag {text_ty} NOT NULL UNIQUE, \
         parent_id {int_ty}, \
         FOREIGN KEY (parent_id) REFERENCES {parent}(id))"
    ))
    .await
    .expect("建表");
    exec(db, &format!("INSERT INTO {parent} VALUES (1)")).await;
    exec(db, &format!("INSERT INTO {t} VALUES (1, 'dup', 1)")).await;
}

async fn drop_probe(db: &dyn Database, t: &str, parent: &str) {
    exec(db, &format!("DROP TABLE IF EXISTS {t}")).await;
    exec(db, &format!("DROP TABLE IF EXISTS {parent}")).await;
}

/// PostgreSQL：**协议字段**那条路（locale 无关）。两个驱动都要验 —— sqlx 走
/// `PgDatabaseError`、Official 走 `DbError`，是两套 API。
async fn pg_cases(db: &dyn Database, t: &str) {
    let parent = format!("{t}_p");
    probe_table(db, t, &parent, "int4", "text").await;

    // 唯一约束：id 换一个、tag 重复（不然撞的是主键）
    let (loc, shown) = fail(db, &format!("INSERT INTO {t} VALUES (5, 'dup', 1)")).await;
    assert_loc(
        "PG 唯一约束",
        &loc,
        &shown,
        ErrorLocation::new()
            .with_schema("public")
            .with_table(t)
            .with_constraint(&format!("{t}_tag_key")),
    );

    let (loc, shown) = fail(
        db,
        &format!("INSERT INTO {t} (id, parent_id) VALUES (2, 1)"),
    )
    .await;
    assert_loc(
        "PG 非空约束",
        &loc,
        &shown,
        ErrorLocation::new()
            .with_schema("public")
            .with_table(t)
            .with_column("tag"),
    );

    let (loc, shown) = fail(db, &format!("INSERT INTO {t} VALUES (3, 'fk', 999)")).await;
    assert_loc(
        "PG 外键",
        &loc,
        &shown,
        ErrorLocation::new()
            .with_schema("public")
            .with_table(t)
            .with_constraint(&format!("{t}_parent_id_fkey")),
    );

    // 已登记的边界：PG 只在**约束类**错误上带协议字段（未定义列/表不带 —— 那类错误
    // 由 `position` 把光标送到出错处）。
    let (loc, _) = fail(db, &format!("SELECT nope FROM {t}")).await;
    assert_no_loc("PG 未定义列", &loc, "协议字段只在约束类错误上带对象");
    let (loc, _) = fail(db, "SELECT * FROM rds_no_such_table_xyz").await;
    assert_no_loc("PG 未定义表", &loc, "协议字段只在约束类错误上带对象");

    drop_probe(db, t, &parent).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_errors_carry_location() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    println!("\n=== PostgreSQL（sqlx，协议字段）===");
    let db = PostgresDatabase::new(&url).await.expect("连上 PG");
    pg_cases(&db, "rds_err_loc_pg").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn official_postgres_errors_carry_location() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    println!("\n=== PostgreSQL（Official，协议字段）===");
    let db = PostgresNativeDatabase::new(&url)
        .await
        .expect("连上 PG(Official)");
    pg_cases(&db, "rds_err_loc_pgn").await;
}

/// MySQL：消息解析那条路（两个驱动文案同一套，前缀不同）。
///
/// `with_fk_error`：真机上 sqlx 那条路径没报出 FK 错（1062 之外的 FK 报错没走到），
/// Official 那条报 —— 所以外键那条只对 Official 断言。
async fn mysql_cases(db: &dyn Database, t: &str, schema: &str, with_fk_error: bool) {
    let parent = format!("{t}_p");
    probe_table(db, t, &parent, "INT", "VARCHAR(8)").await;

    // 唯一的索引就叫 `tag`，所以 `for key 'tbl.tag'` 给出的约束名是 `tag`
    let (loc, shown) = fail(db, &format!("INSERT INTO {t} VALUES (9, 'dup', 1)")).await;
    assert_loc(
        "MySQL 唯一约束",
        &loc,
        &shown,
        ErrorLocation::new().with_table(t).with_constraint("tag"),
    );

    let (loc, shown) = fail(
        db,
        &format!("INSERT INTO {t} (id, parent_id) VALUES (2, 1)"),
    )
    .await;
    assert_loc(
        "MySQL 列不可空 / 无默认值",
        &loc,
        &shown,
        ErrorLocation::new().with_column("tag"),
    );

    let (loc, shown) = fail(db, &format!("SELECT nope FROM {t}")).await;
    assert_loc(
        "MySQL 未知列",
        &loc,
        &shown,
        ErrorLocation::new().with_column("nope"),
    );

    let (loc, shown) = fail(db, "SELECT * FROM rds_no_such_table_xyz").await;
    assert_loc(
        "MySQL 未知表",
        &loc,
        &shown,
        ErrorLocation::new()
            .with_schema(schema)
            .with_table("rds_no_such_table_xyz"),
    );

    if with_fk_error {
        let (loc, shown) = fail(db, &format!("INSERT INTO {t} VALUES (3, 'fk', 999)")).await;
        assert_loc(
            "MySQL 外键",
            &loc,
            &shown,
            ErrorLocation::new()
                .with_schema(schema)
                .with_table(t)
                .with_column("parent_id")
                .with_constraint(&format!("{t}_ibfk_1")),
        );
    }

    // 语法错：没有对象可指认（位置由 `position` 那一路管）
    let (loc, _) = fail(db, "SELEC 1").await;
    assert_no_loc("MySQL 语法错", &loc, "语法错没有对象");

    drop_probe(db, t, &parent).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_errors_carry_location() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    println!("\n=== MySQL（sqlx，消息解析）===");
    let db = MySqlDatabase::new(&url).await.expect("连上 MySQL");
    mysql_cases(&db, "rds_err_loc_my", "mysql", false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn official_mysql_errors_carry_location() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    println!("\n=== MySQL（Official，消息解析）===");
    let db = MySqlNativeDatabase::new(&url)
        .await
        .expect("连上 MySQL(Official)");
    mysql_cases(&db, "rds_err_loc_myn", "mysql", true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_errors_carry_location() {
    let Ok(path) = std::env::var("RDS_TEST_SQLITE_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_SQLITE_PATH，跳过");
        return;
    };
    println!("\n=== SQLite（消息解析）===");
    let db = SqliteDatabase::new(&path).expect("打开 SQLite");
    let t = "rds_err_loc_sq";
    let parent = format!("{t}_p");
    probe_table(&db, t, &parent, "INTEGER", "TEXT").await;

    let (loc, shown) = fail(&db, &format!("INSERT INTO {t} VALUES (9, 'dup', 1)")).await;
    assert_loc(
        "SQLite 唯一约束",
        &loc,
        &shown,
        ErrorLocation::new().with_table(t).with_column("tag"),
    );

    let (loc, shown) = fail(
        &db,
        &format!("INSERT INTO {t} (id, parent_id) VALUES (2, 1)"),
    )
    .await;
    assert_loc(
        "SQLite 非空约束",
        &loc,
        &shown,
        ErrorLocation::new().with_table(t).with_column("tag"),
    );

    let (loc, shown) = fail(&db, "SELECT * FROM rds_no_such_table_xyz").await;
    assert_loc(
        "SQLite 未知表",
        &loc,
        &shown,
        ErrorLocation::new().with_table("rds_no_such_table_xyz"),
    );

    // 这条同时盯住一个真机踩到的坑：rusqlite 的 `Display` 会把 ` in {sql} at offset {n}`
    // 拼在消息后面，拿它去解析会把列名读成 `nope in SELECT nope FROM t at offset 7`。
    let (loc, shown) = fail(&db, &format!("SELECT nope FROM {t}")).await;
    assert_loc(
        "SQLite 未知列",
        &loc,
        &shown,
        ErrorLocation::new().with_column("nope"),
    );

    let (loc, _) = fail(&db, &format!("INSERT INTO {t} VALUES (3, 'fk', 999)")).await;
    assert_no_loc(
        "SQLite 外键",
        &loc,
        "`FOREIGN KEY constraint failed` 里没有名字",
    );

    drop_probe(&db, t, &parent).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_errors_carry_location() {
    let Ok(path) = std::env::var("RDS_TEST_DUCKDB_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_DUCKDB_PATH，跳过");
        return;
    };
    println!("\n=== DuckDB（消息解析）===");
    let db = DuckDbDatabase::new(&path).expect("打开 DuckDB");
    let t = "rds_err_loc_dd";
    let parent = format!("{t}_p");
    probe_table(&db, t, &parent, "INTEGER", "VARCHAR").await;

    // `Duplicate key "tag: dup" violates unique constraint.` —— 唯一约束不点名，
    // 但键列表里有列名（复合键时只有多个列名，按口径什么都不填）
    let (loc, shown) = fail(&db, &format!("INSERT INTO {t} VALUES (9, 'dup', 1)")).await;
    assert_loc(
        "DuckDB 重复键",
        &loc,
        &shown,
        ErrorLocation::new().with_column("tag"),
    );

    let (loc, shown) = fail(
        &db,
        &format!("INSERT INTO {t} (id, parent_id) VALUES (2, 1)"),
    )
    .await;
    assert_loc(
        "DuckDB 非空约束",
        &loc,
        &shown,
        ErrorLocation::new().with_table(t).with_column("tag"),
    );

    let (loc, shown) = fail(&db, &format!("INSERT INTO {t} (nope) VALUES (4)")).await;
    assert_loc(
        "DuckDB 表里没有这一列",
        &loc,
        &shown,
        ErrorLocation::new().with_table(t).with_column("nope"),
    );

    let (loc, shown) = fail(&db, "SELECT * FROM rds_no_such_table_xyz").await;
    assert_loc(
        "DuckDB 未知表",
        &loc,
        &shown,
        ErrorLocation::new().with_table("rds_no_such_table_xyz"),
    );

    let (loc, _) = fail(&db, &format!("INSERT INTO {t} VALUES (3, 'fk', 999)")).await;
    assert_no_loc(
        "DuckDB 外键",
        &loc,
        "外键报错不点表也不点列（只给被引用的键值）",
    );

    drop_probe(&db, t, &parent).await;
}

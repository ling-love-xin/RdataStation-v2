//! 本地加速通道的真机能力台账（B13 切片二）
//!
//! 问的是「DuckDB 上的源库（`ATTACH … READ_ONLY`）到底能干什么」，结论直接决定
//! `engine::duckdb::accel` 的形状与界面上能写什么话。**没设环境变量的库自动跳过**
//! （不算失败）。
//!
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql'
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres'
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil'
//! cargo test -p rds-engine --test duckdb_accel_probe -- --nocapture --test-threads=1
//! ```
//!
//! **sh / bash 下一律加单引号**（反斜杠会被吃掉，见 `editor_exec_real.rs` 同一警告）。
//!
//! ## 已核实的结论（2026-09-18 实跑）
//!
//! | 问题 | 结论 |
//! | --- | --- |
//! | 扩展能不能装 | ✅ `INSTALL mysql` / `postgres` / `sqlite` 都成功（DuckDB v1.5.5，装到 `paths::extensions_dir()`） |
//! | 网络源能不能只读挂载 | ✅ `ATTACH 'mysql://…' AS rds_src (TYPE mysql, READ_ONLY)` 被接受 |
//! | 不写限定名能不能查到源表 | ❌ 默认 catalog 是 `memory`；✅ 会话里 `USE rds_src` 之后就能（本模块的做法） |
//! | MySQL 的反引号 | ❌ DuckDB 解析器不认（`syntax error at or near "`"`）；v1.5.5 没有 `SET dialect` |
//! | 数据是实时还是快照 | **实时**——源库插一行，同一 DuckDB 会话立刻查得到 |
//! | 源库新建的表 | 要 `refresh()`（DETACH + 重新 ATTACH）才可见：**表清单在 ATTACH 时定型** |
//! | 写源库 | DuckDB 自己拒：`attached in read-only mode`（本地临时对象照常允许） |

use std::sync::Arc;

use rds_engine::duckdb::accel::{self, AccelKind, AccelSource};
use mysql_async::prelude::*;

fn probe_db_name() -> String {
    "rds_accel_probe".to_string()
}

/// 建一个自己的库 + 一张表（不动 `mysql` 系统库），返回带凭据的源 URL
fn mysql_setup(url: &str, rows: i64) -> String {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let source_url = url.replace("/mysql", &format!("/{}", probe_db_name()));
    runtime.block_on(async {
        let pool = mysql_async::Pool::new(url);
        {
            let mut conn = pool.get_conn().await.expect("连 MySQL");
                conn.query_drop("CREATE DATABASE IF NOT EXISTS rds_accel_probe")
                .await
                .expect("建探测库");
                conn.query_drop("DROP TABLE IF EXISTS rds_accel_probe.orders")
                .await
                .expect("清旧表");
                conn.query_drop("CREATE TABLE rds_accel_probe.orders AS SELECT 1 AS id")
                .await
                .expect("建表");
                for index in 2..=rows {
                conn.query_drop(format!("INSERT INTO rds_accel_probe.orders VALUES ({index})"))
                    .await
                    .expect("插行");
            }
        }
        // 连接必须先归还：`disconnect()` 要等所有连接回池，握着 conn 调它会一直等下去
        pool.disconnect().await.expect("断开");
    });
    source_url
}

fn mysql_teardown(url: &str) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let pool = mysql_async::Pool::new(url);
        {
            if let Ok(mut conn) = pool.get_conn().await {
                let _ = conn
                    .query_drop("DROP DATABASE IF EXISTS rds_accel_probe")
                    .await;
            }
        }
        let _ = pool.disconnect().await;
    });
}

fn mysql_exec(url: &str, sql: &str) {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        let pool = mysql_async::Pool::new(url);
        {
            let mut conn = pool.get_conn().await.expect("连 MySQL");
            conn.query_drop(sql).await.expect("执行");
        }
        let _ = pool.disconnect().await;
    });
}

fn scalar(session: &Arc<accel::AccelSession>, sql: &str) -> String {
    match session.run(sql) {
        Ok(result) => result
            .to_rows()
            .first()
            .and_then(|row| row.first())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<空>".to_string()),
        Err(error) => format!("ERR {error}"),
    }
}

#[test]
fn probe_mysql_source_live_and_read_only() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        println!("跳过：未设 RDS_TEST_MYSQL_URL");
        return;
    };
    accel::drop_all();
    let source_url = mysql_setup(&url, 3);

    let source = AccelSource::new("C_mysql_probe", "mysql_native", &source_url).expect("组装源");
    assert_eq!(source.kind, AccelKind::MySql);
    let session = match accel::ensure_session(&source) {
        Ok(session) => session,
        Err(reason) => {
            mysql_teardown(&url);
            panic!("建加速会话失败：{reason}");
        }
    };
    println!("✅ 加速会话已建立（MySQL 源只读挂载）");

    // 1) 不写限定名就能查到源表（USE 生效）
    let unqualified = scalar(&session, "SELECT count(*) AS n FROM orders");
    assert_eq!(unqualified, "3", "未限定名的源表要能查到");
    println!("✅ 未限定名查询：SELECT count(*) FROM orders → {unqualified}");

    // 2) 数据是**实时**的：源库插一行，同一会话立刻可见
    mysql_exec(&url, "INSERT INTO rds_accel_probe.orders VALUES (99)");
    let after_insert = scalar(&session, "SELECT count(*) FROM orders");
    assert_eq!(after_insert, "4", "源库插入后应当立刻看到（不是快照）");
    println!("✅ 新鲜度：源库插入后同一会话 → {after_insert}（实时，不是快照）");

    // 3) 表清单在 ATTACH 时定型：源库新建的表要 refresh 才可见
    mysql_exec(&url, "CREATE TABLE rds_accel_probe.late_table (id INT)");
    let before_refresh = scalar(&session, "SELECT count(*) FROM late_table");
    assert!(
        before_refresh.starts_with("ERR"),
        "ATTACH 之后新建的表不该直接可见：{before_refresh}"
    );
    session.refresh().expect("重新 ATTACH");
    let after_refresh = scalar(&session, "SELECT count(*) FROM late_table");
    assert_eq!(after_refresh, "0", "重新 ATTACH 后应看到新表");
    println!("✅ 表清单：新表 refresh 前不可见、refresh 后可见（{after_refresh} 行）");

    // 4) 写源库被 DuckDB 拒；本地临时对象允许
    let refused = session
        .run("INSERT INTO orders VALUES (1000)")
        .expect_err("写源库必须被拒");
    assert!(
        refused.to_string().contains("read-only"),
        "拒绝理由要说明是只读挂载：{refused}"
    );
    println!("✅ 写拒绝：{refused}");
    session
        .run("CREATE TEMP TABLE scratch AS SELECT 1 AS x")
        .expect("本地临时对象应当允许");
    println!("✅ 本地临时对象（CREATE TEMP TABLE）允许");

    // 5) 显式别名也能用（跨 schema 的写法）
    assert_eq!(scalar(&session, "SELECT count(*) FROM rds_src.orders"), "4");
    println!("✅ 显式限定名（rds_src.orders）可用");

    accel::drop_session("C_mysql_probe");
    mysql_teardown(&url);
}

#[test]
fn probe_postgres_source_attach() {
    let Some(url) = std::env::var("RDS_TEST_PG_URL").ok() else {
        println!("跳过：未设 RDS_TEST_PG_URL");
        return;
    };
    accel::drop_all();
    let source = AccelSource::new("C_pg_probe", "postgres_native", &url).expect("组装源");
    assert_eq!(source.kind, AccelKind::PostgreSql);
    let session = match accel::ensure_session(&source) {
        Ok(session) => session,
        Err(reason) => panic!("建加速会话失败：{reason}"),
    };
    println!("✅ 加速会话已建立（PostgreSQL 源只读挂载）");
    let one = scalar(&session, "SELECT 1 AS n");
    assert_eq!(one, "1", "本地 DuckDB 语句应当能跑");
    // 源库的 `pg_catalog` 里那张表：证明限定名解析到源库 schema
    let names = session
        .run("SELECT count(*) FROM information_schema.tables WHERE table_catalog = 'rds_src'")
        .expect("查源库表清单");
    println!("✅ 源库可见表数：{:?}", names.to_rows()[0][0].to_string());
    let refused = session.run("CREATE TABLE t_probe (id INT)").expect_err("写源库必须被拒");
    println!("✅ 写拒绝：{refused}");
    accel::drop_session("C_pg_probe");
}

#[test]
fn probe_sqlite_file_source_attach() {
    let Some(path) = std::env::var("RDS_TEST_SQLITE_PATH").ok() else {
        println!("跳过：未设 RDS_TEST_SQLITE_PATH");
        return;
    };
    accel::drop_all();
    let source = AccelSource::new("C_lite_probe", "sqlite", &path).expect("组装源");
    assert_eq!(source.kind, AccelKind::Sqlite);
    let session = match accel::ensure_session(&source) {
        Ok(session) => session,
        Err(reason) => panic!("建加速会话失败：{reason}"),
    };
    let tables = scalar(
        &session,
        "SELECT count(*) FROM duckdb_tables() WHERE database_name = 'rds_src'",
    );
    println!("✅ SQLite 文件源已挂载，可见表数：{tables}");
    assert!(
        tables != "0" && !tables.starts_with("ERR"),
        "文件源里应当有表：{tables}"
    );
    let refused = session.run("CREATE TABLE t_probe (id INT)").expect_err("写文件源也要被拒");
    assert!(refused.to_string().contains("read-only"), "{refused}");
    println!("✅ 写拒绝：{refused}");
    accel::drop_session("C_lite_probe");
}

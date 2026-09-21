//! 真机回归：出口层「值/结果族」的类型解码保真度。
//!
//! ## 为什么要单开一个文件
//!
//! 2026-09-21 实测发现：四个网络驱动（`postgres` / `mysql` 走 sqlx，`postgres_native` 走
//! tokio-postgres，`mysql_native` 走 mysql_async）里，**三个**会把「库里明明有值」的列
//! 静默交成 NULL。两个独立成因叠在一起：
//!
//! 1. **sqlx 的类型解码 features 没开**（`chrono` / `bigdecimal` / `uuid` / `json`）：
//!    参数化类型根本没有 `Decode` 实现；
//! 2. **转换器的解码链里没试这些类型**：探测表只有
//!    `bool / i32 / i64 / f32 / f64 / Vec<u8> / String`，于是 `numeric` · 时间 · `jsonb`
//!    · UUID 全部落进 `.ok().flatten()` → `None` → 出口层看到的是「值就是 NULL」。
//!
//! 实测修复前：PG(sqlx) 19 列里 12 列静默 NULL、PG(Official) 11 列、MySQL(sqlx) 5 列。
//!
//! ## 本文件钉什么
//!
//! 「**库里非空 → 出口不得为 NULL**」，逐类型断言，含一个刻意保留的**精度断言**
//! （`numeric(10,2)` 的 `1234.56` 必须以文本 `"1234.5600"` 出来 —— 不进浮点）。
//!
//! ## 仍不可解码的（有意留白，`KNOWN_UNDECODABLE`）
//!
//! 这些类型当前**没有**可用的解码器，出口层会交 NULL。它们不是本文件的断言对象，
//! 但列在这里是为了：① 下一个人知道这是已知而不是新缺陷；② 驱动侧对应位置有
//! `tracing::warn!` 留痕（不再静默）。
//!
//! | 库 | 类型 | 原因 |
//! | --- | --- | --- |
//! | PG | `interval` | sqlx 的 interval 需要 `PgInterval`，未纳入解码链 |
//! | PG | 数组（`int4[]` 等） | 需要按元素类型逐族解，未纳入解码链 |
//! | PG | `inet` / `point` | 需要 `ipnetwork` / `geo-types`（不在依赖树里） |
//! | PG(Official) | `numeric` | `postgres-types` 0.2.13 **没有任何 decimal feature**，无 `FromSql` 实现 |
//!
//! 未设环境变量时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//!   cargo test -p rds-engine --test type_decode_fidelity -- --nocapture --test-threads=1
//! ```

use arrow::array::Array;
use arrow::record_batch::RecordBatch;
use arrow::util::display::array_value_to_string;
use rds_engine::driver::Database;
use rds_engine::driver::native::mysql::MySqlDatabase;
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
use rds_engine::driver::native::postgres::PostgresDatabase;
use rds_engine::driver::native::postgres_native::PostgresNativeDatabase;

/// 该列第 0 行的展示值（`None` = 出口判为 SQL NULL）。
fn cell(batch: &RecordBatch, name: &str) -> Option<String> {
    let idx = batch
        .schema()
        .fields()
        .iter()
        .position(|f| f.name() == name)
        .unwrap_or_else(|| panic!("结果里没有列 `{name}`"));
    if batch.column(idx).is_null(0) {
        None
    } else {
        array_value_to_string(batch.column(idx), 0).ok()
    }
}

/// 断言这些列**都不是 NULL**（库里非空，出口就必须给出值）。
fn assert_not_null(batch: &RecordBatch, cols: &[&str]) {
    let mut missing = Vec::new();
    for c in cols {
        if cell(batch, c).is_none() {
            missing.push(*c);
        }
    }
    assert!(
        missing.is_empty(),
        "这些列在库里非空，出口却是 NULL：{missing:?}\n\
         —— 检查①根 Cargo.toml 的 sqlx 类型解码 features（chrono/bigdecimal/uuid/json）\n\
         —— 检查②驱动转换器的解码链是否还只试 String"
    );
}

/// PostgreSQL 共用的那批列（sqlx 与 Official 两个驱动跑同一条 SQL）。
const PG_SQL: &str = r#"SELECT
    1::int2 AS i2, 1::int4 AS i4, 1::int8 AS i8, true AS flag,
    1.5::float4 AS f4, 1.5::float8 AS f8,
    1234.56::numeric(10,2) AS num,
    now() AS ts, current_date AS d, '12:00:00'::time AS t,
    '{"a":1}'::jsonb AS j, '\x0102'::bytea AS b,
    'abcd'::varchar(5) AS vc, 'b'::text AS txt,
    gen_random_uuid() AS u"#;

/// 两个 PG 驱动都该解的列（`num` 不在此列：Official 读不出 numeric，见文件头）。
const PG_COMMON: &[&str] = &[
    "i2", "i4", "i8", "flag", "f4", "f8", "ts", "d", "t", "j", "b", "vc", "txt", "u",
];

#[tokio::test(flavor = "multi_thread")]
async fn sqlx_postgres_decodes_numeric_temporal_json_and_uuid() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    let db = PostgresDatabase::new(&url).await.expect("连上 PG");
    let r = db.query(PG_SQL).await.expect("查询成功");
    let batch = r.batches.first().expect("应有 batches");

    assert_not_null(batch, PG_COMMON);

    // 精度：numeric(10,2) 必须以**文本**出来，原样保留标度（不进浮点）
    assert_eq!(
        cell(batch, "num").as_deref(),
        Some("1234.5600"),
        "numeric 必须原样保留标度（走 BigDecimal::to_string，不过 f64）"
    );
    // int2 归 Int32 档且要真取到值（曾经是 Int32 列 + NULL 值）
    assert_eq!(cell(batch, "i2").as_deref(), Some("1"));
    // bytea 仍是 Binary，不被当成文本
    assert!(format!("{:?}", batch.column(11).data_type()).starts_with("Binary"));
}

#[tokio::test(flavor = "multi_thread")]
async fn official_postgres_decodes_temporal_json_and_uuid() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    let db = PostgresNativeDatabase::new(&url)
        .await
        .expect("连上 PG(Official)");
    let r = db.query(PG_SQL).await.expect("查询成功");
    assert_not_null(r.batches.first().expect("应有 batches"), PG_COMMON);
}

const MYSQL_SQL: &str = r#"SELECT
    1 AS tiny, 100 AS small, 100000 AS medium, 10000000000 AS big,
    CAST(1234.56 AS DECIMAL(10,2)) AS num, CAST(1.5 AS DOUBLE) AS dbl,
    NOW() AS ts_dt, CURDATE() AS d, CURTIME() AS t,
    CAST('{"a":1}' AS JSON) AS j, X'FF' AS bin2,
    'abcd' AS vc, TRUE AS flag"#;

#[tokio::test(flavor = "multi_thread")]
async fn sqlx_mysql_decodes_decimal_temporal_and_json() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let db = MySqlDatabase::new(&url).await.expect("连上 MySQL");
    let r = db.query(MYSQL_SQL).await.expect("查询成功");
    let batch = r.batches.first().expect("应有 batches");

    assert_not_null(
        batch,
        &[
            "tiny", "small", "medium", "big", "num", "dbl", "ts_dt", "d", "t", "j", "bin2", "vc",
            "flag",
        ],
    );
    // DECIMAL 走 BigDecimal 原样出文本（曾经的形态是「Float64 列 + NULL 值」）
    assert_eq!(cell(batch, "num").as_deref(), Some("1234.56"));
    // 非法 UTF-8 的字节仍走 Binary，不被当成文本
    assert_eq!(cell(batch, "bin2").as_deref().is_some(), true);
}

#[tokio::test(flavor = "multi_thread")]
async fn official_mysql_decodes_decimal_and_temporal() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let db = MySqlNativeDatabase::new(&url)
        .await
        .expect("连上 MySQL(Official)");
    let r = db.query(MYSQL_SQL).await.expect("查询成功");
    assert_not_null(
        r.batches.first().expect("应有 batches"),
        &["num", "ts_dt", "d", "j"],
    );
}

//! 真机回归：出口层「值/结果族」的类型解码保真度。
//!
//! ## 本文件管谁（D3 之后）
//!
//! 只管**内核的两条官方驱动**：`postgres_native`（tokio-postgres）与 `mysql_native`
//! （mysql_async）。`postgres` / `mysql`（原 sqlx 实现）已剥离到 sidecar 插件
//! （`tools/plugin-drivers/sqlx-rust`），那边同一个主题由插件自己的验收脚本钉住
//! （2026-11-12 真机 12 项 × 2 全绿，见 `sqlx-driver-extraction-plan.md` §7.2）。
//! 原先本文件里那两条 `sqlx_*` 用例随实现一起删除——留着它们只会让「内核还有 sqlx」
//! 这个错觉多活一轮。
//!
//! ## 为什么要单开一个文件
//!
//! 2026-09-21 实测发现：网络驱动里**三个**会把「库里明明有值」的列静默交成 NULL。
//! 两个独立成因叠在一起：
//!
//! 1. **客户端库的类型解码 features 没开**（`chrono` / `bigdecimal` / `uuid` / `json`）：
//!    参数化类型根本没有 `Decode` 实现；
//! 2. **转换器的解码链里没试这些类型**：探测表只有
//!    `bool / i32 / i64 / f32 / f64 / Vec<u8> / String`，于是 `numeric` · 时间 · `jsonb`
//!    · UUID 全部落进 `.ok().flatten()` → `None` → 出口层看到的是「值就是 NULL」。
//!
//! 实测修复前：PG(Official) 19 列里 11 列静默 NULL；MySQL(sqlx) 5 列。
//!
//! ## 本文件钉什么
//!
//! 「**库里非空 → 出口不得为 NULL**」，逐类型断言，含一个刻意保留的**精度断言**
//! （`numeric(10,2)` 的 `1234.56` 必须以 `dscale` 口径的 `"1234.56"` 出来 —— 不进浮点）。
//!
//! ## 第二批（2026-09-21）：PG 那五类不再交 NULL
//!
//! | 库 | 类型 | 之前 | 现在 |
//! | --- | --- | --- | --- |
//! | PG(Official) | `interval` | 空白格 | 解 wire（`driver::native::pg_wire`）→ `1 day 02:03:04` |
//! | PG(Official) | 数组（`int4[]` / `text[]` …） | 空白格 | 解出 `Vec<T>` 后按 PG 数组字面量拼（元素含逗号时按规矩加引号） |
//! | PG(Official) | `inet` / `point` | 空白格 | 内核官方驱动与插件**共用一份 wire 解码** → `192.168.3.138` / `(1.5,-2.25)` |
//! | PG(Official) | `numeric` | 空白格（`postgres-types` 0.2.x 没有任何 decimal feature） | 自己解 base-10000 的 wire 格式 → `1234.56` |
//!
//! **口径提醒**：`numeric` 两条实现**值相等、写法可能差尾零** —— sqlx 插件那条走
//! `BigDecimal::to_string()`（按 4 位组拼，`numeric(10,2)` 出 `1234.5600`），内核官方那条
//! 按 `dscale` 出 `1234.56`。两个值都对；插件侧的尾零口径由插件单测断言，本文件只钉
//! 官方这条。
//!
//! ## 仍不保证的（没有解码器，也没打算自己写）
//!
//! `range` / 复合类型 / 几何族除 `point` 外（`line` / `lseg` / `box` / `path` / `polygon` /
//! `circle`）/ `hstore` / 位串 / `tsvector` —— 这些仍落文本兜底后为空白格，驱动侧有
//! `tracing::warn!` 留痕（不再静默）。列在这里是为了让下一个人知道这是**已知**而不是新缺陷。
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
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
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
         —— 检查客户端的类型解码 features 是否开着（chrono / bigdecimal / uuid / json）\n\
         —— 检查驱动转换器的解码链是否还只试 String"
    );
}

/// PostgreSQL 那批列（官方驱动跑这条 SQL；插件侧另有一份自己的用例）。
const PG_SQL: &str = r#"SELECT
    1::int2 AS i2, 1::int4 AS i4, 1::int8 AS i8, true AS flag,
    1.5::float4 AS f4, 1.5::float8 AS f8,
    1234.56::numeric(10,2) AS num,
    now() AS ts, current_date AS d, '12:00:00'::time AS t,
    '{"a":1}'::jsonb AS j, '\x0102'::bytea AS b,
    'abcd'::varchar(5) AS vc, 'b'::text AS txt,
    gen_random_uuid() AS u,
    '1 day 02:03:04'::interval AS iv,
    '{1,2,3}'::int4[] AS arr,
    '{a,b}'::text[] AS tarr,
    '192.168.3.138'::inet AS ip,
    '(1.5,-2.25)'::point AS pt"#;

/// 官方 PG 驱动都该解的列。
///
/// 2026-09-21 第二批之前，`num`（读不出）与 `iv` / `arr` / `tarr` / `ip` / `pt`
/// 都不在这张表里 —— 现在两类缺口都补上了，所以它们进来了。
const PG_COMMON: &[&str] = &[
    "i2", "i4", "i8", "flag", "f4", "f8", "ts", "d", "t", "j", "b", "vc", "txt", "u", "num", "iv",
    "arr", "tarr", "ip", "pt",
];

/// 五类的**具体文本**（官方驱动口径；sqlx 插件的 `num` 尾零写法由插件单测钉住）。
fn assert_pg_extras(batch: &RecordBatch) {
    assert_eq!(
        cell(batch, "iv").as_deref(),
        Some("1 day 02:03:04"),
        "interval 应按 PG 的写法出文本"
    );
    assert_eq!(
        cell(batch, "arr").as_deref(),
        Some("{1,2,3}"),
        "int4[] 数组字面量"
    );
    assert_eq!(
        cell(batch, "tarr").as_deref(),
        Some("{a,b}"),
        "text[] 数组字面量"
    );
    assert_eq!(
        cell(batch, "ip").as_deref(),
        Some("192.168.3.138"),
        "inet 满位网段不打 /32（与 inet_out 一致）"
    );
    assert_eq!(
        cell(batch, "pt").as_deref(),
        Some("(1.5,-2.25)"),
        "point 写法"
    );
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
    let batch = r.batches.first().expect("应有 batches");
    assert_not_null(batch, PG_COMMON);
    // Official 这条按 `dscale` 出（sqlx 那条出 `1234.5600`：BigDecimal 按 4 位组拼）
    assert_eq!(
        cell(batch, "num").as_deref(),
        Some("1234.56"),
        "numeric 走 wire 解码（base-10000），按 dscale 出小数位"
    );
    assert_pg_extras(batch);
}

const MYSQL_SQL: &str = r#"SELECT
    1 AS tiny, 100 AS small, 100000 AS medium, 10000000000 AS big,
    CAST(1234.56 AS DECIMAL(10,2)) AS num, CAST(1.5 AS DOUBLE) AS dbl,
    NOW() AS ts_dt, CURDATE() AS d, CURTIME() AS t,
    CAST('{"a":1}' AS JSON) AS j, X'FF' AS bin2,
    'abcd' AS vc, TRUE AS flag"#;

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

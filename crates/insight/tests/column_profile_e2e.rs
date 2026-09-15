//! 列画像**端到端**：真实 DuckDB 临时表 → 规则驱动统计 → 面板视图模型。
//!
//! 为什么放在 `tests/`（集成测试）而不是 lib 单测：
//! - 它要在**进程级 DuckDB 单例**（内存连接，`get_or_create_duckdb`）上建临时表。
//!   集成测试是独立进程，不会与 lib 里那批单测共享同一份连接与规则缓存。
//! - 其余洞察测试都是纯函数（视图模型 / 面板状态机），这里是唯一验证
//!   「取数 → 映射」全链路的地方。
//!
//! 不连任何外部数据库：DuckDB 是本地内存引擎，统计走内置的 18 条规则。
//!
//! **不覆盖**：时间列的统计（`compute_datetime_stats`）——它的规则 SQL 对数据形态更敏感，
//! 留到 Phase 3 的表探查批次一起补，避免在这里用构造数据迁就实现。

use rds_insight::insight_engine::get_or_create_duckdb;
use rds_insight::{ColumnKind, InsightService};

/// 建临时表并插数据。
///
/// 表名每个用例唯一：DuckDB 连接是进程级单例，同一测试进程内的用例共享它。
fn seed(table: &str, ddl: &str, values: &str) {
    let duckdb = get_or_create_duckdb().expect("应能拿到内存 DuckDB 连接");
    let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE \"{table}\" ({ddl})"
    ))
    .expect("建临时表");
    conn.execute_batch(&format!("INSERT INTO \"{table}\" {values}"))
        .expect("插数据");
}

/// 数值列：统计量来自 `numeric-stats` 规则，直方图受样本行数下限约束
#[test]
fn numeric_column_reaches_the_view_model() {
    let table = "t_insight_e2e_numeric";
    // 12 个非空值（达到 HISTOGRAM_MIN_ROWS = 10）+ 2 个空值（空值率 14% > 10% 阈值）
    let mut rows: Vec<String> = (1..=12).map(|i| format!("({i})")).collect();
    rows.push("(NULL)".into());
    rows.push("(NULL)".into());
    seed(table, "amount DECIMAL(12,2)", &format!("VALUES {}", rows.join(",")));

    let view = InsightService::profile_column_view(None, table, "amount").expect("列画像应成功");

    assert_eq!(view.kind, ColumnKind::Numeric, "DECIMAL 应分派到数值族");
    assert_eq!(view.total_count, 14);
    assert_eq!(view.null_count, 2);
    assert!((view.null_rate - 2.0 / 14.0).abs() < 1e-9);

    // 空值率超阈值：既有数值行的 warning 强调，也有质量提示
    assert!(
        view.notes.iter().any(|n| n.text.contains("高空值率")),
        "14% 空值率应触发高空值率提示，实际：{:?}",
        view.notes
    );

    // 基础统计确实来自规则执行（1..=12 的平均数是 6.5）
    let avg = view
        .basics
        .iter()
        .find(|r| r.label == "平均")
        .expect("数值列应有「平均」行");
    assert_eq!(avg.value, "6.5");

    // 非空值达到下限 → 直方图应产出；样本条数不超过 DEFAULT_SAMPLE_SIZE
    assert!(
        !view.distribution.is_empty(),
        "12 个非空值已达直方图下限，应产出分布"
    );
    assert!(
        !view.sample.is_empty() && view.sample.len() <= 5,
        "样本应为 1–5 条，实际 {}",
        view.sample.len()
    );
}

/// 文本 / 布尔列按**真实 DuckDB 类型**分派（不是按目标里用户声明的类型字符串）
#[test]
fn text_and_boolean_columns_dispatch_by_real_types() {
    let table = "t_insight_e2e_dispatch";
    seed(
        table,
        "name VARCHAR, flag BOOLEAN",
        "VALUES ('a', true), ('b', false), ('a', true)",
    );

    let text = InsightService::profile_column_view(None, table, "name").expect("文本列画像");
    assert_eq!(text.kind, ColumnKind::Text);
    assert!(
        text.basics.iter().any(|r| r.label == "长度范围"),
        "文本列应有长度范围"
    );
    assert_eq!(text.distribution.len(), 2, "Top 值应有两个取值");

    let boolean = InsightService::profile_column_view(None, table, "flag").expect("布尔列画像");
    assert_eq!(boolean.kind, ColumnKind::Boolean);
    assert_eq!(boolean.distribution.len(), 1, "布尔列只有一条 True 占比条");
    assert!(
        boolean.notes.iter().all(|n| !n.text.contains("不平衡")),
        "2:1 不算不平衡"
    );
}

/// 二进制列与全空列都归「类型未识别」——两条不同的路径
#[test]
fn binary_and_all_null_columns_stay_unknown() {
    let table = "t_insight_e2e_unknown";
    seed(
        table,
        "payload BLOB, empty_num INTEGER",
        "VALUES ('abc'::BLOB, NULL), ('def'::BLOB, NULL)",
    );

    let binary = InsightService::profile_column_view(None, table, "payload").expect("BLOB 画像");
    assert_eq!(binary.kind, ColumnKind::Unknown, "二进制列不做类型专属统计");
    assert_eq!(binary.basics.len(), 4, "未识别类型只给四行基础计数");

    let empty = InsightService::profile_column_view(None, table, "empty_num").expect("全空列画像");
    assert_eq!(
        empty.kind,
        ColumnKind::Unknown,
        "全 NULL 列没有非空样本可判型，也走未识别"
    );
}

/// 目标不存在时：`describe_error` 的**字符串匹配规则**要对得上 DuckDB 的真实错误文案。
///
/// 这条用例的价值就在这里——它是唯一能让「按消息内容识别」这套权宜拿到真实反馈的地方。
#[test]
fn missing_result_set_is_reported_with_a_friendly_message() {
    let err = InsightService::profile_column_view(None, "t_insight_e2e_absent", "amount")
        .expect_err("表不存在应当报错");
    let info = InsightService::describe_error(&err);

    assert_eq!(
        info.message, "结果集已失效或已过期，请重新执行查询",
        "未能识别 DuckDB 的真实报错（原始：{err}）"
    );
    assert!(!info.retryable, "结果集没了，重试不会变好");
    assert!(
        !info.message.contains('['),
        "不得把内部错误码展示给用户：{}",
        info.message
    );
}

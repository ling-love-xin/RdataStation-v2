//! 列画像与表探查的**端到端**：真实 DuckDB 临时表 → 规则驱动统计 → 面板视图模型。
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
//! 留到后续批次一起补，避免在这里用构造数据迁就实现。

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

// ==================== 表探查（Phase 3.1 / 2.2） ====================

/// 表探查：列元数据与行数来自 DuckDB，类型族按真实类型名判定，且**不产假分数**
#[test]
fn table_profile_lists_columns_without_fake_scores() {
    let table = "t_insight_e2e_table";
    seed(
        table,
        "id BIGINT, amount DECIMAL(12,2), note VARCHAR, payload BLOB",
        "VALUES (1, 1.5, 'a', 'x'::BLOB), (2, NULL, NULL, 'y'::BLOB)",
    );

    let view = InsightService::profile_table_view(None, table, "orders").expect("表探查应成功");
    assert_eq!(view.table_name, "orders", "展示名取目标里的逻辑名");
    assert_eq!(view.row_count, 2);
    let names: Vec<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["id", "amount", "note", "payload"]);
    assert_eq!(view.columns[1].kind, ColumnKind::Numeric, "DECIMAL 归数值族");
    assert_eq!(view.columns[2].kind, ColumnKind::Text);
    assert_eq!(view.columns[3].kind, ColumnKind::Unknown, "BLOB 不参与列统计");
    assert!(
        view.columns.iter().all(|c| c.score.is_none()),
        "只是探查：分数要等用户点「评估全表」"
    );
    assert!(view.quality.is_none());
}

/// 评估全表：逐列统计 → 每列分数 + 表级摘要。
///
/// 这条用例同时钉住「表级分数与列级分数同源」：`compute_table_quality` 内部逐列调用
/// 同一个 `compute_column_quality`，所以列上的分数必须与摘要里的聚合对得上。
#[test]
fn table_evaluation_scores_every_column() {
    let table = "t_insight_e2e_table_eval";
    let rows: Vec<String> = (1..=12)
        .map(|i| format!("({i}, {i}.5, 'v{i}')"))
        .collect();
    seed(
        table,
        "id BIGINT, amount DECIMAL(12,2), note VARCHAR",
        &format!("VALUES {}", rows.join(",")),
    );

    let view = InsightService::profile_table_view(None, table, "orders").expect("表探查");
    let mut evaluated = Vec::new();
    for column in &view.columns {
        let full = InsightService::get_column_insight_full(None, table, &column.name)
            .expect("逐列统计");
        evaluated.push(full);
    }
    let quality = InsightService::compute_table_quality("orders", &evaluated);
    let done = view.evaluating(evaluated.len(), evaluated.len()).evaluated(&quality);

    assert!(done.progress.is_none(), "评估完进度行必须消失");
    let summary = done.quality.expect("应有表级摘要");
    assert_eq!(summary.scored_columns, 3);
    assert!(summary.summary.contains("表质量"), "摘要：{}", summary.summary);
    // 全量非空、无重复的三列：分数应明显偏高（不写死具体数值，只钉区间关系）
    assert!(
        summary.overall > 50.0,
        "干净数据不该被判成差：{}",
        summary.overall
    );
    assert_eq!(summary.grade, rds_insight::Grade::of(summary.overall));
}

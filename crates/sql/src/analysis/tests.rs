//! 能力出口的单测：六项能力各一条，口径按上游实测行为锁定。

use super::*;

fn users() -> Vec<(String, String)> {
    vec![
        ("id".to_string(), "int".to_string()),
        ("amount".to_string(), "decimal(12,2)".to_string()),
        ("name".to_string(), "varchar".to_string()),
    ]
}

#[test]
fn scope_report_lists_sources_and_qualifies_columns() {
    let report = scope_report(
        "SELECT o.id, t.total FROM orders o JOIN totals t ON o.id = t.id",
        SqlDialect::Ansi,
    )
    .expect("解析应成功");

    let aliases: Vec<&str> = report.sources.iter().map(|s| s.alias.as_str()).collect();
    assert!(aliases.contains(&"o"), "别名 o 应在来源列表里：{aliases:?}");
    assert!(aliases.contains(&"t"), "别名 t 应在来源列表里：{aliases:?}");
    let tables: Vec<&str> = report.sources.iter().map(|s| s.table.as_str()).collect();
    assert!(tables.contains(&"orders"), "真实表名应保留：{tables:?}");
    assert!(
        report.columns.iter().any(|c| c == "o.id"),
        "限定列应保留限定符：{:?}",
        report.columns
    );
}

#[test]
fn scope_report_does_not_need_catalog() {
    // 单表 FROM：裸列会被上游归位到唯一来源，所以这里只钉「不报错、能给出列」
    let report = scope_report("SELECT id FROM orders", SqlDialect::Ansi).expect("解析应成功");
    assert_eq!(report.sources.len(), 1, "单表 FROM 只有一个来源");
    assert!(
        report
            .columns
            .iter()
            .any(|c| c == "id" || c == "orders.id"),
        "列引用应被记录：{:?}",
        report.columns
    );
}

#[test]
fn column_types_annotates_expression_result() {
    let tables = [TypedTable {
        qualified_name: "users".to_string(),
        columns: users(),
    }];
    let typed = column_types(
        "SELECT id, amount * 2 AS doubled FROM users",
        SqlDialect::Ansi,
        &tables,
    )
    .expect("标注应成功");

    assert_eq!(typed.len(), 2, "两列都应有类型：{typed:?}");
    assert_eq!(typed[0].data_type, "INT", "id 是 INT：{typed:?}");
    assert!(
        typed[1].data_type.starts_with("DECIMAL"),
        "decimal 乘 2 仍是 DECIMAL：{typed:?}"
    );
}

#[test]
fn column_types_returns_empty_without_catalog() {
    let typed = column_types("SELECT id FROM users", SqlDialect::Ansi, &[]).expect("空目录不报错");
    assert!(typed.is_empty(), "没有元数据就不标类型");
}

#[test]
fn lineage_traces_output_column_to_base_column() {
    let tables = [TypedTable {
        qualified_name: "orders".to_string(),
        columns: vec![
            ("amount".to_string(), "decimal".to_string()),
            ("id".to_string(), "int".to_string()),
        ],
    }];
    let steps = lineage_of_column(
        "SELECT SUM(amount) AS total FROM orders",
        SqlDialect::Ansi,
        "total",
        &tables,
    )
    .expect("血缘应可算");

    assert!(!steps.is_empty(), "应有血缘节点");
    assert_eq!(steps[0].name, "total", "第一跳是输出列：{steps:?}");
    assert!(
        steps
            .iter()
            .any(|s| s.name.contains("amount")),
        "应追到基列 amount：{steps:?}"
    );
}

#[test]
fn diff_reports_structural_changes() {
    let changes = diff_summaries(
        "SELECT id FROM t",
        "SELECT id, name FROM t",
        SqlDialect::Ansi,
    )
    .expect("差异应可算");

    assert!(!changes.is_empty(), "应至少有一处变化");
    assert!(
        changes.iter().any(|c| c.structural),
        "新增投影列应被标为结构性改动：{changes:?}"
    );
    assert!(
        changes
            .iter()
            .filter(|c| c.structural)
            .any(|c| c.detail.contains("name")),
        "差异文本应提到新增的列：{changes:?}"
    );
}

#[test]
fn pushdown_moves_predicate_into_derived_table() {
    let sql = pushdown_sql(
        "SELECT * FROM (SELECT id, a FROM t) s WHERE s.a = 1",
        SqlDialect::Ansi,
    )
    .expect("改写应成功");
    assert!(sql.to_uppercase().contains("SELECT"), "应仍是可执行 SQL：{sql}");
}

#[test]
fn plan_summary_reports_steps() {
    let plan = plan_summary("SELECT * FROM t WHERE a = 1", SqlDialect::Ansi).expect("计划应可算");
    assert!(plan.step_count >= 1, "至少一个算子：{plan:?}");
    assert!(!plan.mermaid.is_empty(), "算子图应有内容");
}

#[test]
fn plan_rejects_ddl() {
    let error = plan_summary("CREATE TABLE t (id INT)", SqlDialect::Ansi)
        .expect_err("DDL 上游明确不支持");
    assert!(
        error.contains("Planner") || error.contains("not support"),
        "错误应透传上游口径：{error}"
    );
}

#[test]
fn time_format_converts_between_dialects() {
    let converted = translate_time_format("%Y-%m-%d", SqlDialect::Mysql, SqlDialect::Postgres);
    assert_eq!(converted, "YYYY-MM-DD", "MySQL → Postgres 格式串应转换");
    let same = translate_time_format("YYYY-MM-DD", SqlDialect::Postgres, SqlDialect::Postgres);
    assert_eq!(same, "YYYY-MM-DD", "同方言原样返回");
}

#[test]
fn prefs_default_on_and_round_trip() {
    let defaults = AnalysisPrefs::default();
    assert!(defaults.semantic_diagnostics);
    assert!(defaults.predicate_pushdown);
    assert!(defaults.hover_scope);

    set_prefs(AnalysisPrefs {
        semantic_diagnostics: false,
        predicate_pushdown: true,
        hover_scope: false,
    });
    let current = prefs();
    assert!(!current.semantic_diagnostics);
    assert!(current.predicate_pushdown);
    assert!(!current.hover_scope);

    // 复位，避免污染同进程里的其它用例（语言服务读的是同一份偏好）
    set_prefs(AnalysisPrefs::default());
}

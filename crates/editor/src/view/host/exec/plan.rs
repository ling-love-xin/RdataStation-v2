//! 执行计划回执的展平（审计 29b）
//!
//! 背景：本地通道（加速 / 联邦）的「执行计划」发的是 `EXPLAIN (FORMAT JSON)`，驱动交回来的是
//! **两列 JSON 原文**（`physical_plan` / 一坨节点数组）。直接铺进网格等于把结构化的树塞回
//! 一段文本——引擎侧（刀29）已经把 JSON 解析成 [`PlanNode`] 与 [`SourceScanFacts`]，这里在
//! **回执落到 store 之前**把树展平成一张可读的表，零 UI 改动。
//!
//! 纪律：只有「标题是执行计划 + 结果值确实是 DuckDB JSON 计划」时才改写；任何一步对不上就
//! 原样放行（宁可让用户看原文，也不能把结果吞掉）。源库方言的 `EXPLAIN`（文本 / `EXPLAIN
//! QUERY PLAN`）不受影响，照旧原样展示。

use crate::store::ResultEntry;

use engine::{ExplainAnalyzer, PlanNode, SourceScanFacts, is_federated_operator};

/// 执行计划结果集的标题
///
/// `explain_current` 贴的就是这个；两处必须同源，否则展平不会触发（宁可漏改写，不要错改写）。
pub(crate) const EXPLAIN_TITLE: &str = "执行计划";

/// 展平后的列
const PLAN_COLUMNS: [&str; 6] = ["层级", "算子", "对象", "预估行数", "下推", "额外信息"];

/// 把「执行计划」回执的两列 JSON 改写成展平的计划表；对不上就原样不动。
pub(crate) fn flatten_explain_result(entry: &mut ResultEntry) {
    if entry.error.is_some() || entry.title.as_deref() != Some(EXPLAIN_TITLE) {
        return;
    }
    let Some(payload) = explain_json_payload(&entry.rows) else {
        return;
    };
    let Ok(plan) = ExplainAnalyzer::parse_json_plan(payload) else {
        return;
    };
    let (columns, rows) = plan_table(&plan);
    entry.columns = columns;
    entry.rows = rows;
    // 两条不变式：空 vec =「没报类型 / 没报注释」，与新列一一对应，不编造
    entry.column_types = Vec::new();
    entry.column_annotations = Vec::new();
    // 计划表没有可写回的行标识
    entry.key_columns = None;
}

/// 从两列回执里找出计划本体（`EXPLAIN (FORMAT JSON)` 的第 1 列）
fn explain_json_payload(rows: &[Vec<String>]) -> Option<&str> {
    rows.iter()
        .flat_map(|row| row.iter())
        .map(|cell| cell.trim())
        .find(|cell| (cell.starts_with('[') || cell.starts_with('{')) && cell.contains("\"name\""))
}

/// 计划树 → （列, 行）
///
/// 行序 = 先序，与 [`ExplainAnalyzer::collect_source_scans`] 的遍历同序，所以外部源扫描的
/// 下推结论可以按序取用，不必按 operator/表名重新配对（同名表出现两次时会配错）。
pub(crate) fn plan_table(plan: &PlanNode) -> (Vec<String>, Vec<Vec<String>>) {
    let facts = ExplainAnalyzer::collect_source_scans(plan);
    let mut rows = Vec::new();
    let mut next_fact = 0;
    push_node(plan, 1, &facts, &mut next_fact, &mut rows);
    (
        PLAN_COLUMNS.iter().map(|c| (*c).to_string()).collect(),
        rows,
    )
}

fn push_node(
    node: &PlanNode,
    depth: usize,
    facts: &[SourceScanFacts],
    next_fact: &mut usize,
    out: &mut Vec<Vec<String>>,
) {
    let indent = "  ".repeat(depth - 1);
    let operator = if node.operator.is_empty() {
        node.description.clone()
    } else {
        node.operator.clone()
    };
    let table = node.extra_info.get("Table").cloned().unwrap_or_default();
    let estimated = node
        .estimated_rows
        .map(|n| n.to_string())
        .unwrap_or_default();
    let pushdown = if is_federated_operator(&node.operator) {
        let summary = facts.get(*next_fact).map(SourceScanFacts::summary);
        *next_fact += 1;
        summary.unwrap_or_default()
    } else {
        String::new()
    };
    out.push(vec![
        depth.to_string(),
        format!("{indent}{operator}"),
        table,
        estimated,
        pushdown,
        extra_info_text(node),
    ]);
    for child in &node.children {
        push_node(child, depth + 1, facts, next_fact, out);
    }
}

/// `extra_info` 里已由「对象 / 预估行数 / 下推」三列表达的键不再重复
fn extra_info_text(node: &PlanNode) -> String {
    const SKIP: [&str; 4] = ["Table", "Projections", "Filters", "Estimated Cardinality"];
    node.extra_info
        .iter()
        .filter(|(key, _)| !SKIP.contains(&key.as_str()))
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{explain_json_payload, plan_table};
    use engine::ExplainAnalyzer;

    /// 外部源扫描 + 扫描之上有 `FILTER`：既要展平出层级，也要把「过滤未推」落在扫描那一行
    const FED_PLAN: &str = r#"[
      {
        "name": "PROJECTION",
        "children": [
          {
            "name": "FILTER",
            "children": [
              {
                "name": "MYSQL_SCAN",
                "extra_info": {"Table": "mysql.t", "Projections": "a"}
              }
            ]
          }
        ],
        "extra_info": {"Estimated Cardinality": "3"}
      }
    ]"#;

    #[test]
    fn federated_scan_keeps_its_pushdown_verdict() {
        let plan = ExplainAnalyzer::parse_json_plan(FED_PLAN).expect("解析真实形状的计划");
        let (columns, rows) = plan_table(&plan);

        assert_eq!(columns.len(), 6);
        assert_eq!(rows.len(), 3, "先序展平后根 + FILTER + 扫描各一行");
        assert_eq!(rows[0][1], "PROJECTION");
        assert_eq!(rows[0][3], "3", "预估行数取自 Estimated Cardinality");
        assert_eq!(rows[1][0], "2", "FILTER 是第 2 层");
        assert_eq!(rows[1][1], "  FILTER");
        assert_eq!(rows[2][1], "    MYSQL_SCAN", "缩进按层级递进");
        assert_eq!(rows[2][2], "mysql.t");
        assert_eq!(rows[2][4], "投影已推 · 过滤未推");
        assert_eq!(rows[2][5], "", "已进前三列的 extra_info 不重复");
    }

    #[test]
    fn payload_is_found_in_either_column() {
        let json = vec![vec![
            "physical_plan".to_string(),
            r#"[{"name":"SEQ_SCAN"}]"#.to_string(),
        ]];
        assert_eq!(
            explain_json_payload(&json),
            Some(r#"[{"name":"SEQ_SCAN"}]"#)
        );

        // 源库方言的文本计划不该被认成 JSON
        let text = vec![vec![
            "QUERY PLAN".to_string(),
            "Seq Scan on t (cost=0.00..1.00 rows=1 width=4)".to_string(),
        ]];
        assert_eq!(explain_json_payload(&text), None);

        // 空结果 / 只有键列：没有计划本体
        let empty = vec![vec!["physical_plan".to_string(), String::new()]];
        assert_eq!(explain_json_payload(&empty), None);
    }

    /// 整条回执：标题对 + 值确实是 JSON 才改写，且不改坏 `column_types` / `key_columns` 的不变式
    #[test]
    fn receipt_turns_into_a_plan_table_only_when_title_and_json_both_match() {
        use super::{EXPLAIN_TITLE, flatten_explain_result};
        use crate::model::DocumentId;
        use crate::store::ResultEntry;

        let payload = r#"[{"name":"SEQ_SCAN","extra_info":{"Table":"memory.main.t"}}]"#;
        let mut entry = ResultEntry::success(
            DocumentId::new("doc"),
            "EXPLAIN (FORMAT JSON) SELECT a FROM t".to_string(),
            3,
            false,
            vec!["explain_key".to_string(), "explain_value".to_string()],
            vec![vec!["physical_plan".to_string(), payload.to_string()]],
        )
        .with_title(EXPLAIN_TITLE.to_string());

        flatten_explain_result(&mut entry);

        assert_eq!(entry.columns.len(), 6, "计划表是 6 列");
        assert_eq!(entry.rows.len(), 1);
        assert_eq!(entry.rows[0][1], "SEQ_SCAN");
        assert_eq!(entry.rows[0][2], "memory.main.t");
        assert!(entry.column_types.is_empty(), "空 vec = 没报类型（合法档）");
        assert!(entry.column_annotations.is_empty(), "同上");
        assert_eq!(entry.key_columns, None, "计划表没有可写回的行标识");

        // 标题不对（普通查询结果里恰好有一段 JSON）：绝不改写
        let mut plain = ResultEntry::success(
            DocumentId::new("doc"),
            "SELECT payload FROM logs".to_string(),
            3,
            false,
            vec!["payload".to_string()],
            vec![vec![payload.to_string()]],
        );
        let before = plain.clone();
        flatten_explain_result(&mut plain);
        assert_eq!(plain, before, "没有「执行计划」标题就不动它");
    }
}

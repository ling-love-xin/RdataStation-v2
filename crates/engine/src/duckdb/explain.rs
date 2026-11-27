//! DuckDB 执行计划解析。
//!
//! 只用 `EXPLAIN (FORMAT JSON)`：DuckDB 的默认 `EXPLAIN` 输出是**框线图**（`┌─┬─┐` 形状，
//! 节点间靠 `│` 列位表达层级），按行缩进解析必然失真；JSON 形态带 `children`，树是可靠的。
//! 2026-11-27 在 DuckDB 2.0.1 实测输出形状（内存表 `SELECT a FROM t WHERE a > 0 ORDER BY a`）：
//!
//! ```text
//! key=physical_plan, value=[{"name":"PROJECTION","children":[…],"extra_info":{"Projections":"…"}}]
//! ```
//!
//! 顶层是**数组**；节点键为 `name` / `children` / `extra_info`，`extra_info` 的值是字符串
//! **或字符串数组**（`Projections` 在 `CAST(#0 AS INTEGER)` 这类单值形态是多值时为数组）。
//! `EXPLAIN` 的结果集是两列：第 0 列 `explain_key`，第 1 列 `explain_value`（计划本体）。

use std::collections::BTreeMap;

use duckdb::Connection;

use shared::error::{CommonError, CoreError};

/// 查询计划节点类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanNodeType {
    /// 顺序扫描
    SeqScan,
    /// 索引扫描
    IndexScan,
    /// 哈希连接
    HashJoin,
    /// 嵌套循环连接
    NestedLoopJoin,
    /// 聚合
    Aggregate,
    /// 排序
    OrderBy,
    /// 过滤
    Filter,
    /// 投影
    Projection,
    /// 其他类型
    Other(String),
}

impl PlanNodeType {
    /// 由 DuckDB 的 operator 名（`name` 字段）判定节点类型。
    ///
    /// 外部源算子（`MYSQL_QUERY` / `POSTGRES_SCAN` / `ORACLE_ATTACHED_SCAN` 等）**不**归为
    /// `SeqScan`：`has_full_table_scan` 的语义是「本地全表扫描」，远端源的扫描另有
    /// [`SourceScanFacts`] 表达，混在一起会让性能提示给出错误结论。
    fn from_operator(operator: &str) -> Self {
        match operator {
            "SEQ_SCAN" => PlanNodeType::SeqScan,
            "INDEX_SCAN" => PlanNodeType::IndexScan,
            "HASH_JOIN" => PlanNodeType::HashJoin,
            "NESTED_LOOP_JOIN" | "PIECEWISE_MERGE_JOIN" => PlanNodeType::NestedLoopJoin,
            "ORDER_BY" => PlanNodeType::OrderBy,
            "FILTER" => PlanNodeType::Filter,
            "PROJECTION" => PlanNodeType::Projection,
            op if op.contains("AGGREGATE") || op.contains("GROUP_BY") => PlanNodeType::Aggregate,
            other => PlanNodeType::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for PlanNodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanNodeType::SeqScan => write!(f, "SeqScan"),
            PlanNodeType::IndexScan => write!(f, "IndexScan"),
            PlanNodeType::HashJoin => write!(f, "HashJoin"),
            PlanNodeType::NestedLoopJoin => write!(f, "NestedLoopJoin"),
            PlanNodeType::Aggregate => write!(f, "Aggregate"),
            PlanNodeType::OrderBy => write!(f, "OrderBy"),
            PlanNodeType::Filter => write!(f, "Filter"),
            PlanNodeType::Projection => write!(f, "Projection"),
            PlanNodeType::Other(s) => write!(f, "{}", s),
        }
    }
}

/// 查询计划节点
#[derive(Debug, Clone)]
pub struct PlanNode {
    /// 节点类型
    pub node_type: PlanNodeType,
    /// 可读描述（operator 名 + 关键 extra_info）
    pub description: String,
    /// 预估成本（DuckDB 的 JSON 计划不提供成本，恒为 None）
    pub estimated_cost: Option<f64>,
    /// 预估行数（取自 `extra_info["Estimated Cardinality"]`）
    pub estimated_rows: Option<u64>,
    /// 子节点
    pub children: Vec<PlanNode>,
    /// DuckDB 原始 operator 名（如 `MYSQL_SCAN`、`ORACLE_QUERY`）
    pub operator: String,
    /// 原样保留的 `extra_info`（键值均为字符串化结果）
    pub extra_info: BTreeMap<String, String>,
}

impl PlanNode {
    /// 创建新的查询计划节点。
    pub fn new(
        node_type: PlanNodeType,
        description: String,
        estimated_cost: Option<f64>,
        estimated_rows: Option<u64>,
    ) -> Self {
        PlanNode {
            node_type,
            description,
            estimated_cost,
            estimated_rows,
            children: Vec::new(),
            operator: String::new(),
            extra_info: BTreeMap::new(),
        }
    }

    /// 添加子节点。
    pub fn add_child(&mut self, child: PlanNode) {
        self.children.push(child);
    }

    /// 递归获取所有节点类型。
    pub fn collect_all_node_types(&self) -> Vec<PlanNodeType> {
        let mut types = vec![self.node_type.clone()];
        for child in &self.children {
            types.extend(child.collect_all_node_types());
        }
        types
    }

    /// 递归检查是否包含指定节点类型。
    pub fn contains_node_type(&self, target: &PlanNodeType) -> bool {
        if self.node_type == *target {
            return true;
        }
        self.children.iter().any(|c| c.contains_node_type(target))
    }

    /// 获取最大深度（根自身算 1 层）。
    pub fn max_depth(&self) -> usize {
        if self.children.is_empty() {
            1
        } else {
            1 + self
                .children
                .iter()
                .map(|c| c.max_depth())
                .max()
                .unwrap_or(0)
        }
    }
}

/// 一次外部源扫描在计划中的事实。
///
/// 供结果区标注「投影已推 / 过滤未推」。判定依据只有计划结构本身：
/// - 投影：扫描节点的 `extra_info` 里有非空 `Projections`；
/// - 过滤：谓词若留在扫描节点**之上**，表现为从根到该节点的路径上出现 `FILTER`；
///   若已下推进扫描，`extra_info` 里会有 `Filters`；
/// - **整句下推**（`whole_statement`）：节点是 `MYSQL_QUERY` / `POSTGRES_QUERY` / `ORACLE_QUERY`
///   这类**表函数形态**。此时 DuckDB 把整条 SELECT 交给源端执行，计划里既没有 `Table` 也没有
///   `Projections` / `Filters`——投影与过滤的细节本地根本看不到，前三个字段恒 `false`，
///   结论只能说「整句下推 · 源端执行」（2026-11-27 真机实测：MySQL 的**所有**外部源查询都是
///   `MYSQL_QUERY`；PG 的简单查询是 `POSTGRES_SCAN`，带本地 join / 表达式时才退化成
///   `POSTGRES_QUERY`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScanFacts {
    /// DuckDB operator 名（`MYSQL_QUERY` / `POSTGRES_SCAN` / `ORACLE_ATTACHED_SCAN` …）
    pub operator: String,
    /// `extra_info["Table"]`（`_SCAN` 形态通常带表名；`_QUERY` 形态没有）
    pub table: Option<String>,
    /// 列投影是否下推（`extra_info["Projections"]` 非空）
    pub projections_pushed: bool,
    /// 谓词是否已下推进扫描（扫描节点自身带 `Filters`）
    pub filters_inside_scan: bool,
    /// 扫描节点之上是否有 `FILTER`（谓词停在扫描之上，未下推）
    pub filter_above_scan: bool,
    /// 整句下推（表函数形态 `*_QUERY`）：投影 / 过滤细节不可见
    pub whole_statement: bool,
}

impl SourceScanFacts {
    /// 一句话结论，直接给面板/结果区用。
    pub fn summary(&self) -> String {
        // 整句下推时再说「投影未推 / 无过滤」是编造：本地压根没拿到这两项事实
        if self.whole_statement {
            return "整句下推 · 源端执行".to_string();
        }
        let projection = if self.projections_pushed {
            "投影已推"
        } else {
            "投影未推"
        };
        let filter = if self.filters_inside_scan {
            "过滤已推"
        } else if self.filter_above_scan {
            "过滤未推"
        } else {
            "无过滤"
        };
        format!("{} · {}", projection, filter)
    }
}

/// 判断 operator 是否代表外部源（联邦/加速源）的读取。
///
/// 本地算子（`SEQ_SCAN` / `INDEX_SCAN`）之外，DuckDB 的外部源算子有两族（2026-11-27 真机实测）：
/// - **扫描形态** `*_SCAN`（`POSTGRES_SCAN` / `SQLITE_SCAN` / `ORACLE_ATTACHED_SCAN`）：
///   计划里带 `Table` / `Projections` / `Filters`，可判下推；
/// - **表函数形态** `*_QUERY`（`MYSQL_QUERY` / `POSTGRES_QUERY` / `ORACLE_QUERY`）：
///   整句下推，细节不可见。MySQL 的所有外部源查询都走这一族（DuckDB 2.0.1 的 MySQL 扩展
///   直接把它绑定成 `MYSQL_QUERY`），所以只认 `_SCAN` 会让「下推」列永远空白。
pub fn is_federated_operator(operator: &str) -> bool {
    const LOCAL_OPERATORS: &[&str] = &["SEQ_SCAN", "INDEX_SCAN"];
    if LOCAL_OPERATORS.contains(&operator) {
        return false;
    }
    operator.ends_with("_SCAN") || operator.ends_with("_QUERY") || operator.starts_with("ORACLE_")
}

/// 查询计划分析器
///
/// 负责 EXPLAIN 查询计划解析与分析。
pub struct ExplainAnalyzer;

impl ExplainAnalyzer {
    /// 执行 EXPLAIN 查询并解析为结构化计划树。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `query`: 要分析的查询 SQL（若自带 `EXPLAIN` 前缀会被剥离）
    ///
    /// # 返回
    /// - `Ok(PlanNode)`: 根查询计划节点
    /// - `Err(CoreError)`: 执行或解析失败
    pub fn analyze_query(conn: &Connection, query: &str) -> Result<PlanNode, CoreError> {
        let json = Self::explain_json(conn, query)?;
        Self::parse_json_plan(&json)
    }

    /// 跑 `EXPLAIN (FORMAT JSON) <query>`，返回 `explain_value` 列的计划 JSON。
    pub fn explain_json(conn: &Connection, query: &str) -> Result<String, CoreError> {
        let sql =
            Self::generate_explain_sql(Self::strip_explain_prefix(query), Some("FORMAT JSON"));
        tracing::info!("[ExplainAnalyzer] EXPLAIN SQL: {}", sql);

        let mut stmt = conn.prepare(&sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "准备 EXPLAIN 查询失败: {}",
                e
            )))
        })?;

        let mut rows = stmt.query([]).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "执行 EXPLAIN 查询失败: {}",
                e
            )))
        })?;

        // EXPLAIN 的结果集是两列：0 = explain_key，1 = explain_value（计划本体）
        let mut chunks = Vec::new();
        while let Some(row) = rows.next().map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "获取 EXPLAIN 结果失败: {}",
                e
            )))
        })? {
            let value: String = row.get::<usize, String>(1).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "EXPLAIN 结果缺少 explain_value 列: {}",
                    e
                )))
            })?;
            chunks.push(value);
        }

        if chunks.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "EXPLAIN 未返回任何计划行".to_string(),
            )));
        }
        Ok(chunks.join("\n"))
    }

    /// 解析 `EXPLAIN (FORMAT JSON)` 的 JSON 文本为计划树。
    pub fn parse_json_plan(json: &str) -> Result<PlanNode, CoreError> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "解析 EXPLAIN JSON 失败: {}",
                e
            )))
        })?;
        let root = match &value {
            // 顶层是数组（实测形状）；多份计划时取第一份
            serde_json::Value::Array(items) => items.first().ok_or_else(|| {
                CoreError::common(CommonError::General("EXPLAIN JSON 是空数组".to_string()))
            })?,
            other => other,
        };
        Self::node_from_json(root).ok_or_else(|| {
            CoreError::common(CommonError::General(
                "EXPLAIN JSON 节点缺少 name 字段".to_string(),
            ))
        })
    }

    fn node_from_json(value: &serde_json::Value) -> Option<PlanNode> {
        let operator = value.get("name")?.as_str()?.to_string();

        let mut extra_info = BTreeMap::new();
        if let Some(obj) = value.get("extra_info").and_then(|v| v.as_object()) {
            for (key, raw) in obj {
                extra_info.insert(key.clone(), Self::scalar_to_string(raw));
            }
        }

        let estimated_rows = extra_info.get("Estimated Cardinality").and_then(|v| {
            v.split('.')
                .next()
                .and_then(|s| s.trim().parse::<u64>().ok())
        });

        let description = if extra_info.is_empty() {
            operator.clone()
        } else {
            let details = extra_info
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("; ");
            format!("{} ({})", operator, details)
        };

        let mut node = PlanNode {
            node_type: PlanNodeType::from_operator(&operator),
            description,
            estimated_cost: None,
            estimated_rows,
            children: Vec::new(),
            operator,
            extra_info,
        };

        if let Some(children) = value.get("children").and_then(|c| c.as_array()) {
            for child in children {
                if let Some(node_child) = Self::node_from_json(child) {
                    node.children.push(node_child);
                }
            }
        }
        Some(node)
    }

    /// `extra_info` 的值是字符串或字符串数组（实测两种都出现），统一成逗号分隔文本。
    fn scalar_to_string(raw: &serde_json::Value) -> String {
        match raw {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(", "),
            other => other.to_string(),
        }
    }

    /// 去掉查询里自带的 `EXPLAIN` / `EXPLAIN ANALYZE` 前缀（可带括号选项）。
    fn strip_explain_prefix(query: &str) -> &str {
        let trimmed = query.trim_start();
        let Some(rest) = trimmed
            .get(..7)
            .filter(|p| p.eq_ignore_ascii_case("explain"))
            .map(|_| &trimmed[7..])
        else {
            return query;
        };
        let mut rest = rest.trim_start();
        // `(FORMAT JSON)` / `ANALYZE` 这类紧跟的修饰词一并剥掉
        if let Some(after_paren) = rest.strip_prefix('(') {
            if let Some(pos) = after_paren.find(')') {
                rest = after_paren[pos + 1..].trim_start();
            }
        } else if rest
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("analyze"))
        {
            rest = &rest[7..];
            rest = rest.trim_start();
        }
        if rest.is_empty() { query } else { rest }
    }

    /// 收集计划里所有外部源扫描的下推事实（自顶向下，深度优先）。
    pub fn collect_source_scans(plan: &PlanNode) -> Vec<SourceScanFacts> {
        let mut facts = Vec::new();
        Self::walk_source_scans(plan, false, &mut facts);
        facts
    }

    fn walk_source_scans(node: &PlanNode, filter_on_path: bool, out: &mut Vec<SourceScanFacts>) {
        let node_is_filter = node.operator == "FILTER" || node.node_type == PlanNodeType::Filter;

        if is_federated_operator(&node.operator) {
            // 表函数形态 = 整句下推：没有 Table / Projections / Filters，别把「没看到」当成「没推」
            let whole_statement = node.operator.ends_with("_QUERY");
            out.push(SourceScanFacts {
                operator: node.operator.clone(),
                table: node.extra_info.get("Table").cloned(),
                projections_pushed: !whole_statement
                    && node
                        .extra_info
                        .get("Projections")
                        .is_some_and(|v| !v.trim().is_empty()),
                filters_inside_scan: !whole_statement && node.extra_info.contains_key("Filters"),
                filter_above_scan: filter_on_path,
                whole_statement,
            });
        }

        for child in &node.children {
            Self::walk_source_scans(child, filter_on_path || node_is_filter, out);
        }
    }

    /// 获取查询计划的性能建议。
    ///
    /// # 参数
    /// - `plan`: 查询计划
    ///
    /// # 返回
    /// 性能建议列表
    pub fn get_performance_suggestions(plan: &PlanNode) -> Vec<String> {
        let mut suggestions = Vec::new();

        if Self::has_full_table_scan(plan) {
            suggestions.push("检测到全表扫描，建议添加索引优化查询".to_string());
        }

        if Self::has_nested_loop_join(plan) {
            suggestions.push("检测到嵌套循环连接，对于大数据集可能性能较差".to_string());
        }

        if plan.max_depth() > 5 {
            suggestions.push("查询计划较深，建议优化查询结构".to_string());
        }

        if let Some(cost) = plan.estimated_cost {
            if cost > 1000000.0 {
                suggestions.push("预估成本较高，建议优化查询或使用索引".to_string());
            }
        }

        for scan in Self::collect_source_scans(plan) {
            if scan.filter_above_scan && !scan.filters_inside_scan {
                suggestions.push(format!(
                    "{} 的过滤条件未下推（谓词停在扫描之上），可考虑把条件写进源查询",
                    scan.operator
                ));
            }
        }

        suggestions
    }

    /// 生成 EXPLAIN 查询的 SQL 语句。
    ///
    /// # 参数
    /// - `query`: 要分析的查询 SQL
    /// - `format`: EXPLAIN 输出格式（可选，如 `FORMAT JSON`）
    ///
    /// # 返回
    /// EXPLAIN SQL 语句
    pub fn generate_explain_sql(query: &str, format: Option<&str>) -> String {
        match format {
            Some(fmt) => format!("EXPLAIN ({}) {}", fmt, query),
            None => format!("EXPLAIN {}", query),
        }
    }

    /// 检查查询计划是否包含全表扫描（仅本地 `SEQ_SCAN`）。
    pub fn has_full_table_scan(plan: &PlanNode) -> bool {
        plan.contains_node_type(&PlanNodeType::SeqScan)
    }

    /// 检查查询计划是否包含嵌套循环连接。
    pub fn has_nested_loop_join(plan: &PlanNode) -> bool {
        plan.contains_node_type(&PlanNodeType::NestedLoopJoin)
    }

    /// 格式化查询计划为可读文本。
    ///
    /// # 参数
    /// - `plan`: 查询计划
    /// - `indent`: 缩进级别
    ///
    /// # 返回
    /// 格式化后的查询计划文本
    pub fn format_plan(plan: &PlanNode, indent: usize) -> String {
        let indent_str = "  ".repeat(indent);
        let mut output = format!("{}{}\n", indent_str, plan.description);

        for child in &plan.children {
            output.push_str(&Self::format_plan(child, indent + 1));
        }

        output
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    /// 真实输出：2026-11-27 在 DuckDB 2.0.1 内存表
    /// `SELECT a FROM t WHERE a > 0 ORDER BY a` 上跑 `EXPLAIN (FORMAT JSON)` 的原样结果。
    const REAL_IN_MEMORY_PLAN: &str = r#"[
        {
            "name": "PROJECTION",
            "children": [
                {
                    "name": "ORDER_BY",
                    "children": [
                        {
                            "name": "PROJECTION",
                            "children": [
                                {
                                    "name": "SEQ_SCAN",
                                    "children": [],
                                    "extra_info": {
                                        "Table": "memory.main.t",
                                        "Type": "Sequential Scan",
                                        "Projections": "a",
                                        "Estimated Cardinality": "1"
                                    }
                                }
                            ],
                            "extra_info": {
                                "Projections": "CAST(#0 AS TINYINT)",
                                "Estimated Cardinality": "1"
                            }
                        }
                    ],
                    "extra_info": {
                        "Order By": "a ASC",
                        "Estimated Cardinality": "1"
                    }
                }
            ],
            "extra_info": {
                "Projections": "CAST(#0 AS INTEGER)",
                "Estimated Cardinality": "1"
            }
        }
    ]"#;

    #[test]
    fn test_plan_node_type_display() {
        assert_eq!(format!("{}", PlanNodeType::SeqScan), "SeqScan");
        assert_eq!(format!("{}", PlanNodeType::HashJoin), "HashJoin");
        assert_eq!(
            format!("{}", PlanNodeType::Other("MYSQL_SCAN".to_string())),
            "MYSQL_SCAN"
        );
    }

    #[test]
    fn test_plan_node_add_child() {
        let mut root = PlanNode::new(PlanNodeType::Projection, "root".to_string(), None, None);
        let child = PlanNode::new(PlanNodeType::SeqScan, "scan".to_string(), None, None);

        root.add_child(child);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn test_plan_node_collect_all_node_types() {
        let mut root = PlanNode::new(PlanNodeType::Projection, "root".to_string(), None, None);
        let child = PlanNode::new(PlanNodeType::SeqScan, "scan".to_string(), None, None);
        root.add_child(child);

        let types = root.collect_all_node_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&PlanNodeType::Projection));
        assert!(types.contains(&PlanNodeType::SeqScan));
    }

    #[test]
    fn test_plan_node_contains_node_type() {
        let mut root = PlanNode::new(PlanNodeType::Projection, "root".to_string(), None, None);
        let child = PlanNode::new(PlanNodeType::SeqScan, "scan".to_string(), None, None);
        root.add_child(child);

        assert!(root.contains_node_type(&PlanNodeType::SeqScan));
        assert!(!root.contains_node_type(&PlanNodeType::HashJoin));
    }

    #[test]
    fn test_plan_node_max_depth() {
        let leaf = PlanNode::new(PlanNodeType::SeqScan, "leaf".to_string(), None, None);
        let mut root = PlanNode::new(PlanNodeType::Projection, "root".to_string(), None, None);
        root.add_child(leaf);

        assert_eq!(root.max_depth(), 2);
    }

    #[test]
    fn test_generate_explain_sql() {
        let sql = ExplainAnalyzer::generate_explain_sql("SELECT * FROM users", None);
        assert_eq!(sql, "EXPLAIN SELECT * FROM users");

        let sql = ExplainAnalyzer::generate_explain_sql("SELECT * FROM users", Some("OPTIMIZED"));
        assert_eq!(sql, "EXPLAIN (OPTIMIZED) SELECT * FROM users");
    }

    /// 真实形状必须建出 4 层树（PROJECTION → ORDER_BY → PROJECTION → SEQ_SCAN），
    /// 而不是把子节点全部平铺进根。
    #[test]
    fn test_parse_real_json_builds_layered_tree() {
        let root = ExplainAnalyzer::parse_json_plan(REAL_IN_MEMORY_PLAN).expect("解析真实计划");

        assert_eq!(root.operator, "PROJECTION");
        assert_eq!(root.max_depth(), 4, "真实计划是 4 层");
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].operator, "ORDER_BY");

        let scan = root.children[0].children[0].children[0].clone();
        assert_eq!(scan.operator, "SEQ_SCAN");
        assert_eq!(scan.node_type, PlanNodeType::SeqScan);
        assert_eq!(scan.estimated_rows, Some(1));
        assert_eq!(
            scan.extra_info.get("Projections").map(String::as_str),
            Some("a")
        );
        assert_eq!(
            scan.extra_info.get("Table").map(String::as_str),
            Some("memory.main.t")
        );
        assert_eq!(root.estimated_rows, Some(1));
    }

    /// `Projections` 为数组时（实测 `[CAST(#0 AS INTEGER), #1]` 形态）要拼成可读文本。
    #[test]
    fn test_parse_json_array_valued_extra_info() {
        let json = r##"[
            {
                "name": "PROJECTION",
                "children": [],
                "extra_info": {
                    "Projections": ["CAST(#0 AS INTEGER)", "#1"],
                    "Estimated Cardinality": "1"
                }
            }
        ]"##;
        let root = ExplainAnalyzer::parse_json_plan(json).expect("解析");
        assert_eq!(
            root.extra_info.get("Projections").map(String::as_str),
            Some("CAST(#0 AS INTEGER), #1")
        );
    }

    /// L1 台账形状：`MYSQL_SCAN` 带 `Projections`、路径上没有 `FILTER` → 投影已推、无过滤。
    #[test]
    fn test_mysql_scan_projection_pushed_without_filter() {
        let json = r#"[
            {
                "name": "PROJECTION",
                "children": [
                    {
                        "name": "MYSQL_SCAN",
                        "children": [],
                        "extra_info": {
                            "Table": "mysql_src.mysql.user",
                            "Projections": "id, name",
                            "Estimated Cardinality": "5"
                        }
                    }
                ],
                "extra_info": { "Projections": "id, name" }
            }
        ]"#;
        let root = ExplainAnalyzer::parse_json_plan(json).expect("解析");
        let facts = ExplainAnalyzer::collect_source_scans(&root);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].operator, "MYSQL_SCAN");
        assert_eq!(facts[0].table.as_deref(), Some("mysql_src.mysql.user"));
        assert!(facts[0].projections_pushed);
        assert!(!facts[0].filters_inside_scan);
        assert!(!facts[0].filter_above_scan, "路径上没有 FILTER");
        assert_eq!(facts[0].summary(), "投影已推 · 无过滤");
    }

    /// Oracle 台账结论（`oracle_probe.rs`：`FILTER` 在 `ORACLE_QUERY` / `ORACLE_ATTACHED_SCAN`
    /// **之上**；列投影会推、谓词不推）在 JSON 形态下的等价计划。
    #[test]
    fn test_oracle_filter_stays_above_scan() {
        let json = r#"[
            {
                "name": "PROJECTION",
                "children": [
                    {
                        "name": "FILTER",
                        "children": [
                            {
                                "name": "ORACLE_ATTACHED_SCAN",
                                "children": [],
                                "extra_info": {
                                    "Table": "ora.RDS_PROBE_ORDERS",
                                    "Projections": "ID, AMOUNT"
                                }
                            }
                        ],
                        "extra_info": { "Expressions": "(AMOUNT > 100)" }
                    }
                ],
                "extra_info": { "Projections": "ID, AMOUNT" }
            }
        ]"#;
        let root = ExplainAnalyzer::parse_json_plan(json).expect("解析");
        let facts = ExplainAnalyzer::collect_source_scans(&root);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].operator, "ORACLE_ATTACHED_SCAN");
        assert!(facts[0].projections_pushed, "列投影会推");
        assert!(!facts[0].filters_inside_scan, "谓词不在扫描内");
        assert!(facts[0].filter_above_scan, "FILTER 在扫描之上");
        assert_eq!(facts[0].summary(), "投影已推 · 过滤未推");
    }

    /// 谓词已下推进扫描（扫描节点自身带 `Filters`）→ 过滤已推。
    #[test]
    fn test_filters_inside_scan_counts_as_pushed() {
        let json = r#"[
            {
                "name": "MYSQL_SCAN",
                "children": [],
                "extra_info": {
                    "Table": "mysql_src.shop.orders",
                    "Projections": "id",
                    "Filters": "status='paid'",
                    "Estimated Cardinality": "3"
                }
            }
        ]"#;
        let root = ExplainAnalyzer::parse_json_plan(json).expect("解析");
        let facts = ExplainAnalyzer::collect_source_scans(&root);
        assert_eq!(facts.len(), 1);
        assert!(facts[0].filters_inside_scan);
        assert!(!facts[0].filter_above_scan);
        assert_eq!(facts[0].summary(), "投影已推 · 过滤已推");
    }

    /// 2026-11-27 真机（MySQL）实测的整句下推形态：`EXPLAIN (FORMAT JSON)` 只有一个
    /// `MYSQL_QUERY` 节点，`extra_info` 里只有 `Function` 与 `Estimated Cardinality`。
    /// 此处必须识别成外部源，并且**不能**用「投影未推 / 无过滤」替它说话。
    #[test]
    fn test_whole_statement_pushdown_is_not_a_missing_projection() {
        let json = r#"[
            {
                "name": "MYSQL_QUERY",
                "children": [],
                "extra_info": {
                    "Function": "MYSQL_QUERY",
                    "Estimated Cardinality": "1"
                }
            }
        ]"#;
        let root = ExplainAnalyzer::parse_json_plan(json).expect("解析");
        let facts = ExplainAnalyzer::collect_source_scans(&root);
        assert_eq!(facts.len(), 1, "整句下推节点也要被认出来");
        assert_eq!(facts[0].operator, "MYSQL_QUERY");
        assert_eq!(facts[0].table, None, "表函数形态拿不到表名");
        assert!(facts[0].whole_statement);
        assert!(!facts[0].projections_pushed);
        assert!(!facts[0].filters_inside_scan);
        assert_eq!(facts[0].summary(), "整句下推 · 源端执行");
    }

    /// 跨源算子识别：本地算子/文件读取不算；各源 scanner 与表函数形态算。
    #[test]
    fn test_federated_operator_detection() {
        for op in [
            "MYSQL_SCAN",
            "POSTGRES_SCAN",
            "SQLITE_SCAN",
            "ORACLE_ATTACHED_SCAN",
            "ORACLE_QUERY",
            // 表函数形态（MySQL 的所有外部源查询都是这一族，PG 在本地 join / 表达式时退化到这族）
            "MYSQL_QUERY",
            "POSTGRES_QUERY",
        ] {
            assert!(is_federated_operator(op), "{op} 应判为外部源");
        }
        for op in ["SEQ_SCAN", "INDEX_SCAN", "READ_CSV", "PROJECTION", "FILTER"] {
            assert!(!is_federated_operator(op), "{op} 不应判为外部源");
        }
    }

    /// 真实链路：连内存 DuckDB 跑 `EXPLAIN (FORMAT JSON)`，必须从第 1 列（explain_value）取计划。
    #[test]
    fn test_analyze_query_reads_value_column() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute("CREATE TABLE t(a INT, b VARCHAR)", [])
            .expect("create");
        conn.execute("INSERT INTO t VALUES (1, 'x')", [])
            .expect("insert");

        let plan =
            ExplainAnalyzer::analyze_query(&conn, "SELECT a FROM t ORDER BY a").expect("分析成功");
        assert_eq!(plan.operator, "PROJECTION");
        assert_eq!(plan.max_depth(), 4);
        assert!(
            plan.contains_node_type(&PlanNodeType::SeqScan),
            "内存表扫描是 SeqScan"
        );
    }

    /// 查询自带 `EXPLAIN` 前缀时不能拼成 `EXPLAIN (FORMAT JSON) EXPLAIN …`。
    #[test]
    fn test_explain_prefix_is_stripped() {
        assert_eq!(
            ExplainAnalyzer::strip_explain_prefix("EXPLAIN SELECT 1"),
            "SELECT 1"
        );
        assert_eq!(
            ExplainAnalyzer::strip_explain_prefix("explain analyze SELECT 1"),
            "SELECT 1"
        );
        assert_eq!(
            ExplainAnalyzer::strip_explain_prefix("EXPLAIN (FORMAT JSON) SELECT 1"),
            "SELECT 1"
        );
        assert_eq!(
            ExplainAnalyzer::strip_explain_prefix("SELECT 1"),
            "SELECT 1"
        );
    }

    #[test]
    fn test_has_full_table_scan() {
        let plan = PlanNode::new(PlanNodeType::SeqScan, "scan".to_string(), None, None);
        assert!(ExplainAnalyzer::has_full_table_scan(&plan));

        let plan = PlanNode::new(PlanNodeType::IndexScan, "scan".to_string(), None, None);
        assert!(!ExplainAnalyzer::has_full_table_scan(&plan));
    }

    /// 外部源扫描不算「本地全表扫描」：Oracle 表函数不该触发「加索引」提示。
    #[test]
    fn test_federated_scan_is_not_local_full_table_scan() {
        let json = r#"[{"name":"ORACLE_QUERY","children":[],"extra_info":{}}]"#;
        let plan = ExplainAnalyzer::parse_json_plan(json).expect("解析");
        assert!(!ExplainAnalyzer::has_full_table_scan(&plan));
    }

    #[test]
    fn test_get_performance_suggestions() {
        let plan = PlanNode::new(
            PlanNodeType::SeqScan,
            "scan".to_string(),
            Some(2000000.0),
            None,
        );
        let suggestions = ExplainAnalyzer::get_performance_suggestions(&plan);

        assert!(suggestions.iter().any(|s| s.contains("全表扫描")));
        assert!(suggestions.iter().any(|s| s.contains("预估成本较高")));
    }

    #[test]
    fn test_format_plan() {
        let mut root = PlanNode::new(
            PlanNodeType::Projection,
            "PROJECTION".to_string(),
            None,
            None,
        );
        let child = PlanNode::new(PlanNodeType::SeqScan, "SEQ_SCAN".to_string(), None, None);
        root.add_child(child);

        let formatted = ExplainAnalyzer::format_plan(&root, 0);
        assert!(formatted.contains("PROJECTION"));
        assert!(formatted.contains("SEQ_SCAN"));
        assert!(formatted.contains("  SEQ_SCAN"), "子节点要缩进一层");
    }
}

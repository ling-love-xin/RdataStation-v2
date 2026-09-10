use duckdb::Connection;

use crate::core::error::{CommonError, CoreError};

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
    /// 节点描述
    pub description: String,
    /// 预估成本
    pub estimated_cost: Option<f64>,
    /// 预估行数
    pub estimated_rows: Option<u64>,
    /// 子节点
    pub children: Vec<PlanNode>,
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

    /// 获取最大深度。
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

/// 查询计划分析器
///
/// 负责 EXPLAIN 查询计划解析与分析。
///
/// # 输出格式
/// DuckDB 支持多种 EXPLAIN 输出格式：
/// - STANDARD: 标准文本格式
/// - OPTIMIZED: 优化后格式
/// - PHYSICAL: 物理执行计划
pub struct ExplainAnalyzer;

impl ExplainAnalyzer {
    /// 执行 EXPLAIN 查询并解析为结构化计划树。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `query`: 要分析的查询 SQL
    ///
    /// # 返回
    /// - `Ok(PlanNode)`: 根查询计划节点
    /// - `Err(CoreError)`: 执行失败
    pub fn analyze_query(conn: &Connection, query: &str) -> Result<PlanNode, CoreError> {
        let sql = Self::generate_explain_sql(query, None);
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

        let mut output_lines = Vec::new();
        while let Some(row) = rows.next().map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "获取 EXPLAIN 结果失败: {}",
                e
            )))
        })? {
            if let Ok(line) = row.get::<usize, String>(0) {
                output_lines.push(line);
            }
        }

        let full_output = output_lines.join("\n");
        tracing::debug!("[ExplainAnalyzer] EXPLAIN 输出:\n{}", full_output);

        let tree = Self::parse_explain_tree(&full_output);

        Ok(tree)
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

        suggestions
    }

    /// 生成 EXPLAIN 查询的 SQL 语句。
    ///
    /// # 参数
    /// - `query`: 要分析的查询 SQL
    /// - `format`: EXPLAIN 输出格式（可选）
    ///
    /// # 返回
    /// EXPLAIN SQL 语句
    pub fn generate_explain_sql(query: &str, format: Option<&str>) -> String {
        match format {
            Some(fmt) => format!("EXPLAIN ({}) {}", fmt, query),
            None => format!("EXPLAIN {}", query),
        }
    }

    /// 解析 EXPLAIN 输出为结构化数据（扁平列表）。
    ///
    /// # 参数
    /// - `explain_output`: EXPLAIN 输出文本
    ///
    /// # 返回
    /// 解析后的查询计划节点列表
    ///
    /// # 注意
    /// 简化解析实现，实际应使用更复杂的解析逻辑
    #[deprecated(since = "2.0.0", note = "使用 parse_explain_tree 替代")]
    pub fn parse_explain_output(explain_output: &str) -> Vec<PlanNode> {
        let mut nodes = Vec::new();
        let lines: Vec<&str> = explain_output.lines().collect();

        for line in lines {
            if let Some(node) = Self::parse_line(line) {
                nodes.push(node);
            }
        }

        nodes
    }

    /// 解析 EXPLAIN 输出为树形结构。
    ///
    /// # 参数
    /// - `explain_output`: EXPLAIN 输出文本
    ///
    /// # 返回
    /// 解析后的查询计划根节点
    fn parse_explain_tree(explain_output: &str) -> PlanNode {
        let lines: Vec<&str> = explain_output.lines().collect();
        if lines.is_empty() {
            return PlanNode::new(
                PlanNodeType::Other("empty".to_string()),
                "Empty EXPLAIN output".to_string(),
                None,
                None,
            );
        }

        // 使用栈来构建树结构
        let mut stack: Vec<(usize, PlanNode)> = Vec::new();

        for line in &lines {
            if line.trim().is_empty() {
                continue;
            }

            let indent = line.len() - line.trim_start().len();
            let node = Self::parse_line(line).unwrap_or_else(|| {
                PlanNode::new(
                    PlanNodeType::Other(line.trim().to_string()),
                    line.trim().to_string(),
                    None,
                    None,
                )
            });

            // 弹出栈中比当前缩进深的节点
            while let Some((stack_indent, _)) = stack.last() {
                if *stack_indent >= indent {
                    stack.pop();
                } else {
                    break;
                }
            }

            // 将当前节点作为最后一个子节点
            if let Some((_, parent)) = stack.last_mut() {
                parent.children.push(node);
            } else {
                // 如果是根节点，直接入栈
                stack.push((indent, node));
            }
        }

        // 栈底元素为根节点（或其子节点）
        if let Some((_, root)) = stack.into_iter().next() {
            root
        } else {
            PlanNode::new(
                PlanNodeType::Other("unknown".to_string()),
                "Unknown EXPLAIN output".to_string(),
                None,
                None,
            )
        }
    }

    /// 检查查询计划是否包含全表扫描。
    ///
    /// # 参数
    /// - `plan`: 查询计划
    ///
    /// # 返回
    /// true 表示包含全表扫描
    pub fn has_full_table_scan(plan: &PlanNode) -> bool {
        plan.contains_node_type(&PlanNodeType::SeqScan)
    }

    /// 检查查询计划是否包含嵌套循环连接。
    ///
    /// # 参数
    /// - `plan`: 查询计划
    ///
    /// # 返回
    /// true 表示包含嵌套循环连接
    pub fn has_nested_loop_join(plan: &PlanNode) -> bool {
        plan.contains_node_type(&PlanNodeType::NestedLoopJoin)
    }

    /// 内部：解析单行 EXPLAIN 输出。
    ///
    /// # 参数
    /// - `line`: EXPLAIN 输出行
    ///
    /// # 返回
    /// 解析后的 PlanNode（如果可解析）
    fn parse_line(line: &str) -> Option<PlanNode> {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            return None;
        }

        // 简单解析逻辑，实际应更复杂
        let node_type = if trimmed.contains("SEQ_SCAN") {
            PlanNodeType::SeqScan
        } else if trimmed.contains("INDEX_SCAN") {
            PlanNodeType::IndexScan
        } else if trimmed.contains("HASH_JOIN") {
            PlanNodeType::HashJoin
        } else if trimmed.contains("NESTED_LOOP_JOIN") {
            PlanNodeType::NestedLoopJoin
        } else if trimmed.contains("AGGREGATE") || trimmed.contains("GROUP_BY") {
            PlanNodeType::Aggregate
        } else if trimmed.contains("ORDER_BY") {
            PlanNodeType::OrderBy
        } else if trimmed.contains("FILTER") {
            PlanNodeType::Filter
        } else if trimmed.contains("PROJECTION") {
            PlanNodeType::Projection
        } else {
            PlanNodeType::Other(trimmed.to_string())
        };

        Some(PlanNode::new(node_type, trimmed.to_string(), None, None))
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
        let mut output = format!("{}{}: {}\n", indent_str, plan.node_type, plan.description);

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

    #[test]
    fn test_plan_node_type_display() {
        assert_eq!(format!("{}", PlanNodeType::SeqScan), "SeqScan");
        assert_eq!(format!("{}", PlanNodeType::HashJoin), "HashJoin");
        assert_eq!(
            format!("{}", PlanNodeType::Other("Custom".to_string())),
            "Custom"
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

    #[test]
    #[allow(deprecated)]
    fn test_parse_explain_output() {
        let output = "PROJECTION\n  SEQ_SCAN";
        let nodes = ExplainAnalyzer::parse_explain_output(output);
        assert!(!nodes.is_empty());
    }

    #[test]
    fn test_has_full_table_scan() {
        let plan = PlanNode::new(PlanNodeType::SeqScan, "scan".to_string(), None, None);
        assert!(ExplainAnalyzer::has_full_table_scan(&plan));

        let plan = PlanNode::new(PlanNodeType::IndexScan, "scan".to_string(), None, None);
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
        let mut root = PlanNode::new(PlanNodeType::Projection, "root".to_string(), None, None);
        let child = PlanNode::new(PlanNodeType::SeqScan, "scan".to_string(), None, None);
        root.add_child(child);

        let formatted = ExplainAnalyzer::format_plan(&root, 0);
        assert!(formatted.contains("Projection"));
        assert!(formatted.contains("SeqScan"));
    }
}

//! sqlglot 高级能力出口（0.11.1 能力释放，2026-11-08）
//!
//! 本模块把 sqlglot-rust 里**已验证可用但此前只有测试探针在用**的六项能力收成
//! 业务可调的公开 API，供编辑器 / 结果面板 / 设置项直接消费：
//!
//! | 能力 | 上游 API | 是否需要表目录 | 本模块出口 |
//! | --- | --- | --- | --- |
//! | 作用域分析 | `optimizer::scope_analysis::build_scope` | 否 | [`scope_report`] |
//! | 类型标注 | `optimizer::annotate_types::annotate_types` | 是（`MappingSchema`） | [`column_types`] |
//! | 列级血缘 | `optimizer::lineage::lineage_sql` | 是（`MappingSchema`） | [`lineage_of_column`] |
//! | AST 差异 | `diff::diff_sql` | 否 | [`diff_summaries`] |
//! | 谓词下推 | `optimizer::pushdown_predicates` | 否 | [`pushdown_sql`] |
//! | 本地计划 | `planner::plan` | 否 | [`plan_summary`] |
//!
//! 另有两个轻量出口：方言时间格式串转换 [`translate_time_format`]（上游
//! `dialects::time::format_time_dialect`）与目标方言兼容性预检
//! [`unsupported_for_target`]（上游 `validate_dialect_support`，0.11.1 新增）。
//!
//! ## 口径（与上游/产品一致的边界）
//!
//! - **作用域不需要目录**：`build_scope` 只读 AST，别名映射是解析结果而不是元数据查询，
//!   所以编辑器在没有连接时也能用（补全排序、hover 补充行都走它）。
//! - **类型标注 / 血缘需要目录**：两者都以 `MappingSchema` 为输入，缺目录时上游会给
//!   `Unknown` / 空图——本模块把"目录为空"当作**调用方没元数据**直接返回空，宁缺毋滥
//!   （与 `semantic::diagnose` 的无目录退化同口径）。
//! - **本地计划 ≠ 源库 EXPLAIN**：`plan` 只产出 sqlglot 自建的算子图（`Scan/Filter/Join/...`），
//!   DDL 语句上游直接报不支持（`Planner does not support Discriminant(..)`），本模块如实透传错误。
//! - **谓词下推**只做"把 WHERE 推入派生表/子查询"的等价改写，不做 `unnest` 改写
//!   （后者 NULL 语义与原 SQL 不等价，见 `editor-prototype-design.md` §7.4）。
//!
//! ## 偏好开关
//!
//! 编辑器侧是否启用这些能力由设置项决定（[`AnalysisPrefs`]）。偏好存在进程内原子量里，
//! 由装配层在设置变更时推入（见 `crates/app/src/main.rs` 的 sink 安装），
//! 读侧只做一次 `Relaxed` 载入——语言服务在打字路径上，不引入锁。

use std::sync::atomic::{AtomicBool, Ordering};

use sqlglot_rust::ast::{DataType, SelectItem, Statement, TableRef};
use sqlglot_rust::schema::MappingSchema;
use sqlglot_rust::optimizer::scope_analysis::Source;
use sqlglot_rust::{
    AstNode, ChangeAction, Dialect, LineageConfig, LineageNode, Scope, annotate_types, build_scope,
    diff_sql, format_time_dialect, generate, parse, plan, pushdown_predicates,
};

use super::engine::SqlDialect;
use super::refs::to_inner_dialect;

// ===========================================================================
// 偏好开关（设置项 → 原子量 → 语言服务读侧）
// ===========================================================================

/// 编辑器侧的能力开关（默认全开：能力已实测可用，默认给用户看到收益）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisPrefs {
    /// 打字期语义诊断（未知表 / 未知列）。关掉 = 只跑词法层诊断，省解析开销
    pub semantic_diagnostics: bool,
    /// 代码动作里的「谓词下推」改写
    pub predicate_pushdown: bool,
    /// hover 补一行「来源表」/「列类型」
    pub hover_scope: bool,
}

impl Default for AnalysisPrefs {
    fn default() -> Self {
        Self {
            semantic_diagnostics: true,
            predicate_pushdown: true,
            hover_scope: true,
        }
    }
}

static PREF_SEMANTIC_DIAGNOSTICS: AtomicBool = AtomicBool::new(true);
static PREF_PREDICATE_PUSHDOWN: AtomicBool = AtomicBool::new(true);
static PREF_HOVER_SCOPE: AtomicBool = AtomicBool::new(true);

/// 装配层推送偏好（设置变更时调用；可重复调用）
pub fn set_prefs(prefs: AnalysisPrefs) {
    PREF_SEMANTIC_DIAGNOSTICS.store(prefs.semantic_diagnostics, Ordering::Relaxed);
    PREF_PREDICATE_PUSHDOWN.store(prefs.predicate_pushdown, Ordering::Relaxed);
    PREF_HOVER_SCOPE.store(prefs.hover_scope, Ordering::Relaxed);
}

/// 读侧取当前偏好（打字路径：三次无锁原子读）
#[must_use]
pub fn prefs() -> AnalysisPrefs {
    AnalysisPrefs {
        semantic_diagnostics: PREF_SEMANTIC_DIAGNOSTICS.load(Ordering::Relaxed),
        predicate_pushdown: PREF_PREDICATE_PUSHDOWN.load(Ordering::Relaxed),
        hover_scope: PREF_HOVER_SCOPE.load(Ordering::Relaxed),
    }
}

// ===========================================================================
// 1. 作用域分析（不需要表目录）
// ===========================================================================

/// 一个来源（FROM / JOIN 里的一项）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSource {
    /// 语句里使用的名字（别名优先，与 `Scope::sources` 的键一致）
    pub alias: String,
    /// 真实表名（`schema.table` / `catalog.schema.table` 已拼好）；派生表为空
    pub table: String,
    /// 是否派生来源（子查询 / CTE）
    pub derived: bool,
}

/// 一条语句顶层作用域的摘要
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeReport {
    pub sources: Vec<ScopeSource>,
    /// 出现过的列引用，限定列写成 `t.col`，裸列写成 `col`
    pub columns: Vec<String>,
    /// 解析不到任何来源的列（`ColumnRef.table == None`）——语义诊断的同一判据
    pub unqualified_columns: Vec<String>,
    /// 引用外层作用域的列（相关子查询）
    pub external_columns: Vec<String>,
    /// 是否有相关引用
    pub is_correlated: bool,
    /// 派生表 / CTE / 子查询作用域个数（只计数，不展开）
    pub nested_scopes: usize,
}

/// 作用域分析：解析 → `build_scope` → 摘要
///
/// 只读 AST，**不需要表目录**（别名映射来自解析结果）。
pub fn scope_report(sql: &str, dialect: SqlDialect) -> Result<ScopeReport, String> {
    let statement = parse(sql, to_inner_dialect(dialect)).map_err(|e| e.to_string())?;
    Ok(summarize_scope(&build_scope(&statement)))
}

fn summarize_scope(scope: &Scope) -> ScopeReport {
    let mut report = ScopeReport::default();
    for (alias, source) in &scope.sources {
        let (table, derived) = match source {
            Source::Table(table) => (table_path(table), false),
            Source::Scope(_) => (String::new(), true),
        };
        report.sources.push(ScopeSource {
            alias: alias.clone(),
            table,
            derived,
        });
    }
    report
        .sources
        .sort_by(|a, b| (a.derived, &a.alias).cmp(&(b.derived, &b.alias)));

    for column in &scope.columns {
        report.columns.push(match &column.table {
            Some(table) => format!("{table}.{}", column.name),
            None => column.name.clone(),
        });
        if column.table.is_none() {
            report.unqualified_columns.push(column.name.clone());
        }
    }
    for column in &scope.external_columns {
        report.external_columns.push(match &column.table {
            Some(table) => format!("{table}.{}", column.name),
            None => column.name.clone(),
        });
    }
    report.is_correlated = scope.is_correlated;
    report.nested_scopes = scope.derived_table_scopes.len()
        + scope.cte_scopes.len()
        + scope.subquery_scopes.len()
        + scope.union_scopes.len();
    report
}

/// `TableRef` → 限定表名（`catalog.schema.name` 已出现的段拼接，空段跳过）
fn table_path(table: &TableRef) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(catalog) = &table.catalog {
        parts.push(catalog.as_str());
    }
    if let Some(schema) = &table.schema {
        parts.push(schema.as_str());
    }
    parts.push(table.name.as_str());
    parts.join(".")
}

// ===========================================================================
// 2. 类型标注（需要表目录）
// ===========================================================================

/// 调用方给的一张表（列名 + 驱动报的类型名）
///
/// **owned**：调用方的目录快照常要跨线程送进后台任务（hover 就这条路径），
/// 借用式结构在那里活不过 `'static`，所以这里直接收字符串。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypedTable {
    /// 限定表名（与语句里出现的形式一致即可：`t` / `s.t` / `c.s.t`）
    pub qualified_name: String,
    /// 列定义：`(列名, 类型名)`。类型名按 [`parse_type_name`] 归一
    pub columns: Vec<(String, String)>,
}

/// 一条投影列的实际类型（`annotate_types` 的结果）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedColumn {
    /// 输出列名（别名优先，退化为表达式文本）
    pub name: String,
    /// 类型标签（见 [`data_type_label`]）
    pub data_type: String,
}

/// 解析 SELECT → 组装 `MappingSchema` → 标注类型 → 取投影列类型
///
/// `tables` 为空 = 调用方没有元数据 → 返回空（宁缺毋滥）。
pub fn column_types(
    sql: &str,
    dialect: SqlDialect,
    tables: &[TypedTable],
) -> Result<Vec<TypedColumn>, String> {
    if tables.is_empty() {
        return Ok(Vec::new());
    }
    let inner = to_inner_dialect(dialect);
    let statement = parse(sql, inner).map_err(|e| e.to_string())?;
    let Statement::Select(select) = &statement else {
        return Ok(Vec::new());
    };
    let schema = mapping_schema(inner, tables)?;
    let annotations = annotate_types(&statement, &schema);

    Ok(select
        .columns
        .iter()
        .filter_map(|item| match item {
            SelectItem::Expr { expr, alias, .. } => {
                let data_type = annotations.get_type(expr)?;
                Some(TypedColumn {
                    // 没别名时用**表达式文本**（`amount` / `SUM(amount)`）：调用方拿它跟
                    // 悬停词 / 结果列名对得上；`{:?}` 是结构体调试串，用户看不懂也匹配不上
                    name: alias
                        .clone()
                        .unwrap_or_else(|| node_text(&AstNode::Expr(expr.clone()))),
                    data_type: data_type_label(data_type),
                })
            }
            // `*` / `t.*` 的展开要按目录，交给调用方（本模块只报表达式列）
            SelectItem::Wildcard | SelectItem::QualifiedWildcard { .. } => None,
        })
        .collect())
}

/// 列定义类型名 → `ast::DataType`（认不出的按 `Text` 兜底：标类型只用于展示）
fn parse_type_name(name: &str) -> DataType {
    let lower = name.trim().to_ascii_lowercase();
    // 去掉长度/精度括号：`decimal(12,2)` → `decimal`
    let base = lower.split('(').next().unwrap_or(&lower).trim();
    match base {
        "tinyint" => DataType::TinyInt,
        "smallint" => DataType::SmallInt,
        "int" | "integer" | "int4" => DataType::Int,
        "bigint" | "int8" | "long" => DataType::BigInt,
        "float" | "real" | "float4" => DataType::Float,
        "double" | "float8" | "double precision" => DataType::Double,
        "decimal" | "numeric" | "number" => DataType::Decimal {
            precision: None,
            scale: None,
        },
        "varchar" | "character varying" | "string" | "varchar2" => DataType::Varchar(None),
        "char" | "nchar" => DataType::Char(None),
        "text" | "clob" | "longtext" => DataType::Text,
        "boolean" | "bool" => DataType::Boolean,
        "date" => DataType::Date,
        "datetime" => DataType::DateTime,
        "timestamp" | "timestamptz" => DataType::Timestamp {
            precision: None,
            with_tz: base.ends_with("tz"),
        },
        "time" => DataType::Time { precision: None },
        "blob" | "bytea" | "binary" | "varbinary" => DataType::Blob,
        _ => DataType::Text,
    }
}

/// 类型 → 展示标签（只覆盖驱动常见类型，其余退化为 Debug 文本）
#[must_use]
pub fn data_type_label(data_type: &DataType) -> String {
    match data_type {
        DataType::TinyInt => "TINYINT".to_string(),
        DataType::SmallInt => "SMALLINT".to_string(),
        DataType::Int => "INT".to_string(),
        DataType::BigInt => "BIGINT".to_string(),
        DataType::Float | DataType::Real => "FLOAT".to_string(),
        DataType::Double => "DOUBLE".to_string(),
        DataType::Decimal { precision, scale } => match (precision, scale) {
            (Some(p), Some(s)) => format!("DECIMAL({p},{s})"),
            (Some(p), None) => format!("DECIMAL({p})"),
            _ => "DECIMAL".to_string(),
        },
        DataType::Varchar(length) => match length {
            Some(n) => format!("VARCHAR({n})"),
            None => "VARCHAR".to_string(),
        },
        DataType::Char(length) => match length {
            Some(n) => format!("CHAR({n})"),
            None => "CHAR".to_string(),
        },
        DataType::Text | DataType::String => "TEXT".to_string(),
        DataType::Boolean => "BOOLEAN".to_string(),
        DataType::Date => "DATE".to_string(),
        DataType::DateTime => "DATETIME".to_string(),
        DataType::Time { .. } => "TIME".to_string(),
        DataType::Timestamp { with_tz, .. } => {
            if *with_tz {
                "TIMESTAMPTZ".to_string()
            } else {
                "TIMESTAMP".to_string()
            }
        }
        DataType::Blob => "BLOB".to_string(),
        other => format!("{other:?}"),
    }
}

/// 调用方表目录 → `MappingSchema`
fn mapping_schema(dialect: Dialect, tables: &[TypedTable]) -> Result<MappingSchema, String> {
    let mut schema = MappingSchema::new(dialect);
    for table in tables {
        let path: Vec<&str> = table.qualified_name.split('.').collect();
        let columns: Vec<(String, DataType)> = table
            .columns
            .iter()
            .map(|(name, ty)| (name.clone(), parse_type_name(ty)))
            .collect();
        schema
            .replace_table(&path, columns)
            .map_err(|e| format!("表目录组装失败（{}）：{e}", table.qualified_name))?;
    }
    Ok(schema)
}

// ===========================================================================
// 3. 列级血缘（需要表目录）
// ===========================================================================

/// 血缘图的一跳（从输出列往源头走）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageStep {
    /// 节点名（`total` / `SUM(amount)` / `o.amount`）
    pub name: String,
    /// 所属来源（表名 / CTE 名；表达式节点为空）
    pub source: String,
    /// 深度（0 = 输出列本身）
    pub depth: usize,
}

/// 追一条输出列的列级血缘（`column` 是输出列名或别名）
///
/// `tables` 为空 → 返回空（没有元数据时上游只能给空图，报错反而更吵）。
pub fn lineage_of_column(
    sql: &str,
    dialect: SqlDialect,
    column: &str,
    tables: &[TypedTable],
) -> Result<Vec<LineageStep>, String> {
    if tables.is_empty() {
        return Ok(Vec::new());
    }
    let inner = to_inner_dialect(dialect);
    let schema = mapping_schema(inner, tables)?;
    let config = LineageConfig {
        dialect: inner,
        trim_qualifiers: true,
        sources: Default::default(),
    };
    let graph = sqlglot_rust::lineage_sql(column, sql, &schema, &config).map_err(|e| e.to_string())?;
    let mut steps = Vec::new();
    collect_lineage(&graph.node, &mut steps);
    Ok(steps)
}

fn collect_lineage(node: &LineageNode, out: &mut Vec<LineageStep>) {
    out.push(LineageStep {
        name: node.name.clone(),
        source: node.source_name.clone().unwrap_or_default(),
        depth: node.depth,
    });
    for child in &node.downstream {
        collect_lineage(child, out);
    }
}

// ===========================================================================
// 4. AST 差异
// ===========================================================================

/// 一处语句差异（只描述动作与位置，不丢 AST）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSummary {
    /// `插入` / `删除` / `移动` / `替换` / `保留`
    pub action: &'static str,
    /// 变动节点的文本（插入/删除/替换/移动给新文本，保留给原文）
    pub detail: String,
    /// 是否属于结构性改动（供调用方过滤噪音：`Keep` 一律 false）
    pub structural: bool,
}

/// 两条 SQL 的 AST 级差异（不含 `Keep` 之外的逐节点对比噪音：`Keep` 一并返回，调用方自筛）
pub fn diff_summaries(
    source_sql: &str,
    target_sql: &str,
    dialect: SqlDialect,
) -> Result<Vec<ChangeSummary>, String> {
    let changes =
        diff_sql(source_sql, target_sql, to_inner_dialect(dialect)).map_err(|e| e.to_string())?;
    Ok(changes
        .into_iter()
        .map(|change| match change {
            ChangeAction::Insert(node) => ChangeSummary {
                action: "插入",
                detail: node_text(&node),
                structural: true,
            },
            ChangeAction::Remove(node) => ChangeSummary {
                action: "删除",
                detail: node_text(&node),
                structural: true,
            },
            ChangeAction::Move(before, after) => ChangeSummary {
                action: "移动",
                detail: format!("{} → {}", node_text(&before), node_text(&after)),
                structural: true,
            },
            ChangeAction::Update(before, after) => ChangeSummary {
                action: "替换",
                detail: format!("{} → {}", node_text(&before), node_text(&after)),
                structural: true,
            },
            ChangeAction::Keep(before, _) => ChangeSummary {
                action: "保留",
                detail: node_text(&before),
                structural: false,
            },
        })
        .collect())
}

/// `AstNode` → 一行文本（表达式/投影项直接生成 SQL，语句整体生成）
fn node_text(node: &sqlglot_rust::AstNode) -> String {
    use sqlglot_rust::AstNode;
    match node {
        AstNode::Statement(statement) => generate(statement, Dialect::Ansi),
        AstNode::Expr(expr) => generate(&Statement::Expression(expr.clone()), Dialect::Ansi),
        AstNode::SelectItem(item) => match item {
            SelectItem::Expr { expr, alias, .. } => match alias {
                Some(alias) => format!("{} AS {alias}", generate(&Statement::Expression(expr.clone()), Dialect::Ansi)),
                None => generate(&Statement::Expression(expr.clone()), Dialect::Ansi),
            },
            SelectItem::Wildcard => "*".to_string(),
            SelectItem::QualifiedWildcard { table } => format!("{table}.*"),
        },
        other => format!("{other:?}"),
    }
}

// ===========================================================================
// 5. 谓词下推
// ===========================================================================

/// 把 WHERE 谓词下推进派生表/子查询，返回改写后的 SQL（原方言重新生成）
pub fn pushdown_sql(sql: &str, dialect: SqlDialect) -> Result<String, String> {
    let inner = to_inner_dialect(dialect);
    let statement = parse(sql, inner).map_err(|e| e.to_string())?;
    let rewritten: Statement = pushdown_predicates(statement);
    Ok(generate(&rewritten, inner))
}

// ===========================================================================
// 6. 本地计划
// ===========================================================================

/// 本地计划摘要（**不是**源库 EXPLAIN）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSummary {
    /// 算子个数
    pub step_count: usize,
    /// 算子图（mermaid，可直接贴进文档/UI）
    pub mermaid: String,
}

/// sqlglot 自建计划；DDL / 非查询语句上游直接报不支持，错误原样透传
pub fn plan_summary(sql: &str, dialect: SqlDialect) -> Result<PlanSummary, String> {
    let statement = parse(sql, to_inner_dialect(dialect)).map_err(|e| e.to_string())?;
    let plan = plan(&statement).map_err(|e| e.to_string())?;
    Ok(PlanSummary {
        step_count: plan.len(),
        mermaid: plan.to_mermaid(),
    })
}

// ===========================================================================
// 7. 方言时间格式串转换 / 目标方言兼容性预检
// ===========================================================================

/// 时间格式串在两种方言之间转换（如 MySQL `%Y-%m-%d` → Postgres `YYYY-MM-DD`）
#[must_use]
pub fn translate_time_format(format: &str, from: SqlDialect, to: SqlDialect) -> String {
    format_time_dialect(format, to_inner_dialect(from), to_inner_dialect(to))
}

/// 目标方言不支持的结构（空 = 可安全转译）
///
/// 上游 `validate_dialect_support`（0.11.1 新增）：转译前先问一句，比事后给用户
/// 一个语义变了的 SQL 好。
pub fn unsupported_for_target(sql: &str, target: SqlDialect) -> Result<Vec<String>, String> {
    let statement = parse(sql, to_inner_dialect(target)).map_err(|e| e.to_string())?;
    match sqlglot_rust::validate_dialect_support(&statement, to_inner_dialect(target)) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => Ok(vec![error.to_string()]),
    }
}

#[cfg(test)]
mod tests;

//! 台账候选探针：sqlglot-rust 高级能力的**真实行为**核验（离线，不连数据库）
//!
//! ## 为什么需要
//!
//! 原型设计 §7.4 的能力台账目前只核到「**签名 + 关键实现路径**」。但「签名存在」不等于「行为符合
//! 预期」——血缘树的形状、类型推断的覆盖度、差异的粒度、下推是否真改了结构，都不是文档承诺过的
//! 东西。这些能力将来要接「结果血缘 / 补全排序 / 结果对比 / 值查看器类型」，**先看真实输出，
//! 再决定接线**，否则就是拿猜测当设计。
//!
//! ## 为什么以「打印报告」为主
//!
//! 确切输出尚未被任何文档承诺。若现在把猜测写成断言，测试会「红」但没人知道是谁错。因此本文件：
//! - **打印**每个探针的真实输出（供回写台账、判断可用性）；
//! - 只断言**与语义无关的不变量**：不 panic、生成的 SQL **可再次解析**、明确的文档化边界。
//! 行为一旦确认为「产品需要」，再按结论提升为 `engine::sql::*` 的正式 API + 带断言的单测。
//!
//! ## 运行方式
//!
//! ```sh
//! cargo test -p rds-engine --test sqlglot_capabilities -j 2 -- --nocapture --test-threads=1
//! ```
//!
//! ## 边界
//!
//! `crates/engine` 是 sqlglot-rust 的**唯一接入点**（架构硬约束）。本探针位于 engine crate 自己的
//! 测试树内，用于决定「是否把某项能力提升为 `sql::*` 的公开 API」；**其它 crate 不得直接引 sqlglot**。

use sqlglot_rust::ast::{DataType, SelectItem};
use sqlglot_rust::optimizer::optimize;
use sqlglot_rust::optimizer::qualify_columns::qualify_columns;
use sqlglot_rust::optimizer::scope_analysis::{ColumnRef, Source, build_scope, find_all_in_scope};
use sqlglot_rust::optimizer::unnest_subqueries::unnest_subqueries;
use sqlglot_rust::schema::{MappingSchema, Schema};
use sqlglot_rust::{
    ChangeAction, Dialect, LineageConfig, LineageNode, Statement, annotate_types, diff_sql,
    generate, generate_pretty, lineage_sql, parse, parse_statements_with_comments, plan,
    pushdown_predicates, transpile, transpile_statements,
};

// 集成测试是独立 crate：本包库目标名是 `rds_engine`（包名 `rds-engine`），不是其它 crate 里的依赖别名
use rds_engine::{SqlDialect, SqlEngine};

// ═══════════════════════════════════════════════════════════════════════
// 探针 SQL（结构化、贴近真实报表写法）
// ═══════════════════════════════════════════════════════════════════════

const SCOPE_SQL: &str =
    "SELECT o.id, c.name FROM orders o JOIN customers c ON o.customer_id = c.id WHERE o.total > 100";

const LINEAGE_SQL: &str =
    "SELECT c.name AS customer_name, o.total * 2 AS doubled FROM orders o JOIN customers c ON o.customer_id = c.id";

const DIFF_BEFORE: &str = "SELECT id, name, total FROM orders WHERE status = 'paid'";
const DIFF_AFTER: &str =
    "SELECT id, name, total, created_at FROM orders WHERE status = 'shipped' AND total > 100";

const PUSHDOWN_SQL: &str =
    "SELECT * FROM (SELECT id, status, total FROM orders WHERE total > 100) t WHERE t.status = 'paid'";

const QUALIFY_SQL: &str =
    "SELECT id, name FROM orders JOIN customers ON orders.customer_id = customers.id WHERE total > 100";

const UNNEST_SQL: &str =
    "SELECT id, (SELECT MAX(total) FROM orders) AS peak FROM orders WHERE id IN (SELECT id FROM orders WHERE status = 'paid')";

const PLAN_SQL: &str =
    "SELECT c.name, COUNT(*) AS n FROM orders o JOIN customers c ON o.customer_id = c.id GROUP BY c.name";

// ═══════════════════════════════════════════════════════════════════════
// 辅助
// ═══════════════════════════════════════════════════════════════════════

fn banner(title: &str) {
    println!("\n════════ {title} ════════");
}

fn parse_ansi(sql: &str) -> Statement {
    parse(sql, Dialect::Ansi).expect("探针 SQL 自身必须能解析")
}

/// 生成的 SQL 必须能再次解析（与语义无关的硬不变量）
fn assert_reparseable(label: &str, sql: &str) {
    assert!(
        parse_statements_with_comments(sql, Dialect::Ansi).is_ok(),
        "{label} 的输出无法再次解析：{sql}"
    );
}

/// 探针 schema（orders / customers）——模拟「从 MetadataService 组装」
fn probe_schema() -> MappingSchema {
    let mut schema = MappingSchema::new(Dialect::Ansi);
    add_table(
        &mut schema,
        &["orders"],
        vec![
            ("id".to_string(), DataType::Int),
            ("customer_id".to_string(), DataType::Int),
            (
                "total".to_string(),
                DataType::Decimal {
                    precision: Some(12),
                    scale: Some(2),
                },
            ),
            ("status".to_string(), DataType::Varchar(Some(32))),
            ("created_at".to_string(), DataType::Varchar(Some(32))),
        ],
    );
    add_table(
        &mut schema,
        &["customers"],
        vec![
            ("id".to_string(), DataType::Int),
            ("name".to_string(), DataType::Varchar(Some(255))),
        ],
    );
    schema
}

fn add_table(schema: &mut MappingSchema, path: &[&str], columns: Vec<(String, DataType)>) {
    if let Err(err) = schema.add_table(path, columns) {
        panic!("add_table({path:?}) 失败：{err}");
    }
}

fn change_label(change: &ChangeAction) -> &'static str {
    match change {
        ChangeAction::Insert(_) => "Insert",
        ChangeAction::Remove(_) => "Remove",
        ChangeAction::Keep(..) => "Keep",
        ChangeAction::Move(..) => "Move",
        ChangeAction::Update(..) => "Update",
    }
}

fn print_lineage(node: &LineageNode, depth: usize) {
    println!(
        "{}- {} (source_name={:?}, 上游 {} 个)",
        "  ".repeat(depth),
        node.name,
        node.source_name,
        node.downstream.len()
    );
    for child in &node.downstream {
        print_lineage(child, depth + 1);
    }
}

/// 只取前 N 个字符，避免把 Debug 长串全打出来
fn clip(text: impl Into<String>, limit: usize) -> String {
    let text: String = text.into();
    if text.chars().count() <= limit {
        return text;
    }
    let head: String = text.chars().take(limit).collect();
    format!("{head}…")
}

// ═══════════════════════════════════════════════════════════════════════
// 1. 作用域分析 → 「当前语句引用了哪些表」
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_scope_analysis() {
    banner("1. 作用域分析 build_scope（用途：补全排序 / 权限预检 / 重建 SELECT）");
    let stmt = parse_ansi(SCOPE_SQL);
    let scope = build_scope(&stmt);

    println!("SQL: {SCOPE_SQL}");
    println!("scope_type: {:?}", scope.scope_type);
    assert!(
        !scope.source_names().is_empty(),
        "FROM 有表的语句必须至少解析出一个 source"
    );

    let mut names = scope.source_names();
    names.sort_unstable();
    println!("source_names（别名或表名，键）: {names:?}");

    let mut tables: Vec<String> = scope
        .sources
        .values()
        .filter_map(|source| match source {
            Source::Table(table) => Some(match &table.schema {
                Some(schema) => format!("{schema}.{}", table.name),
                None => table.name.clone(),
            }),
            Source::Scope(_) => None,
        })
        .collect();
    tables.sort_unstable();
    println!("真实表名（Source::Table）: {tables:?}");

    println!("直接列引用 {} 条：", scope.columns.len());
    for column in &scope.columns {
        println!("  - {:?}.{}", column.table, column.name);
    }

    let all = find_all_in_scope(&scope, &|_: &ColumnRef| true);
    let qualified = find_all_in_scope(&scope, &|column: &ColumnRef| column.table.is_some());
    println!(
        "find_all_in_scope：全量 {} 条，带表限定 {} 条",
        all.len(),
        qualified.len()
    );

    let mut selected: Vec<&String> = scope.selected_sources.keys().collect();
    selected.sort();
    println!("selected_sources（SELECT 实际引用）: {selected:?}");
    println!(
        "子作用域：derived={} subquery={} union={} cte={}；correlated={}",
        scope.derived_table_scopes.len(),
        scope.subquery_scopes.len(),
        scope.union_scopes.len(),
        scope.cte_scopes.len(),
        scope.is_correlated
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 2. 列级血缘 → 结果血缘 / 单元依赖图
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_column_lineage() {
    banner("2. 列级血缘 lineage_sql（用途：结果血缘 / 单元依赖图）");
    let schema = probe_schema();
    let config = LineageConfig::new(Dialect::Ansi);
    println!("SQL: {LINEAGE_SQL}");

    for column in ["customer_name", "doubled"] {
        match lineage_sql(column, LINEAGE_SQL, &schema, &config) {
            Ok(graph) => {
                println!("列 `{column}` 血缘树（保留原 SQL：{}）:", graph.sql.is_some());
                print_lineage(&graph.node, 1);
            }
            Err(err) => println!("列 `{column}` 血缘失败：{err}"),
        }
    }

    // 别名解析是否必要：不存在的列
    match lineage_sql("nope", LINEAGE_SQL, &schema, &config) {
        Ok(graph) => println!("不存在的列：仍返回图（根节点 {}）", graph.node.name),
        Err(err) => println!("不存在的列：报错 → {err}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 3. 类型标注 → 值查看器 / 图表轴 / 数字格式化
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_type_annotation() {
    banner("3. 类型标注 annotate_types（用途：值查看器 / 图表轴 / 数字格式化）");
    let schema = probe_schema();
    let sql = "SELECT o.id, c.name, o.total * 2 AS doubled, COUNT(*) AS n, 'x' AS tag \
               FROM orders o JOIN customers c ON o.customer_id = c.id \
               GROUP BY o.id, c.name, o.total";
    let stmt = parse_ansi(sql);
    let annotations = annotate_types(&stmt, &schema);

    println!("已标注节点数：{}", annotations.len());
    match &stmt {
        Statement::Select(select) => {
            for item in &select.columns {
                match item {
                    SelectItem::Expr { expr, alias, .. } => {
                        println!("  {:?} → {:?}", alias, annotations.get_type(expr));
                    }
                    other => println!("  （非表达式列）{}", clip(format!("{other:?}"), 60)),
                }
            }
        }
        other => panic!("探针 SQL 必须是 SELECT：{}", clip(format!("{other:?}"), 60)),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. AST 差异 → 结果集对比 / 笔记版本链
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_ast_diff() {
    banner("4. AST 差异 diff_sql（用途：结果集对比 / 笔记版本链）");
    print_diff("改列 + 改条件", DIFF_BEFORE, DIFF_AFTER);
    print_diff("同一份 SQL", DIFF_BEFORE, DIFF_BEFORE);
    print_diff("整条语句换型", "SELECT 1", "UPDATE t SET x = 1");
}

fn print_diff(label: &str, before: &str, after: &str) {
    let changes = diff_sql(before, after, Dialect::Ansi).expect("diff_sql");
    let labels: Vec<&str> = changes.iter().map(change_label).collect();

    let (mut insert, mut remove, mut update, mut moved, mut keep) = (0, 0, 0, 0, 0);
    for change in &changes {
        match change {
            ChangeAction::Insert(_) => insert += 1,
            ChangeAction::Remove(_) => remove += 1,
            ChangeAction::Update(..) => update += 1,
            ChangeAction::Move(..) => moved += 1,
            ChangeAction::Keep(..) => keep += 1,
        }
    }

    println!("\n--- {label} ---");
    println!("  before: {before}");
    println!("  after : {after}");
    println!(
        "  {} 条：Insert={insert} Remove={remove} Update={update} Move={moved} Keep={keep}",
        changes.len()
    );
    println!("  序列：{labels:?}");
    for change in changes.iter().take(2) {
        println!("  示例：{}", clip(format!("{change:?}"), 140));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. 谓词下推 → 本地加速通道的改写
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_pushdown_predicates() {
    banner("5. 谓词下推 pushdown_predicates（用途：本地加速通道改写）");
    println!("输入：{PUSHDOWN_SQL}");

    let pushed = pushdown_predicates(parse_ansi(PUSHDOWN_SQL));
    let out = generate(&pushed, Dialect::Ansi);
    println!("输出（紧凑）：{out}");
    println!("输出（美化）：\n{}", generate_pretty(&pushed, Dialect::Ansi));
    assert_reparseable("pushdown_predicates", &out);

    println!(
        "结构信号：子查询数 {}，WHERE 数 {}，是否仍含派生表限定符 `t.` = {}",
        out.matches("SELECT").count(),
        out.matches("WHERE").count(),
        out.contains("t.")
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 6. 列限定 / 子查询展开 / 优化管线
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_qualify_unnest_optimize() {
    banner("6. 列限定 qualify_columns / 子查询展开 unnest_subqueries / 优化管线 optimize");
    let schema = probe_schema();

    println!("① qualify_columns 输入：{QUALIFY_SQL}");
    let qualified = qualify_columns(parse_ansi(QUALIFY_SQL), &schema);
    let qualified_sql = generate(&qualified, Dialect::Ansi);
    println!("   输出：{qualified_sql}");
    assert_reparseable("qualify_columns", &qualified_sql);

    println!("② unnest_subqueries 输入：{UNNEST_SQL}");
    let unnested = unnest_subqueries(parse_ansi(UNNEST_SQL));
    let unnested_sql = generate(&unnested, Dialect::Ansi);
    println!("   输出：{unnested_sql}");
    assert_reparseable("unnest_subqueries", &unnested_sql);

    match optimize(parse_ansi(QUALIFY_SQL)) {
        Ok(optimized) => {
            let optimized_sql = generate(&optimized, Dialect::Ansi);
            println!("③ optimize 输出：{optimized_sql}");
            assert_reparseable("optimize", &optimized_sql);
        }
        Err(err) => println!("③ optimize 失败：{err}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 7. 本地计划 → 确认它**不是**源库 EXPLAIN
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_local_plan() {
    banner("7. 本地计划 plan（仅离线预览：**不是源库 EXPLAIN**）");
    println!("SQL: {PLAN_SQL}");

    match plan(&parse_ansi(PLAN_SQL)) {
        Ok(planned) => {
            println!("步骤数：{}", planned.steps().len());
            for (index, step) in planned.steps().iter().enumerate() {
                println!("  [{index}] {}", clip(format!("{step:?}"), 200));
            }
        }
        Err(err) => println!("plan 失败：{err}"),
    }

    let ddl = parse_ansi("CREATE TABLE t_demo (id INT, name VARCHAR(20))");
    match plan(&ddl) {
        Ok(planned) => {
            println!(
                "DDL 也有计划：{} 步（与源码文档预期不符，需回写台账）",
                planned.steps().len()
            );
        }
        Err(err) => println!("DDL 计划失败（符合源码文档预期）：{err}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 8. 方言转译：单条限制（架构 §12 #19 的实证）
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_transpile_multi_statement() {
    banner("8. 方言转译 transpile 的单条限制（架构 §12 #19 实证）");
    let single = "SELECT CAST(x AS INT) FROM t";
    let script = "SELECT 1; SELECT 2;";

    println!(
        "① transpile（单条）：{:?}",
        transpile(single, Dialect::Mysql, Dialect::Postgres)
    );

    match transpile(script, Dialect::Mysql, Dialect::Postgres) {
        Ok(out) => println!("② transpile（脚本）：**成功** → {out}（注意是否静默丢弃了第二条）"),
        Err(err) => println!("② transpile（脚本）：报错（符合预期）→ {err}"),
    }

    match transpile_statements(script, Dialect::Mysql, Dialect::Postgres) {
        Ok(list) => {
            println!("③ transpile_statements（脚本）：{} 条 → {list:?}", list.len());
            assert!(!list.is_empty(), "多语句版至少应产出 1 条");
        }
        Err(err) => panic!("transpile_statements 是多语句的文档化入口，不应失败：{err}"),
    }

    // 生产路径（engine 的封装）在脚本上的行为——B10 接线前必须知道
    println!(
        "④ 生产路径 SqlEngine::transpile（脚本）：{:?}",
        SqlEngine::transpile(script, SqlDialect::Mysql, SqlDialect::Postgres)
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 9. 格式化对注释的保真度（架构 §12 #3 的实证，走生产路径）
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn probe_format_comment_fidelity() {
    banner("9. 格式化注释保真度（生产路径 SqlEngine::format；架构 §12 #3 实证）");

    // （用例, 期望格式化后仍可解析）——解析失败的用例故意不可解析：它断言的是“原样返回”
    let cases = [
        ("前导注释", "-- 报表口径说明\nselect a, b from t where x = 1", true),
        ("行内注释", "select a, /* 保留? */ b from t", true),
        ("尾随注释", "select a from t -- 尾注释", true),
        ("多语句", "select 1; select 2;", true),
        ("解析失败", "select from where", false),
    ];

    for (label, sql, expect_parseable) in cases {
        let out = SqlEngine::format(sql, SqlDialect::Mysql);
        let marks_in = comment_marks(sql);
        let marks_out = comment_marks(&out);
        println!("\n--- {label} ---");
        println!("  输入：{sql}");
        println!("  输出：{out}");
        println!("  注释标记数：{marks_in} → {marks_out}");

        if expect_parseable {
            assert!(
                parse_statements_with_comments(&out, Dialect::Mysql).is_ok(),
                "格式化输出必须可再次解析：{out}"
            );
        } else {
            assert_eq!(out, sql, "解析失败时应**原样返回**，不得改写用户内容");
        }
    }

    // `#` 注释的方言改写：MySQL 目标 vs 非 MySQL 目标
    let hash = "# 口径\nselect a from t";
    println!("\n--- `#` 注释的方言改写 ---");
    println!("  输入：{hash}");
    println!("  目标 MySQL   ：{}", SqlEngine::format(hash, SqlDialect::Mysql));
    println!("  目标 Postgres：{}", SqlEngine::format(hash, SqlDialect::Postgres));
}

/// 注释标记计数（行注释 / 块注释 / MySQL `#`）
fn comment_marks(sql: &str) -> usize {
    sql.matches("--").count() + sql.matches("/*").count() + sql.matches('#').count()
}

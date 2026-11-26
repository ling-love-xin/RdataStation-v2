//! SQL 写范围判定（全局分析库写守卫的唯一分类口）。
//!
//! 设计见 `docs/architecture/analytics_resource/duckdb-ownership-m6-slimming.md` §6：
//! `rds_global` 在根连接上是 RW，但 user / process 会话只能读；写必须走 system 会话
//! 或资产动作。守卫在 `AnalysisSession::run_with_timeout` 入口调用本模块，不靠正则猜。
//!
//! 判定口径（保守）：
//! - `SELECT` / `WITH` / `EXPLAIN` / `SHOW` / `DESCRIBE`：读，放行；
//! - 写语句只看**目标表**：目标落在 `rds_global` 才拒；读 `rds_global` 的子查询不误伤；
//! - 目标不带 catalog / schema 时按会话当前默认 catalog 判（`USE rds_global` 之后的裸名就是全局）；
//! - `CREATE TEMP` 只落 temp schema，不算写全局；
//! - `ATTACH` / `DETACH` / `USE rds_global` / 含 `rds_global` 的 `SET search_path`：直接拒；
//! - 分类不了的 `Command`（`CALL` / `COPY` 到库文件 / `VACUUM` / `EXPORT` …）：默认拒，
//!   宁可在真实语句上收集误伤再放宽。

use sqlglot_rust::ast::{Statement, TableRef};
use sqlglot_rust::parse;

use super::engine::SqlDialect;
use super::refs::to_inner_dialect;

/// 全局分析库的挂载别名（与 `shared::duckdb_root::GLOBAL_ALIAS` 同值；rds-sql 不反向依赖 shared）。
const GLOBAL_CATALOG: &str = "rds_global";

/// 一条（或一批）SQL 的写范围报告。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SqlWriteReport {
    /// 目标表落在全局分析库 `rds_global`（含裸名 + 默认 catalog = rds_global）。
    pub writes_global: bool,
    /// 出现 `USE rds_global` / `ATTACH` / `DETACH` / 含全局的 `SET search_path`。
    pub touches_global_catalog: bool,
    /// 执行成功后应把会话默认 catalog 改成这个（`USE <name>`）。
    pub use_catalog: Option<String>,
    /// 无法安全分类的语句（原因为中文，直接进用户错误）。
    pub unclassifiable: Option<String>,
}

impl SqlWriteReport {
    /// user / process 会话是否必须拒绝这条 SQL。
    pub fn denied_for_global_reader(&self) -> bool {
        self.writes_global || self.touches_global_catalog || self.unclassifiable.is_some()
    }
}

/// 判定 `sql` 在默认 catalog 为 `default_catalog` 的会话里会写到哪里。
///
/// `default_catalog` 来自 `AnalysisSession` 的 `USE` 跟踪；解析失败 / 空 SQL 不会 panic。
pub fn write_report(sql: &str, dialect: SqlDialect, default_catalog: &str) -> SqlWriteReport {
    let mut report = SqlWriteReport::default();
    for statement in crate::split_statements(sql) {
        let text = statement.text(sql);
        if text.trim().is_empty() {
            continue;
        }
        classify_statement(text, dialect, default_catalog, &mut report);
        if report.denied_for_global_reader() {
            // 多语句里只要有一条越界就整批拒；不再继续解析后面的。
            break;
        }
    }
    report
}

fn classify_statement(
    text: &str,
    dialect: SqlDialect,
    default_catalog: &str,
    report: &mut SqlWriteReport,
) {
    let parsed = parse(text, to_inner_dialect(dialect));
    let Ok(statement) = parsed else {
        classify_command(text, report);
        return;
    };
    match statement {
        Statement::Select(_) | Statement::SetOperation(_) | Statement::Explain(_) => {}
        Statement::Transaction(_) => {}
        Statement::Use(use_stmt) => {
            let name = trim_identifier(&use_stmt.name);
            if name.eq_ignore_ascii_case(GLOBAL_CATALOG) {
                report.touches_global_catalog = true;
            } else {
                report.use_catalog = Some(name);
            }
        }
        Statement::Insert(stmt) => mark_write(report, &stmt.table, default_catalog, false),
        Statement::Update(stmt) => mark_write(report, &stmt.table, default_catalog, false),
        Statement::Delete(stmt) => mark_write(report, &stmt.table, default_catalog, false),
        Statement::Merge(stmt) => mark_write(report, &stmt.target, default_catalog, false),
        Statement::CreateTable(stmt) => {
            mark_write(report, &stmt.table, default_catalog, stmt.temporary)
        }
        Statement::DropTable(stmt) => mark_write(report, &stmt.table, default_catalog, false),
        Statement::AlterTable(stmt) => mark_write(report, &stmt.table, default_catalog, false),
        Statement::CreateView(stmt) => mark_write(report, &stmt.name, default_catalog, false),
        Statement::DropView(stmt) => mark_write(report, &stmt.name, default_catalog, false),
        Statement::Truncate(stmt) => mark_write(report, &stmt.table, default_catalog, false),
        Statement::Expression(_) => {}
        Statement::Command(command) => classify_command_parts(&command.kind, &command.body, report),
    }
}

fn mark_write(
    report: &mut SqlWriteReport,
    table: &TableRef,
    default_catalog: &str,
    temporary: bool,
) {
    if temporary {
        // TEMP 表落 temp schema，不写底座；但显式写 `rds_global.xxx` 仍然拒。
        if explicit_global(table) {
            report.writes_global = true;
        }
        return;
    }
    if target_is_global(table, default_catalog) {
        report.writes_global = true;
    }
}

fn target_is_global(table: &TableRef, default_catalog: &str) -> bool {
    if explicit_global(table) {
        return true;
    }
    let unqualified = table.catalog.is_none() && table.schema.is_none();
    unqualified && default_catalog.eq_ignore_ascii_case(GLOBAL_CATALOG)
}

fn explicit_global(table: &TableRef) -> bool {
    [table.catalog.as_deref(), table.schema.as_deref()]
        .into_iter()
        .flatten()
        .any(|name| trim_identifier(name).eq_ignore_ascii_case(GLOBAL_CATALOG))
}

fn classify_command(text: &str, report: &mut SqlWriteReport) {
    let trimmed = text.trim_start();
    let kind = trimmed
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches(';');
    let body = trimmed[kind.len()..].trim_start();
    classify_command_parts(kind, body, report);
}

fn classify_command_parts(kind: &str, body: &str, report: &mut SqlWriteReport) {
    let kind = kind.trim().to_ascii_uppercase();
    let body_lower = body.to_ascii_lowercase();
    match kind.as_str() {
        "SELECT" | "WITH" | "VALUES" | "TABLE" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" => {}
        "USE" => {
            let name = trim_identifier(body.trim().trim_end_matches(';'));
            if name.eq_ignore_ascii_case(GLOBAL_CATALOG) {
                report.touches_global_catalog = true;
            } else if !name.is_empty() {
                report.use_catalog = Some(name);
            }
        }
        "BEGIN" | "COMMIT" | "ROLLBACK" | "START" | "SAVEPOINT" | "RELEASE" => {}
        "SET" => {
            if body_lower.contains(GLOBAL_CATALOG) {
                report.touches_global_catalog = true;
            }
        }
        "ATTACH" | "DETACH" => report.touches_global_catalog = true,
        "COPY" => classify_copy(&body_lower, report),
        // 明确会改文件 / 会话之外状态，或分类不了：user / process 一律拒。
        "CALL" | "EXPORT" | "IMPORT" | "LOAD" | "INSTALL" | "VACUUM" | "CHECKPOINT" | "ANALYZE"
        | "PRAGMA" | "GRANT" | "REVOKE" | "COMMENT" | "CREATE" | "DROP" | "ALTER" | "TRUNCATE"
        | "INSERT" | "UPDATE" | "DELETE" | "MERGE" => {
            report.unclassifiable = Some(format!(
                "语句「{kind}」不在可安全分类的白名单里（全局写守卫默认拒绝）"
            ));
        }
        _ => {
            report.unclassifiable = Some(format!("语句「{kind}」无法分类（全局写守卫默认拒绝）"));
        }
    }
}

fn classify_copy(body_lower: &str, report: &mut SqlWriteReport) {
    // COPY ... TO：读表 / 查询，写外部文件；只拦「往库文件里写」。
    // COPY ... FROM：目标表写在 FROM 之前，目标含全局就拒。
    if let Some(index) = body_lower.find(" from ") {
        let target = body_lower[..index].trim();
        if target.contains(GLOBAL_CATALOG) {
            report.writes_global = true;
        }
    } else if body_lower.contains(".duckdb") || body_lower.contains("global.db") {
        report.unclassifiable =
            Some("COPY 目标是数据库文件（可能写入全局分析库），全局写守卫拒绝".to_string());
    }
}

fn trim_identifier(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('`')
        .trim_matches('\'')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(sql: &str, default_catalog: &str) -> SqlWriteReport {
        write_report(sql, SqlDialect::Duckdb, default_catalog)
    }

    #[test]
    fn reading_global_is_allowed() {
        let r = report("SELECT * FROM rds_global.sales", "main");
        assert!(!r.denied_for_global_reader());
        let r = report(
            "INSERT INTO analytics.local_copy SELECT * FROM rds_global.sales",
            "main",
        );
        assert!(!r.denied_for_global_reader(), "读全局不该误判为写全局");
    }

    #[test]
    fn writing_global_is_denied() {
        for sql in [
            "INSERT INTO rds_global.sales VALUES (1)",
            "UPDATE rds_global.sales SET x = 1",
            "DELETE FROM rds_global.sales",
            "CREATE TABLE rds_global.new_table AS SELECT 1",
            "DROP TABLE rds_global.sales",
            "ALTER TABLE rds_global.sales ADD COLUMN y INT",
            "TRUNCATE rds_global.sales",
        ] {
            let r = report(sql, "main");
            assert!(r.writes_global, "{sql}");
            assert!(r.denied_for_global_reader(), "{sql}");
        }
    }

    #[test]
    fn unqualified_write_uses_default_catalog() {
        assert!(report("CREATE TABLE t AS SELECT 1", "rds_global").writes_global);
        assert!(!report("CREATE TABLE t AS SELECT 1", "main").writes_global);
        assert!(!report("CREATE TABLE memory.t AS SELECT 1", "rds_global").writes_global);
        assert!(!report("CREATE TEMP TABLE t AS SELECT 1", "rds_global").writes_global);
    }

    #[test]
    fn catalog_switches_and_attach_are_denied() {
        assert!(report("USE rds_global", "main").touches_global_catalog);
        assert!(!report("USE analytics", "main").denied_for_global_reader());
        assert_eq!(
            report("USE analytics", "main").use_catalog.as_deref(),
            Some("analytics")
        );
        assert!(report("ATTACH 'x.duckdb' AS other", "main").touches_global_catalog);
        assert!(report("DETACH other", "main").touches_global_catalog);
        assert!(report("SET search_path = 'main,rds_global'", "main").touches_global_catalog);
    }

    #[test]
    fn copy_is_allowed_only_outside_database_files() {
        assert!(!report(
            "COPY memory.tmp_i_1 TO '/tmp/x.parquet' (FORMAT PARQUET)",
            "rds_global"
        )
        .denied_for_global_reader());
        assert!(report("COPY t TO '/tmp/analytics.duckdb'", "main")
            .unclassifiable
            .is_some());
        assert!(report("COPY rds_global.t FROM '/tmp/x.csv'", "main").writes_global);
    }

    #[test]
    fn unclassifiable_commands_are_denied() {
        assert!(report("CALL some_procedure()", "main")
            .unclassifiable
            .is_some());
        assert!(report("VACUUM", "main").unclassifiable.is_some());
        assert!(report("EXPORT DATABASE '/tmp/out'", "main")
            .unclassifiable
            .is_some());
    }
}

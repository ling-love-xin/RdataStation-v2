//! 表版本对比：schema 摘要 + 有界行级差异。
//!
//! 表资产的一版是 manifest + 不可变快照。对比先比 manifest 里的逻辑 schema 与
//! 行列数，再仅在两侧列结构可对齐时用 `EXCEPT ALL` 取差异行，避免把整表读进内存。
//! 输出沿用版本历史 diff 对话框的 unified patch，不另造一套展示协议。

use duckdb::types::Value;
use similar::TextDiff;

use crate::store::table_asset::{SnapshotState, TableSnapshot};
use crate::table_asset::manifest::{TableManifest, TableSchema};
use shared::error::{CommonError, CoreError};

/// 每侧最多列出的差异行（只影响展示，不影响版本链路）。
pub const ROW_DIFF_LIMIT: usize = 200;

/// 一次表版本对比的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDiffReport {
    /// 对话框顶部的一行摘要（列变化 / 行数 / 差异行）。
    pub summary: String,
    /// 统一 patch 文本，交给 `Diff` 只读查看器。
    pub patch: String,
}

/// 对比两版表 manifest 与快照。
///
/// `conn` 只在需要行级差异时使用；快照缺失或物理表不存在时给可读错误，
/// 不静默降级成“看起来没有差异”。
pub fn table_version_diff(
    conn: &duckdb::Connection,
    name: &str,
    old_version: i32,
    new_version: i32,
    old_manifest: &TableManifest,
    new_manifest: &TableManifest,
    old_snapshot: &TableSnapshot,
    new_snapshot: &TableSnapshot,
) -> Result<TableDiffReport, CoreError> {
    if old_manifest.manifest_id == new_manifest.manifest_id {
        return Err(CoreError::common(CommonError::General(
            "两版表的 manifest 完全相同，没有差异可看".to_string(),
        )));
    }

    let old_schema = parse_schema(old_manifest, old_version)?;
    let new_schema = parse_schema(new_manifest, new_version)?;
    let schema_stats = SchemaStats::between(&old_schema, &new_schema);
    let rows_comparable = rows_comparable(&old_schema, &new_schema);
    let mut old_doc = schema_doc(name, old_version, old_manifest, &old_schema);
    let mut new_doc = schema_doc(name, new_version, new_manifest, &new_schema);

    let mut row_stats = RowDiffStats::default();
    if rows_comparable {
        let old_rows = diff_rows(
            conn,
            old_snapshot,
            new_snapshot,
            &old_schema,
            RowSide::OldOnly,
            old_version,
            new_version,
        )?;
        let new_rows = diff_rows(
            conn,
            old_snapshot,
            new_snapshot,
            &old_schema,
            RowSide::NewOnly,
            old_version,
            new_version,
        )?;
        old_doc.push(format!(
            "# 仅在 v{old_version} 的差异行（最多 {ROW_DIFF_LIMIT} 行）"
        ));
        new_doc.push(format!(
            "# 仅在 v{new_version} 的差异行（最多 {ROW_DIFF_LIMIT} 行）"
        ));
        old_doc.extend(old_rows.rows.iter().cloned());
        new_doc.extend(new_rows.rows.iter().cloned());
        row_stats = RowDiffStats {
            old_only: old_rows.total,
            new_only: new_rows.total,
            truncated: old_rows.truncated || new_rows.truncated,
        };
    } else {
        old_doc.push("# 列结构不可对齐，未做行级差异".to_string());
        new_doc.push("# 列结构不可对齐，未做行级差异".to_string());
    }

    let patch = TextDiff::from_lines(&old_doc.join("\n"), &new_doc.join("\n"))
        .unified_diff()
        .context_radius(2)
        .header(
            &format!("a/{name}@v{old_version}"),
            &format!("b/{name}@v{new_version}"),
        )
        .to_string();

    Ok(TableDiffReport {
        summary: format_summary(
            &schema_stats,
            old_manifest,
            new_manifest,
            &row_stats,
            rows_comparable,
        ),
        patch,
    })
}

fn parse_schema(manifest: &TableManifest, version: i32) -> Result<TableSchema, CoreError> {
    serde_json::from_str(&manifest.schema_json).map_err(|error| {
        CoreError::common(CommonError::General(format!(
            "v{version} 的 manifest schema 无法解析：{error}"
        )))
    })
}

fn schema_doc(
    name: &str,
    version: i32,
    manifest: &TableManifest,
    schema: &TableSchema,
) -> Vec<String> {
    let mut lines = vec![
        format!("# 表 {name} · v{version}"),
        format!("# 行数 {}", manifest.row_count),
        format!("# 列结构（{} 列）", schema.columns.len()),
    ];
    for column in &schema.columns {
        lines.push(format!(
            "{} {} {}{}",
            column.name,
            column.data_type,
            if column.nullable { "NULL" } else { "NOT NULL" },
            column
                .default_value
                .as_deref()
                .map(|value| format!(" DEFAULT {value}"))
                .unwrap_or_default()
        ));
    }
    lines
}

fn rows_comparable(old: &TableSchema, new: &TableSchema) -> bool {
    old.columns.len() == new.columns.len()
        && old.columns.iter().zip(&new.columns).all(|(left, right)| {
            left.name.eq_ignore_ascii_case(&right.name)
                && left.data_type.eq_ignore_ascii_case(&right.data_type)
        })
}

#[derive(Debug, Default)]
struct RowDiffStats {
    old_only: usize,
    new_only: usize,
    truncated: bool,
}

#[derive(Debug)]
struct DiffRows {
    rows: Vec<String>,
    total: usize,
    truncated: bool,
}

#[derive(Clone, Copy)]
enum RowSide {
    OldOnly,
    NewOnly,
}

fn diff_rows(
    conn: &duckdb::Connection,
    old_snapshot: &TableSnapshot,
    new_snapshot: &TableSnapshot,
    schema: &TableSchema,
    side: RowSide,
    old_version: i32,
    new_version: i32,
) -> Result<DiffRows, CoreError> {
    ensure_snapshot_live(conn, old_snapshot, old_version)?;
    ensure_snapshot_live(conn, new_snapshot, new_version)?;

    let columns = schema
        .columns
        .iter()
        .map(|column| quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let (from, against) = match side {
        RowSide::OldOnly => (old_snapshot, new_snapshot),
        RowSide::NewOnly => (new_snapshot, old_snapshot),
    };
    let base_sql = format!(
        "SELECT {columns} FROM {} EXCEPT ALL SELECT {columns} FROM {}",
        qualified_table(from),
        qualified_table(against)
    );
    let count: i64 = conn
        .query_row(&format!("SELECT count(*) FROM ({base_sql})"), [], |row| {
            row.get(0)
        })
        .map_err(|error| {
            CoreError::common(CommonError::General(format!("对比行差异失败：{error}")))
        })?;
    let total = count.max(0) as usize;
    let sql = format!("{base_sql} LIMIT {ROW_DIFF_LIMIT}");
    let mut stmt = conn.prepare(&sql).map_err(|error| {
        CoreError::common(CommonError::General(format!("对比行差异失败：{error}")))
    })?;
    let column_count = stmt.column_count();
    let mut rows = stmt.query([]).map_err(|error| {
        CoreError::common(CommonError::General(format!("对比行差异失败：{error}")))
    })?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|error| {
        CoreError::common(CommonError::General(format!("读取差异行失败：{error}")))
    })? {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let value: Value = row.get(index).map_err(|error| {
                CoreError::common(CommonError::General(format!("读取差异行失败：{error}")))
            })?;
            values.push(render_value(&value));
        }
        out.push(values.join(" | "));
    }
    Ok(DiffRows {
        rows: out,
        total,
        truncated: total > ROW_DIFF_LIMIT,
    })
}

fn ensure_snapshot_live(
    conn: &duckdb::Connection,
    snapshot: &TableSnapshot,
    version: i32,
) -> Result<(), CoreError> {
    if snapshot.state == SnapshotState::Missing {
        return Err(CoreError::common(CommonError::General(format!(
            "v{version} 的快照已缺失，无法对比"
        ))));
    }
    conn.prepare(&format!(
        "SELECT * FROM {} LIMIT 0",
        qualified_table(snapshot)
    ))
    .map_err(|error| {
        CoreError::common(CommonError::General(format!(
            "v{version} 的快照物理表不存在，无法对比：{error}"
        )))
    })?;
    Ok(())
}

fn qualified_table(snapshot: &TableSnapshot) -> String {
    snapshot
        .physical_schema
        .split('.')
        .map(quote_ident)
        .chain(std::iter::once(quote_ident(&snapshot.physical_table)))
        .collect::<Vec<_>>()
        .join(".")
}

fn quote_ident(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

fn render_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        _ => shared::duckdb_value_text::display_text(value)
            .map(|text| {
                // 单行展示：只有换行会破坏 patch 的一行一行结构，才做 JSON 转义。
                if text.contains(['\n', '\r']) {
                    serde_json::to_string(&text).unwrap_or_else(|_| format!("{text:?}"))
                } else {
                    text
                }
            })
            .unwrap_or_else(|| format!("{value:?}")),
    }
}

#[derive(Debug, Default)]
struct SchemaStats {
    added: usize,
    removed: usize,
    changed: usize,
}

impl SchemaStats {
    fn between(old: &TableSchema, new: &TableSchema) -> Self {
        let old_map = old
            .columns
            .iter()
            .map(|column| (column.name.to_ascii_lowercase(), column))
            .collect::<std::collections::HashMap<_, _>>();
        let new_map = new
            .columns
            .iter()
            .map(|column| (column.name.to_ascii_lowercase(), column))
            .collect::<std::collections::HashMap<_, _>>();
        let added = new_map
            .keys()
            .filter(|name| !old_map.contains_key(*name))
            .count();
        let removed = old_map
            .keys()
            .filter(|name| !new_map.contains_key(*name))
            .count();
        let changed = old_map
            .iter()
            .filter(|(name, column)| {
                new_map.get(*name).is_some_and(|other| {
                    other.data_type != column.data_type
                        || other.nullable != column.nullable
                        || other.default_value != column.default_value
                })
            })
            .count();
        Self {
            added,
            removed,
            changed,
        }
    }
}

fn format_summary(
    schema: &SchemaStats,
    old_manifest: &TableManifest,
    new_manifest: &TableManifest,
    rows: &RowDiffStats,
    rows_comparable: bool,
) -> String {
    let mut text = format!(
        "列结构：{} → {} 列（+{} / -{} / ~{}）",
        old_manifest.column_count,
        new_manifest.column_count,
        schema.added,
        schema.removed,
        schema.changed
    );
    if rows_comparable {
        text.push_str(&format!(
            " · 行数：{} → {}（+{} / -{}）",
            old_manifest.row_count, new_manifest.row_count, rows.new_only, rows.old_only
        ));
        if rows.truncated {
            text.push_str(&format!(" · 差异行仅列前 {ROW_DIFF_LIMIT} 条"));
        }
    } else {
        text.push_str(&format!(
            " · 行数：{} → {}（列结构不可对齐，未做行级差异）",
            old_manifest.row_count, new_manifest.row_count
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::table_asset::SnapshotInstance;
    use crate::table_asset::manifest::{DataFingerprint, TableColumn};
    use chrono::Utc;

    fn schema(columns: Vec<TableColumn>) -> TableSchema {
        TableSchema::new(columns).expect("schema")
    }

    fn manifest(schema: &TableSchema, rows: i64) -> TableManifest {
        manifest_with(schema, rows, "sum")
    }

    fn manifest_with(schema: &TableSchema, rows: i64, sum: &str) -> TableManifest {
        TableManifest::build(
            schema,
            &DataFingerprint::new(rows, schema.columns.len() as i64, sum, "xor", None, None),
        )
        .expect("manifest")
    }

    fn snapshot(table: &str) -> TableSnapshot {
        TableSnapshot {
            snapshot_id: format!("snap_{table}"),
            manifest_id: String::new(),
            instance: SnapshotInstance::Project,
            physical_schema: "main".to_string(),
            physical_table: table.to_string(),
            state: SnapshotState::Live,
            created_at: Utc::now(),
            verified_at: None,
        }
    }

    #[test]
    fn table_diff_reports_schema_and_row_changes() {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE old_t (id BIGINT, name VARCHAR);
             INSERT INTO old_t VALUES (1, 'a'), (2, 'b');
             CREATE TABLE new_t (id BIGINT, name VARCHAR, extra VARCHAR);
             INSERT INTO new_t VALUES (1, 'a', NULL), (3, 'c', NULL);",
        )
        .expect("fixtures");

        let old_schema = schema(vec![
            TableColumn::new("id", "BIGINT", false, None, 0),
            TableColumn::new("name", "VARCHAR", true, None, 1),
        ]);
        let new_schema = schema(vec![
            TableColumn::new("id", "BIGINT", false, None, 0),
            TableColumn::new("name", "VARCHAR", true, None, 1),
            TableColumn::new("extra", "VARCHAR", true, None, 2),
        ]);
        let old_manifest = manifest(&old_schema, 2);
        let new_manifest = manifest(&new_schema, 2);

        // 列数不同，行不可对齐；仍必须给出 schema 变化与行数。
        let report = table_version_diff(
            &conn,
            "orders",
            1,
            2,
            &old_manifest,
            &new_manifest,
            &snapshot("old_t"),
            &snapshot("new_t"),
        )
        .expect("diff");
        assert!(
            report.summary.contains("+1"),
            "结构新增列要计数：{}",
            report.summary
        );
        assert!(report.summary.contains("未做行级差异"));
        assert!(report.patch.contains("+extra VARCHAR NULL"));
    }

    #[test]
    fn table_diff_lists_added_and_removed_rows_when_schema_aligns() {
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        conn.execute_batch(
            "CREATE TABLE old_t (id BIGINT, name VARCHAR);
             INSERT INTO old_t VALUES (1, 'a'), (2, 'b');
             CREATE TABLE new_t (id BIGINT, name VARCHAR);
             INSERT INTO new_t VALUES (1, 'a'), (3, 'c');",
        )
        .expect("fixtures");
        let schema = schema(vec![
            TableColumn::new("id", "BIGINT", false, None, 0),
            TableColumn::new("name", "VARCHAR", true, None, 1),
        ]);
        let old_manifest = manifest(&schema, 2);
        let new_manifest = manifest_with(&schema, 2, "sum2");

        let report = table_version_diff(
            &conn,
            "orders",
            1,
            2,
            &old_manifest,
            &new_manifest,
            &snapshot("old_t"),
            &snapshot("new_t"),
        )
        .expect("diff");
        assert_eq!(report.summary.lines().count(), 1);
        assert!(
            report.summary.contains("+1 / -1"),
            "行差异计数：{}",
            report.summary
        );
        assert!(report.patch.contains("-2"), "旧版独有行：{}", report.patch);
        assert!(report.patch.contains("+3"), "新版独有行：{}", report.patch);
    }

    #[test]
    fn identical_manifest_refuses_to_invent_a_diff() {
        let schema = schema(vec![TableColumn::new("id", "BIGINT", false, None, 0)]);
        let manifest = manifest(&schema, 1);
        let conn = duckdb::Connection::open_in_memory().expect("duckdb");
        assert!(
            table_version_diff(
                &conn,
                "orders",
                1,
                1,
                &manifest,
                &manifest,
                &snapshot("t"),
                &snapshot("t"),
            )
            .is_err()
        );
    }
}

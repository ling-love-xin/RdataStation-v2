use crate::core::error::CommonError;
use crate::core::error::CoreError;
use crate::core::get_connection_manager;
use crate::core::services::sql_service::{SqlExecuteOptions, SqlService};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SchemaInsightReport {
    pub schema_name: String,
    pub table_count: u32,
    pub total_columns: u32,
    pub fk_candidates: Vec<ForeignKeyCandidate>,
    pub type_mismatches: Vec<TypeMismatch>,
    pub orphan_tables: Vec<OrphanTable>,
    pub redundant_columns: Vec<RedundantColumn>,
    pub summary: String,
    pub health_score: f64,
    pub health_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableColumnInfo {
    pub table_name: String,
    pub column_name: String,
    pub data_type: String,
    pub is_nullable: String,
    pub column_key: String,
    pub ordinal_position: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ForeignKeyCandidate {
    pub source_table: String,
    pub source_column: String,
    pub target_table: String,
    pub target_column: String,
    pub confidence: String,
    pub naming_pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TypeMismatch {
    pub column_name: String,
    pub tables: Vec<TypeMismatchEntry>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TypeMismatchEntry {
    pub table_name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OrphanTable {
    pub table_name: String,
    pub column_count: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RedundantColumn {
    pub column_name: String,
    pub table_count: u32,
    pub tables: Vec<String>,
    pub suggestion: String,
}

fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''").replace('\\', "\\\\")
}

pub struct SchemaAnalyzer;

impl SchemaAnalyzer {
    pub async fn analyze(
        conn_id: String,
        database: &str,
        schema: &str,
    ) -> Result<SchemaInsightReport, CoreError> {
        let manager = get_connection_manager().clone();
        let service = SqlService::new(manager);

        let all_columns = Self::fetch_all_columns(&service, Some(conn_id.clone()), schema).await?;
        let all_tables =
            Self::fetch_all_tables(&service, Some(conn_id.clone()), database, schema).await?;

        let table_count = all_tables.len() as u32;
        let total_columns = all_columns.len() as u32;

        let fk_candidates = Self::infer_foreign_keys(&all_columns, &all_tables);
        let type_mismatches = Self::detect_type_mismatches(&all_columns);
        let orphan_tables = Self::detect_orphan_tables(&all_tables, &all_columns);
        let redundant_columns = Self::detect_redundant_columns(&all_columns);

        let (health_score, health_level, summary) = Self::compute_health(
            table_count,
            total_columns,
            &fk_candidates,
            &type_mismatches,
            &orphan_tables,
        );

        Ok(SchemaInsightReport {
            schema_name: schema.to_string(),
            table_count,
            total_columns,
            fk_candidates,
            type_mismatches,
            orphan_tables,
            redundant_columns,
            summary,
            health_score,
            health_level,
        })
    }

    async fn fetch_all_tables(
        service: &SqlService,
        conn_id: Option<String>,
        database: &str,
        schema: &str,
    ) -> Result<Vec<String>, CoreError> {
        let escaped_schema = escape_sql_string(schema);
        let escaped_database = escape_sql_string(database);
        let sql = format!(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = '{}' AND table_catalog = '{}' \
             ORDER BY table_name",
            escaped_schema, escaped_database
        );

        let opts = SqlExecuteOptions {
            record_history: false,
            use_transaction: false,
            timeout_ms: Some(10000),
            use_cache: false,
        };

        let result = service.execute(conn_id, &sql, opts).await?;
        let json = serde_json::to_value(&result.result).map_err(|e| {
            CoreError::common(CommonError::General(format!("Serialize error: {}", e)))
        })?;

        let tables = Self::get_batch_rows(&json)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        row.as_array()
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(tables)
    }

    async fn fetch_all_columns(
        service: &SqlService,
        conn_id: Option<String>,
        schema: &str,
    ) -> Result<Vec<TableColumnInfo>, CoreError> {
        let escaped_schema = escape_sql_string(schema);
        let sql = format!(
            "SELECT table_name, column_name, data_type, is_nullable, \
             COALESCE(column_key, '') as column_key, ordinal_position \
             FROM information_schema.columns \
             WHERE table_schema = '{}' \
             ORDER BY table_name, ordinal_position",
            escaped_schema
        );

        let opts = SqlExecuteOptions {
            record_history: false,
            use_transaction: false,
            timeout_ms: Some(15000),
            use_cache: false,
        };

        let result = service.execute(conn_id, &sql, opts).await?;
        let json = serde_json::to_value(&result.result).map_err(|e| {
            CoreError::common(CommonError::General(format!("Serialize error: {}", e)))
        })?;

        let (col_names, rows) = Self::parse_batch_schema(&json);
        let col_idx = |name: &str| -> Option<usize> { col_names.iter().position(|c| c == name) };

        let columns: Vec<TableColumnInfo> = rows
            .iter()
            .filter_map(|row| {
                let table = row.get(col_idx("table_name")?)?.as_str()?.to_string();
                let column = row.get(col_idx("column_name")?)?.as_str()?.to_string();
                let dtype = row.get(col_idx("data_type")?)?.as_str()?.to_string();
                let nullable = row
                    .get(col_idx("is_nullable")?)
                    .and_then(|v| v.as_str())
                    .unwrap_or("YES")
                    .to_string();
                let key = row
                    .get(col_idx("column_key")?)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let ord = row
                    .get(col_idx("ordinal_position")?)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;

                Some(TableColumnInfo {
                    table_name: table,
                    column_name: column,
                    data_type: dtype,
                    is_nullable: nullable,
                    column_key: key,
                    ordinal_position: ord,
                })
            })
            .collect();

        Ok(columns)
    }

    fn find_compound_fk_target(
        base_prefix: &str,
        table_set: &std::collections::HashSet<&str>,
    ) -> Option<(String, String)> {
        let parts: Vec<&str> = base_prefix.split('_').collect();

        for i in 0..parts.len() {
            let candidate_prefix = parts[i..].join("_");
            let plural = format!("{}s", candidate_prefix);

            let target = if table_set.contains(plural.as_str()) {
                plural
            } else if table_set.contains(candidate_prefix.as_str()) {
                candidate_prefix
            } else {
                continue;
            };

            return Some((target.clone(), target));
        }

        None
    }

    fn infer_foreign_keys(
        columns: &[TableColumnInfo],
        tables: &[String],
    ) -> Vec<ForeignKeyCandidate> {
        let mut candidates = Vec::new();
        let table_set: std::collections::HashSet<&str> =
            tables.iter().map(|t| t.as_str()).collect();

        let fk_patterns: &[(&str, &str)] = &[
            ("_id$", "id"),
            ("_key$", "key"),
            ("_ref$", "ref"),
            ("_uuid$", "uuid"),
        ];

        for col in columns {
            if col.column_name == "id" || col.column_name == "key" {
                continue;
            }

            for (suffix, target_col) in fk_patterns {
                if let Some(prefix_end) = col.column_name.strip_suffix(&suffix[1..suffix.len() - 1])
                {
                    let base_prefix = prefix_end.strip_suffix('_').unwrap_or(prefix_end);

                    if let Some((_prefix, matched_table)) =
                        Self::find_compound_fk_target(base_prefix, &table_set)
                    {
                        let target_column = *target_col;

                        let confidence = if col.column_key == "MUL" {
                            "high"
                        } else if col.column_name.ends_with("_id") {
                            "medium"
                        } else {
                            "low"
                        };

                        candidates.push(ForeignKeyCandidate {
                            source_table: col.table_name.clone(),
                            source_column: col.column_name.clone(),
                            target_table: matched_table.clone(),
                            target_column: target_column.to_string(),
                            confidence: confidence.into(),
                            naming_pattern: format!("{} → {}", col.column_name, matched_table),
                        });
                    }
                    break;
                }
            }
        }

        candidates.sort_by_key(|c| match c.confidence.as_str() {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        });

        candidates
    }

    fn detect_type_mismatches(columns: &[TableColumnInfo]) -> Vec<TypeMismatch> {
        use std::collections::HashMap;

        let mut by_name: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for col in columns {
            by_name
                .entry(col.column_name.as_str())
                .or_default()
                .push((col.table_name.as_str(), col.data_type.as_str()));
        }

        let mut mismatches = Vec::new();
        for (col_name, entries) in &by_name {
            if entries.len() < 2 {
                continue;
            }

            let base_type = entries[0].1.to_lowercase();
            let has_diff = entries.iter().any(|(_, dt)| dt.to_lowercase() != base_type);

            if has_diff {
                let type_map: HashMap<&str, &str> =
                    entries.iter().map(|(t, dt)| (*t, *dt)).collect();
                let unique_types: std::collections::HashSet<_> = type_map.values().collect();

                let severity = if unique_types.len() >= 3 {
                    "critical"
                } else if unique_types.len() == 2 {
                    "warning"
                } else {
                    "info"
                };

                mismatches.push(TypeMismatch {
                    column_name: col_name.to_string(),
                    tables: type_map
                        .iter()
                        .map(|(t, dt)| TypeMismatchEntry {
                            table_name: t.to_string(),
                            data_type: dt.to_string(),
                        })
                        .collect(),
                    severity: severity.into(),
                });
            }
        }

        mismatches.sort_by_key(|m| match m.severity.as_str() {
            "critical" => 0,
            "warning" => 1,
            _ => 2,
        });

        mismatches
    }

    fn detect_orphan_tables(tables: &[String], columns: &[TableColumnInfo]) -> Vec<OrphanTable> {
        use std::collections::HashSet;

        let table_set: HashSet<&str> = tables.iter().map(|t| t.as_str()).collect();

        let mut refs_from: HashSet<&str> = HashSet::new();
        for col in columns {
            for suffix in &["_id", "_key", "_ref", "_uuid"] {
                if let Some(prefix) = col.column_name.strip_suffix(suffix) {
                    let target_singular = prefix.strip_suffix('_').unwrap_or(prefix);
                    let plural = format!("{}s", target_singular);
                    if table_set.contains(plural.as_str()) {
                        refs_from.insert(col.table_name.as_str());
                        break;
                    }
                }
            }
        }

        let mut orphans: Vec<OrphanTable> = tables
            .iter()
            .filter(|t| !refs_from.contains(t.as_str()))
            .map(|t| {
                let col_count = columns.iter().filter(|c| c.table_name == *t).count();
                OrphanTable {
                    table_name: t.to_string(),
                    column_count: col_count as u32,
                    reason: if col_count <= 2 {
                        "列数少，可能为配置表".into()
                    } else {
                        "未检测到外键引用关系".into()
                    },
                }
            })
            .collect();

        orphans.sort_by_key(|o| o.column_count);
        orphans
    }

    fn detect_redundant_columns(columns: &[TableColumnInfo]) -> Vec<RedundantColumn> {
        use std::collections::HashMap;

        let mut by_name: HashMap<&str, Vec<&str>> = HashMap::new();
        for col in columns {
            by_name
                .entry(col.column_name.as_str())
                .or_default()
                .push(col.table_name.as_str());
        }

        let mut redundant: Vec<RedundantColumn> = by_name
            .iter()
            .filter(|(_, tables)| tables.len() >= 3)
            .filter(|(name, _)| {
                let n = **name;
                n == "created_at"
                    || n == "updated_at"
                    || n == "deleted_at"
                    || n == "created_by"
                    || n == "updated_by"
                    || n == "status"
                    || n == "is_active"
                    || n == "is_deleted"
                    || n.ends_with("_at")
                    || n.ends_with("_by")
            })
            .map(|(name, tables)| RedundantColumn {
                column_name: (*name).to_string(),
                table_count: tables.len() as u32,
                tables: tables.iter().map(|t| t.to_string()).collect(),
                suggestion: if tables.len() >= 5 {
                    format!(
                        "\"{}\" 出现在 {} 张表中，考虑使用审计表统一管理",
                        name,
                        tables.len()
                    )
                } else {
                    format!("\"{}\" 出现在 {} 张表中，可考虑规范化", name, tables.len())
                },
            })
            .collect();

        redundant.sort_by_key(|r| -(r.table_count as i32));
        redundant
    }

    fn compute_health(
        table_count: u32,
        total_columns: u32,
        fk_candidates: &[ForeignKeyCandidate],
        type_mismatches: &[TypeMismatch],
        orphan_tables: &[OrphanTable],
    ) -> (f64, String, String) {
        if table_count == 0 {
            return (0.0, "空Schema".into(), "Schema 中无表".into());
        }

        let mut score = 80.0f64;

        let high_conf_fks = fk_candidates
            .iter()
            .filter(|f| f.confidence == "high")
            .count();
        if table_count > 1 && high_conf_fks == 0 {
            score -= 15.0;
        } else if high_conf_fks > 0 {
            score += (high_conf_fks as f64).min(15.0);
        }

        let critical_mismatches = type_mismatches
            .iter()
            .filter(|m| m.severity == "critical")
            .count();
        if critical_mismatches > 0 {
            score -= critical_mismatches as f64 * 5.0;
        }
        let warning_mismatches = type_mismatches
            .iter()
            .filter(|m| m.severity == "warning")
            .count();
        score -= warning_mismatches as f64 * 2.0;

        let orphan_ratio = if table_count > 0 {
            orphan_tables.len() as f64 / table_count as f64
        } else {
            0.0
        };
        if orphan_ratio > 0.5 {
            score -= 20.0;
        } else if orphan_ratio > 0.3 {
            score -= 10.0;
        } else if orphan_ratio > 0.1 {
            score -= 5.0;
        }

        score = score.clamp(0.0, 100.0);

        let level = if score >= 85.0 {
            "优秀"
        } else if score >= 70.0 {
            "良好"
        } else if score >= 50.0 {
            "需改进"
        } else if score >= 30.0 {
            "较差"
        } else {
            "差"
        };

        let mut parts = vec![format!("{} 张表, {} 个列", table_count, total_columns)];

        let fk_count = fk_candidates.len();
        if fk_count > 0 {
            parts.push(format!(
                "{} 个外键候选 (高置信 {})",
                fk_count, high_conf_fks
            ));
        } else if table_count > 1 {
            parts.push("未检测到外键关系".into());
        }

        if !type_mismatches.is_empty() {
            parts.push(format!("{} 个类型不一致", type_mismatches.len()));
        }

        if !orphan_tables.is_empty() {
            parts.push(format!("{} 个孤立表", orphan_tables.len()));
        }

        let summary = format!(
            "Schema健康评分 {:.0} ({})。{}",
            score,
            level,
            parts.join("；")
        );

        (score, level.into(), summary)
    }

    fn get_batch_rows(json: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
        json["batches"]
            .as_array()
            .and_then(|batches| batches.first())
            .and_then(|batch| batch["rows"].as_array())
    }

    fn parse_batch_schema(json: &serde_json::Value) -> (Vec<String>, Vec<Vec<serde_json::Value>>) {
        match json["batches"].as_array().and_then(|b| b.first()) {
            Some(batch) => {
                let cols: Vec<String> = batch["columns"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| c.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let rows: Vec<Vec<serde_json::Value>> = batch["rows"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .map(|row| row.as_array().cloned().unwrap_or_default())
                            .collect()
                    })
                    .unwrap_or_default();

                (cols, rows)
            }
            None => (vec![], vec![]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_col(table: &str, column: &str, dtype: &str, key: &str) -> TableColumnInfo {
        TableColumnInfo {
            table_name: table.into(),
            column_name: column.into(),
            data_type: dtype.into(),
            is_nullable: "YES".into(),
            column_key: key.into(),
            ordinal_position: 1,
        }
    }

    #[test]
    fn test_infer_fk_user_id_to_users() {
        let cols = vec![
            make_col("orders", "id", "int", "PRI"),
            make_col("orders", "user_id", "int", "MUL"),
            make_col("users", "id", "int", "PRI"),
        ];
        let tables: Vec<String> = vec!["orders".into(), "users".into()];
        let fks = SchemaAnalyzer::infer_foreign_keys(&cols, &tables);
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].source_table, "orders");
        assert_eq!(fks[0].source_column, "user_id");
        assert_eq!(fks[0].target_table, "users");
        assert_eq!(fks[0].target_column, "id");
        assert_eq!(fks[0].confidence, "high");
    }

    #[test]
    fn test_no_fk_on_id_column() {
        let cols = vec![make_col("t", "id", "int", "PRI")];
        let tables: Vec<String> = vec!["t".into()];
        let fks = SchemaAnalyzer::infer_foreign_keys(&cols, &tables);
        assert_eq!(fks.len(), 0);
    }

    #[test]
    fn test_infer_fk_compound_name() {
        let cols = vec![
            make_col("line_items", "id", "int", "PRI"),
            make_col("shipments", "order_line_item_id", "int", "MUL"),
        ];
        let tables: Vec<String> = vec!["shipments".into(), "line_items".into()];
        let fks = SchemaAnalyzer::infer_foreign_keys(&cols, &tables);
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].source_table, "shipments");
        assert_eq!(fks[0].source_column, "order_line_item_id");
        assert_eq!(fks[0].target_table, "line_items");
        assert_eq!(fks[0].confidence, "high");
    }

    #[test]
    fn test_infer_fk_compound_name_full_match_preferred() {
        let cols = vec![
            make_col("order_line_items", "id", "int", "PRI"),
            make_col("shipments", "order_line_item_id", "int", "MUL"),
            make_col("line_items", "id", "int", "PRI"),
        ];
        let tables: Vec<String> = vec![
            "order_line_items".into(),
            "shipments".into(),
            "line_items".into(),
        ];
        let fks = SchemaAnalyzer::infer_foreign_keys(&cols, &tables);
        assert_eq!(fks.len(), 1);
        assert_eq!(fks[0].target_table, "order_line_items");
    }

    #[test]
    fn test_detect_type_mismatch() {
        let cols = vec![
            make_col("a", "status", "int", ""),
            make_col("b", "status", "varchar", ""),
        ];
        let mismatches = SchemaAnalyzer::detect_type_mismatches(&cols);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].column_name, "status");
        assert_eq!(mismatches[0].severity, "warning");
    }

    #[test]
    fn test_no_mismatch_when_same_type() {
        let cols = vec![
            make_col("a", "id", "int", ""),
            make_col("b", "id", "int", ""),
        ];
        let mismatches = SchemaAnalyzer::detect_type_mismatches(&cols);
        assert_eq!(mismatches.len(), 0);
    }

    #[test]
    fn test_detect_critical_mismatch_three_types() {
        let cols = vec![
            make_col("a", "flag", "int", ""),
            make_col("b", "flag", "varchar", ""),
            make_col("c", "flag", "bool", ""),
        ];
        let mismatches = SchemaAnalyzer::detect_type_mismatches(&cols);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].severity, "critical");
    }

    #[test]
    fn test_detect_orphan_tables() {
        let cols = vec![
            make_col("orders", "id", "int", "PRI"),
            make_col("orders", "user_id", "int", "MUL"),
            make_col("users", "id", "int", "PRI"),
        ];
        let tables: Vec<String> = vec!["orders".into(), "users".into(), "logs".into()];
        let orphans = SchemaAnalyzer::detect_orphan_tables(&tables, &cols);
        assert_eq!(orphans.len(), 2); // logs (no columns) + users (no FK column to reference it)
    }

    #[test]
    fn test_detect_redundant_columns() {
        let cols = vec![
            make_col("a", "created_at", "timestamp", ""),
            make_col("b", "created_at", "timestamp", ""),
            make_col("c", "created_at", "timestamp", ""),
        ];
        let redundant = SchemaAnalyzer::detect_redundant_columns(&cols);
        assert_eq!(redundant.len(), 1);
        assert_eq!(redundant[0].column_name, "created_at");
        assert_eq!(redundant[0].table_count, 3);
    }

    #[test]
    fn test_no_redundant_if_less_than_3_tables() {
        let cols = vec![
            make_col("a", "created_at", "timestamp", ""),
            make_col("b", "created_at", "timestamp", ""),
        ];
        let redundant = SchemaAnalyzer::detect_redundant_columns(&cols);
        assert_eq!(redundant.len(), 0);
    }

    #[test]
    fn test_compute_health_perfect() {
        let fks: Vec<ForeignKeyCandidate> = vec![];
        let mismatches: Vec<TypeMismatch> = vec![];
        let orphans: Vec<OrphanTable> = vec![];
        let (score, _level, _) = SchemaAnalyzer::compute_health(1, 3, &fks, &mismatches, &orphans);
        assert!(score >= 70.0, "Expected score >= 70, got {}", score);
        assert!(score <= 100.0);
    }

    #[test]
    fn test_compute_health_with_critical_mismatches() {
        let fks: Vec<ForeignKeyCandidate> = vec![];
        let mismatches = vec![
            TypeMismatch {
                column_name: "x".into(),
                tables: vec![],
                severity: "critical".into(),
            },
            TypeMismatch {
                column_name: "y".into(),
                tables: vec![],
                severity: "critical".into(),
            },
        ];
        let orphans: Vec<OrphanTable> = vec![];
        let (score, _, _) = SchemaAnalyzer::compute_health(3, 10, &fks, &mismatches, &orphans);
        assert!(score < 70.0, "Expected score < 70, got {}", score);
    }

    #[test]
    fn test_compute_health_many_orphans() {
        let fks: Vec<ForeignKeyCandidate> = vec![];
        let mismatches: Vec<TypeMismatch> = vec![];
        let orphans = vec![
            OrphanTable {
                table_name: "a".into(),
                column_count: 3,
                reason: "x".into(),
            },
            OrphanTable {
                table_name: "b".into(),
                column_count: 3,
                reason: "x".into(),
            },
            OrphanTable {
                table_name: "c".into(),
                column_count: 3,
                reason: "x".into(),
            },
        ];
        let (score, _, _) = SchemaAnalyzer::compute_health(4, 12, &fks, &mismatches, &orphans);
        assert!(
            score < 70.0,
            "Expected score < 70 with many orphans, got {}",
            score
        );
    }

    #[test]
    fn test_escape_sql_string_no_change() {
        assert_eq!(escape_sql_string("my_schema"), "my_schema");
    }

    #[test]
    fn test_escape_sql_string_with_quote() {
        assert_eq!(escape_sql_string("it's_data"), "it''s_data");
    }

    #[test]
    fn test_escape_sql_string_with_backslash() {
        assert_eq!(escape_sql_string("path\\name"), "path\\\\name");
    }
}

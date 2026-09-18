use engine::driver::{ColumnDetail, DynDatabase};
use engine::get_connection_manager;
use shared::error::{CommonError, ConnectionError, CoreError};
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

impl TableColumnInfo {
    /// 驱动元数据（[`ColumnDetail`]）→ 报告用的列信息。
    ///
    /// 两处映射要知道：
    /// - `column_key` 不再是 MySQL 的 `COLUMN_KEY` 文本列，而是驱动算好的
    ///   **主键 / 外键标志**（`PRI` / `MUL`——保留这两个取值，下游的外键置信度分档不变）；
    /// - `ordinal_position` 取**列表下标**：驱动侧都按序数排好
    ///   （MySQL / PG 的查询带 `ORDER BY ordinal_position`，SQLite 的 `PRAGMA table_info` 本就按 `cid`）。
    pub fn from_detail(table: &str, index: usize, detail: &ColumnDetail) -> Self {
        let column_key = if detail.is_primary_key {
            "PRI"
        } else if detail.is_foreign_key {
            "MUL"
        } else {
            ""
        };
        Self {
            table_name: table.to_string(),
            column_name: detail.name.clone(),
            data_type: detail.data_type.clone(),
            is_nullable: if detail.nullable { "YES" } else { "NO" }.to_string(),
            column_key: column_key.to_string(),
            ordinal_position: (index + 1) as i32,
        }
    }
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

/// 结构洞察（M8）：从**驱动元数据**读 schema 结构，再算外键候选 / 类型不一致 / 孤立表 / 冗余列。
///
/// # 元数据只有一个来源
///
/// `information_schema` 的表 / 列写法各家不同（MySQL 用 `table_schema`、PG 用 catalog + schema、
/// SQLite 走 `PRAGMA table_info`、DuckDB 走 `duckdb_*`），但这些**已经写在各自驱动里**
/// （`MetadataBrowser`），左侧导航树 / 属性面板用的是同一套接口。洞察再抄一份方言 SQL，
/// 代价是三样：两套实现在同一边界上慢慢走偏（报告里的表 / 列数可能与树里不一致）、
/// 每个驱动特有的坑（MySQL 元数据列名全大写、`column_key` 是私有列）每个消费方各踩一遍、
/// 新驱动（或旧驱动的修复）两边都要改。
///
/// 所以本模块的边界与导航树一致：**能被导航树看见的对象**（有真正驱动 + 活连接）
/// 才能做结构洞察；草稿箱 / 分析存档那类文件源只做**数据**画像
/// （[`crate::SampleSource::on_duckdb`]），它们没有可报告的 schema 结构。
pub struct SchemaAnalyzer;

/// 元数据整段超时（导航树那种量级的库几十毫秒就回来；真卡住时不能让后台作业永远转圈）。
///
/// 逐条超时做不到：驱动接口（`get_tables` / `get_table_detail`）不走 `SqlService`，
/// 没有超时形参。
const METADATA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl SchemaAnalyzer {
    /// 报告标题：导航树给的 schema 优先（无独立 Schema 层的驱动会用 catalog 顶上），
    /// 两边都空的极端情况才退回一个占位串——不显示空标题。
    fn report_title(database: &str, schema: &str) -> String {
        let schema = schema.trim();
        let database = database.trim();
        if !schema.is_empty() {
            schema.to_string()
        } else if !database.is_empty() {
            database.to_string()
        } else {
            "（未命名）".to_string()
        }
    }

    /// 「名写错了」的统一文案：把可见取值一并列出，用户才知道该填什么
    fn unknown_target_error(kind: &str, name: &str, available: &[String]) -> CoreError {
        CoreError::common(CommonError::General(format!(
            "连接上看不到{kind} `{name}`（可见：{}）",
            available.join(" / ")
        )))
    }
    pub async fn analyze(
        conn_id: String,
        database: &str,
        schema: &str,
    ) -> Result<SchemaInsightReport, CoreError> {
        let manager = get_connection_manager().clone();
        let db = manager
            .get_connection(&conn_id)
            .await
            .ok_or_else(|| CoreError::connection(ConnectionError::not_found(&conn_id)))?;

        // 元数据取数（含「名写错了」的早失败）整段给一个上界
        let metadata = tokio::time::timeout(METADATA_TIMEOUT, async {
            Self::ensure_target_exists(&db, database, schema).await?;
            Self::fetch_metadata(&db, database, schema).await
        })
        .await
        .map_err(|_| {
            CoreError::common(CommonError::General(format!(
                "读取元数据超时（{} 秒）：连接无响应，可稍后重试",
                METADATA_TIMEOUT.as_secs()
            )))
        })??;

        let (tables, all_columns, unreadable) = metadata;
        let table_count = tables.len() as u32;
        let total_columns = all_columns.len() as u32;

        let fk_candidates = Self::infer_foreign_keys(&all_columns, &tables);
        let type_mismatches = Self::detect_type_mismatches(&all_columns);
        let orphan_tables = Self::detect_orphan_tables(&tables, &all_columns);
        let redundant_columns = Self::detect_redundant_columns(&all_columns);

        let (health_score, health_level, mut summary) = Self::compute_health(
            table_count,
            total_columns,
            &fk_candidates,
            &type_mismatches,
            &orphan_tables,
        );
        // 单表读列失败不毁整份报告，但也不能不说——否则用户看到的是一张「干净的」报告
        if !unreadable.is_empty() {
            summary.push_str(&format!(
                "；{} 张表未能读取列（不计入本报告）：{}",
                unreadable.len(),
                unreadable.join("、")
            ));
        }

        Ok(SchemaInsightReport {
            schema_name: Self::report_title(database, schema),
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

    /// 认不认得这个（库, schema）：**早失败**，而不是拿「零张表」冒充结论。
    ///
    /// 为什么需要：驱动层是按名过滤，名写错了不报错、只返回空列表——那会变成一张
    /// 「结构完美、其实什么都没看」的报告。有独立 Schema 层的驱动才校 schema：
    /// MySQL / SQLite / DuckDB 的 schema 是导航侧用 catalog 顶上的合成值
    /// （`has_schema_level` = false），拿它去比没有意义。
    async fn ensure_target_exists(
        db: &DynDatabase,
        database: &str,
        schema: &str,
    ) -> Result<(), CoreError> {
        let (catalogs, schema_level) = match db.as_metadata_browser() {
            Some(browser) => (
                browser
                    .get_catalogs()
                    .await?
                    .into_iter()
                    .map(|node| node.name)
                    .collect::<Vec<String>>(),
                browser.has_schema_level(),
            ),
            None => (db.list_catalogs().await?, true),
        };
        if !catalogs.is_empty() && !catalogs.iter().any(|c| c.eq_ignore_ascii_case(database)) {
            return Err(Self::unknown_target_error("库", database, &catalogs));
        }

        if schema_level && !schema.trim().is_empty() {
            let schemas: Vec<String> = match db.as_metadata_browser() {
                Some(browser) => browser
                    .get_schemas(database)
                    .await?
                    .into_iter()
                    .map(|node| node.name)
                    .collect(),
                None => db.list_schemas(database).await?,
            };
            if !schemas.is_empty() && !schemas.iter().any(|s| s.eq_ignore_ascii_case(schema)) {
                return Err(Self::unknown_target_error("schema", schema, &schemas));
            }
        }
        Ok(())
    }

    /// 表清单 → 逐表列（元数据只经过驱动接口这一条）。
    ///
    /// 第三项是**读不到列的表**：单张表报错（视图依赖缺失、权限不够、并发 DDL）不该把
    /// 整份报告弄没，但也不能静默咽下去。
    async fn fetch_metadata(
        db: &DynDatabase,
        database: &str,
        schema: &str,
    ) -> Result<(Vec<String>, Vec<TableColumnInfo>, Vec<String>), CoreError> {
        let browser = db.as_metadata_browser();
        let tables: Vec<String> = match browser {
            Some(browser) => browser
                .get_tables(database, schema)
                .await?
                .into_iter()
                .map(|node| node.name)
                .collect(),
            None => db
                .list_tables(database, Some(schema))
                .await?
                .into_iter()
                .map(|object| object.name)
                .collect(),
        };

        let mut columns: Vec<TableColumnInfo> = Vec::new();
        let mut unreadable: Vec<String> = Vec::new();
        for table in &tables {
            let details = match browser {
                Some(browser) => browser
                    .get_table_detail(database, schema, table)
                    .await
                    .map(|detail| detail.columns),
                None => db.list_columns(database, Some(schema), table).await,
            };
            match details {
                Ok(details) => columns.extend(
                    details
                        .iter()
                        .enumerate()
                        .map(|(index, detail)| TableColumnInfo::from_detail(table, index, detail)),
                ),
                Err(error) => {
                    tracing::warn!("[schema] 读取表 {table} 的列失败：{error}");
                    unreadable.push(table.clone());
                }
            }
        }
        Ok((tables, columns, unreadable))
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

        let level = crate::quality_scorer::Grade::of(score).label();

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

    /// 报告标题：导航树给的 schema 优先，schema 为空时用库名顶上（不显示空标题）。
    #[test]
    fn report_title_prefers_the_schema_then_the_database() {
        assert_eq!(SchemaAnalyzer::report_title("shop", "public"), "public");
        assert_eq!(
            SchemaAnalyzer::report_title("shop", ""),
            "shop",
            "无独立 Schema 层的驱动（MySQL / SQLite / DuckDB）会传空，用库名顶上"
        );
        assert_eq!(
            SchemaAnalyzer::report_title("shop", "shop"),
            "shop",
            "导航侧无层驱动会把 catalog 当 schema 传（同一个名字）"
        );
        assert_eq!(SchemaAnalyzer::report_title("", ""), "（未命名）");
    }

    /// 驱动元数据 → 报告列信息：类型 / 可空 / 主键外键角标 / 序数（取列表下标）。
    #[test]
    fn column_detail_maps_to_the_report_row() {
        let detail = |name: &str, dtype: &str, nullable: bool, pk: bool, fk: bool| ColumnDetail {
            name: name.into(),
            data_type: dtype.into(),
            nullable,
            is_primary_key: pk,
            is_foreign_key: fk,
            default_value: None,
            comment: None,
            extra: std::collections::HashMap::new(),
        };

        let pk = TableColumnInfo::from_detail("orders", 0, &detail("id", "int", false, true, false));
        assert_eq!(pk.column_name, "id");
        assert_eq!(pk.data_type, "int");
        assert_eq!(pk.is_nullable, "NO");
        assert_eq!(pk.column_key, "PRI", "主键角标（原来是 MySQL COLUMN_KEY）");
        assert_eq!(pk.ordinal_position, 1, "序数取列表下标 + 1");

        let fk = TableColumnInfo::from_detail("orders", 1, &detail("user_id", "int", true, false, true));
        assert_eq!(fk.column_key, "MUL", "外键给 MUL：外键置信度用它分档");
        assert_eq!(fk.is_nullable, "YES");

        let plain = TableColumnInfo::from_detail("orders", 2, &detail("note", "text", true, false, false));
        assert_eq!(plain.column_key, "");
        assert_eq!(plain.ordinal_position, 3);
        assert_eq!(plain.table_name, "orders");
    }

    /// 「名写错了」的文案：要把可见取值列出来，用户才知道该填什么。
    #[test]
    fn unknown_target_names_the_available_ones() {
        let error = SchemaAnalyzer::unknown_target_error(
            "schema",
            "pubilc",
            &["public".to_string(), "information_schema".to_string()],
        );
        let text = error.to_string();
        assert!(text.contains("pubilc"), "点出写错的那个名字：{text}");
        assert!(text.contains("public"), "列出可见取值：{text}");
    }
}

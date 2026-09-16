//! Schema 健康报告的**视图模型与导出**（M8 Phase 4）。
//!
//! # 职责边界
//!
//! 本文件只做两件事：把领域报告[`SchemaInsightReport`] 转成界面好渲染的分组行，
//! 以及把它导出成 JSON / Markdown。**不取数、不连库**（那是 `SchemaAnalyzer` 与
//! `InsightService::schema_report_view` 的事），因此全部可脱窗口单测。
//!
//! # 为什么导出放在视图模型上
//!
//! 导出的是「用户看到的这份结论」，而不是领域对象的 `Debug`：两者字段几乎一样，
//! 但分组的空态提示、严重度分级、表格归属都是**界面语义**。放在同一处就不至于
//! 出现「界面说 3 个孤立表、导出的 JSON 里 4 个」这类两套口径。

use std::fmt::Write as _;

use crate::quality_scorer::Grade;
use crate::schema_analyzer::{
    ForeignKeyCandidate, OrphanTable, RedundantColumn, SchemaInsightReport, TypeMismatch,
};

/// 报告的四个分区（顺序即渲染顺序）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaSection {
    ForeignKeys,
    TypeMismatches,
    OrphanTables,
    RedundantColumns,
}

impl SchemaSection {
    pub const ALL: [SchemaSection; 4] = [
        SchemaSection::ForeignKeys,
        SchemaSection::TypeMismatches,
        SchemaSection::OrphanTables,
        SchemaSection::RedundantColumns,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SchemaSection::ForeignKeys => "外键候选",
            SchemaSection::TypeMismatches => "类型不一致",
            SchemaSection::OrphanTables => "孤立表",
            SchemaSection::RedundantColumns => "冗余列",
        }
    }

    /// 空态提示：说清「空」是什么意思（没检测到 vs. 没问题）
    pub fn empty_hint(self) -> &'static str {
        match self {
            SchemaSection::ForeignKeys => "没有命名可推断的外键关系（不是问题，只是没能推断出）",
            SchemaSection::TypeMismatches => "同名列的声明类型在各表一致",
            SchemaSection::OrphanTables => "每张表都至少与另一张表有关联",
            SchemaSection::RedundantColumns => "没有在多数表里重复出现的列",
        }
    }

    /// 导出时的分组键（稳定英文，不拿中文展示名当键）
    pub fn key(self) -> &'static str {
        match self {
            SchemaSection::ForeignKeys => "foreign_key_candidates",
            SchemaSection::TypeMismatches => "type_mismatches",
            SchemaSection::OrphanTables => "orphan_tables",
            SchemaSection::RedundantColumns => "redundant_columns",
        }
    }
}

/// 行的语气（视图映射到主题角色，模型不持色值）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaTone {
    /// 中性信息（候选、建议）
    Normal,
    /// 提示（warning 级不一致 / 冗余列）
    Warning,
    /// 问题（critical 级不一致 / 孤立表）
    Danger,
}

/// 一行结论
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaRowView {
    /// 主文案（如 `orders.user_id → users.id`）
    pub title: String,
    /// 副文案（置信度 / 严重度 / 建议）
    pub detail: String,
    pub tone: SchemaTone,
    /// 涉及的表（非空时视图给下钻热点：点它跳到该表的表探查）
    pub tables: Vec<String>,
}

/// 一个分区的渲染数据
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaGroupView {
    pub section: SchemaSection,
    pub rows: Vec<SchemaRowView>,
}

impl SchemaGroupView {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Schema 健康报告视图模型
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaReportView {
    pub schema_name: String,
    pub table_count: u32,
    pub total_columns: u32,
    pub health: f64,
    pub grade: Grade,
    pub summary: String,
    pub groups: Vec<SchemaGroupView>,
}

impl SchemaReportView {
    pub fn group(&self, section: SchemaSection) -> Option<&SchemaGroupView> {
        self.groups.iter().find(|g| g.section == section)
    }

    /// 问题总数（用于「要不要担心」的一眼判断）
    pub fn issue_count(&self) -> usize {
        self.groups
            .iter()
            .map(|g| {
                g.rows
                    .iter()
                    .filter(|r| r.tone != SchemaTone::Normal)
                    .count()
            })
            .sum()
    }

    /// 领域报告 → 视图模型（纯函数）
    pub fn from_report(report: &SchemaInsightReport) -> Self {
        Self {
            schema_name: report.schema_name.clone(),
            table_count: report.table_count,
            total_columns: report.total_columns,
            health: report.health_score,
            // 等级按**分数现算**，不解析领域里的 `health_level` 字符串：
            // 阈值与文案只有 `quality_scorer` 一份，字符串只是它的一个投影
            grade: Grade::of(report.health_score),
            summary: report.summary.clone(),
            groups: vec![
                fk_group(&report.fk_candidates),
                mismatch_group(&report.type_mismatches),
                orphan_group(&report.orphan_tables),
                redundant_group(&report.redundant_columns),
            ],
        }
    }
}

fn fk_group(candidates: &[ForeignKeyCandidate]) -> SchemaGroupView {
    SchemaGroupView {
        section: SchemaSection::ForeignKeys,
        rows: candidates
            .iter()
            .map(|fk| SchemaRowView {
                title: format!(
                    "{}.{} → {}.{}",
                    fk.source_table, fk.source_column, fk.target_table, fk.target_column
                ),
                detail: format!(
                    "{} · 命名模式 {}",
                    confidence_label(&fk.confidence),
                    fk.naming_pattern
                ),
                // 候选不是问题：只是「看起来像外键」，按中性展示
                tone: SchemaTone::Normal,
                tables: vec![fk.source_table.clone(), fk.target_table.clone()],
            })
            .collect(),
    }
}

fn mismatch_group(mismatches: &[TypeMismatch]) -> SchemaGroupView {
    SchemaGroupView {
        section: SchemaSection::TypeMismatches,
        rows: mismatches
            .iter()
            .map(|mismatch| {
                let detail = mismatch
                    .tables
                    .iter()
                    .map(|entry| format!("{}: {}", entry.table_name, entry.data_type))
                    .collect::<Vec<_>>()
                    .join(" / ");
                SchemaRowView {
                    title: format!("列 {}", mismatch.column_name),
                    detail,
                    tone: if mismatch.severity == "critical" {
                        SchemaTone::Danger
                    } else {
                        SchemaTone::Warning
                    },
                    tables: mismatch
                        .tables
                        .iter()
                        .map(|entry| entry.table_name.clone())
                        .collect(),
                }
            })
            .collect(),
    }
}

fn orphan_group(orphans: &[OrphanTable]) -> SchemaGroupView {
    SchemaGroupView {
        section: SchemaSection::OrphanTables,
        rows: orphans
            .iter()
            .map(|orphan| SchemaRowView {
                title: orphan.table_name.clone(),
                detail: format!("{} 列 · {}", orphan.column_count, orphan.reason),
                // 孤立表是「没人引用、也不引用别人」：可能是漏了外键，也可能是真的独立
                tone: SchemaTone::Danger,
                tables: vec![orphan.table_name.clone()],
            })
            .collect(),
    }
}

fn redundant_group(columns: &[RedundantColumn]) -> SchemaGroupView {
    SchemaGroupView {
        section: SchemaSection::RedundantColumns,
        rows: columns
            .iter()
            .map(|column| SchemaRowView {
                title: format!("列 {}（{} 张表）", column.column_name, column.table_count),
                detail: if column.suggestion.trim().is_empty() {
                    column.tables.join("、")
                } else {
                    format!("{} · {}", column.suggestion, column.tables.join("、"))
                },
                tone: SchemaTone::Warning,
                tables: column.tables.clone(),
            })
            .collect(),
    }
}

/// 置信度中文（未知取值原样展示：规则/分析器加了新档时不至于显示成空白）
pub fn confidence_label(raw: &str) -> String {
    match raw {
        "high" => "高置信".to_string(),
        "medium" => "中置信".to_string(),
        "low" => "低置信".to_string(),
        other => other.to_string(),
    }
}

// ==================== 导出（纯函数） ====================

impl SchemaReportView {
    /// 导出 JSON（稳定结构：分组键取英文常量，便于比对与二次处理）
    pub fn to_json(&self) -> String {
        let groups: Vec<serde_json::Value> = self
            .groups
            .iter()
            .map(|group| {
                serde_json::json!({
                    "section": group.section.key(),
                    "title": group.section.title(),
                    "rows": group.rows.iter().map(|row| serde_json::json!({
                        "title": row.title,
                        "detail": row.detail,
                        "tone": tone_key(row.tone),
                        "tables": row.tables,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();

        let payload = serde_json::json!({
            "schema": self.schema_name,
            "health_score": self.health,
            "health_level": self.grade.label(),
            "summary": self.summary,
            "table_count": self.table_count,
            "column_count": self.total_columns,
            "issue_count": self.issue_count(),
            "groups": groups,
        });
        // 美化输出：导出的文件是给人看的（也能直接进 diff / 版本库）
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
    }

    /// 导出 Markdown（四个分区各一张表；空分区写明「空」的含义）
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "# Schema 健康报告 · {}", self.schema_name);
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "- 健康评分：**{:.0}**（{}）",
            self.health,
            self.grade.label()
        );
        let _ = writeln!(out, "- 规模：{} 张表 / {} 个列", self.table_count, self.total_columns);
        let _ = writeln!(out, "- 需关注：{} 项", self.issue_count());
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", self.summary);
        for group in &self.groups {
            let _ = writeln!(out);
            let _ = writeln!(out, "## {}（{}）", group.section.title(), group.rows.len());
            if group.rows.is_empty() {
                let _ = writeln!(out, "{}", group.section.empty_hint());
                continue;
            }
            let _ = writeln!(out);
            let _ = writeln!(out, "| 结论 | 说明 | 涉及表 |");
            let _ = writeln!(out, "| --- | --- | --- |");
            for row in &group.rows {
                // 表格里的竖线会让 Markdown 断列：换成全角竖线而不是删掉
                let _ = writeln!(
                    out,
                    "| {} | {} | {} |",
                    md_escape(&row.title),
                    md_escape(&row.detail),
                    md_escape(&row.tables.join("、"))
                );
            }
        }
        out
    }
}

fn tone_key(tone: SchemaTone) -> &'static str {
    match tone {
        SchemaTone::Normal => "normal",
        SchemaTone::Warning => "warning",
        SchemaTone::Danger => "danger",
    }
}

/// Markdown 表格单元格转义：竖线与换行必须处理，否则表会断列
fn md_escape(text: &str) -> String {
    text.replace('|', "｜").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::{
        SchemaReportView, SchemaSection, SchemaTone, confidence_label,
    };
    use crate::quality_scorer::Grade;
    use crate::schema_analyzer::{
        ForeignKeyCandidate, OrphanTable, RedundantColumn, SchemaInsightReport, TypeMismatch,
        TypeMismatchEntry,
    };

    fn report() -> SchemaInsightReport {
        SchemaInsightReport {
            schema_name: "public".into(),
            table_count: 4,
            total_columns: 21,
            fk_candidates: vec![ForeignKeyCandidate {
                source_table: "orders".into(),
                source_column: "user_id".into(),
                target_table: "users".into(),
                target_column: "id".into(),
                confidence: "high".into(),
                naming_pattern: "{table}_id".into(),
            }],
            type_mismatches: vec![
                TypeMismatch {
                    column_name: "status".into(),
                    tables: vec![
                        TypeMismatchEntry {
                            table_name: "orders".into(),
                            data_type: "VARCHAR".into(),
                        },
                        TypeMismatchEntry {
                            table_name: "users".into(),
                            data_type: "INTEGER".into(),
                        },
                    ],
                    severity: "critical".into(),
                },
                TypeMismatch {
                    column_name: "note".into(),
                    tables: vec![TypeMismatchEntry {
                        table_name: "orders".into(),
                        data_type: "TEXT".into(),
                    }],
                    severity: "warning".into(),
                },
            ],
            orphan_tables: vec![OrphanTable {
                table_name: "logs".into(),
                column_count: 5,
                reason: "既没有外键指向它，它也不引用别的表".into(),
            }],
            redundant_columns: vec![RedundantColumn {
                column_name: "created_at".into(),
                table_count: 3,
                tables: vec!["orders".into(), "users".into(), "logs".into()],
                suggestion: "考虑抽到公共表".into(),
            }],
            summary: "Schema健康评分 68 (需改进)。4 张表, 21 个列".into(),
            health_score: 68.0,
            health_level: "需改进".into(),
        }
    }

    #[test]
    fn groups_follow_the_prototype_order_and_keep_all_rows() {
        let view = SchemaReportView::from_report(&report());
        let sections: Vec<SchemaSection> = view.groups.iter().map(|g| g.section).collect();
        assert_eq!(sections, SchemaSection::ALL.to_vec());
        assert_eq!(view.group(SchemaSection::ForeignKeys).unwrap().rows.len(), 1);
        assert_eq!(view.group(SchemaSection::TypeMismatches).unwrap().rows.len(), 2);
        assert_eq!(view.group(SchemaSection::OrphanTables).unwrap().rows.len(), 1);
        assert_eq!(
            view.group(SchemaSection::RedundantColumns).unwrap().rows.len(),
            1
        );
        assert_eq!(view.schema_name, "public");
        assert_eq!(view.table_count, 4);
    }

    /// 等级按分数现算，不解析领域里的 `health_level` 字符串（两者同源，但只有一处是权威）
    #[test]
    fn grade_comes_from_the_score_not_the_domain_string() {
        let mut report = report();
        report.health_score = 91.0;
        // 故意写一个与分数不符的字符串：视图应无视它
        report.health_level = "差".into();
        let view = SchemaReportView::from_report(&report);
        assert_eq!(view.grade, Grade::Excellent);
        assert_eq!(view.health, 91.0);
    }

    #[test]
    fn tones_follow_severity_and_kind() {
        let view = SchemaReportView::from_report(&report());
        let fk = &view.group(SchemaSection::ForeignKeys).unwrap().rows[0];
        assert_eq!(fk.tone, SchemaTone::Normal, "候选不是问题");
        assert_eq!(fk.title, "orders.user_id → users.id");
        assert_eq!(fk.detail, "高置信 · 命名模式 {table}_id");
        assert_eq!(fk.tables, vec!["orders".to_string(), "users".to_string()]);

        let mismatches = &view.group(SchemaSection::TypeMismatches).unwrap().rows;
        assert_eq!(mismatches[0].tone, SchemaTone::Danger, "critical 用危险色");
        assert_eq!(mismatches[1].tone, SchemaTone::Warning);
        assert_eq!(
            mismatches[0].detail, "orders: VARCHAR / users: INTEGER",
            "详情要把各表的实际类型摊开"
        );

        let orphan = &view.group(SchemaSection::OrphanTables).unwrap().rows[0];
        assert_eq!(orphan.tone, SchemaTone::Danger);
        assert!(orphan.detail.contains("5 列"));

        let redundant = &view.group(SchemaSection::RedundantColumns).unwrap().rows[0];
        assert_eq!(redundant.title, "列 created_at（3 张表）");
        assert!(redundant.detail.contains("考虑抽到公共表"));
    }

    #[test]
    fn issue_count_ignores_neutral_candidates() {
        let view = SchemaReportView::from_report(&report());
        // 1 critical + 1 warning 不一致 + 1 孤立表 + 1 冗余列 = 4；外键候选不算问题
        assert_eq!(view.issue_count(), 4);
    }

    #[test]
    fn empty_report_still_lists_four_sections_with_hints() {
        let view = SchemaReportView::from_report(&SchemaInsightReport {
            schema_name: "public".into(),
            table_count: 0,
            total_columns: 0,
            fk_candidates: Vec::new(),
            type_mismatches: Vec::new(),
            orphan_tables: Vec::new(),
            redundant_columns: Vec::new(),
            summary: "Schema 中无表".into(),
            health_score: 0.0,
            health_level: "空Schema".into(),
        });
        assert_eq!(view.groups.len(), 4);
        assert!(view.groups.iter().all(|g| g.is_empty()));
        assert_eq!(view.issue_count(), 0);
        assert_eq!(view.grade, Grade::Bad, "0 分就是最差，不另立「空Schema」等级");
    }

    #[test]
    fn json_export_keeps_stable_keys_and_counts() {
        let view = SchemaReportView::from_report(&report());
        let json: serde_json::Value = serde_json::from_str(&view.to_json()).expect("导出应是合法 JSON");
        assert_eq!(json["schema"], "public");
        assert_eq!(json["health_score"], 68.0);
        assert_eq!(json["health_level"], "一般", "等级取现算的那一份");
        assert_eq!(json["issue_count"], 4);
        let groups = json["groups"].as_array().expect("应有分组");
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[0]["section"], "foreign_key_candidates", "分组键用稳定英文");
        assert_eq!(groups[1]["rows"][0]["tone"], "danger");
        assert_eq!(groups[3]["rows"][0]["tables"][2], "logs");
    }

    #[test]
    fn markdown_export_is_a_readable_report() {
        let md = SchemaReportView::from_report(&report()).to_markdown();
        assert!(md.starts_with("# Schema 健康报告 · public"));
        assert!(md.contains("- 健康评分：**68**（一般）"));
        assert!(md.contains("## 外键候选（1）"));
        assert!(md.contains("| orders.user_id → users.id | 高置信 · 命名模式 {table}_id | orders、users |"));
        assert!(md.contains("## 类型不一致（2）"));
        assert!(md.contains("## 孤立表（1）"));
        assert!(md.contains("## 冗余列（1）"));
    }

    #[test]
    fn markdown_escapes_pipes_and_keeps_empty_sections_explained() {
        let mut report = report();
        report.fk_candidates[0].naming_pattern = "{table}_id | 变体".into();
        report.type_mismatches.clear();
        report.orphan_tables.clear();
        report.redundant_columns.clear();
        let md = SchemaReportView::from_report(&report).to_markdown();
        assert!(
            md.contains("{table}_id ｜ 变体"),
            "竖线要换成全角，否则 Markdown 表会断列：{md}"
        );
        assert!(md.contains("同名列的声明类型在各表一致"), "空分区要解释「空」的含义");
        assert!(md.contains("每张表都至少与另一张表有关联"));
    }

    #[test]
    fn unknown_confidence_is_shown_as_is() {
        assert_eq!(confidence_label("high"), "高置信");
        assert_eq!(confidence_label("medium"), "中置信");
        assert_eq!(confidence_label("low"), "低置信");
        assert_eq!(confidence_label("very-high"), "very-high", "加了新档也不至于空白");
    }
}

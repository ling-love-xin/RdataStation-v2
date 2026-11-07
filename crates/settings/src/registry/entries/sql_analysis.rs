//! SQL 分析设置项登记（sqlglot 高级能力在编辑器侧的开关）。
//!
//! 三个开关都在打字/悬停路径上，`SettingEffect::Immediate`：改完下一帧就生效
//! （装配层把值推成 `rds_sql::analysis::AnalysisPrefs`，读侧无锁）。
use crate::registry::{SettingEffect, SettingEntry, SettingKind, SettingSpec};

/// 本域全部登记项（`registry::registry()` 惰性合并，顺序 = 登记顺序）。
pub fn entries() -> Vec<SettingSpec> {
    vec![
        SettingSpec {
            key: "sql_analysis.semantic_diagnostics",
            section: "sql_analysis",
            section_label: "SQL 分析",
            group: "sql_analysis",
            group_label: None,
            label: "语义诊断",
            hint: "打字期报「未知表 / 未知列」（需要表目录：未绑定连接时不生效）；关闭后只留词法层诊断（未闭合引号、括号不平衡），省下每次解析开销",
            hint_inline: true,
            kind: SettingKind::Bool,
            presets: &[],
            default_json: "true",
            effect: SettingEffect::Immediate,
            entry: SettingEntry::Page,
            consumer: "crates/editor/src/language/language_service/face_diag.rs（读 AnalysisPrefs 决定是否跑 semantic::diagnose）",
            composite: false,
            disabled: false,
        },
        SettingSpec {
            key: "sql_analysis.predicate_pushdown",
            section: "sql_analysis",
            section_label: "SQL 分析",
            group: "sql_analysis",
            group_label: None,
            label: "谓词下推改写",
            hint: "代码动作里给一条「把 WHERE 下推进子查询」，把外层过滤条件推到派生表里少扫行（只读语法树，结果就地替换那句 SQL）；关闭后这条动作不出现",
            hint_inline: true,
            kind: SettingKind::Bool,
            presets: &[],
            default_json: "true",
            effect: SettingEffect::Immediate,
            entry: SettingEntry::Page,
            consumer: "crates/editor/src/language/language_service/face_actions.rs（读 AnalysisPrefs + pushdown_sql 产动作）",
            composite: false,
            disabled: false,
        },
        SettingSpec {
            key: "sql_analysis.hover_scope",
            section: "sql_analysis",
            section_label: "SQL 分析",
            group: "sql_analysis",
            group_label: None,
            label: "悬停显示来源表",
            hint: "悬停列 / 表时补一行「来源：orders、totals」（读语法树作用域，不依赖连接）；关闭后悬停只给原有的表名 / 函数签名",
            hint_inline: true,
            kind: SettingKind::Bool,
            presets: &[],
            default_json: "true",
            effect: SettingEffect::Immediate,
            entry: SettingEntry::Page,
            consumer: "crates/editor/src/lsp/sql_hover.rs（读 AnalysisPrefs 决定是否补来源行）",
            composite: false,
            disabled: false,
        },
    ]
}

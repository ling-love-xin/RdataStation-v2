//! SQL 分析设置 model（sqlglot 高级能力在编辑器侧的开关）。
//!
//! 三个开关对应 `crates/sql/src/analysis.rs` 的 [`AnalysisPrefs`]：
//!
//! - `semantic_diagnostics`：打字期语义诊断（未知表 / 未知列），关掉只留词法层诊断；
//! - `predicate_pushdown`：编辑器给「把 WHERE 下推进子查询」这条改写动作；
//! - `hover_scope`：悬停时补一行「来源表」（不查元数据，只读 AST 作用域）。
//!
//! 三者默认开启：能力已在 0.11.1 上实测可用，默认把收益给到用户；关掉各自的
//! 理由不同——诊断/悬停是为"大文件打字卡顿"留退路（解析是共同成本），
//! 下推改写则是"不想看到这条动作"的用户口味。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlAnalysis {
    #[serde(default = "default_sql_analysis_semantic_diagnostics")]
    pub semantic_diagnostics: bool,
    #[serde(default = "default_sql_analysis_predicate_pushdown")]
    pub predicate_pushdown: bool,
    #[serde(default = "default_sql_analysis_hover_scope")]
    pub hover_scope: bool,
}

impl Default for SqlAnalysis {
    fn default() -> Self {
        Self {
            semantic_diagnostics: default_sql_analysis_semantic_diagnostics(),
            predicate_pushdown: default_sql_analysis_predicate_pushdown(),
            hover_scope: default_sql_analysis_hover_scope(),
        }
    }
}

/// 语义诊断默认开启（有表目录才生效，无目录时上游本来就退化）。
fn default_sql_analysis_semantic_diagnostics() -> bool {
    true
}

/// 谓词下推动作默认开启（只读 AST，不查元数据）。
fn default_sql_analysis_predicate_pushdown() -> bool {
    true
}

/// 悬停补「来源表」默认开启。
fn default_sql_analysis_hover_scope() -> bool {
    true
}

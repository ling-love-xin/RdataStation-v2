//! 规则执行与查询：执行指定规则、列出规则、按列类型筛选规则

use std::collections::HashMap;

use crate as insight;
use crate::{RuleExecutor, RuleRegistry};
use shared::error::{CommonError, CoreError};

use super::*;

/// 执行指定 id 的规则（如 `numeric-stats` / `histogram`），返回带列元数据的
/// [`insight::ExecutionResult`]（含 QualityRule 质量门控结果）。
///
/// 调用方需已持有 DuckDB 锁；规则由调用方传入（规则分层随项目变化，见
/// [`crate::registry_for`]）。
pub fn execute_insight_rule(
    registry: &RuleRegistry,
    rule_id: &str,
    conn: &duckdb::Connection,
    params: &HashMap<String, String>,
) -> Result<insight::ExecutionResult, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let rule = registry.get(rule_id).ok_or_else(|| {
        CoreError::common(CommonError::General(format!(
            "Rule '{}' not found",
            rule_id
        )))
    })?;
    RuleExecutor::execute_qualified(rule, conn, params)
}

/// 列出注册的规则（可按分类过滤，如 `column` / `multi` / `table` / `quality`）。
///
/// 返回面向前端的 JSON（含 `scope` 与 `source_path`，便于界面按作用域分组展示）。
pub fn list_insight_rules(
    registry: &RuleRegistry,
    category: Option<&str>,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let rules: Vec<&crate::RuleFile> = match category {
        Some(cat) => registry.list_by_category(cat),
        None => registry.all_rules(),
    };
    let result: Vec<serde_json::Value> = rules
        .iter()
        .map(|r| {
            let source = registry.source_of(&r.meta.id);
            serde_json::json!({
                "id": r.meta.id,
                "name": r.meta.name,
                "description": r.meta.description,
                "version": r.meta.version,
                "category": r.meta.category,
                "applies_to": r.meta.applies_to,
                "builtin": r.meta.builtin,
                "parameters": r.query.parameters,
                "result_type": r.query.result_type,
                // 实际生效的来源与文件路径（被覆盖后指向胜出的那一层）
                "scope": source.map(|s| s.scope.label()).unwrap_or("未知"),
                "scope_path": source.map(|s| s.path.as_str()).unwrap_or(""),
            })
        })
        .collect();
    Ok(result)
}

/// 列出适用于指定列类型的规则（如 `Numeric` / `Text` / `Any`）。
pub fn list_rules_for_column(
    registry: &RuleRegistry,
    column_type: &str,
) -> Result<Vec<serde_json::Value>, CoreError> {
    let result: Vec<serde_json::Value> = registry
        .rules_for_column_type(column_type)
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.meta.id,
                "name": r.meta.name,
                "category": r.meta.category,
                "applies_to": r.meta.applies_to,
                "parameters": r.query.parameters,
            })
        })
        .collect();
    Ok(result)
}

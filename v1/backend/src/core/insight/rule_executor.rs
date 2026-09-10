use std::collections::HashMap;

use duckdb::Connection;
use serde_json::{json, Value};

use crate::core::error::{CommonError, CoreError};

use super::rule_types::{ExecutionResult, OutputField, QualityCheck, QualityReport, RuleFile};

pub struct RuleExecutor;

impl RuleExecutor {
    pub fn execute(
        rule: &RuleFile,
        conn: &Connection,
        params: &HashMap<String, String>,
    ) -> Result<Value, CoreError> {
        let sql = Self::build_sql(rule, params)?;
        Self::validate_identifiers(params)?;

        let result_type = rule.query.result_type.as_deref().unwrap_or("single");

        match result_type {
            "list" => Self::execute_list(rule, conn, &sql),
            _ => Self::execute_single(rule, conn, &sql),
        }
    }

    pub fn execute_qualified(
        rule: &RuleFile,
        conn: &Connection,
        params: &HashMap<String, String>,
    ) -> Result<ExecutionResult, CoreError> {
        let data = Self::execute(rule, conn, params)?;
        let quality = Self::evaluate_quality(rule, &data);
        Ok(ExecutionResult { data, quality })
    }

    fn build_sql(rule: &RuleFile, params: &HashMap<String, String>) -> Result<String, CoreError> {
        let mut sql = rule.query.template.clone();

        for param_name in &rule.query.parameters {
            let marker = format!("@{}@", param_name);
            let placeholder = format!("{{{}}}", param_name);
            let value = params.get(param_name).ok_or_else(|| {
                CoreError::common(CommonError::General(format!(
                    "Missing parameter '{}' for rule '{}'",
                    param_name, rule.meta.id
                )))
            })?;
            sql = sql.replace(&placeholder, &marker);
            sql = sql.replace(&marker, value);
        }

        if sql.contains('{') {
            let missing: Vec<String> = rule
                .query
                .parameters
                .iter()
                .filter(|p| !params.contains_key(*p))
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Err(CoreError::common(CommonError::General(format!(
                    "Missing parameters for rule '{}': {:?}",
                    rule.meta.id, missing
                ))));
            }
        }

        Ok(sql)
    }

    fn validate_identifiers(params: &HashMap<String, String>) -> Result<(), CoreError> {
        for (key, value) in params {
            if !value
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
            {
                return Err(CoreError::common(CommonError::General(format!(
                    "Invalid characters in parameter '{}' value: '{}'",
                    key, value
                ))));
            }
        }
        Ok(())
    }

    fn build_col_map(stmt: &duckdb::Statement) -> HashMap<String, usize> {
        let mut map = HashMap::new();
        for i in 0.. {
            if let Ok(name) = stmt.column_name(i) {
                map.insert(name.to_lowercase(), i);
            } else {
                break;
            }
        }
        map
    }

    fn col_index(col_map: &HashMap<String, usize>, col_name: &str) -> Option<usize> {
        col_map.get(&col_name.to_lowercase()).copied()
    }

    fn extract_field_value(
        row: &duckdb::Row,
        field: &OutputField,
        col_map: &HashMap<String, usize>,
    ) -> Result<Value, CoreError> {
        let idx = Self::col_index(col_map, &field.sql_name).ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "Column '{}' not found in result for rule output field '{}'",
                field.sql_name, field.json_name
            )))
        })?;

        let val: Value = match field.value_type.as_str() {
            "f64" => {
                let v: f64 = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get f64 value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                json!(v)
            }
            "f64?" => {
                let v: Option<f64> = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get Option<f64> value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                match v {
                    Some(x) => json!(x),
                    None => Value::Null,
                }
            }
            "i64" => {
                let v: i64 = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get i64 value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                json!(v)
            }
            "i64?" => {
                let v: Option<i64> = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get Option<i64> value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                match v {
                    Some(x) => json!(x),
                    None => Value::Null,
                }
            }
            "String" | "string" => {
                let v: String = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get String value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                json!(v)
            }
            "String?" | "string?" => {
                let v: Option<String> = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get Option<String> value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                match v {
                    Some(s) => json!(s),
                    None => Value::Null,
                }
            }
            "bool" => {
                let v: bool = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get bool value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                json!(v)
            }
            "bool?" => {
                let v: Option<bool> = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get Option<bool> value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                match v {
                    Some(b) => json!(b),
                    None => Value::Null,
                }
            }
            "usize" => {
                let v: i64 = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get usize value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                json!(v as usize)
            }
            _ => {
                let v: String = row.get(idx).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to get String value for field '{}' at col {}: {}",
                        field.json_name, idx, e
                    )))
                })?;
                json!(v)
            }
        };
        Ok(val)
    }

    fn execute_single(rule: &RuleFile, conn: &Connection, sql: &str) -> Result<Value, CoreError> {
        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Rule '{}' prepare failed: {}",
                rule.meta.id, e
            )))
        })?;

        // DuckDB requires query execution before column_name() is available.
        // Pre-execute to enable column metadata access.
        {
            let _ = stmt.query([]).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Rule '{}' query failed: {}",
                    rule.meta.id, e
                )))
            })?;
        }

        let col_map = Self::build_col_map(&stmt);

        for field in &rule.output {
            if Self::col_index(&col_map, &field.sql_name).is_none() {
                return Err(CoreError::common(CommonError::General(format!(
                    "Rule '{}' output field '{}' references non-existent column '{}'",
                    rule.meta.id, field.json_name, field.sql_name
                ))));
            }
        }

        let mut map = serde_json::Map::new();

        let has_row = stmt
            .query_map([], |row| {
                for field in &rule.output {
                    let val = Self::extract_field_value(row, field, &col_map)
                        .map_err(|e| duckdb::Error::InvalidParameterName(e.to_string()))?;
                    map.insert(field.json_name.clone(), val);
                }
                Ok(())
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Rule '{}' query failed: {}",
                    rule.meta.id, e
                )))
            })?;

        let mut found = false;
        for _row in has_row {
            found = true;
        }

        if found {
            Ok(Value::Object(map))
        } else {
            Ok(Value::Null)
        }
    }

    fn execute_list(rule: &RuleFile, conn: &Connection, sql: &str) -> Result<Value, CoreError> {
        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Rule '{}' prepare failed: {}",
                rule.meta.id, e
            )))
        })?;

        // DuckDB requires query execution before column_name() is available.
        {
            let _ = stmt.query([]).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Rule '{}' query failed: {}",
                    rule.meta.id, e
                )))
            })?;
        }

        let col_map = Self::build_col_map(&stmt);

        for field in &rule.output {
            if Self::col_index(&col_map, &field.sql_name).is_none() {
                return Err(CoreError::common(CommonError::General(format!(
                    "Rule '{}' output field '{}' references non-existent column '{}'",
                    rule.meta.id, field.json_name, field.sql_name
                ))));
            }
        }

        let rows: Vec<Value> = stmt
            .query_map([], |row| {
                let mut map = serde_json::Map::new();
                for field in &rule.output {
                    let val = Self::extract_field_value(row, field, &col_map)
                        .map_err(|e| duckdb::Error::InvalidParameterName(e.to_string()))?;
                    map.insert(field.json_name.clone(), val);
                }
                Ok(Value::Object(map))
            })
            .map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Rule '{}' list query failed: {}",
                    rule.meta.id, e
                )))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Value::Array(rows))
    }

    fn evaluate_quality(rule: &RuleFile, data: &Value) -> Option<QualityReport> {
        let quality_rules = rule.quality.as_ref()?;
        if quality_rules.is_empty() {
            return None;
        }

        let mut checks = Vec::new();
        let mut all_passed = true;

        for qr in quality_rules {
            let field = &qr.field;
            let actual = match data {
                Value::Object(map) => map.get(field).and_then(|v| v.as_f64()),
                _ => None,
            };

            let mut field_passed = true;
            let mut rule_desc = String::new();

            if let Some(min) = qr.min {
                if let Some(val) = actual {
                    if val < min {
                        field_passed = false;
                        rule_desc.push_str(&format!("min={}, actual={}", min, val));
                    }
                } else if actual.is_none() {
                    field_passed = false;
                    rule_desc.push_str(&format!("min={}, actual=null", min));
                }
            }

            if let Some(max) = qr.max {
                if let Some(val) = actual {
                    if val > max {
                        if !rule_desc.is_empty() {
                            rule_desc.push_str("; ");
                        }
                        field_passed = false;
                        rule_desc.push_str(&format!("max={}, actual={}", max, val));
                    }
                }
            }

            if field_passed && rule_desc.is_empty() {
                if let Some(min) = qr.min {
                    rule_desc.push_str(&format!("min={}", min));
                }
                if let Some(max) = qr.max {
                    if !rule_desc.is_empty() {
                        rule_desc.push_str(", ");
                    }
                    rule_desc.push_str(&format!("max={}", max));
                }
            }

            if !field_passed {
                all_passed = false;
            }

            checks.push(QualityCheck {
                field: field.clone(),
                passed: field_passed,
                rule: rule_desc,
                actual,
                severity: qr.severity.clone().unwrap_or_else(|| "warning".to_string()),
                message: qr
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("Quality check for {}", field)),
            });
        }

        Some(QualityReport {
            passed: all_passed,
            checks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::insight::rule_types;
    use std::collections::HashMap;

    #[test]
    fn test_build_col_map_extracts_column_names() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE test (id INTEGER, name TEXT, value REAL)")?;
        let mut stmt = conn.prepare("SELECT id, name, value FROM test")?;
        // DuckDB requires query execution before column_name() is available
        {
            let _ = stmt.query([])?;
        }
        let map = RuleExecutor::build_col_map(&stmt);
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("id"), Some(&0));
        assert_eq!(map.get("name"), Some(&1));
        assert_eq!(map.get("value"), Some(&2));
        Ok(())
    }

    #[test]
    fn test_col_index_case_insensitive() {
        let mut map = HashMap::new();
        map.insert("column_a".to_string(), 0);
        map.insert("column_b".to_string(), 1);
        assert_eq!(RuleExecutor::col_index(&map, "COLUMN_A"), Some(0));
        assert_eq!(RuleExecutor::col_index(&map, "column_b"), Some(1));
        assert_eq!(RuleExecutor::col_index(&map, "missing"), None);
    }

    #[test]
    fn test_validate_identifiers_allows_alphanumeric() {
        let mut params = HashMap::new();
        params.insert("table".to_string(), "my_table_123".to_string());
        assert!(RuleExecutor::validate_identifiers(&params).is_ok());
    }

    #[test]
    fn test_validate_identifiers_rejects_special_chars() {
        let mut params = HashMap::new();
        params.insert("table".to_string(), "malicious; DROP".to_string());
        assert!(RuleExecutor::validate_identifiers(&params).is_err());
    }

    #[test]
    fn test_build_sql_replaces_params() -> Result<(), CoreError> {
        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "test".into(),
                name: "Test".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT {col} FROM {table}".into(),
                parameters: vec!["table".into(), "col".into()],
                result_type: Some("single".into()),
            },
            output: vec![],
            quality: None,
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("table".to_string(), "users".to_string());
        params.insert("col".to_string(), "COUNT(*)".to_string());
        let sql = RuleExecutor::build_sql(&rule, &params)?;
        assert_eq!(sql, "SELECT COUNT(*) FROM users");
        Ok(())
    }

    #[test]
    fn test_build_sql_handles_similar_param_names() -> Result<(), CoreError> {
        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "test".into(),
                name: "Test".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "{col} vs {col_name}".into(),
                parameters: vec!["col".into(), "col_name".into()],
                result_type: Some("single".into()),
            },
            output: vec![],
            quality: None,
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("col".to_string(), "A".to_string());
        params.insert("col_name".to_string(), "B".to_string());
        let sql = RuleExecutor::build_sql(&rule, &params)?;
        assert_eq!(sql, "A vs B");
        Ok(())
    }

    #[test]
    fn test_build_sql_missing_param_returns_error() {
        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "test".into(),
                name: "Test".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT {col} FROM {table}".into(),
                parameters: vec!["table".into(), "col".into()],
                result_type: Some("single".into()),
            },
            output: vec![],
            quality: None,
            render: None,
        };
        let params = HashMap::new();
        assert!(RuleExecutor::build_sql(&rule, &params).is_err());
    }

    #[test]
    fn test_execute_single_with_valid_rule() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER); INSERT INTO t VALUES (42);")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "test_single".into(),
                name: "Test Single".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT {col} FROM {table}".into(),
                parameters: vec!["table".into(), "col".into()],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "col1".into(),
                value_type: "i64".into(),
            }],
            quality: None,
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("table".to_string(), "t".to_string());
        params.insert("col".to_string(), "col1".to_string());
        let result = RuleExecutor::execute(&rule, &conn, &params)?;
        assert_eq!(result["value"], json!(42));
        Ok(())
    }

    #[test]
    fn test_execute_list_with_valid_rule() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER); INSERT INTO t VALUES (1), (2), (3);")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "test_list".into(),
                name: "Test List".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT {col} FROM {table}".into(),
                parameters: vec!["table".into(), "col".into()],
                result_type: Some("list".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "col1".into(),
                value_type: "i64".into(),
            }],
            quality: None,
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("table".to_string(), "t".to_string());
        params.insert("col".to_string(), "col1".to_string());
        let result = RuleExecutor::execute(&rule, &conn, &params)?;
        let arr = result.as_array().ok_or_else(|| {
            CoreError::common(CommonError::General("Expected array result".into()))
        })?;
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["value"], json!(1));
        assert_eq!(arr[2]["value"], json!(3));
        Ok(())
    }

    #[test]
    fn test_execute_rejects_missing_column() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER)")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "bad_rule".into(),
                name: "Bad".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT col1 FROM t".into(),
                parameters: vec![],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "nonexistent".into(),
                value_type: "i64".into(),
            }],
            quality: None,
            render: None,
        };
        let result = RuleExecutor::execute(&rule, &conn, &HashMap::new());
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_qualified_with_quality_rules_passing() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER); INSERT INTO t VALUES (42);")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "q_pass".into(),
                name: "Quality Passing".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT col1 FROM t".into(),
                parameters: vec![],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "col1".into(),
                value_type: "i64".into(),
            }],
            quality: Some(vec![rule_types::QualityRule {
                field: "value".into(),
                min: Some(0.0),
                max: Some(100.0),
                severity: Some("error".to_string()),
                message: Some("Out of range".to_string()),
            }]),
            render: None,
        };

        let result = RuleExecutor::execute_qualified(&rule, &conn, &HashMap::new())?;
        assert_eq!(result.data["value"], json!(42));

        let quality = result.quality.ok_or_else(|| {
            CoreError::common(CommonError::General("Expected quality report".into()))
        })?;
        assert!(quality.passed);
        assert_eq!(quality.checks.len(), 1);
        assert!(quality.checks[0].passed);
        assert_eq!(quality.checks[0].field, "value");
        assert_eq!(quality.checks[0].actual, Some(42.0));
        Ok(())
    }

    #[test]
    fn test_execute_qualified_with_quality_rules_failing_min() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER); INSERT INTO t VALUES (-5);")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "q_fail_min".into(),
                name: "Quality Failing Min".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT col1 FROM t".into(),
                parameters: vec![],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "col1".into(),
                value_type: "i64".into(),
            }],
            quality: Some(vec![rule_types::QualityRule {
                field: "value".into(),
                min: Some(0.0),
                max: None,
                severity: Some("error".to_string()),
                message: Some("Must be non-negative".to_string()),
            }]),
            render: None,
        };

        let result = RuleExecutor::execute_qualified(&rule, &conn, &HashMap::new())?;
        let quality = result.quality.ok_or_else(|| {
            CoreError::common(CommonError::General("Expected quality report".into()))
        })?;
        assert!(!quality.passed);
        assert_eq!(quality.checks.len(), 1);
        assert!(!quality.checks[0].passed);
        assert_eq!(quality.checks[0].actual, Some(-5.0));
        assert_eq!(quality.checks[0].severity, "error");
        Ok(())
    }

    #[test]
    fn test_execute_qualified_no_quality_rules() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER); INSERT INTO t VALUES (1)")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "no_q".into(),
                name: "No Quality".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT col1 FROM t".into(),
                parameters: vec![],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "col1".into(),
                value_type: "i64".into(),
            }],
            quality: None,
            render: None,
        };

        let result = RuleExecutor::execute_qualified(&rule, &conn, &HashMap::new())?;
        assert!(result.quality.is_none());
        assert_eq!(result.data["value"], json!(1));
        Ok(())
    }
}

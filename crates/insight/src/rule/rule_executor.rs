use std::collections::HashMap;

use duckdb::Connection;
use serde_json::{Value, json};

use shared::error::{CommonError, CoreError};

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

    pub(crate) fn build_sql(
        rule: &RuleFile,
        params: &HashMap<String, String>,
    ) -> Result<String, CoreError> {
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

    /// 执行一次 SQL 并把行交给 `consume`（列名在执行后才可读）。
    ///
    /// 列名从执行后的 `Rows` 反取 `Statement`（`get_temp_table_sample_on` 的同一姿势）。
    /// 旧实现先 `query` 一次丢弃结果、再 `query_map` 执行第二次——**每条规则跑两遍
    /// SQL**（含 MEDIAN / PERCENTILE_DISC 的重聚合在全量档大表上代价直接翻倍）。
    /// 用回调而不是返回 `Rows`：后者借用本地 `stmt`，出不了本函数。
    fn run_query<T>(
        rule: &RuleFile,
        conn: &Connection,
        sql: &str,
        consume: impl FnOnce(&mut duckdb::Rows<'_>, &HashMap<String, usize>) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Rule '{}' prepare failed: {}",
                rule.meta.id, e
            )))
        })?;
        let mut rows = stmt.query([]).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Rule '{}' query failed: {}",
                rule.meta.id, e
            )))
        })?;
        let col_map = rows.as_ref().map(Self::build_col_map).ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "Rule '{}' result statement is no longer available",
                rule.meta.id
            )))
        })?;
        Self::validate_output_columns(rule, &col_map)?;
        consume(&mut rows, &col_map)
    }

    /// 输出字段必须在结果列里真实存在（single / list 共用）。
    fn validate_output_columns(
        rule: &RuleFile,
        col_map: &HashMap<String, usize>,
    ) -> Result<(), CoreError> {
        for field in &rule.output {
            if Self::col_index(col_map, &field.sql_name).is_none() {
                return Err(CoreError::common(CommonError::General(format!(
                    "Rule '{}' output field '{}' references non-existent column '{}'",
                    rule.meta.id, field.json_name, field.sql_name
                ))));
            }
        }
        Ok(())
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

    /// `single` 语义 = 聚合单行：无行返回 `Null`（空表），多行是规则写错（如漏
    /// GROUP BY）——**早失败**优于静默取某一行（旧实现对多行结果取「最后一行」，
    /// 没有任何提示，与 D48 / D49 同一立场：声明与事实不符就报出来）。
    fn execute_single(rule: &RuleFile, conn: &Connection, sql: &str) -> Result<Value, CoreError> {
        Self::run_query(rule, conn, sql, |rows, col_map| {
            let Some(row) = rows.next().map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Rule '{}' row read failed: {}",
                    rule.meta.id, e
                )))
            })?
            else {
                return Ok(Value::Null);
            };

            let mut map = serde_json::Map::new();
            for field in &rule.output {
                let val = Self::extract_field_value(row, field, col_map)?;
                map.insert(field.json_name.clone(), val);
            }

            if rows
                .next()
                .map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Rule '{}' row read failed: {}",
                        rule.meta.id, e
                    )))
                })?
                .is_some()
            {
                return Err(CoreError::common(CommonError::General(format!(
                    "Rule '{}' declares result_type = single but returned multiple rows; \
                     add an aggregate / GROUP BY, or declare result_type = \"list\"",
                    rule.meta.id
                ))));
            }

            Ok(Value::Object(map))
        })
    }

    /// `list`：逐行读全。行级读取错误**向上抛**（D19 立场：`filter_map(r.ok())` 会
    /// 静默少行——直方图 / Top 频次无声缺一段，比报错难查得多）。
    fn execute_list(rule: &RuleFile, conn: &Connection, sql: &str) -> Result<Value, CoreError> {
        Self::run_query(rule, conn, sql, |rows, col_map| {
            let mut out: Vec<Value> = Vec::new();
            while let Some(row) = rows.next().map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Rule '{}' row read failed: {}",
                    rule.meta.id, e
                )))
            })? {
                let mut map = serde_json::Map::new();
                for field in &rule.output {
                    let val = Self::extract_field_value(row, field, col_map)?;
                    map.insert(field.json_name.clone(), val);
                }
                out.push(Value::Object(map));
            }
            Ok(Value::Array(out))
        })
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

            // D49 语义：值**合法地为 null**（如空表算不出空值率）时门控不判违规——
            // `min` 与 `max` 同一口径。旧实现是 `min` 遇 null 判失败、`max` 遇 null
            // 通过：同一个门控两种答案（只设 `min` 的规则在空表上必误报）。
            // 字段本身不存在的情况已在解析期挡掉（`validate_rule`），这里的 `None`
            // 只意味着「字段在、值是 null」。
            match actual {
                None => rule_desc.push_str("actual=null（无值不判）"),
                Some(val) => {
                    match qr.min {
                        Some(min) if val < min => {
                            field_passed = false;
                            rule_desc.push_str(&format!("min={}, actual={}", min, val));
                        }
                        _ => {}
                    }
                    match qr.max {
                        Some(max) if val > max => {
                            if !rule_desc.is_empty() {
                                rule_desc.push_str("; ");
                            }
                            field_passed = false;
                            rule_desc.push_str(&format!("max={}, actual={}", max, val));
                        }
                        _ => {}
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
    use crate::rule_types;
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

    /// K11 回归：内置的 `null-check` 门控要**真的能触发**。
    ///
    /// 以前它指向一个不存在的 `null_rate`，而「只设 `max`」的判定在 `actual` 为 `None`
    /// 时静默通过——门控形同虚设。现在 `null_rate` 由 SQL 真算出来，这里用真表钉住两侧：
    /// 50% 空值必须失败、空表不得误报（空表没有空值率可言，NULL 不算违规）。
    ///
    /// 直接解析**二进制里那一条**（不是测试里另写一份）：验的就是发出去的那个版本。
    #[test]
    fn test_builtin_null_check_gate_actually_fires() -> Result<(), CoreError> {
        let content = crate::BUILTIN_RULES_DIR
            .get_file("column/null-check.rule.toml")
            .and_then(|file| file.contents_utf8())
            .expect("内置规则应在二进制里");
        let rule = crate::parse_rule_toml(content)?;

        let mut params = HashMap::new();
        params.insert("table".to_string(), "t_rule_null_check".to_string());
        params.insert("col".to_string(), "amount".to_string());

        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE t_rule_null_check (amount INTEGER);
             INSERT INTO t_rule_null_check VALUES (1), (2), (NULL), (NULL);",
        )?;
        let result = RuleExecutor::execute_qualified(&rule, &conn, &params)?;
        assert_eq!(result.data["total_count"], json!(4));
        assert_eq!(result.data["null_rate"], json!(0.5));
        let quality = result.quality.expect("这条规则带门控，必须给出报告");
        assert!(
            !quality.passed,
            "50% 空值率应当触发 max=0.1 的门控（K11 就是不触发）"
        );
        assert_eq!(quality.checks[0].actual, Some(0.5));

        // 空表：NULLIF 给出 NULL → 不算违规（总行数为 0 时没有空值率可言）
        conn.execute_batch("DELETE FROM t_rule_null_check;")?;
        let result = RuleExecutor::execute_qualified(&rule, &conn, &params)?;
        assert_eq!(result.data["null_rate"], Value::Null);
        assert!(
            result.quality.expect("仍有报告").passed,
            "空表不该误报空值率问题"
        );
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

    /// `single` 声明但 SQL 返回多行：**早失败**。旧实现静默取「最后一行」，
    /// 规则作者永远不知道自己的 GROUP BY 漏了（与 D48 / D49 同一立场）。
    #[test]
    fn test_execute_single_rejects_multiple_rows() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 INTEGER); INSERT INTO t VALUES (1), (2)")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "multi_row_single".into(),
                name: "Multi Row Single".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                // 漏聚合：两行都回来
                template: "SELECT {col} AS v FROM {table}".into(),
                parameters: vec!["table".into(), "col".into()],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "v".into(),
                value_type: "i64".into(),
            }],
            quality: None,
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("table".to_string(), "t".to_string());
        params.insert("col".to_string(), "col1".to_string());

        let err = RuleExecutor::execute(&rule, &conn, &params)
            .expect_err("声明 single 却返回多行必须报错");
        assert!(
            err.to_string().contains("multiple rows"),
            "报错要点明多行：{err}"
        );
        Ok(())
    }

    /// D49 语义：门控字段的值**合法地为 null** 时 `min` 不判违规——与 `max` 同一口径。
    /// 旧实现 `min` 遇 null 判失败、`max` 遇 null 通过：只设 `min` 的规则（如
    /// 「完成率 ≥ 90%」）在空表上必误报。
    #[test]
    fn test_quality_min_gate_passes_on_null_value() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        // 空表：聚合输出 NULL（f64? 读出 Value::Null）
        conn.execute_batch("CREATE TABLE t (col1 INTEGER)")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "min_gate_null".into(),
                name: "Min Gate Null".into(),
                description: "".into(),
                version: "1.0".into(),
                category: "test".into(),
                applies_to: vec![],
                builtin: true,
            },
            query: rule_types::RuleQuery {
                template: "SELECT AVG({col})::DOUBLE AS v FROM {table}".into(),
                parameters: vec!["table".into(), "col".into()],
                result_type: Some("single".into()),
            },
            output: vec![rule_types::OutputField {
                json_name: "value".into(),
                sql_name: "v".into(),
                value_type: "f64?".into(),
            }],
            quality: Some(vec![rule_types::QualityRule {
                field: "value".into(),
                min: Some(0.9),
                max: None,
                severity: Some("error".to_string()),
                message: Some("完成率不低于 90%".to_string()),
            }]),
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("table".to_string(), "t".to_string());
        params.insert("col".to_string(), "col1".to_string());

        let result = RuleExecutor::execute_qualified(&rule, &conn, &params)?;
        assert_eq!(result.data["value"], Value::Null, "空表的 AVG 是 NULL");
        let quality = result.quality.expect("带门控必有报告");
        assert!(
            quality.passed,
            "值为合法 null 时 min 门控不判违规（D49）：{:?}",
            quality.checks
        );
        assert_eq!(quality.checks[0].actual, None);
        Ok(())
    }

    /// D19 立场（行级读取错误向上抛）：list 结果里某行类型不匹配（声明 i64、
    /// 实际 VARCHAR）必须报错。旧实现 `filter_map(r.ok())` 会**静默丢掉那行**——
    /// 直方图 / Top 频次无声缺一段，比报错难查得多。
    #[test]
    fn test_execute_list_type_mismatch_errors_instead_of_dropping_rows() -> Result<(), CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("CREATE TABLE t (col1 VARCHAR); INSERT INTO t VALUES ('a'), ('b')")?;

        let rule = RuleFile {
            meta: rule_types::RuleMeta {
                id: "bad_type_list".into(),
                name: "Bad Type List".into(),
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
                // 声明 i64，实际是 VARCHAR：每行读取都失败
                value_type: "i64".into(),
            }],
            quality: None,
            render: None,
        };
        let mut params = HashMap::new();
        params.insert("table".to_string(), "t".to_string());
        params.insert("col".to_string(), "col1".to_string());

        let result = RuleExecutor::execute(&rule, &conn, &params);
        assert!(
            result.is_err(),
            "类型不匹配的行必须报错，不得静默丢弃（D19）：{result:?}"
        );
        Ok(())
    }
}

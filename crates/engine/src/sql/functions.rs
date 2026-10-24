//! SQL 函数签名目录（#54 ③ 签名帮助的数据源）
//!
//! ## 双源设计（「全」靠系统表，兜底靠内置表）
//!
//! 1. **在线：数据库系统表**（权威、含用户自定义 UDF）——各引擎的函数目录查询：
//!
//!    | 引擎 | 查询 | 列结构（官方文档） |
//!    | --- | --- | --- |
//!    | DuckDB | `duckdb_functions()` | `function_name / description / return_type / parameters / parameter_types`（最全） |
//!    | SQLite | `PRAGMA function_list` | `name / narg`（只有参数个数，**无参数名**——SQLite 不暴露） |
//!    | PostgreSQL | `pg_proc` + `pg_get_function_arguments(oid)` | `proname / arguments 文本 / return_type`（参数名+类型在 arguments 文本里） |
//!    | MySQL | `information_schema.ROUTINES` + `PARAMETERS` | `ROUTINE_NAME / DTD_IDENTIFIER`；参数在 `PARAMETERS`（`PARAMETER_NAME / DTD_IDENTIFIER / ORDINAL_POSITION`） |
//!
//!    查询生成见 [`catalog_query`]；结果解析见 [`parse_catalog_rows`]（**按列名**取，
//!    缺列给 `None`，容忍驱动差异）。本版覆盖 DuckDB / SQLite / PostgreSQL / MySQL；
//!    其余方言（MsSQL / Oracle / Snowflake 等）标注 TODO——它们的系统表结构各有
//!    各的（`sys.parameters` / `ALL_ARGUMENTS` / `INFORMATION_SCHEMA.PARAMETERS`），
//!    不编造 SQL，接入时逐个实证。
//!
//! 2. **离线兜底：[`builtin_signatures`]**——未连接 / 系统表拉不到 / 正在输入但目录
//!    未载时给高频常用函数（约 50 个，跨方言通用为主 + 少量常用方言特有）。定位是
//!    「兜底」不是「全」：编辑器行为 = 在线目录优先，缺失时回落内置表。
//!
//! ## 限制（口径固定）
//!
//! - 系统表的**列结构依据官方文档**编写；真实连接下的列名差异由「按列名解析 +
//!   None 容忍」吸收，尚未在真实数据库上跑通（等待有环境后补集成测试）。
//! - SQLite 无参数名：签名帮助只能给「函数名 + 参数个数」，参数名位为空。

/// 一个函数签名（编辑器签名帮助的数据单元）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    pub name: String,
    pub params: Vec<ParamInfo>,
    pub return_type: Option<String>,
    pub description: Option<String>,
}

/// 一个参数（签名帮助里可高亮「当前参数位」）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamInfo {
    pub name: Option<String>,
    pub type_name: Option<String>,
    pub default: Option<String>,
}

impl ParamInfo {
    fn new(name: Option<String>, type_name: Option<String>) -> Self {
        Self {
            name,
            type_name,
            default: None,
        }
    }
}

use super::engine::SqlDialect;

/// 按方言返回「函数目录查询 SQL」（不支持的方言 = `None`）
pub fn catalog_query(dialect: SqlDialect) -> Option<&'static str> {
    match dialect {
        // DuckDB：duckdb_functions() 自带 name/description/return_type/parameters
        SqlDialect::Duckdb => Some(
            "SELECT function_name, description, return_type, parameters, parameter_types \
             FROM duckdb_functions()",
        ),
        // SQLite：PRAGMA function_list（name/narg；narg=-1 表示可变参数）
        SqlDialect::Sqlite => Some("PRAGMA function_list"),
        // PostgreSQL：pg_proc + 系统函数给出参数与返回类型文本
        SqlDialect::Postgres => Some(
            "SELECT p.proname AS name, \
                    pg_get_function_arguments(p.oid) AS arguments, \
                    pg_get_function_result(p.oid) AS return_type \
             FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname IN ('pg_catalog', 'public')",
        ),
        // MySQL：ROUTINES 给函数名/返回类型；参数在 PARAMETERS（另行查询）
        SqlDialect::Mysql => Some(
            "SELECT ROUTINE_NAME AS name, DTD_IDENTIFIER AS return_type \
             FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = DATABASE() AND ROUTINE_TYPE = 'FUNCTION'",
        ),
        // MsSQL / Oracle / Snowflake / 其余：系统表结构各异，接入时逐个实证
        _ => None,
    }
}

/// 按方言返回「函数参数目录查询 SQL」（仅 MySQL 需要；其余在 catalog_query 内联）
pub fn parameter_query(dialect: SqlDialect) -> Option<&'static str> {
    match dialect {
        SqlDialect::Mysql => Some(
            "SELECT SPECIFIC_NAME AS function_name, PARAMETER_NAME AS param_name, \
                    DTD_IDENTIFIER AS param_type, ORDINAL_POSITION AS pos \
             FROM information_schema.PARAMETERS \
             WHERE SPECIFIC_SCHEMA = DATABASE() AND PARAMETER_MODE = 'IN' \
             ORDER BY SPECIFIC_NAME, ORDINAL_POSITION",
        ),
        _ => None,
    }
}

/// 把系统表结果行解析成签名
///
/// `headers` = 列名（大小写不敏感匹配）；`rows` 的每一行元素与 headers 同序。
/// **按列名取**：缺列给 `None`，驱动/版本差异不 panic。
///
/// - DuckDB：`parameters` / `parameter_types` 是 LIST——行值若已是 Rust 侧的
///   `Vec` 数组，直接逐位取；若是逗号文本（JSON 化传输），拆逗号。
/// - PostgreSQL：`arguments` 是 `"name type, name type"` 文本，按逗号拆、空格分段。
/// - SQLite：只有 `name` + `narg`（参数个数），参数名位留空。
pub fn parse_catalog_rows(
    dialect: SqlDialect,
    headers: &[String],
    rows: &[Vec<String>],
) -> Vec<FunctionSignature> {
    let idx = |name: &str| {
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
    };
    rows.iter()
        .filter_map(|row| {
            let name = idx("name")
                .or_else(|| idx("function_name"))
                .and_then(|i| row.get(i).cloned())?;
            let return_type = idx("return_type")
                .or_else(|| idx("dt_identifier"))
                .and_then(|i| row.get(i).cloned());
            let description = idx("description").and_then(|i| row.get(i).cloned());
            let params = match dialect {
                SqlDialect::Postgres => {
                    let arguments = idx("arguments").and_then(|i| row.get(i).cloned());
                    arguments
                        .map(|s| parse_pg_arguments(&s))
                        .unwrap_or_default()
                }
                SqlDialect::Sqlite => {
                    // narg = 参数个数（-1 = 可变参数）；无参数名
                    let narg = idx("narg")
                        .and_then(|i| row.get(i).cloned())
                        .and_then(|s| s.parse::<i64>().ok());
                    match narg {
                        Some(-1) => vec![ParamInfo {
                            name: Some("args".to_string()),
                            type_name: None,
                            default: None,
                        }],
                        Some(n) if n >= 0 => (0..n)
                            .map(|i| ParamInfo::new(Some(format!("arg{}", i + 1)), None))
                            .collect(),
                        _ => Vec::new(),
                    }
                }
                _ => {
                    // DuckDB：parameters（LIST 文本，逗号分隔）；MySQL：本版参数
                    // 由 parameter_query 单独解析，行级解析留参数名为空
                    let parameters = idx("parameters").and_then(|i| row.get(i).cloned());
                    parameters
                        .map(|s| {
                            s.trim_start_matches('[')
                                .trim_end_matches(']')
                                .split(',')
                                .filter_map(|p| {
                                    let p = p.trim().trim_matches('"');
                                    if p.is_empty() {
                                        None
                                    } else {
                                        Some(ParamInfo::new(Some(p.to_string()), None))
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                }
            };
            Some(FunctionSignature {
                name,
                params,
                return_type,
                description,
            })
        })
        .collect()
}

/// 解析 PostgreSQL 的 `pg_get_function_arguments` 文本：`"a integer, b text"`
/// → 两个参数（名 + 类型）。带默认值的 `"x integer DEFAULT 1"` 一并拆出。
fn parse_pg_arguments(arguments: &str) -> Vec<ParamInfo> {
    arguments
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            // 拆分 `name type [DEFAULT default]`：第一个空格后是类型段
            let mut bits = part.splitn(2, char::is_whitespace);
            let name = bits.next().map(|s| s.to_string());
            let rest = bits.next().unwrap_or("");
            let mut type_and_default = rest.splitn(2, "DEFAULT").map(|s| s.trim().to_string());
            let type_name = type_and_default.next().filter(|s| !s.is_empty());
            let default = type_and_default.next().filter(|s| !s.is_empty());
            Some(ParamInfo {
                name,
                type_name,
                default,
            })
        })
        .collect()
}

/// 离线兜底：高频常用函数签名（跨方言通用为主）
///
/// 定位是「兜底」不是「全」：在线目录（系统表）拉不到时才用。
pub fn builtin_signatures() -> Vec<FunctionSignature> {
    // (名, 参数名列表, 返回类型, 说明)
    const ROWS: &[(&str, &[&str], &str, &str)] = &[
        ("COUNT", &["*"], "bigint", "统计行数"),
        ("SUM", &["expr"], "numeric", "求和"),
        ("AVG", &["expr"], "numeric", "平均值"),
        ("MIN", &["expr"], "same", "最小值"),
        ("MAX", &["expr"], "same", "最大值"),
        ("ABS", &["expr"], "same", "绝对值"),
        ("ROUND", &["expr", "decimals"], "numeric", "四舍五入"),
        ("CEIL", &["expr"], "numeric", "向上取整"),
        ("FLOOR", &["expr"], "numeric", "向下取整"),
        ("MOD", &["a", "b"], "numeric", "取余"),
        ("POWER", &["base", "exp"], "numeric", "幂"),
        ("SQRT", &["expr"], "numeric", "平方根"),
        ("COALESCE", &["expr", "..."], "same", "第一个非空值"),
        ("NULLIF", &["a", "b"], "same", "两值相等返回 NULL"),
        ("IFNULL", &["expr", "fallback"], "same", "NULL 时的兜底值"),
        ("CONCAT", &["str", "..."], "text", "拼接字符串"),
        ("SUBSTR", &["str", "start", "length"], "text", "子串"),
        ("SUBSTRING", &["str", "start", "length"], "text", "子串"),
        ("TRIM", &["str"], "text", "去首尾空白"),
        ("LTRIM", &["str"], "text", "去左侧空白"),
        ("RTRIM", &["str"], "text", "去右侧空白"),
        ("UPPER", &["str"], "text", "转大写"),
        ("LOWER", &["str"], "text", "转小写"),
        ("LENGTH", &["str"], "integer", "字符数"),
        ("CHAR_LENGTH", &["str"], "integer", "字符数"),
        ("REPLACE", &["str", "from", "to"], "text", "替换子串"),
        ("INSTR", &["str", "needle"], "integer", "子串位置"),
        ("STRPOS", &["str", "needle"], "integer", "子串位置"),
        ("CAST", &["expr", "AS type"], "type", "类型转换"),
        ("NOW", &[], "timestamp", "当前时间戳"),
        ("CURRENT_DATE", &[], "date", "当前日期"),
        ("CURRENT_TIMESTAMP", &[], "timestamp", "当前时间戳"),
        ("DATE_ADD", &["date", "interval"], "date", "日期加区间"),
        ("DATE_TRUNC", &["field", "date"], "timestamp", "截断到粒度"),
        ("EXTRACT", &["field", "source"], "numeric", "提取时间字段"),
        ("TO_CHAR", &["value", "format"], "text", "格式化输出"),
        ("TO_DATE", &["text", "format"], "date", "文本转日期"),
        ("JSON_EXTRACT", &["json", "path"], "text", "JSON 取路径"),
        ("JSON_QUERY", &["json", "path"], "text", "JSON 取路径"),
        ("ARRAY_AGG", &["expr"], "array", "聚合成数组"),
        ("STRING_AGG", &["expr", "sep"], "text", "聚合成串"),
        ("GROUP_CONCAT", &["expr", "sep"], "text", "聚合成串"),
        ("ROW_NUMBER", &[], "integer", "窗口行号"),
        ("RANK", &[], "integer", "窗口排名"),
        ("DENSE_RANK", &[], "integer", "窗口连续排名"),
        ("LAG", &["expr", "offset", "default"], "same", "窗口前一行"),
        ("LEAD", &["expr", "offset", "default"], "same", "窗口后一行"),
        ("FIRST_VALUE", &["expr"], "same", "窗口首值"),
        ("LAST_VALUE", &["expr"], "same", "窗口末值"),
    ];
    ROWS.iter()
        .map(|(name, params, ret, desc)| FunctionSignature {
            name: name.to_string(),
            params: params
                .iter()
                .map(|p| ParamInfo {
                    name: Some(p.to_string()),
                    type_name: None,
                    default: None,
                })
                .collect(),
            return_type: Some(ret.to_string()),
            description: Some(desc.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_covers_common_functions() {
        let sigs = builtin_signatures();
        assert!(sigs.len() >= 40, "兜底表要够常用（当前 {}）", sigs.len());
        let count = sigs.iter().find(|s| s.name == "COUNT").expect("有 COUNT");
        assert_eq!(count.params.len(), 1);
        assert_eq!(count.return_type.as_deref(), Some("bigint"));
        let coalesce = sigs.iter().find(|s| s.name == "COALESCE").expect("有 COALESCE");
        assert!(coalesce.params.iter().any(|p| p.name.as_deref() == Some("...")));
    }

    #[test]
    fn pg_arguments_text_is_parsed() {
        let params = parse_pg_arguments("a integer, b text, c numeric DEFAULT 1.5");
        assert_eq!(params.len(), 3);
        assert_eq!(params[0].name.as_deref(), Some("a"));
        assert_eq!(params[0].type_name.as_deref(), Some("integer"));
        assert_eq!(params[2].default.as_deref(), Some("1.5"), "DEFAULT 拆出");
    }

    #[test]
    fn pg_rows_are_parsed_by_column_name() {
        let headers = vec!["name".into(), "arguments".into(), "return_type".into()];
        let rows = vec![vec![
            "date_trunc".into(),
            "field text, source timestamp".into(),
            "timestamp".into(),
        ]];
        let sigs = parse_catalog_rows(SqlDialect::Postgres, &headers, &rows);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "date_trunc");
        assert_eq!(sigs[0].params.len(), 2);
        assert_eq!(sigs[0].params[0].type_name.as_deref(), Some("text"));
        assert_eq!(sigs[0].return_type.as_deref(), Some("timestamp"));
    }

    #[test]
    fn sqlite_rows_use_narg_for_param_count() {
        let headers = vec!["name".into(), "narg".into()];
        let rows = vec![
            vec!["abs".into(), "1".into()],
            vec!["printf".into(), "-1".into()], // 可变参数
        ];
        let sigs = parse_catalog_rows(SqlDialect::Sqlite, &headers, &rows);
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].params.len(), 1, "abs 1 个参数");
        assert_eq!(sigs[0].params[0].name.as_deref(), Some("arg1"));
        assert_eq!(sigs[1].params[0].name.as_deref(), Some("args"), "变参给 args");
    }

    #[test]
    fn duckdb_list_parameter_text_is_parsed() {
        let headers = vec![
            "function_name".into(),
            "parameters".into(),
            "return_type".into(),
            "description".into(),
        ];
        let rows = vec![vec![
            "regexp_extract".into(),
            "[string, pattern, group]".into(),
            "varchar".into(),
            "提取正则匹配组".into(),
        ]];
        let sigs = parse_catalog_rows(SqlDialect::Duckdb, &headers, &rows);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].params.len(), 3);
        assert_eq!(sigs[0].params[0].name.as_deref(), Some("string"));
        assert_eq!(sigs[0].description.as_deref(), Some("提取正则匹配组"));
    }

    #[test]
    fn missing_columns_tolerated() {
        // 驱动没给 description 列也能解析（按列名容错）
        let headers = vec!["function_name".into(), "return_type".into()];
        let rows = vec![vec!["f".into(), "int".into()]];
        let sigs = parse_catalog_rows(SqlDialect::Duckdb, &headers, &rows);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].description, None);
        assert!(sigs[0].params.is_empty());
    }

    #[test]
    fn unsupported_dialects_have_no_catalog_query() {
        assert!(catalog_query(SqlDialect::MsSQL).is_none());
        assert!(catalog_query(SqlDialect::Oracle).is_none());
        assert!(catalog_query(SqlDialect::Ansi).is_none());
    }
}

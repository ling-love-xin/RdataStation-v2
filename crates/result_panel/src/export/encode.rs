//! 编码为导出文本（纯函数）——自 export.rs 拆分（2026-10-29）
//!
//! CSV / JSON / INSERT / SQL Updates / WHERE 子句 的纯编码 + 值字面量与标识符引号
//! 启发式（[`quote_ident`] / [`cell_literal`] / [`is_null_cell`]）；DuckDB 两档本地不编码。

use crate::store::ResultEntry;
use super::{ExportFormat, RowKey};
/// 一条 INSERT 语句最多带几行（SQLite 对多行 VALUES 有编译期上限，200 行对四库都稳）
const INSERT_ROWS_PER_STATEMENT: usize = 200;

/// 编码成导出文本
///
/// `table` 在 [`ExportFormat::Insert`] / [`ExportFormat::SqlUpdates`] 用（表名；调用方给
/// [`default_table_name`] 的结果）。没有网格（失败 / 写语句）时返回空串——调用方该先看
/// [`ResultEntry::has_grid`]。
pub fn encode(entry: &ResultEntry, format: ExportFormat, table: &str) -> String {
    encode_rows(entry, format, table, &entry.rows)
}

/// 编码指定的行集（**本地筛选后导出**走这里：原型 §5.5 的口径是“导出的是当前筛选后的行集”）
///
/// 列名 / 表名 / 空判定仍按 `entry`（筛选只动“哪些行”，不动“哪些列”）。
///
/// 【B7 切片三】`SQL Updates` 走这里会**回绝**（返回一行 `--` 注释）：它要键列，
/// 而这里拿不到——该调 [`encode_updates`]（没有键就把这一档从菜单里收起，见 [`menu_items_for`]）。
pub fn encode_rows(
    entry: &ResultEntry,
    format: ExportFormat,
    table: &str,
    rows: &[Vec<String>],
) -> String {
    if !entry.has_grid() {
        return String::new();
    }
    match format {
        ExportFormat::Csv => encode_csv(&entry.columns, rows),
        ExportFormat::Json => encode_json(&entry.columns, rows),
        ExportFormat::Insert => encode_insert(entry, rows, table),
        ExportFormat::SqlUpdates => {
            refusal("UPDATE 需要键列（哪几列能唯一定位一行），这份导出没拿到键信息")
        }
        // WHERE 档不需要键：没键就按**全部列**拼谓词（它只是“把这几行挑出来”，
        // 挑多了（全列相同的重复行）不会改错数据，只会多带几行）
        ExportFormat::WhereClause => encode_where(entry, None, rows),
        // 走 DuckDB 的两档不在这里编码（空串 = “本地没有可写的文本”，
        // 调用方应当先看 `is_duckdb_backed`；给空串比给一份错的文本好）
        ExportFormat::Parquet | ExportFormat::Xlsx => String::new(),
    }
}

/// 【B7 切片三】带「行定位依据」的编码分发（**接线用**）：菜单给了键，落盘这里要给同一份
///
/// `key = None` 时与 [`encode_rows`] 完全一致（所以 `WHERE 子句` 档用它不加参数也能用）；
/// `key = Some` 时 `SQL Updates` 走 [`encode_updates`]，其余档不受影响。
///
/// 面板的落盘路径只需把「`encode(entry, …)` / `encode_rows(entry, …, rows)`」换成这个
/// （两处用同一个 `key`），界面就不需要认识“哪一档要键”这件闲事（那是格式自己的属性，
/// 见 [`ExportFormat::requires_row_key`]）。
pub fn encode_rows_with_key(
    entry: &ResultEntry,
    format: ExportFormat,
    table: &str,
    key: Option<&RowKey>,
    rows: &[Vec<String>],
) -> String {
    match (format, key) {
        (ExportFormat::SqlUpdates, Some(key)) => encode_updates(entry, table, key, rows),
        _ => encode_rows(entry, format, table, rows),
    }
}

/// CSV：表头一行 + 每行一条，字段按 RFC 4180 转义（含 `,` `"` 换行时用双引号包裹）
///
/// 行分隔用 `\n`（Excel 与各类命令行工具都认；CRLF 只在某些老旧 Excel 上更保险，不值得
/// 让每次导出都多一批 `\r`）。
fn encode_csv(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut text = csv_row(columns);
    for row in rows {
        text.push('\n');
        text.push_str(&csv_row(row));
    }
    text
}

fn csv_row(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| csv_field(cell))
        .collect::<Vec<_>>()
        .join(",")
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// JSON：对象数组，缩进两格（人也要读得动）
///
/// `NULL` 给 JSON 的 `null`（与网格把 `NULL` 画成斜体同一个判据）；其余值都是**字符串**
/// ——展示文本是什么就写什么，`1` 不会变数字（要类型化得有列类型，见模块说明）。
fn encode_json(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut text = String::from("[");
    for (row_ix, row) in rows.iter().enumerate() {
        if row_ix > 0 {
            text.push(',');
        }
        text.push_str("\n  {");
        for (col_ix, column) in columns.iter().enumerate() {
            if col_ix > 0 {
                text.push(',');
            }
            text.push_str(&format!(
                "\n    {}: {}",
                json_string(column),
                json_value(row.get(col_ix).map(String::as_str))
            ));
        }
        text.push_str("\n  }");
    }
    if rows.is_empty() {
        text.push(']');
    } else {
        text.push_str("\n]");
    }
    text
}

/// JSON 里的一个值：`NULL` → `null`，其余（含缺失）→ 字符串
fn json_value(value: Option<&str>) -> String {
    if is_null_cell(value) {
        "null".to_string()
    } else {
        json_string(value.unwrap_or_default())
    }
}

/// 网格里的「这一格是 NULL」判据：展示文本就是字面 `NULL`（缺列的格子也算）
///
/// **只此一处**：JSON 的 `null`、SQL 的 `NULL` 字面量、`WHERE … IS NULL` 都读它，
/// 免得同一个结果集在不同档里对“是不是 NULL”给出不同答案。
pub(crate) fn is_null_cell(value: Option<&str>) -> bool {
    matches!(value, None | Some("NULL"))
}

/// 【A9】单值 JSON：`NULL` → `null`，其余 → JSON 字符串（与 [`encode_json`] 同口径）
///
/// 右键「复制此值为 JSON」用它：一个值就是一份合法 JSON，贴进接口 / 配置直接可用。
pub(crate) fn value_to_json(value: &str) -> String {
    json_value(Some(value))
}

/// 【A9】单行 JSON 对象（右键「复制整行为 JSON」）
///
/// 单元格口径与 [`encode_json`] 完全一致（`NULL` → `null`、其余字符串），
/// 只是不包数组：一行就是一个对象，贴给接口 / 存配置时不用再剥一层。
pub(crate) fn row_to_json(columns: &[String], row: &[String]) -> String {
    let mut text = String::from("{");
    for (col_ix, column) in columns.iter().enumerate() {
        if col_ix > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            "\n  {}: {}",
            json_string(column),
            json_value(row.get(col_ix).map(String::as_str))
        ));
    }
    text.push_str("\n}");
    text
}

/// 【A9】单行 INSERT（右键「复制整行为 INSERT」）
///
/// 值字面量与标识符引号**与导出档同一套口径**（[`cell_literal`] / [`quote_ident`]）：
/// `NULL` 不加引号、数字布尔原样、其余单引号双写；表名 / 列名按需引号——
/// 同一个结果集里，右键拷走的行与导出的 INSERT 写法一致，不会两套。
pub(crate) fn row_to_insert(table: &str, columns: &[String], row: &[String]) -> String {
    let columns = columns
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Vec<_>>()
        .join(", ");
    let values = row
        .iter()
        .map(|value| cell_literal(Some(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {} ({columns}) VALUES ({values});",
        quote_ident(table)
    )
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // 其余控制字符按 JSON 规矩转义（直接写进去是非法 JSON）
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// INSERT：每 [`INSERT_ROWS_PER_STATEMENT`] 行一条语句，多行 `VALUES` 一次插入
///
/// 值字面量按**能跑起来**的写法给：`NULL` 不加引号、看起来是数字/布尔的也不加引号
/// （展示文本来自数字列时就是数字的样子）、其余一律单引号并双写内部单引号。
/// 这条启发式与 DBeaver 的默认行为同类：**字符串列里存着 `123` 会被写成数字**——
/// 按展示文本导出就必然有这个取舍。
fn encode_insert(entry: &ResultEntry, rows: &[Vec<String>], table: &str) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let columns = entry
        .columns
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Vec<_>>()
        .join(", ");
    let target = quote_ident(table);
    let mut text = String::new();
    for chunk in rows.chunks(INSERT_ROWS_PER_STATEMENT) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("INSERT INTO {target} ({columns}) VALUES\n"));
        for (row_ix, row) in chunk.iter().enumerate() {
            let values = row
                .iter()
                .map(|value| sql_literal(value))
                .collect::<Vec<_>>()
                .join(", ");
            if row_ix > 0 {
                text.push_str(",\n");
            }
            text.push_str(&format!("  ({values})"));
        }
        text.push_str(";\n");
    }
    text
}

/// SQL 值字面量（INSERT / UPDATE / WHERE 三档**共用一套口径**，见 [`encode_insert`]）
///
/// `NULL` → 字面 `NULL`；`true` / `TRUE` 这类布尔 → 大写；看起来是数字的 → 原样；
/// 其余一律单引号包裹并把内部单引号双写。
///
/// 启发式的取舍：**字符串列里存着 `123` 会被写成数字**——按展示文本导出就必然有这条，
/// 要精确得分得先有列类型（计划 B7 余项）。三档共用同一份判据，是为了同一个结果集
/// 在 INSERT / UPDATE / WHERE 里对同一个值给出同一种写法。
fn sql_literal(text: &str) -> String {
    if is_null_cell(Some(text)) {
        return "NULL".to_string();
    }
    if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
        return text.to_ascii_uppercase();
    }
    if is_numeric_literal(text) {
        return text.to_string();
    }
    format!("'{}'", text.replace('\'', "''"))
}

/// 一个格子写成 SQL 值字面量（缺格 / 展示文本是 `NULL` → 字面 `NULL`，其余走 [`sql_literal`]）
///
/// 为什么要多这层：`sql_literal("")` 的空串是**合法值**（该写 `''`），而“缺格”是另一回事
/// （行比列短——分段抓取的边界），两者不能混。`NULL` 的判据仍只此一处（[`is_null_cell`]）。
pub(crate) fn cell_literal(cell: Option<&str>) -> String {
    if is_null_cell(cell) {
        "NULL".to_string()
    } else {
        sql_literal(cell.unwrap_or_default())
    }
}

/// 【B7 切片三】`UPDATE <表> SET <非键列> = <值>, … WHERE <键列> = <值>;`（每行一条）
///
/// ## 口径
///
/// - **键列不进 `SET`**：它是定位用的，不是要改的值。
/// - **`SET` 里给全部非键列**（不管用户是不是只改了一格）：我们手里只有**一份快照**
///   （`ResultEntry` 不记“哪些格被改过”），于是“把这一行写成结果里的样子”是唯一说得清的语义。
/// - 值字面量用 [`sql_literal`]（与 INSERT 档同一套启发式）。
/// - 多列键（复合键）拼成 `k1 = v1 AND k2 = v2`，顺序就是 [`RowKey`] 给入的顺序。
///
/// ## 回绝（宁可回绝也不猜）
///
/// 返回文本以 `--` 开头（SQL 注释，贴进编辑器不会误跑）：
///
/// - **键列不在结果列里** / 结果只有键列（没有可 `SET` 的列）：整份回绝。
///   （不静默降级成“拿第一列当键”——那不是降级，是改错行。）
/// - **某一行的键值是 `NULL`**：`WHERE k = NULL` 永远不成立，而 `IS NULL` 又定位不到**一行**
///   ——那一行**跳过并在文首说明**，其它行照常给（全拒了用户还得自己找哪行有问题）。
pub fn encode_updates(
    entry: &ResultEntry,
    table: &str,
    key: &RowKey,
    rows: &[Vec<String>],
) -> String {
    if !entry.has_grid() {
        return String::new();
    }
    let Some(key_ix) = key.indices(&entry.columns) else {
        return refusal(&format!(
            "键列 {} 不在结果列里（列名要按结果集里的原样给）",
            key.columns().join(", ")
        ));
    };
    let value_ix: Vec<usize> = (0..entry.columns.len())
        .filter(|ix| !key_ix.contains(ix))
        .collect();
    if value_ix.is_empty() {
        return refusal("结果里只有键列，没有可 SET 的列");
    }

    let target = quote_ident(table);
    let mut lines = Vec::with_capacity(rows.len());
    let mut skipped = 0usize;
    for row in rows {
        // 键值里有 NULL（或缺列）→ 这一行定位不到，跳过
        if key_ix
            .iter()
            .any(|ix| is_null_cell(row.get(*ix).map(String::as_str)))
        {
            skipped += 1;
            continue;
        }
        let assignments = value_ix
            .iter()
            .map(|ix| {
                format!(
                    "{} = {}",
                    quote_ident(&entry.columns[*ix]),
                    cell_literal(row.get(*ix).map(String::as_str))
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let predicate = key_ix
            .iter()
            .map(|ix| {
                format!(
                    "{} = {}",
                    quote_ident(&entry.columns[*ix]),
                    cell_literal(row.get(*ix).map(String::as_str))
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        lines.push(format!(
            "UPDATE {target} SET {assignments} WHERE {predicate};"
        ));
    }

    let mut text = String::new();
    if skipped > 0 {
        text.push_str(&refusal(&format!(
            "{skipped} 行的键值是 NULL（定位不到一行），已跳过"
        )));
    }
    if lines.is_empty() {
        return if text.is_empty() {
            refusal("没有行可生成 UPDATE")
        } else {
            text
        };
    }
    text.push_str(&lines.join("\n"));
    text.push('\n');
    text
}

/// 【B7 切片三】只出谓词：`WHERE\n  (… AND …)\n  OR (…)`;
///
/// ## 语义（口径定在这里，调用方不用再猜）
///
/// - **每组括号 = 一行**：括号内各列用 `AND` 串（全列相等），行与行用 `OR` 串。
///   所以它是“把这几行挑出来”的谓词，贴进 `DELETE … WHERE` / `UPDATE … SET … WHERE` /
///   带筛选的 `SELECT` 都对得上。
/// - **条件列**：`key` 给了就**只用键列**（短、能命中索引，且能区分全列相同的重复行）；
///   没给就**用全部列**（不需要表结构就能用）。两档不混用：一旦给了键列却发现它不在
///   结果列里，就回绝（一声不响地改成全列，语义变了却看不出来）。
/// - **`NULL` 写成 `IS NULL`**：`列 = NULL` 在 SQL 里永远不成立（三值逻辑），照那样生成就是
///   一份“看着对、跑起来一行也匹配不上”的谓词。
/// - **没有行**时给 `WHERE FALSE;`：空谓词等于“匹配所有行”，那是最危险的一种“差不多”。
pub fn encode_where(entry: &ResultEntry, key: Option<&RowKey>, rows: &[Vec<String>]) -> String {
    if !entry.has_grid() {
        return String::new();
    }
    let columns_ix: Vec<usize> = match key {
        Some(key) => match key.indices(&entry.columns) {
            Some(ix) => ix,
            None => {
                return refusal(&format!(
                    "键列 {} 不在结果列里（列名要按结果集里的原样给）",
                    key.columns().join(", ")
                ));
            }
        },
        None => (0..entry.columns.len()).collect(),
    };
    if rows.is_empty() {
        return "WHERE FALSE;\n".to_string();
    }

    let clauses = rows
        .iter()
        .map(|row| {
            let conditions = columns_ix
                .iter()
                .map(|ix| {
                    let column = quote_ident(&entry.columns[*ix]);
                    let cell = row.get(*ix).map(String::as_str);
                    if is_null_cell(cell) {
                        format!("{column} IS NULL")
                    } else {
                        format!("{column} = {}", cell_literal(cell))
                    }
                })
                .collect::<Vec<_>>()
                .join(" AND ");
            format!("({conditions})")
        })
        .collect::<Vec<_>>();
    format!("WHERE\n  {};\n", clauses.join("\n  OR "))
}

/// 回绝：宁可给一段说清「为什么不给」的注释，也不给一份猜出来的 SQL
///
/// `--` 前缀是形状的一部分：SQL 里它就是注释，用户把导出的文件当脚本跑也不会跑错东西。
fn refusal(reason: &str) -> String {
    format!("-- 无法生成：{reason}\n")
}

/// 是不是一个能直接当数字字面量用的文本
///
/// 只认**能原样嵌进 SQL 的数字形态**：`123` / `-1.5` / `1e9`。`007` 也算数字（前导零在
/// 数值字面量里合法），但科学计数法要排除掉逗号与下划线这类只在显示里出现的写法。
fn is_numeric_literal(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let mut chars = text.chars().peekable();
    if matches!(chars.peek(), Some('-') | Some('+')) {
        chars.next();
    }
    let mut digits = false;
    let mut dot = false;
    let mut exponent = false;
    while let Some(ch) = chars.next() {
        match ch {
            '0'..='9' => digits = true,
            '.' if !dot && !exponent => dot = true,
            'e' | 'E' if digits && !exponent => {
                exponent = true;
                // 指数部分允许带符号（`1e-9`）
                if matches!(chars.peek(), Some('-') | Some('+')) {
                    chars.next();
                }
            }
            _ => return false,
        }
    }
    digits
}

/// 标识符：简单名字原样，其余用双引号包裹（内部引号双写）
///
/// MySQL 默认不把双引号当标识符引号——那是方言的事，这里不猜；含特殊字符的列名本来就该
/// 让用户看到引号（比默默生成一条跑不了的 INSERT 好）。
pub(crate) fn quote_ident(name: &str) -> String {
    let simple = !name.is_empty()
        && name.chars().enumerate().all(|(ix, ch)| {
            ch == '_' || ch.is_ascii_alphanumeric() && !(ix == 0 && ch.is_ascii_digit())
        });
    if simple {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：显式列举（避免与 gpui-kit 测试宏冲突）
    use super::{ExportFormat, RowKey, encode, encode_rows_with_key, encode_updates, encode_where};
    use crate::export::default_file_name;
    use crate::store::ResultEntry;

    /// 结果集（`sql` 决定默认表名，`rows` 就是网格里那些字符串）
    fn entry(columns: &[&str], rows: &[&[&str]]) -> ResultEntry {
        ResultEntry::success(
            workbench_shell::model::DocumentId::new("doc-1"),
            "SELECT * FROM orders".to_string(),
            12,
            false,
            columns.iter().map(|c| c.to_string()).collect(),
            rows.iter()
                .map(|row| row.iter().map(|v| v.to_string()).collect())
                .collect(),
        )
    }
    #[test]
    fn csv_escapes_rfc4180_fields() {
        let entry = entry(
            &["id", "name", "note"],
            &[&["1", "a,b", "say \"hi\""], &["2", "line\nbreak", "NULL"]],
        );
        let text = encode(&entry, ExportFormat::Csv, "orders");
        assert_eq!(
            text, "id,name,note\n1,\"a,b\",\"say \"\"hi\"\"\"\n2,\"line\nbreak\",NULL",
            "逗号 / 引号 / 换行都要按 RFC 4180 转义；NULL 按展示文本原样"
        );
    }

    #[test]
    fn json_gives_null_for_null_and_escapes_strings() {
        let entry = entry(&["id", "note"], &[&["1", "NULL"], &["2", "a\"b\\c\td"]]);
        let text = encode(&entry, ExportFormat::Json, "orders");
        assert_eq!(
            text,
            "[\n  {\n    \"id\": \"1\",\n    \"note\": null\n  },\n  {\n    \"id\": \"2\",\n    \"note\": \"a\\\"b\\\\c\\td\"\n  }\n]",
            "NULL 是 JSON 的 null；引号 / 反斜杠 / 制表符要转义（值本身是字符串）"
        );
    }

    #[test]
    fn json_empty_result_is_an_empty_array() {
        let entry = entry(&["id"], &[]);
        assert_eq!(encode(&entry, ExportFormat::Json, "orders"), "[]");
    }

    #[test]
    fn insert_literals_keep_numbers_and_quote_strings() {
        let entry = entry(
            &["id", "name", "ok", "note"],
            &[
                &["1", "o'brien", "true", "NULL"],
                &["2.5", "中文", "FALSE", "-1e3"],
            ],
        );
        let text = encode(&entry, ExportFormat::Insert, "orders");
        assert_eq!(
            text,
            "INSERT INTO orders (id, name, ok, note) VALUES\n  (1, 'o''brien', TRUE, NULL),\n  (2.5, '中文', FALSE, -1e3);\n",
            "数字与布尔不加引号、NULL 是字面量、字符串单引号包裹并把内部单引号双写"
        );
    }

    #[test]
    fn insert_splits_into_multiple_statements() {
        // 201 行 → 两条语句（200 + 1）：SQLite 的多行 VALUES 有编译期上限
        let rows: Vec<Vec<String>> = (1..=201).map(|n| vec![n.to_string()]).collect();
        let entry = ResultEntry::success(
            workbench_shell::model::DocumentId::new("doc-1"),
            "SELECT n FROM t".to_string(),
            10,
            false,
            vec!["n".to_string()],
            rows,
        );
        let text = encode(&entry, ExportFormat::Insert, "t");
        assert_eq!(
            text.matches("INSERT INTO t (n) VALUES").count(),
            2,
            "201 行要分两条语句"
        );
        assert!(text.ends_with(";\n"), "每条语句以分号结束");
    }

    #[test]
    fn insert_quotes_awkward_identifiers_only() {
        let mut entry = entry(&["weird name", "ok"], &[&["1", "2"]]);
        entry.columns = vec!["weird name".to_string(), "ok".to_string()];
        let text = encode(&entry, ExportFormat::Insert, "my table");
        assert!(
            text.starts_with("INSERT INTO \"my table\" (\"weird name\", ok) VALUES"),
            "含空格的标识符才加引号：{text}"
        );
    }

    #[test]
    fn failed_or_write_only_results_encode_to_nothing() {
        let failed = ResultEntry::failure(
            workbench_shell::model::DocumentId::new("doc-1"),
            "SELECT nope".to_string(),
            "no such column: nope".to_string(),
            5,
        );
        assert_eq!(encode(&failed, ExportFormat::Csv, "t"), "");
        assert_eq!(encode(&failed, ExportFormat::Json, "t"), "");
        assert_eq!(encode(&failed, ExportFormat::Insert, "t"), "");
        assert_eq!(encode(&failed, ExportFormat::SqlUpdates, "t"), "");
        assert_eq!(encode(&failed, ExportFormat::WhereClause, "t"), "");
        assert_eq!(encode_updates(&failed, "t", &RowKey::single("id"), &[]), "");
        assert_eq!(encode_where(&failed, None, &[]), "");
    }

    #[test]
    fn row_json_matches_export_cell_rules() {
        use super::{row_to_json, value_to_json};
        // 单值：NULL → null，其余 JSON 字符串（引号 / 换行转义）
        assert_eq!(value_to_json("NULL"), "null");
        assert_eq!(value_to_json("abc"), "\"abc\"");
        assert_eq!(value_to_json("a\"b"), "\"a\\\"b\"");
        assert_eq!(value_to_json("a\nb"), "\"a\\nb\"");
        // 行对象：列名与值同口径（NULL → null，字符串照写）
        let columns = vec!["id".to_string(), "name".to_string(), "note".to_string()];
        let row = vec!["1".to_string(), "a\"b".to_string(), "NULL".to_string()];
        assert_eq!(
            row_to_json(&columns, &row),
            "{\n  \"id\": \"1\",\n  \"name\": \"a\\\"b\",\n  \"note\": null\n}"
        );
        // 缺列（分段抓取边界）：按缺失处理
        let short = vec!["1".to_string()];
        assert_eq!(
            row_to_json(&columns, &short),
            "{\n  \"id\": \"1\",\n  \"name\": null,\n  \"note\": null\n}"
        );
    }

    #[test]
    fn row_insert_shares_export_literal_rules() {
        use super::row_to_insert;
        let columns = vec!["id".to_string(), "name".to_string(), "qty".to_string()];
        // NULL 不加引号、数字原样、字符串单引号双写
        let row = vec!["1".to_string(), "O'Brien".to_string(), "NULL".to_string()];
        assert_eq!(
            row_to_insert("orders", &columns, &row),
            "INSERT INTO orders (id, name, qty) VALUES (1, 'O''Brien', NULL);"
        );
        // 标识符按需引号（只有含特殊字符的才包）
        let quoted = vec!["select".to_string(), "Order Details".to_string()];
        assert_eq!(
            row_to_insert("orders", &quoted, &["x".to_string(), "y".to_string()]),
            "INSERT INTO orders (select, \"Order Details\") VALUES ('x', 'y');"
        );
        // 表名同样按需引号
        let plain = vec!["id".to_string()];
        assert_eq!(
            row_to_insert("Order Details", &plain, &["7".to_string()]),
            "INSERT INTO \"Order Details\" (id) VALUES (7);"
        );
    }

    /// 三档 SQL 文本**共用一套**字面量口径：转义、数字 / 布尔不加引号、NULL 是字面量
    #[test]
    fn sql_text_formats_share_one_literal_rule() {
        let entry = entry(
            &["id", "name", "ok", "note"],
            &[
                &["1", "o'brien", "TRUE", "NULL"],
                &["2.5", "中文", "false", "-1e3"],
            ],
        );
        // UPDATE：键列不进 SET，其余列全给
        assert_eq!(
            encode_updates(&entry, "orders", &RowKey::single("id"), &entry.rows),
            "UPDATE orders SET name = 'o''brien', ok = TRUE, note = NULL WHERE id = 1;\n\
             UPDATE orders SET name = '中文', ok = FALSE, note = -1e3 WHERE id = 2.5;\n",
            "与 INSERT 档同一套字面量；`o'brien` 的单引号要双写"
        );
        // 无键的 WHERE：条件列 = **全部列**，每行一个括号
        assert_eq!(
            encode_where(&entry, None, &entry.rows),
            "WHERE\n  (id = 1 AND name = 'o''brien' AND ok = TRUE AND note IS NULL)\n  \
             OR (id = 2.5 AND name = '中文' AND ok = FALSE AND note = -1e3);\n",
            "`= NULL` 永远不成立，要写 `IS NULL`"
        );
        // 有键的 WHERE：只用键列（短、能命中索引）
        assert_eq!(
            encode_where(&entry, Some(&RowKey::single("id")), &entry.rows),
            "WHERE\n  (id = 1)\n  OR (id = 2.5);\n",
            "给了键列就只用它，不混用全列"
        );
    }

    /// 复合键：顺序就是调用方给入的顺序；键值是 NULL 的行**跳过并在文首说明**
    #[test]
    fn updates_use_a_composite_key_and_skip_null_keys() {
        let entry = entry(
            &["id", "tenant", "name"],
            &[
                &["1", "a", "x"],
                &["NULL", "a", "该行没有键"],
                &["3", "b", "z"],
            ],
        );
        let key = RowKey::new(["tenant", "id"]).expect("两列复合键");
        assert_eq!(
            encode_updates(&entry, "orders", &key, &entry.rows),
            "-- 无法生成：1 行的键值是 NULL（定位不到一行），已跳过\n\
             UPDATE orders SET name = 'x' WHERE tenant = 'a' AND id = 1;\n\
             UPDATE orders SET name = 'z' WHERE tenant = 'b' AND id = 3;\n",
            "`WHERE k = NULL` 永远不成立，而 `IS NULL` 可能匹配多行：只能跳过"
        );
    }

    /// 回绝口径（宁可回绝也不猜）：每种“拿不准”都有自己的说法
    #[test]
    fn sql_text_format_refusals_say_why() {
        let grid = entry(&["id", "name"], &[&["1", "x"]]);
        assert_eq!(
            encode_updates(&grid, "orders", &RowKey::single("oid"), &grid.rows),
            "-- 无法生成：键列 oid 不在结果列里（列名要按结果集里的原样给）\n",
            "键列不在结果里：不静默降级成“拿第一列当键”"
        );
        assert_eq!(
            encode_where(&grid, Some(&RowKey::single("oid")), &grid.rows),
            "-- 无法生成：键列 oid 不在结果列里（列名要按结果集里的原样给）\n"
        );
        let only_key = entry(&["id"], &[&["1"]]);
        assert_eq!(
            encode_updates(&only_key, "orders", &RowKey::single("id"), &only_key.rows),
            "-- 无法生成：结果里只有键列，没有可 SET 的列\n",
            "没有可改的列：不生成一句空 SET 的 UPDATE"
        );
        let no_rows = entry(&["id", "name"], &[]);
        assert_eq!(
            encode_updates(&no_rows, "orders", &RowKey::single("id"), &[]),
            "-- 无法生成：没有行可生成 UPDATE\n"
        );
        let all_null_keys = entry(&["id", "name"], &[&["NULL", "x"]]);
        assert_eq!(
            encode_updates(
                &all_null_keys,
                "orders",
                &RowKey::single("id"),
                &all_null_keys.rows
            ),
            "-- 无法生成：1 行的键值是 NULL（定位不到一行），已跳过\n",
            "全被跳过时给的就是一行说明（不静默给空文件）"
        );
        // 编码层直调（没有键信息）：回绝，不给一份猜的 UPDATE
        assert_eq!(
            encode(&grid, ExportFormat::SqlUpdates, "orders"),
            "-- 无法生成：UPDATE 需要键列（哪几列能唯一定位一行），这份导出没拿到键信息\n"
        );
        // WHERE 档：没有行时给 `WHERE FALSE`（空谓词 = 匹配所有行，那是最危险的一种“差不多”）
        assert_eq!(
            encode(&no_rows, ExportFormat::WhereClause, "orders"),
            "WHERE FALSE;\n"
        );
    }

    /// 行比列短（抓取中的边界）：缺的格子按 NULL 算，与 JSON / INSERT 档同一条判据
    #[test]
    fn short_rows_treat_missing_cells_as_null() {
        let entry = entry(&["id", "note"], &[&["1"]]);
        assert_eq!(
            encode(&entry, ExportFormat::WhereClause, "orders"),
            "WHERE\n  (id = 1 AND note IS NULL);\n"
        );
        assert_eq!(
            encode_updates(&entry, "orders", &RowKey::single("id"), &entry.rows),
            "UPDATE orders SET note = NULL WHERE id = 1;\n"
        );
    }

    /// `RowKey`：空列表不是键；列名大小写敏感（结果集里叫什么就给什么）
    #[test]
    fn row_key_is_not_guessed() {
        assert!(RowKey::new(Vec::<String>::new()).is_none(), "空列表不是键");
        assert_eq!(RowKey::single("id").columns(), ["id".to_string()]);
        assert_eq!(
            RowKey::new(["tenant", "id"]).unwrap().columns(),
            ["tenant".to_string(), "id".to_string()]
        );
        let entry = entry(&["ID", "name"], &[&["1", "x"]]);
        assert!(
            encode_updates(&entry, "orders", &RowKey::single("id"), &entry.rows)
                .starts_with("-- 无法生成："),
            "`id` 与结果里的 `ID` 不是同一列（大小写敏感，不给就回绝）"
        );
        assert_eq!(
            encode_updates(&entry, "orders", &RowKey::single("ID"), &entry.rows),
            "UPDATE orders SET name = 'x' WHERE ID = 1;\n"
        );
    }

    /// 接线用的分发：同一个 `key` 同时给菜单与落盘；没键时与 `encode_rows` 一致
    #[test]
    fn encode_rows_with_key_dispatches_only_for_updates() {
        let grid = entry(&["id", "name"], &[&["1", "x"]]);
        let key = RowKey::single("id");
        assert_eq!(
            encode_rows_with_key(
                &grid,
                ExportFormat::SqlUpdates,
                "orders",
                Some(&key),
                &grid.rows
            ),
            "UPDATE orders SET name = 'x' WHERE id = 1;\n",
            "有键时 SQL Updates 真的出得来"
        );
        assert_eq!(
            encode_rows_with_key(&grid, ExportFormat::SqlUpdates, "orders", None, &grid.rows),
            encode(&grid, ExportFormat::SqlUpdates, "orders"),
            "没键时回绝的口径与 `encode_rows` 一致"
        );
        for format in [
            ExportFormat::Csv,
            ExportFormat::Json,
            ExportFormat::Insert,
            ExportFormat::WhereClause,
        ] {
            assert_eq!(
                encode_rows_with_key(&grid, format, "orders", Some(&key), &grid.rows),
                encode(&grid, format, "orders"),
                "{} 不受键影响",
                format.label()
            );
        }
    }

    /// 两档的元数据：与 INSERT 档共用 `.sql` 扩展名；哪一档要键由数据（不是调用点）说
    #[test]
    fn sql_text_format_metadata_is_complete() {
        let labels: Vec<&str> = ExportFormat::SQL_TEXT.iter().map(|f| f.label()).collect();
        assert_eq!(labels, ["SQL Updates", "WHERE 子句"]);
        let extensions: Vec<&str> = ExportFormat::SQL_TEXT
            .iter()
            .map(|f| f.extension())
            .collect();
        assert_eq!(extensions, ["sql", "sql"], "三档 SQL 文本共用一个扩展名");
        for format in ExportFormat::SQL_TEXT {
            assert!(!format.is_duckdb_backed(), "{} 在本地编码", format.label());
            assert!(!format.detail().is_empty(), "文件对话框说明不能空着");
            assert_eq!(
                default_file_name(Some("SELECT * FROM orders"), 1, format),
                "orders.sql",
                "默认文件名跟 INSERT 档同口径"
            );
        }
        assert_eq!(
            default_file_name(None, 2, ExportFormat::WhereClause),
            "result_2.sql"
        );
        assert!(ExportFormat::SqlUpdates.requires_row_key(), "UPDATE 要键列");
        assert!(
            !ExportFormat::WhereClause.requires_row_key(),
            "WHERE 子句不要键列"
        );
        // `ALL` 是“有网格就能给”的那几档：新两档不塞进去（塞了所有调用方的菜单都会变）
        assert!(!ExportFormat::ALL.contains(&ExportFormat::SqlUpdates));
        assert!(!ExportFormat::ALL.contains(&ExportFormat::WhereClause));
    }
}

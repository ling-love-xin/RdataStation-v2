//! 结果集导出（B7 切片一：CSV / JSON / INSERT）
//!
//! ## 三条口径
//!
//! - **值按展示文本导出**：`ResultEntry` 里的值就是网格里那些字符串，`NULL` 也是字面 `NULL`
//!   （B5 定的字符串化口径）。所以 CSV 里 NULL 与字符串 `"NULL"` 同形、JSON 里给 `null`
//!   （与网格的斜体判据同源）——要类型化得先有列类型，那是另一笔工作（计划 B7 余项）。
//! - **只导出已抓到的行**：分段抓取只把窗口里的行拿回来（`ExportScope::Fetched`）；「抓全量后
//!   导出」是另一档，取数由面板循环取段（`fetch_next`），本模块只管编码与命名。
//! - **纯函数**：编码、默认表名、默认文件名都在这里，写成能逐条断言的形状；落盘在面板里。
//!
//! Parquet / XLSX 属切片二：它们要经 DuckDB（`COPY … TO … (FORMAT …)`）与对应扩展，
//! 没实现就不在菜单里摆（与「没实现就不摆按钮」同一口径）。

use crate::store::ResultEntry;

/// 导出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// 逗号分隔文本（RFC 4180 的转义规则）
    Csv,
    /// 对象数组（`[{"列": "值"}, …]`）
    Json,
    /// `INSERT INTO … VALUES (…);`（每 200 行一条语句）
    Insert,
}

impl ExportFormat {
    /// 菜单里的顺序（也是 `ALL` 的顺序）
    pub const ALL: [ExportFormat; 3] = [Self::Csv, Self::Json, Self::Insert];

    pub fn label(self) -> &'static str {
        match self {
            Self::Csv => "CSV",
            Self::Json => "JSON",
            Self::Insert => "INSERT",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Insert => "sql",
        }
    }

    /// 给文件对话框用的扩展名（与 `label` 不同：这里要小写、不带点）
    pub fn detail(self) -> &'static str {
        match self {
            Self::Csv => "逗号分隔文本",
            Self::Json => "对象数组（值按展示文本；NULL 是 null）",
            Self::Insert => "INSERT 语句（每 200 行一条）",
        }
    }
}

/// 导出范围（原型 §2.4 的「仅已抓取 / 抓全量」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportScope {
    /// 只导出已经抓到的行（不发新查询）
    Fetched,
    /// 先把剩下的段抓完，再导出（会重跑查询，见 B5b 的取舍）
    All,
}

impl ExportScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fetched => "仅已抓取",
            Self::All => "抓全量后导出",
        }
    }
}

/// 导出菜单里的一项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportMenuItem {
    pub label: String,
    /// 可点项的（格式，范围）；`None` = 分组标题（不可点）
    pub action: Option<(ExportFormat, ExportScope)>,
    /// 这项前面要不要加一条分隔线
    pub separator_before: bool,
}

/// 导出菜单的项（**纯函数**：界面只负责把这份列表画出来）
///
/// 「抓全量」那一组只在 `has_more` 时出现——已经抓完了再摆就是骗人（点下去只是一次多余的重跑）。
/// `rows_text` 是已抓行数的展示文本（千分位由调用方格式化）。
pub fn menu_items(has_more: bool, rows_text: &str) -> Vec<ExportMenuItem> {
    let mut items = Vec::new();
    if has_more {
        items.push(ExportMenuItem {
            label: format!("已抓取 {rows_text} 行"),
            action: None,
            separator_before: false,
        });
    }
    for format in ExportFormat::ALL {
        items.push(ExportMenuItem {
            label: format.label().to_string(),
            action: Some((format, ExportScope::Fetched)),
            separator_before: false,
        });
    }
    if has_more {
        items.push(ExportMenuItem {
            label: "抓全量后导出（会重跑查询）".to_string(),
            action: None,
            separator_before: true,
        });
        for format in ExportFormat::ALL {
            items.push(ExportMenuItem {
                label: format.label().to_string(),
                action: Some((format, ExportScope::All)),
                separator_before: false,
            });
        }
    }
    items
}

/// 一条 INSERT 语句最多带几行（SQLite 对多行 VALUES 有编译期上限，200 行对四库都稳）
const INSERT_ROWS_PER_STATEMENT: usize = 200;

/// 编码成导出文本
///
/// `table` 只在 [`ExportFormat::Insert`] 用（表名；调用方给 [`default_table_name`] 的结果）。
/// 没有网格（失败 / 写语句）时返回空串——调用方该先看 [`ResultEntry::has_grid`]。
pub fn encode(entry: &ResultEntry, format: ExportFormat, table: &str) -> String {
    encode_rows(entry, format, table, &entry.rows)
}

/// 编码指定的行集（**本地筛选后导出**走这里：原型 §5.5 的口径是“导出的是当前筛选后的行集”）
///
/// 列名 / 表名 / 空判定仍按 `entry`（筛选只动“哪些行”，不动“哪些列”）。
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
    match value {
        Some("NULL") | None => "null".to_string(),
        Some(text) => json_string(text),
    }
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
                .map(|value| insert_literal(value))
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

/// SQL 值字面量（见 [`encode_insert`] 的口径）
fn insert_literal(text: &str) -> String {
    if text == "NULL" {
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
fn quote_ident(name: &str) -> String {
    let simple = !name.is_empty()
        && name
            .chars()
            .enumerate()
            .all(|(ix, ch)| ch == '_' || ch.is_ascii_alphanumeric() && !(ix == 0 && ch.is_ascii_digit()));
    if simple {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// 默认表名：从结果集的 SQL 里猜首个 `FROM` 表，猜不出用 `result_<序号>`
///
/// 词法级（不认识 AST）：`FROM (SELECT …)` 这种子查询**放弃猜**并回退到 `result_N`——
/// 猜错表名比给个中性的默认名更糟（用户会照着一个不存在的表去插）。
pub fn default_table_name(sql: Option<&str>, index: usize) -> String {
    sql.and_then(table_from_sql)
        .unwrap_or_else(|| format!("result_{index}"))
}

/// 默认文件名（带扩展名）：`orders.csv` / `result_1.csv`
pub fn default_file_name(sql: Option<&str>, index: usize, format: ExportFormat) -> String {
    format!(
        "{}.{}",
        default_table_name(sql, index),
        format.extension()
    )
}

/// 从 SQL 文本里取首个 `FROM` 后面的表名（词法级）
///
/// - 大小写不敏感，`FROM` 必须是独立单词（`performance.fromage` 不算）
/// - 去掉 schema / catalog 前缀（`public.orders` → `orders`）
/// - 后面紧跟 `(` 的当子查询，返回 `None`
pub fn table_from_sql(sql: &str) -> Option<String> {
    let bytes = sql.as_bytes();
    let mut ix = 0;
    while ix < bytes.len() {
        // 找 `FROM`（按字节扫：SQL 里的关键字都是 ASCII）
        if !bytes[ix].eq_ignore_ascii_case(&b'f') {
            ix += 1;
            continue;
        }
        if ix + 4 > bytes.len() || !bytes[ix..ix + 4].eq_ignore_ascii_case(b"from") {
            ix += 1;
            continue;
        }
        let before_ok = ix == 0 || !is_ident_byte(bytes[ix - 1]);
        let after = ix + 4;
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if !before_ok || !after_ok {
            ix += 1;
            continue;
        }
        let rest = &sql[after..];
        let trimmed = rest.trim_start();
        if trimmed.starts_with('(') {
            // 子查询 / 表函数：`FROM (SELECT …)` —— 不猜
            return None;
        }
        return table_name_from(trimmed);
    }
    None
}

/// 从 `FROM` 后面那段文本里读出表名（可带 `schema.` 前缀，可为带引号的标识符）
///
/// 逐段读：`public.orders` / `"sch"."Orders"` 都读到最后一段——**最后一段才是表名**
/// （INSERT 的目标名用的是表本身，不带 schema）。
fn table_name_from(text: &str) -> Option<String> {
    let mut rest = text;
    let mut last: Option<String> = None;
    loop {
        let segment = if let Some(closing) = rest
            .chars()
            .next()
            .filter(|ch| matches!(ch, '"' | '`' | '['))
            .map(|quote| if quote == '[' { ']' } else { quote })
        {
            // 带引号的标识符：读到闭合引号（要记住多消耗掉的那一个）
            let mut name = String::new();
            let mut consumed = rest.chars().next().map(char::len_utf8).unwrap_or(0);
            for ch in rest.chars().skip(1) {
                consumed += ch.len_utf8();
                if ch == closing {
                    break;
                }
                name.push(ch);
            }
            rest = &rest[consumed..];
            name
        } else {
            let name: String = rest.chars().take_while(|ch| is_ident_char(*ch)).collect();
            rest = &rest[name.len()..];
            name
        };
        if segment.is_empty() {
            break;
        }
        last = Some(segment);
        let after_dot = rest.trim_start();
        match after_dot.strip_prefix('.') {
            Some(tail) => rest = tail.trim_start(),
            None => break,
        }
    }
    last
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

#[cfg(test)]
mod tests {
    use super::{
        ExportFormat, ExportScope, default_file_name, default_table_name, encode, table_from_sql,
    };
    use crate::store::ResultEntry;

    /// 结果集（`sql` 决定默认表名，`rows` 就是网格里那些字符串）
    fn entry(columns: &[&str], rows: &[&[&str]]) -> ResultEntry {
        ResultEntry::success(
            crate::model::DocumentId::new("doc-1"),
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
            &[
                &["1", "a,b", "say \"hi\""],
                &["2", "line\nbreak", "NULL"],
            ],
        );
        let text = encode(&entry, ExportFormat::Csv, "orders");
        assert_eq!(
            text,
            "id,name,note\n1,\"a,b\",\"say \"\"hi\"\"\"\n2,\"line\nbreak\",NULL",
            "逗号 / 引号 / 换行都要按 RFC 4180 转义；NULL 按展示文本原样"
        );
    }

    #[test]
    fn json_gives_null_for_null_and_escapes_strings() {
        let entry = entry(
            &["id", "note"],
            &[&["1", "NULL"], &["2", "a\"b\\c\td"]],
        );
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
            crate::model::DocumentId::new("doc-1"),
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
            crate::model::DocumentId::new("doc-1"),
            "SELECT nope".to_string(),
            "no such column: nope".to_string(),
            5,
        );
        assert_eq!(encode(&failed, ExportFormat::Csv, "t"), "");
        assert_eq!(encode(&failed, ExportFormat::Json, "t"), "");
        assert_eq!(encode(&failed, ExportFormat::Insert, "t"), "");
    }

    #[test]
    fn table_name_comes_from_the_first_from() {
        assert_eq!(
            table_from_sql("SELECT * FROM orders WHERE id = 1").as_deref(),
            Some("orders")
        );
        assert_eq!(
            table_from_sql("select * from public.orders o").as_deref(),
            Some("orders"),
            "schema 前缀去掉"
        );
        assert_eq!(
            table_from_sql("SELECT * FROM \"Orders\"").as_deref(),
            Some("Orders"),
            "引号要去掉"
        );
        assert_eq!(
            table_from_sql("SELECT * FROM `orders` o").as_deref(),
            Some("orders"),
            "MySQL 的反引号同样认"
        );
        assert_eq!(
            table_from_sql("SELECT * FROM \"public\".\"Orders\"").as_deref(),
            Some("Orders"),
            "逐段读、取最后一段"
        );
        assert_eq!(
            table_from_sql("SELECT 1 FROM\n  orders").as_deref(),
            Some("orders"),
            "换行与空白没关系"
        );
        // 子查询：猜不出就不猜（宁可用 result_N）
        assert!(table_from_sql("SELECT * FROM (SELECT 1) x").is_none());
        // 没有 FROM / 只是名字里带 from
        assert!(table_from_sql("SELECT 1").is_none());
        assert!(table_from_sql("SELECT fromage FROM t").as_deref() == Some("t"));
    }

    #[test]
    fn default_names_fall_back_to_result_index() {
        assert_eq!(default_table_name(Some("SELECT * FROM orders"), 1), "orders");
        assert_eq!(default_table_name(Some("SELECT 1"), 2), "result_2");
        assert_eq!(default_table_name(None, 3), "result_3");
        assert_eq!(
            default_file_name(Some("SELECT * FROM orders"), 1, ExportFormat::Json),
            "orders.json"
        );
        assert_eq!(
            default_file_name(None, 2, ExportFormat::Insert),
            "result_2.sql"
        );
    }

    #[test]
    fn menu_items_grow_with_a_next_segment() {
        use super::menu_items;

        let fetched: Vec<(String, Option<(ExportFormat, ExportScope)>)> = menu_items(false, "1,000")
            .into_iter()
            .map(|item| (item.label, item.action))
            .collect();
        assert_eq!(
            fetched,
            vec![
                ("CSV".to_string(), Some((ExportFormat::Csv, ExportScope::Fetched))),
                ("JSON".to_string(), Some((ExportFormat::Json, ExportScope::Fetched))),
                ("INSERT".to_string(), Some((ExportFormat::Insert, ExportScope::Fetched))),
            ],
            "已经抓完时只给“仅已抓取”三项（不摆一个多余的重跑入口）"
        );

        let items = menu_items(true, "1,000");
        let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();
        assert_eq!(
            labels,
            vec![
                "已抓取 1,000 行",
                "CSV",
                "JSON",
                "INSERT",
                "抓全量后导出（会重跑查询）",
                "CSV",
                "JSON",
                "INSERT",
            ],
            "还有下一段时：先说清已抓多少，再给两档范围"
        );
        assert!(
            items[4].separator_before,
            "“抓全量”那一组前面要有分隔线"
        );
        assert!(
            items[0].action.is_none() && items[4].action.is_none(),
            "两个分组标题不可点"
        );
        assert_eq!(
            items[5].action,
            Some((ExportFormat::Csv, ExportScope::All)),
            "第二组指向“抓全量”"
        );
    }

    #[test]
    fn format_metadata_is_complete() {
        let labels: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.label()).collect();
        assert_eq!(labels, ["CSV", "JSON", "INSERT"]);
        let extensions: Vec<&str> = ExportFormat::ALL.iter().map(|f| f.extension()).collect();
        assert_eq!(extensions, ["csv", "json", "sql"]);
        assert_eq!(ExportScope::Fetched.label(), "仅已抓取");
        assert_eq!(ExportScope::All.label(), "抓全量后导出");
    }
}

//! CSV 解析（零依赖纯函数）
//!
//! 只做「拆成单元格」这一件事：`parse` 返回每行字段数组，供编辑器的表格展示
//! （遗留 #14 的 CSV 走表格、不上文本色）。不负责类型推断 / 方言探测——那是
//! 消费方（结果网格）的职责，本模块保持最小。
//!
//! 覆盖 RFC 4180 的基本子集：逗号分隔、双引号字段（含内嵌逗号 / 双引号转义 `""`）、
//! `\r\n` 与 `\n` 换行、末行无换行。不 panic：任何输入都返回结构化的行数组，
//! 未闭合引号按「字段吃到行尾」处理（引号内换行 = 行结束，容错优先，见 [`parse`]）。

/// 解析 CSV 文本 → 每行一个字段数组
///
/// 约定：
/// - 空文本 → 空 `Vec`（没有行）；尾随换行不产生额外的空行
/// - 表头语义由消费方决定（第一行当表头是编辑器的展示约定，不在这里固化）
/// - 引号只出现在字段**开头**才进入引号模式；引号内的 `""` 是转义的双引号
/// - 未闭合引号：字段持续到行尾（行尾即结束，不 panic）
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let bytes = text.as_bytes();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    // 字段开头标记：引号只在字段开头生效（`a"b` 的 `"` 是普通字符）
    let mut at_field_start = true;
    // 本行是否产出过至少一个字段（用于判断末行要不要收尾；纯空行产出 `[""]`）
    let mut row_has_field = false;

    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quotes {
            match b {
                b'"' => {
                    // `""` 转义 → 一个字面双引号；单个 `"` → 引号字段闭合
                    if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        field.push('"');
                        i += 2;
                    } else {
                        in_quotes = false;
                        i += 1;
                    }
                }
                // 引号内换行 = 行结束（容错优先）：未闭合引号在行尾收尾，不吞后续
                // 行。规范 CSV 的多行字段（RFC 4180 允许）因此不支持——但预览场景里
                // 「用户敲了 `"` 忘了闭合」远比多行字段常见，吞文件不可接受。
                b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    in_quotes = false;
                    at_field_start = true;
                    row_has_field = false;
                    i += 2;
                }
                b'\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    in_quotes = false;
                    at_field_start = true;
                    row_has_field = false;
                    i += 1;
                }
                _ => {
                    push_char(&mut field, text, i);
                    i += char_width(b);
                }
            }
            continue;
        }
        match b {
            b'"' if at_field_start => {
                in_quotes = true;
                row_has_field = true;
                i += 1;
            }
            b',' => {
                row.push(std::mem::take(&mut field));
                at_field_start = true;
                row_has_field = true;
                i += 1;
            }
            b'\r' if i + 1 < bytes.len() && bytes[i + 1] == b'\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                at_field_start = true;
                row_has_field = false;
                i += 2;
            }
            b'\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                at_field_start = true;
                row_has_field = false;
                i += 1;
            }
            _ => {
                push_char(&mut field, text, i);
                at_field_start = false;
                row_has_field = true;
                i += char_width(b);
            }
        }
    }
    // 末行无换行：有内容才收尾（`a,` 的尾部空字段也是内容；纯空文本不是）
    if row_has_field {
        row.push(std::mem::take(&mut field));
        rows.push(std::mem::take(&mut row));
    }
    rows
}

/// 往字段追加一个 UTF-8 字符（从 `text[i..]` 取，兼容多字节）
fn push_char(field: &mut String, text: &str, i: usize) {
    let width = char_width(text.as_bytes()[i]);
    field.push_str(&text[i..i + width]);
}

/// 首个字节 → UTF-8 字符宽度（ASCII = 1；续字节不会出现在这里）
fn char_width(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first & 0xE0 == 0xC0 {
        2
    } else if first & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    /// 测试助手：把 &str 表格转成 Vec<Vec<String>>（避免每个断言写 to_string）
    fn rows(table: &[&[&str]]) -> Vec<Vec<String>> {
        table
            .iter()
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect()
    }

    #[test]
    fn simple_comma_separated() {
        assert_eq!(
            parse("a,b,c\n1,2,3"),
            rows(&[&["a", "b", "c"], &["1", "2", "3"]])
        );
    }

    #[test]
    fn quoted_fields_with_commas_and_escaped_quotes() {
        assert_eq!(
            parse("\"a,b\",\"say \"\"hi\"\"\"\nc,d"),
            rows(&[&["a,b", "say \"hi\""], &["c", "d"]])
        );
    }

    #[test]
    fn crlf_and_lf_both_terminate_rows() {
        assert_eq!(
            parse("a,b\r\nc,d\ne,f"),
            rows(&[&["a", "b"], &["c", "d"], &["e", "f"]])
        );
    }

    #[test]
    fn empty_fields_and_trailing_comma() {
        assert_eq!(
            parse("a,,c\n,"),
            rows(&[&["a", "", "c"], &["", ""]])
        );
    }

    #[test]
    fn unterminated_quote_extends_to_eol_without_panic() {
        // 未闭合引号：字段吃到行尾；不 panic
        assert_eq!(parse("\"abc\nx,y"), rows(&[&["abc"], &["x", "y"]]));
    }

    #[test]
    fn empty_text_has_no_rows() {
        assert_eq!(parse(""), Vec::<Vec<String>>::new());
    }

    #[test]
    fn trailing_newline_does_not_emit_an_extra_row() {
        assert_eq!(parse("a,b\n"), rows(&[&["a", "b"]]));
    }

    #[test]
    fn single_cell_without_newline() {
        assert_eq!(parse("only"), rows(&[&["only"]]));
    }

    #[test]
    fn quoted_newline_terminates_the_row_tolerantly() {
        // 未闭合引号内换行 = 行结束（容错；不吞后续行）
        assert_eq!(parse("\"abc\nx,y"), rows(&[&["abc"], &["x", "y"]]));
    }

    #[test]
    fn cjk_and_multibyte_round_trip() {
        assert_eq!(
            parse("名称,值\n数据库,42"),
            rows(&[&["名称", "值"], &["数据库", "42"]])
        );
    }
}

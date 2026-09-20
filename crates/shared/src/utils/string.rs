/// 截断字符串到指定长度
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{:.max_len$}...", s, max_len = max_len - 3)
    }
}

/// 安全地获取字符串的一部分
pub fn safe_substring(s: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let start = start.min(chars.len());
    let end = end.min(chars.len()).max(start);
    chars[start..end].iter().collect()
}

/// 一个单元格的 TSV 文本（**剪贴板与导出共用的唯一实现**）
///
/// 只有制表符 / 换行 / 双引号会破坏 TSV 的形状（列错位、一行被拆成两行），
/// 所以只有它们才包裹并双写内部引号，其余原样给——不做无谓的包裹，
/// 粘进表格软件不用再逐格去掉引号。
///
/// 使用方：`editor` 的结果集（`ResultEntry::to_tsv` / 结果网格的右键复制）与
/// `mock` 的预览取样（复制整行 / 整列）。同一份规则只写一次，两边不允许各自实现。
pub fn tsv_cell(cell: &str) -> String {
    if !cell.contains(['\t', '\n', '\r', '"']) {
        return cell.to_string();
    }
    let mut quoted = String::with_capacity(cell.len() + 2);
    quoted.push('"');
    quoted.push_str(&cell.replace('"', "\"\""));
    quoted.push('"');
    quoted
}

/// 一行的 TSV：单元格之间用制表符分隔，逐格按 [`tsv_cell`] 转义
pub fn tsv_row(cells: &[String]) -> String {
    let mut line = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            line.push('\t');
        }
        line.push_str(&tsv_cell(cell));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::{tsv_cell, tsv_row};

    /// 只有破坏 TSV 形状的三种字符才加引号
    #[test]
    fn only_shape_breaking_cells_are_quoted() {
        assert_eq!(tsv_cell("plain"), "plain");
        assert_eq!(tsv_cell(""), "");
        assert_eq!(tsv_cell("a b,c"), "a b,c", "空格与逗号不破坏 TSV，不加引号");
        assert_eq!(tsv_cell("two\tcells"), "\"two\tcells\"");
        assert_eq!(tsv_cell("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(tsv_cell("carriage\rreturn"), "\"carriage\rreturn\"");
        assert_eq!(tsv_cell("say \"hi\""), "\"say \"\"hi\"\"\"", "内部引号双写");
    }

    /// 一行里只有需要的格子才被包裹，列数不变（转义的目的是**保住形状**）
    #[test]
    fn a_row_keeps_its_shape() {
        let row = vec![
            "1".to_string(),
            "two\tcells".to_string(),
            "plain".to_string(),
            "say \"hi\"".to_string(),
        ];
        assert_eq!(
            tsv_row(&row),
            "1\t\"two\tcells\"\tplain\t\"say \"\"hi\"\"\""
        );
        assert!(tsv_row(&[]).is_empty());
    }
}

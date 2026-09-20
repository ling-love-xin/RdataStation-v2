//! 内容预览（原型 §3.1 的「内容预览」区）：**文本型给前 N 行，二进制 / 大文件只给元信息**。
//!
//! 两个纪律：
//! - **渲染层零计算**：本模块把「读到的字符串」变成「几行文字 + 一句说明」，
//!   详情面板只负责摆（与 `present.rs` 里其它字段同一口径——面板 render 不做字符串处理）；
//! - **读多少由这里定**：行数上限、大文件阈值、可按文本预览的扩展名都在这一个模块，
//!   宿主侧只按这些常量取数（两处各写一份就会漂：面板说“前 20 行”，读的人给 5 行）。

/// 预览行数上限（原型 §3.1：文本型显示**前 20 行**）。
pub const PREVIEW_MAX_LINES: usize = 20;
/// 读多少字节就够 20 行了（单行很长时也不会把内存拉满）。
pub const PREVIEW_READ_BYTES: usize = 8 * 1024;
/// 超过这个体积**不去读**（大文件只给元信息，与原型一致）。
pub const PREVIEW_MAX_FILE_BYTES: i64 = 1024 * 1024;
/// 非文本 / 大文件时的一句话（详情面板直接显示它）。
pub const ONLY_META_NOTE: &str = "仅元信息（不预览内容）";
/// 截断时的一句话。
pub const TRUNCATED_NOTE: &str = "仅显示前 20 行";
/// 读不出来（本体不在 / 没登记路径）时的一句话。
pub const UNAVAILABLE_NOTE: &str = "本体不在，无可预览的内容";

/// 可按文本预览的扩展名（**只此一处**；小写比较）。
///
/// 这份清单是**给人看的内容**的清单，不是“能当数据读”的清单：`.csv` 在编辑器里是表格，
/// 但作为文本预览一眼扫过去最有用；`.parquet` / `.xlsx` / `.duckdb` 是二进制，不给预览。
const TEXT_EXTENSIONS: [&str; 14] = [
    "sql", "md", "markdown", "txt", "text", "csv", "tsv", "json", "jsonl", "yaml", "yml", "toml",
    "ini", "log",
];

/// 详情面板的预览内容（**已是人读形态**：行已切好、说明已备好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// 文本：已截到 [`PREVIEW_MAX_LINES`] 行；`truncated` = 后面还有内容。
    Text { lines: Vec<String>, truncated: bool },
    /// 只给元信息（二进制 / 大文件 / 本体不在），带一句为什么。
    OnlyMeta(&'static str),
}

impl Default for Preview {
    /// 缺省 = 仅元信息：宿主没给预览（分析表 / 引用型 / 旧快照）时**不假装有内容**。
    fn default() -> Self {
        Preview::OnlyMeta(ONLY_META_NOTE)
    }
}

impl Preview {
    /// 说明行（渲染层直接摆；`None` = 文本型且没截断，不需要额外说明）。
    pub fn note(&self) -> Option<&'static str> {
        match self {
            Preview::Text { truncated, .. } => truncated.then_some(TRUNCATED_NOTE),
            Preview::OnlyMeta(note) => Some(note),
        }
    }

    /// 是否真有可摆的行。
    pub fn lines(&self) -> &[String] {
        match self {
            Preview::Text { lines, .. } => lines,
            Preview::OnlyMeta(_) => &[],
        }
    }
}

/// 文件名（或路径）是否可按文本预览（按扩展名，大小写不敏感）。
pub fn is_textual(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or_default();
    TEXT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// 本体的体积是否允许预览（`None` = 没记体积：宁可读一次，也不因此拒绝预览）。
pub fn size_allows_preview(file_size: Option<i64>) -> bool {
    file_size.is_none_or(|size| size <= PREVIEW_MAX_FILE_BYTES)
}

/// **读到的开头** → 预览快照（纯函数，带单测）。
///
/// `head` 是已经按 [`PREVIEW_READ_BYTES`] 截过、且（截断时）在最后一个换行处切齐的开头；
/// `complete` = 文件已经全部读到（`false` 表示后面还有内容）。
///
/// 处理三件小事：`\r\n` 归一（Windows 上写的草稿很常见，不归一就会在行尾留下一个看不见的
/// `\r`）、制表符展开成 4 空格（等宽块里 `\t` 的宽度由字体决定，对不齐反而更乱）、
/// 尾部空行去掉（文件末尾那个换行不该占一行预览）。
pub fn from_head(head: &str, complete: bool) -> Preview {
    let mut lines: Vec<String> = head
        .lines()
        .map(|line| line.replace('\r', "").replace('\t', "    "))
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let truncated = !complete || lines.len() > PREVIEW_MAX_LINES;
    lines.truncate(PREVIEW_MAX_LINES);
    Preview::Text { lines, truncated }
}

#[cfg(test)]
mod tests {
    use super::{ONLY_META_NOTE, PREVIEW_MAX_LINES, Preview, is_textual, size_allows_preview};

    #[test]
    fn only_known_text_extensions_are_previewable() {
        for name in ["月报.sql", "notes.md", "data.CSV", "config.yaml", "q.sql"] {
            assert!(is_textual(name), "{name} 应该能预览");
        }
        // 二进制与无扩展名不给预览（原型：二进制 / 大文件只给元信息）。
        for name in [
            "model.parquet",
            "book.xlsx",
            "logic.duckdb",
            "README",
            "a.png",
        ] {
            assert!(!is_textual(name), "{name} 不该预览");
        }
    }

    /// 大文件阈值：没记体积时**宁可读一次**（旧行没登记体积，不等于不能预览）。
    #[test]
    fn large_files_are_not_read_but_unknown_size_is() {
        assert!(size_allows_preview(None));
        assert!(size_allows_preview(Some(1024)));
        assert!(!size_allows_preview(Some(
            super::PREVIEW_MAX_FILE_BYTES + 1
        )));
    }

    /// 行切分：CRLF 归一、制表符展开、尾部空行不占一行；超过上限只留前 N 行并标截断。
    #[test]
    fn head_becomes_lines_with_notes() {
        let Preview::Text { lines, truncated } = super::from_head("a\r\nb\tc\n\n", true) else {
            panic!("文本型应给出 Text");
        };
        assert_eq!(lines, vec!["a".to_string(), "b    c".to_string()]);
        assert!(!truncated, "已经读到文件末尾，不算截断");

        let long = (1..=PREVIEW_MAX_LINES + 5)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let Preview::Text { lines, truncated } = super::from_head(&long, true) else {
            panic!("文本型应给出 Text");
        };
        assert_eq!(
            lines.len(),
            PREVIEW_MAX_LINES,
            "只留前 {PREVIEW_MAX_LINES} 行"
        );
        assert_eq!(lines[0], "line 1");
        assert!(truncated, "还有内容没显示 → 标截断");

        // 读了但没读完（字节上限截的）：即便行数没到上限也标截断。
        let Preview::Text { truncated, .. } = super::from_head("只有一行", false) else {
            panic!("文本型应给出 Text");
        };
        assert!(truncated);
    }

    /// 说明行的口径：文本型只在截断时给说明，仅元信息型总是给一句。
    #[test]
    fn notes_explain_only_when_there_is_something_to_explain() {
        assert_eq!(
            Preview::default().note(),
            Some(ONLY_META_NOTE),
            "缺省是仅元信息：宿主没给预览时不能假装有内容"
        );
        assert!(Preview::default().lines().is_empty());

        let whole = super::from_head("a\nb", true);
        assert_eq!(whole.note(), None, "整篇都在眼前，不需要额外说明");
        assert_eq!(whole.lines(), ["a".to_string(), "b".to_string()]);

        let cut = super::from_head("a\nb", false);
        assert_eq!(cut.note(), Some(super::TRUNCATED_NOTE));
    }
}

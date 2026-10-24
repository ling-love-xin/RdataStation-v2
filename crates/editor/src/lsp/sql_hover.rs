//! SQL 语义 hover provider（#54 ② 上层接线）：把 [`engine::sql::hover_at`] 接到
//! gpui-kit 编辑内核的 LSP hover 槽位上
//!
//! ## 与 LSP hover 的关系（谁挂谁，宿主定）
//!
//! 内核的 hover 槽位是单槽 `Option`（`Lsp::hover_provider`）：通用/分析模式且语言服务
//! 可用时挂 [`super::bridge::LspHoverProvider`]（走 `textDocument/hover`），SQL 模式挂
//! 本 provider（走自研语义层，`engine::sql::hover_at`）。宿主在 `view/host.rs` 的
//! `CompletionChoice::Sql` 分支安装——与补全同源（一次决策、一起挂摘）。
//!
//! ## 无目录口径
//!
//! editor 不依赖 database crate（无 NavCache），hover 时传**空表目录**：函数 hover
//! 照常（签名 + 中文描述 + 示例是内置的），表 / 列 hover 退化为「仅引用词」
//! （`hover_at` 内部对无目录表只给表名、列给所属表名，列列表不可得）——与
//! `semantic` 无目录退化同一口径，宁缺毋滥。目录注入（跨 crate，见 backlog §建议优先级）
//! 后表 / 列列表自然解锁。

use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{HoverProvider, Rope};
use gpui_kit::*;
use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Range};

use engine::sql::{HoverInfo, HoverKind, SqlDialect, hover_at};

/// SQL 语义 hover provider（无状态：方言创建时定，可随时重建）
pub struct SqlHoverProvider {
    /// 源方言（宿主按文档绑定连接的驱动给；未绑定时 Ansi——解析失败返回 None，安全）
    dialect: SqlDialect,
}

impl SqlHoverProvider {
    pub fn new(dialect: SqlDialect) -> Rc<Self> {
        Rc::new(Self { dialect })
    }
}

impl HoverProvider for SqlHoverProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Option<Hover>>> {
        let sql = text.to_string();
        let dialect = self.dialect;
        // `hover_at` 是纯计算（sqlglot 解析），放后台线程跑，别卡 UI 帧
        cx.background_spawn(async move {
            Ok(hover_at(&sql, dialect, &[], offset).map(|info| hover_to_lsp(&sql, info)))
        })
    }
}

/// 自研 HoverInfo → LSP Hover（Markdown 内容；range = 引用词字节区间）
///
/// 纯函数便于测试：内容结构（加粗标题 + 行）与区间换算都在这锁住。
fn hover_to_lsp(sql: &str, info: HoverInfo) -> Hover {
    let mut markdown = String::new();
    match info.kind {
        HoverKind::Function => markdown.push_str("**函数** "),
        HoverKind::Table => markdown.push_str("**表** "),
        HoverKind::Column => markdown.push_str("**列** "),
    }
    markdown.push_str("`");
    markdown.push_str(&info.title);
    markdown.push_str("`");
    for line in &info.lines {
        markdown.push('\n');
        markdown.push_str(line);
    }
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(Range {
            start: byte_position(sql, info.start),
            end: byte_position(sql, info.end),
        }),
    }
}

/// 字节偏移 → LSP Position（**0 基**、列按**字符**数；与 `language_service::line_column`
/// 同口径——行按 `\n` 数，列按字符数，不是字节也不是 UTF-16 码元）
fn byte_position(text: &str, offset: usize) -> lsp_types::Position {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    lsp_types::Position::new(line as u32, text[line_start..offset].chars().count() as u32)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{byte_position, hover_to_lsp};
    use engine::sql::{HoverInfo, HoverKind};

    #[test]
    fn function_hover_renders_signature_and_chinese() {
        let info = HoverInfo {
            kind: HoverKind::Function,
            title: "substr".into(),
            lines: vec![
                "substr(x, start[, length])".into(),
                "截取子串".into(),
                "SELECT substr('hello', 1, 3) → 'hel'".into(),
            ],
            start: 7,
            end: 13,
        };
        let sql = "SELECT substr('hello', 1, 3)";
        let hover = hover_to_lsp(sql, info);
        let lsp_types::HoverContents::Markup(markup) = &hover.contents else {
            panic!("应为 Markup 内容");
        };
        assert!(markup.value.contains("**函数** `substr`"), "{}", markup.value);
        assert!(markup.value.contains("substr(x, start[, length])"), "{}", markup.value);
        assert!(markup.value.contains("截取子串"), "{}", markup.value);
        assert!(markup.value.contains("SELECT substr('hello', 1, 3)"), "{}", markup.value);
        // 区间：'substr' 是第 8 个字符起、5 个字符长（0 基列 7..13）
        let range = hover.range.expect("有区间");
        assert_eq!((range.start.line, range.start.character), (0, 7));
        assert_eq!((range.end.line, range.end.character), (0, 13));
    }

    #[test]
    fn table_hover_lists_columns_when_catalog_present() {
        let info = HoverInfo {
            kind: HoverKind::Table,
            title: "public.customers".into(),
            lines: vec!["列：id、name".into()],
            start: 5,
            end: 21,
        };
        let sql = "FROM public.customers";
        let hover = hover_to_lsp(sql, info);
        let lsp_types::HoverContents::Markup(markup) = &hover.contents else {
            panic!("应为 Markup 内容");
        };
        assert!(markup.value.contains("**表** `public.customers`"), "{}", markup.value);
        assert!(markup.value.contains("列：id、name"), "{}", markup.value);
        let range = hover.range.expect("有区间");
        assert_eq!((range.start.character, range.end.character), (5, 21));
    }

    #[test]
    fn byte_position_counts_lines_and_chars() {
        // 行按 \n 数、列按字符数（中文按字符不是字节）
        let sql = "SELECT 1\n  FROM t";
        assert_eq!(byte_position(sql, 0), lsp_types::Position::new(0, 0));
        assert_eq!(byte_position(sql, 7), lsp_types::Position::new(0, 7));
        // 偏移 8 是换行符本身（第 1 行第 8 列）；9 起是第 2 行
        assert_eq!(byte_position(sql, 8), lsp_types::Position::new(0, 8));
        assert_eq!(byte_position(sql, 9), lsp_types::Position::new(1, 0));
        assert_eq!(byte_position(sql, 11), lsp_types::Position::new(1, 2));
        // 中文列按字符数：'中' 占 3 字节但算 1 个字符
        let cn = "SELECT 1\n  中文";
        assert_eq!(byte_position(cn, 15), lsp_types::Position::new(1, 3));
    }
}

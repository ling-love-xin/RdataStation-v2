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
//! ## 目录口径（2026-11-08 起可注入）
//!
//! editor 不依赖 database crate（无 NavCache），但宿主早就在推**每文档的目录快照**
//! （[`crate::shared::EditorShared::set_catalog_snapshot`]，文档绑定连接 / Schema 切换 /
//! 导航树刷新时推）。本 provider 现在可以从那条既有通道取快照：
//! - 有快照 → 表 / 列 hover 能列出列、并给一行「类型：X」（`column_types` + `annotate_types`）；
//! - 无快照（未绑定连接 / 目录没载好）→ 退化回原来的「仅引用词」口径，宁缺毋滥。
//!
//! 快照来自**该文档绑定的那条连接**（MySQL / PG / SQLite / Oracle / DuckDB 都一样），
//! 不是 DuckDB 分析模式专有——hover、补全、语义诊断走的都是同一份快照。

use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{HoverProvider, Rope};
use gpui_kit::*;
use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Range};

use engine::sql::{
    CatalogTable, HoverInfo, HoverKind, SqlDialect, TypedTable, column_types, hover_at,
};

use crate::language::completion::Catalog;
use crate::model::DocumentId;
use crate::shared::EditorShared;

/// SQL 语义 hover provider（方言创建时定；可带一份目录快照来源）
pub struct SqlHoverProvider {
    /// 源方言（宿主按文档绑定连接的驱动给；未绑定时 Ansi——解析失败返回 None，安全）
    dialect: SqlDialect,
    /// 目录快照来源：`(宿主共享态, 文档)`。未接 = 仍走「无目录」退化口径（测试用）
    catalog: Option<(EditorShared, DocumentId)>,
}

impl SqlHoverProvider {
    /// 无目录（测试 / 未接宿主）：表列 hover 只给引用词
    pub fn new(dialect: SqlDialect) -> Rc<Self> {
        Rc::new(Self {
            dialect,
            catalog: None,
        })
    }

    /// 接宿主目录快照：hover 时现取该文档当前的快照（连接 / Schema 切换后自动跟随）
    pub fn with_catalog(
        dialect: SqlDialect,
        shared: EditorShared,
        document: DocumentId,
    ) -> Rc<Self> {
        Rc::new(Self {
            dialect,
            catalog: Some((shared, document)),
        })
    }

    /// 取当前目录快照（内存读；未注入 / 未载好 = `None`）
    fn snapshot(&self) -> Option<std::sync::Arc<Catalog>> {
        let (shared, document) = self.catalog.as_ref()?;
        shared.catalog_snapshot_for(document)
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
        // 设置闸：`sql_analysis.hover_scope`（读进程内原子量，后台线程里读也安全）
        let with_scope = engine::sql::prefs().hover_scope;
        // 目录快照：**主线程现取**（快照是 `Rc<Catalog>`，不能跨线程），再摊成 owned 的
        // `TypedTable` 送进后台——连接 / Schema 一切换，下一帧 hover 就跟着变。
        let typed: Vec<TypedTable> = self
            .snapshot()
            .map(|catalog| typed_tables(&catalog))
            .unwrap_or_default();
        // `hover_at` 是纯计算（sqlglot 解析），放后台线程跑，别卡 UI 帧
        cx.background_spawn(async move {
            // 借用窗口只在本次调用内：从 owned 数据临时拼 `CatalogTable`
            let columns: Vec<Vec<&str>> = typed
                .iter()
                .map(|table| table.columns.iter().map(|(name, _)| name.as_str()).collect())
                .collect();
            let tables: Vec<CatalogTable<'_>> = typed
                .iter()
                .zip(columns.iter())
                .map(|(table, cols)| CatalogTable {
                    qualified_name: table.qualified_name.as_str(),
                    columns: cols.clone(),
                })
                .collect();
            Ok(hover_at(&sql, dialect, &tables, offset).map(|mut info| {
                if let Some(line) = column_type_line(&sql, dialect, &typed, &info) {
                    info.lines.push(line);
                }
                if with_scope {
                    if let Some(line) = scope_source_line(&sql, dialect, info.kind) {
                        info.lines.push(line);
                    }
                }
                hover_to_lsp(&sql, info)
            }))
        })
    }
}

/// 宿主目录快照 → `TypedTable`（名字与类型都照快照来；快照没给类型就留空串）
fn typed_tables(catalog: &Catalog) -> Vec<TypedTable> {
    let mut tables: Vec<TypedTable> = Vec::new();
    for (qualified, column, type_name) in &catalog.columns {
        match tables
            .iter_mut()
            .find(|table| &table.qualified_name == qualified)
        {
            Some(table) => table
                .columns
                .push((column.clone(), type_name.clone().unwrap_or_default())),
            None => tables.push(TypedTable {
                qualified_name: qualified.clone(),
                columns: vec![(column.clone(), type_name.clone().unwrap_or_default())],
            }),
        }
    }
    tables
}

/// 悬停到的列 / 表达式 → 一行「类型：X」
///
/// 只在**有目录**且 `annotate_types` 能给出类型时才有（没有元数据就不编）；表与函数不给。
fn column_type_line(
    sql: &str,
    dialect: SqlDialect,
    tables: &[TypedTable],
    info: &HoverInfo,
) -> Option<String> {
    if tables.is_empty() || !matches!(info.kind, HoverKind::Column) {
        return None;
    }
    let typed = column_types(sql, dialect, tables).ok()?;
    let found = typed
        .iter()
        .find(|column| column.name.eq_ignore_ascii_case(&info.title))?;
    Some(format!("类型：{}", found.data_type))
}

/// 悬停内容里补的那一行「来源：orders、totals」
///
/// 只给表 / 列（函数没有来源）；作用域里全是有名字的表才给——派生表/CTE 混在里面时
/// 一句话说不清，宁可不加（与 usage 口径一致：说不准就不说）。
fn scope_source_line(sql: &str, dialect: engine::sql::SqlDialect, kind: HoverKind) -> Option<String> {
    if matches!(kind, HoverKind::Function) {
        return None;
    }
    let report = engine::sql::scope_report(sql, dialect).ok()?;
    if report.sources.is_empty() {
        return None;
    }
    let mut tables: Vec<String> = report
        .sources
        .iter()
        .filter(|source| !source.derived && !source.table.is_empty())
        .map(|source| source.table.clone())
        .collect();
    tables.sort();
    tables.dedup();
    if tables.is_empty() {
        return None;
    }
    Some(format!("来源：{}", tables.join("、")))
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
    use super::{byte_position, column_type_line, hover_to_lsp, typed_tables};
    use crate::language::completion::Catalog;
    use engine::sql::{HoverInfo, HoverKind, SqlDialect};

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
        assert!(
            markup.value.contains("**函数** `substr`"),
            "{}",
            markup.value
        );
        assert!(
            markup.value.contains("substr(x, start[, length])"),
            "{}",
            markup.value
        );
        assert!(markup.value.contains("截取子串"), "{}", markup.value);
        assert!(
            markup.value.contains("SELECT substr('hello', 1, 3)"),
            "{}",
            markup.value
        );
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
        assert!(
            markup.value.contains("**表** `public.customers`"),
            "{}",
            markup.value
        );
        assert!(markup.value.contains("列：id、name"), "{}", markup.value);
        let range = hover.range.expect("有区间");
        assert_eq!((range.start.character, range.end.character), (5, 21));
    }

    #[test]
    fn catalog_snapshot_becomes_typed_tables_and_a_type_line() {
        // 宿主快照 → `TypedTable`：每个限定表一组，列名与类型名都照快照来
        let catalog = Catalog {
            objects: Vec::new(),
            columns: vec![
                ("public.users".to_string(), "id".to_string(), Some("int".to_string())),
                ("public.users".to_string(), "amount".to_string(), Some("decimal(12,2)".to_string())),
                ("public.orders".to_string(), "total".to_string(), None),
            ],
            truncated: false,
        };
        let typed = typed_tables(&catalog);
        assert_eq!(typed.len(), 2, "两张表各一组：{typed:?}");
        let users = typed
            .iter()
            .find(|table| table.qualified_name == "public.users")
            .expect("users 在");
        assert_eq!(users.columns.len(), 2, "{users:?}");
        assert_eq!(users.columns[1].1, "decimal(12,2)", "类型名照抄快照");
        let orders = typed
            .iter()
            .find(|table| table.qualified_name == "public.orders")
            .expect("orders 在");
        assert_eq!(orders.columns[0].1, "", "快照没给类型就留空，不编");

        // 列 hover → 一行「类型：X」（annotate_types 按目录算出来的）
        let info = HoverInfo {
            kind: HoverKind::Column,
            title: "amount".into(),
            lines: Vec::new(),
            start: 0,
            end: 6,
        };
        let line = column_type_line(
            "SELECT amount FROM public.users",
            SqlDialect::Postgres,
            &typed,
            &info,
        );
        assert!(
            line.as_deref().is_some_and(|line| line.contains("DECIMAL")),
            "{line:?}"
        );
    }

    #[test]
    fn no_catalog_means_no_type_line() {
        // 未绑定连接 / 目录没载好：宁可不说，也不猜类型
        let info = HoverInfo {
            kind: HoverKind::Table,
            title: "public.users".into(),
            lines: Vec::new(),
            start: 0,
            end: 6,
        };
        assert!(column_type_line("SELECT id FROM public.users", SqlDialect::Postgres, &[], &info).is_none());
    }

    #[test]
    fn byte_position_counts_lines_and_chars() {
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

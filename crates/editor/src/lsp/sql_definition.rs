//! SQL 语义跳转定义 provider（#54 C6/C7 上层接线）：把 [`engine::sql::jump_at`] 接到
//! gpui-kit 编辑内核的 LSP `definition_provider` 槽位上
//!
//! ## 机制（设计见 `docs/architecture/editor/sql-semantic-remaining-design.md` §一）
//!
//! 内核的跳转全链路是现成的：`DefinitionProvider` 槽位 + hover 预取
//! （`handle_hover_definition` 在 hover 处理里自动调——SQL hover 已接，挂上本 provider
//! 即自动获得「悬停下划线 + Ctrl+Click hitbox」）+ `go_to_definition` 分派：
//!
//! - 外部 URI（http/https）→ 内核 `open_url`；
//! - **其他 scheme** → 先问 `show_document` handler（宿主返回 `true` = 自己处理）；
//! - 默认 → 当前文档内移动光标。
//!
//! 本 provider 把跳转目标编码成 `LocationLink`：
//!
//! - `origin_selection_range` = 符号字节区间（内核画下划线 + Ctrl+Click 命中区）
//! - `target_uri` = `rds://navigator/<限定表名>` **虚拟 scheme**——不会真的被打开，
//!   而是被宿主（workbench/database 侧）的 `show_document` handler 解析：按文档绑定
//!   连接在导航树定位该限定名，命中展开选中并返回 `true`；未命中返回 `false` 让内核
//!   走默认（当前文档内移动光标到词，无副作用）
//! - `target_range` / `target_selection_range` = 空区间（表/列真实位置由宿主展开决定）
//!
//! ## 无目录口径
//!
//! 同 hover / semantic：editor 不依赖 database crate，跳转传**空表目录**——`jump_at`
//! 对裸表名仍给引用限定名（表可跳），列需要目录（退化不产）——宁缺毋滥。

use std::rc::Rc;
use std::str::FromStr;

use anyhow::Result;
use gpui_kit::component::input::{DefinitionProvider, Rope};
use gpui_kit::*;
use lsp_types::{LocationLink, Position, Range, Uri};

use engine::sql::{SqlDialect, jump_at};

/// SQL 语义跳转 provider（无状态：方言创建时定，可随时重建）
pub struct SqlDefinitionProvider {
    /// 源方言（宿主按文档绑定连接的驱动给；未绑定时 Ansi——解析失败返回 None，安全）
    dialect: SqlDialect,
}

impl SqlDefinitionProvider {
    pub fn new(dialect: SqlDialect) -> Rc<Self> {
        Rc::new(Self { dialect })
    }
}

impl DefinitionProvider for SqlDefinitionProvider {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<LocationLink>>> {
        let sql = text.to_string();
        let dialect = self.dialect;
        // `jump_at` 是纯计算（sqlglot 解析），放后台线程跑，别卡 UI 帧
        cx.background_spawn(async move {
            Ok(jump_at(&sql, dialect, &[], offset)
                .map(|target| jump_to_lsp(&sql, target))
                .into_iter()
                .collect())
        })
    }
}

/// 自研 JumpTarget → LSP LocationLink（虚拟 navigator scheme + 符号字节区间）
///
/// 纯函数便于测试：URI 拼法、区间换算、列/表差异都在这锁住。
fn jump_to_lsp(sql: &str, target: engine::sql::JumpTarget) -> LocationLink {
    let origin = Range {
        start: byte_position(sql, target.start),
        end: byte_position(sql, target.end),
    };
    // 虚拟 scheme：`rds://navigator/<限定表名>`。限定名里可能含 `.`（schema.table），
    // 用双斜杠后的路径承载，宿主按「连接 → 导航树」反查。
    let uri = Uri::from_str(&format!("rds://navigator/{}", target.table))
        .expect("rds://navigator 是合法 scheme，不会失败");
    let empty = Range {
        start: Position::new(0, 0),
        end: Position::new(0, 0),
    };
    LocationLink {
        origin_selection_range: Some(origin),
        target_uri: uri,
        target_range: empty,
        target_selection_range: empty,
    }
}

/// 字节偏移 → LSP Position（**0 基**、列按**字符**数；与 `sql_hover::byte_position`
/// 同一口径——行按 `\n` 数，列按字符数，不是字节也不是 UTF-16 码元）
fn byte_position(text: &str, offset: usize) -> Position {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    Position::new(line as u32, text[line_start..offset].chars().count() as u32)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{byte_position, jump_to_lsp};
    use engine::sql::{JumpKind, JumpTarget};
    use lsp_types::Position;

    #[test]
    fn table_jump_targets_navigator_scheme_with_symbol_range() {
        let target = JumpTarget {
            kind: JumpKind::Table,
            table: "public.customers".into(),
            column: None,
            start: 5,
            end: 21,
        };
        let sql = "FROM public.customers";
        let link = jump_to_lsp(sql, target);
        assert_eq!(
            link.target_uri.to_string(),
            "rds://navigator/public.customers",
            "表跳转 URI = navigator scheme + 限定名"
        );
        // 符号区间：'public.customers' 0 基列 5..21
        let origin = link.origin_selection_range.expect("有符号区间");
        assert_eq!((origin.start.line, origin.start.character), (0, 5));
        assert_eq!((origin.end.line, origin.end.character), (0, 21));
        // 目标区间空（宿主展开时决定）
        assert_eq!(
            (link.target_range.start, link.target_selection_range.start),
            (Position::new(0, 0), Position::new(0, 0))
        );
    }

    #[test]
    fn column_jump_uses_table_uri() {
        let target = JumpTarget {
            kind: JumpKind::Column,
            table: "public.customers".into(),
            column: Some("name".into()),
            start: 7,
            end: 11,
        };
        let sql = "SELECT name FROM public.customers";
        let link = jump_to_lsp(sql, target);
        // 列跳转也定位到所属表（宿主选中表后再选中列）
        assert_eq!(link.target_uri.to_string(), "rds://navigator/public.customers");
        let origin = link.origin_selection_range.expect("有符号区间");
        assert_eq!((origin.start.character, origin.end.character), (7, 11));
    }

    #[test]
    fn byte_position_counts_lines_and_chars() {
        // 与 sql_hover 同口径：行按 \n、列按字符数（中文按字符不是字节）
        let sql = "SELECT id\n  FROM t";
        assert_eq!(byte_position(sql, 0), Position::new(0, 0));
        assert_eq!(byte_position(sql, 9), Position::new(0, 9));
        assert_eq!(byte_position(sql, 10), Position::new(1, 0));
        assert_eq!(byte_position(sql, 12), Position::new(1, 2));
        let cn = "SELECT id\n  中文";
        assert_eq!(byte_position(cn, 15), Position::new(1, 3));
    }
}

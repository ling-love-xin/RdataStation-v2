//! SQL 改写型 code action provider（A1：把自研动作挂内核槽位）
//!
//! ## 一句话
//!
//! gpui-kit 编辑内核的 LSP 槽位里 `code_action_providers` 是**多槽 Vec**，宿主只需把
//! 实现了 [`CodeActionProvider`] 的 provider push 进去，内核就自动获得：
//! `Ctrl+.` 快捷触发（`ToggleCodeActions`，`state.rs` 已绑 `ctrl-.`）、
//! 代码操作弹出层、以及右键菜单里的「代码操作」能力位（`context_menu_capabilities`
//! 第二项随 provider 非空自动置真）。本模块把自研的 [`language_service::code_actions`]
//! （ADD_WHERE / ADD_LIMIT 两条改写动作）翻译成 LSP `CodeAction`，`perform` 时把
//! `TextEdit` 应用回编辑器。
//!
//! ## 与补全 / hover 的同构
//!
//! - 无目录口径与 hover 一致：不依赖 database crate，空目录参与（ADD_WHERE / ADD_LIMIT
//!   只按文本与通道判，不看目录）。
//! - 方言 / 通道在安装时捕获（与 `SqlHoverProvider` 的方言捕获同一来源），请求组装
//!   走 [`language_service::Request`] 同一形状，命中口径（选区优先 / 光标所在语句 /
//!   脚本末尾）与执行族、诊断族共用。

use std::rc::Rc;
use std::str::FromStr as _;

use anyhow::Result;
use gpui_kit::component::input::{CodeActionProvider, EditorState, RopeExt};
use gpui_kit::*;
use lsp_types::{
    CodeAction as LspCodeAction, CodeActionKind, OneOf, OptionalVersionedTextDocumentIdentifier,
    Position, Range as LspRange, TextDocumentEdit, TextEdit as LspTextEdit, Uri as LspUri,
    WorkspaceEdit,
};

use engine::sql::SqlDialect;

use crate::channel::ExecChannel;
use crate::completion::Catalog;
use crate::language_service::{self, Request, TextEdit};

/// SQL 改写型 code action provider（无状态：方言与通道创建时定，可随时重建）
pub struct SqlCodeActionProvider {
    /// 源方言（与 hover / 跳转 / 语义诊断同一来源；未绑定时 Ansi——解析失败返回 None，安全）
    dialect: SqlDialect,
    /// 执行通道（ADD_WHERE 只在语句能跑的通道上给：加速 / 联邦对源库只读挂载不给）
    channel: ExecChannel,
    /// 文档 URI（`WorkspaceEdit` 的 `textDocument` 定位；无真实文件时用占位——内核应用
    /// 编辑走的是**编辑器本地坐标**，URI 只用于结构完整性）
    document_uri: String,
}

impl SqlCodeActionProvider {
    pub fn new(dialect: SqlDialect, channel: ExecChannel, document_uri: impl Into<String>) -> Rc<Self> {
        Rc::new(Self {
            dialect,
            channel,
            document_uri: document_uri.into(),
        })
    }
}

/// 自研 [`TextEdit`] → LSP `TextEdit`（字节偏移 → 0 基行列，与 `language_service`
/// 的 `line_column` 同一换算：行按 `\n` 数、列按字符数）
fn to_lsp_edit(text: &str, edit: &TextEdit) -> LspTextEdit {
    LspTextEdit {
        range: byte_range_to_lsp(text, edit.start, edit.end),
        new_text: edit.replacement.clone(),
    }
}

/// 字节区间 → LSP `Range`（0 基、列按字符数；与 `sql_hover::byte_position` 同口径）
fn byte_range_to_lsp(text: &str, start: usize, end: usize) -> LspRange {
    LspRange {
        start: byte_position(text, start),
        end: byte_position(text, end),
    }
}

/// 字节偏移 → LSP `Position`（0 基、列按**字符**数，不是字节也不是 UTF-16 码元）
fn byte_position(text: &str, offset: usize) -> Position {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    Position::new(line as u32, text[line_start..offset].chars().count() as u32)
}

impl CodeActionProvider for SqlCodeActionProvider {
    fn id(&self) -> SharedString {
        "rds-sql-rewrite".into()
    }

    fn code_actions(
        &self,
        state: Entity<EditorState>,
        range: std::ops::Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Vec<LspCodeAction>>> {
        // 读一次文本（内核 Rope → str）；code action 是纯计算，直接在后台算完就绪返回
        let text = state.read(cx).text().to_string();
        let cursor = range.start;
        let selection = (range.start != range.end).then_some((range.start, range.end));
        let actions = language_service::code_actions(&Request {
            text: &text,
            cursor,
            selection,
            catalog: &Catalog::default(),
            connection: None,
            channel: self.channel,
            dialect: self.dialect,
        });
        let uri = self.document_uri.clone();
        let items = actions
            .into_iter()
            .map(|action| {
                let edit = to_lsp_edit(&text, &action.edit);
                LspCodeAction {
                    title: action.title,
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(WorkspaceEdit {
                        document_changes: Some(lsp_types::DocumentChanges::Edits(vec![
                            TextDocumentEdit {
                                text_document: OptionalVersionedTextDocumentIdentifier {
                                    // `rds://editor/<id>` 这类占位 URI 能被 fluent-uri 解析；
                                    // 万一解析失败（非法字符串）给稳定的 `file:///rds-untitled`
                                    uri: LspUri::from_str(&uri)
                                        .unwrap_or_else(|_| LspUri::from_str("file:///rds-untitled").expect("字面量必合法")),
                                    version: None,
                                },
                                edits: vec![OneOf::Left(edit)],
                            },
                        ])),
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            })
            .collect();
        Task::ready(Ok(items))
    }

    fn perform_code_action(
        &self,
        state: Entity<EditorState>,
        action: LspCodeAction,
        push_to_history: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<()>> {
        // 只认我们自己发的 edit（一条 `TextDocumentEdit`）；拿不到就空转（返回 Ok）
        let Some(WorkspaceEdit {
            document_changes: Some(lsp_types::DocumentChanges::Edits(changes)),
            ..
        }) = action.edit
        else {
            return Task::ready(Ok(()));
        };
        let Some(change) = changes.first() else {
            return Task::ready(Ok(()));
        };
        let Some(OneOf::Left(edit)) = change.edits.first() else {
            return Task::ready(Ok(()));
        };
        // 重算字节区间：LSP 行列 → 字节（`RopeExt::position_to_offset`，与发送时同一编码）
        let start = state.read(cx).text().position_to_offset(&edit.range.start);
        let end = state.read(cx).text().position_to_offset(&edit.range.end);
        // 应用改写：内核 `replace`（替换当前选区）保留撤销栈——这里显式设选区再替换，
        // 与 `push_to_history` 对齐（true = 进撤销栈；内核 replace 默认就进）
        let _ = push_to_history;
        state.update(cx, |state, cx| {
            state.set_selected_range(start..end, cx);
            state.replace_text_in_range(None, &edit.new_text, window, cx);
            cx.notify();
        });
        Task::ready(Ok(()))
    }
}

/// 装到某个文档的编辑内核上（`code_action_providers` 是多槽 Vec：push 即可，不顶掉别的）
///
/// 泛型：SQL 自研改写（本模块）与 LSP 转发（[`super::bridge::LspCodeActionProvider`]）都走
/// 这个入口；宿主按模式决策挂谁。
pub fn install(state: &mut EditorState, provider: Rc<dyn CodeActionProvider>) {
    state.lsp_mut().code_action_providers.push(provider);
}

/// 本线安装过的 code action provider id（uninstall 按 id 摘，不动宿主别的 provider）
const OUR_PROVIDER_IDS: [&str; 2] = ["rds-sql-rewrite", "rds-lsp"];

/// 摘掉（文本模式 / 档位禁用时；按 id 移除本线的 provider，不动别的）
pub fn uninstall(state: &mut EditorState) {
    state
        .lsp_mut()
        .code_action_providers
        .retain(|provider| !OUR_PROVIDER_IDS.contains(&provider.id().as_ref()));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{byte_position, byte_range_to_lsp, to_lsp_edit};
    use crate::language_service::TextEdit;

    #[test]
    fn byte_position_counts_lines_and_chars() {
        // 行按 \n 数、列按字符数（中文按字符不是字节）——与 hover 同一口径
        let sql = "SELECT 1\n  FROM t";
        assert_eq!(byte_position(sql, 0), lsp_types::Position::new(0, 0));
        assert_eq!(byte_position(sql, 7), lsp_types::Position::new(0, 7));
        assert_eq!(byte_position(sql, 8), lsp_types::Position::new(0, 8));
        assert_eq!(byte_position(sql, 9), lsp_types::Position::new(1, 0));
        // 中文列按字符数：'中' 占 3 字节但算 1 个字符
        let cn = "SELECT 1\n  中文";
        assert_eq!(byte_position(cn, 15), lsp_types::Position::new(1, 3));
    }

    #[test]
    fn edit_range_maps_bytes_to_lines_and_chars() {
        let sql = "UPDATE t SET a = 1";
        let range = byte_range_to_lsp(sql, 7, 18);
        assert_eq!((range.start.line, range.start.character), (0, 7));
        assert_eq!((range.end.line, range.end.character), (0, 18));
    }

    #[test]
    fn self_edit_carries_range_and_replacement() {
        let sql = "SELECT * FROM t";
        let edit = to_lsp_edit(
            sql,
            &TextEdit {
                start: 14,
                end: 14,
                replacement: " WHERE 1 = 0".to_string(),
            },
        );
        assert_eq!(edit.new_text, " WHERE 1 = 0");
        assert_eq!(edit.range.start.character, 14);
        assert_eq!(edit.range.end.character, 14);
    }
}

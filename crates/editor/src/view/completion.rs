//! SQL 补全的**视图侧**（B9）：把候选目录接到编辑内核的 LSP 补全接口上
//!
//! ## 内核给的是什么（读源码得来，别猜）
//!
//! `gpui_base` 的编辑内核已经有一套完整的补全交互：`Lsp::completion_provider` + 弹出层 +
//! 键盘上下选择 + 替换已敲的那一段。我们**只提供候选**——不画下拉、不管键盘
//! （与 A11 查找 / 替换“零自建”同一结论）。
//!
//! ## 三条实现约束
//!
//! 1. **只给 `label`，不要给 `insert_text`**：内核的 `insert_completion` 在 `insert_text`
//!    存在时把替换范围改成「光标处」（`range.end..range.end`），于是 `from ord` 选 `public.orders`
//!    会变成 `from ordpublic.orders`；只给 `label` 才会替换掉已敲的那一段。
//! 2. **触发时机**由 `is_completion_trigger` 决定（每次文本输入后被问一次）：标识符字符与 `.`
//!    触发；空格 / 退格 / 括号 / 粘贴不触发。
//! 3. **档位**：编辑器只读、文本模式、超大文件档位下一律不触发（能力表 + 文件档位，
//!    与 A13 的“大文件关重能力”一致）。
//!
//! ## 候选从哪来
//!
//! 全部来自宿主端口（[`crate::completion::CompletionPort`]）：**同步 + 内存**。没接端口时
//! 只给关键字与函数——如实，不假装有元数据。

use std::rc::Rc;

use anyhow::Result;
use gpui_kit::component::input::{CompletionProvider, EditorState, Rope};
use gpui_kit::*;
use lsp_types::{CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse};

use crate::completion::{self, CandidateKind};
use crate::model::DocumentId;
use crate::shared::EditorShared;

/// 每次弹层最多给几条（够选就行；再多反而难找）
const MAX_ITEMS: usize = 60;

/// SQL 补全 provider：上下文判定与候选挑选是纯函数（`crate::completion`），这里只做翻译与接线
pub struct SqlCompletionProvider {
    shared: EditorShared,
    document: DocumentId,
}

impl CompletionProvider for SqlCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        // 整篇拷贝一次：补全路径（每次按键至多一次）可以接受；换来的是纯函数可测的输入
        let plain = text.to_string();
        let request = completion::request_at(&plain, offset);
        let catalog = self.shared.completion_catalog(&self.document);
        let items = completion::candidates(&catalog, &request, MAX_ITEMS)
            .into_iter()
            .map(to_item)
            .collect();
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    /// 刚敲下一个标识符字符或 `.` 时才问候选（退格 / 空格 / 标点 / 粘贴都不问）
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        if !self.shared.completion_enabled(&self.document) {
            return false;
        }
        let mut chars = new_text.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '.',
            _ => false,
        }
    }
}

/// 把候选翻译成内核要的 LSP 项
///
/// **不给 `insert_text` / `text_edit`**（见模块头第 1 条）：内核拿 `label` 替换已敲的那一段。
fn to_item(candidate: completion::Candidate) -> CompletionItem {
    CompletionItem {
        label: candidate.label,
        kind: Some(match candidate.kind {
            CandidateKind::Column => CompletionItemKind::FIELD,
            CandidateKind::Table => CompletionItemKind::STRUCT,
            CandidateKind::View => CompletionItemKind::INTERFACE,
            CandidateKind::Function => CompletionItemKind::FUNCTION,
            CandidateKind::Keyword => CompletionItemKind::KEYWORD,
        }),
        detail: candidate.detail,
        ..Default::default()
    }
}

/// 【B9 切片二】给一个偏移算出：替换范围的起点 + 查询词 + 候选（LSP 项）
///
/// 打字触发的路径由内核自己算这三样（`handle_completion_trigger`），然后调 [`CompletionProvider`]；
/// **手动触发**（`Ctrl+Space`）没有内核帮忙，所以在这里算好再 `present_completion_items`——
/// 两条路用**同一份**候选逻辑（否则手动补出来的东西会和打字补的不一样）。
pub(crate) fn items_at(
    shared: &EditorShared,
    document: &DocumentId,
    text: &str,
    offset: usize,
) -> (usize, String, Vec<CompletionItem>) {
    let start = completion::word_start(text, offset);
    let query = text.get(start..offset).unwrap_or_default().to_string();
    let request = completion::request_at(text, offset);
    let catalog = shared.completion_catalog(document);
    let items = completion::candidates(&catalog, &request, MAX_ITEMS)
        .into_iter()
        .map(to_item)
        .collect();
    (start, query, items)
}

/// 装到某个文档的编辑内核上（`EditorState::lsp_mut`）
pub fn install(state: &mut EditorState, shared: EditorShared, document: DocumentId) {
    state.lsp_mut().completion_provider = Some(Rc::new(SqlCompletionProvider { shared, document }));
}

/// 摘掉（文本模式、或不该补全的档位）
pub fn uninstall(state: &mut EditorState) {
    state.lsp_mut().completion_provider = None;
}

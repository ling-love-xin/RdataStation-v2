//! 折叠的**视图侧**（B17）：把纯函数算出的候选喂给编辑内核
//!
//! ## 内核这边是什么情况（读 `gpui-base` 0.6.1 源码得来，别猜）
//!
//! 机制齐全、候选外供：
//!
//! | 事 | 在哪 | 我们的处境 |
//! | --- | --- | --- |
//! | 折叠开关 | `LayoutMode::CodeEditor { folding }`（`mode.rs:77`，默认 `true`） | 我们的编辑器本来就在 code-editor 模式（`EditorMode::CODE_EDITOR = true`）→ **默认开着** |
//! | 候选来源 | `highlighter.fold_ranges(text)`（`state.rs:3604`） | 我们走语义 token、不注册 grammar（架构 D12）→ **内核自己产不出候选** |
//! | 编辑后的增量维护 | `highlighter.fold_ranges_for_edit`（`state.rs:3627`） | 没有 highlighter 就**早退**（`:3591` / `:3611`）→ 候选不会自己平移 |
//! | 喂候选的公开入口 | `EditorState::apply_highlighter_fold_candidates`（`state.rs:825`） | 公开可用（自带 `mode.is_folding()` 门） |
//! | 渲染与点击折叠 | `element.rs` 的 `paint_fold_icons`（chevron + 点击） | 内核负责，我们不画 |
//!
//! 于是本模块只做两件事：**打开文档时喂一次**、**每次文本变更后再喂一次**（不重喂会指到别的行——
//! 这一点是本模块存在的主要理由，不是"顺手补个功能"）。
//!
//! ## 三条取舍
//!
//! 1. **不注册 grammar**：候选与高亮共用同一份词法（`engine::sql::highlight_spans`），
//!    不引第二个词法器、不锁方言（D12 的同一理由）。代价是候选由我们负责刷新（见上表）；
//! 2. **超门槛不扫**：`fold::MAX_BYTES` 与着色门槛同量级——大文件退化为"不可折"，
//!    而不是每次按键全量扫一遍；
//! 3. **档位真关开关**：`FileTier::disables_folding` 为真时调 `set_folding(false)`
//!    （不只是留一个判据）——与"降级不静默"一致：提示卡已经说明大文件关掉了重能力。

use gpui_kit::component::input::{EditorState, FoldRange};
use gpui_kit::*;

use crate::fold::{self, FoldSpan};
use crate::model::EditorMode;

/// 该模式是否启用折叠
///
/// 折叠是**缓冲区的编辑能力**，不是语言服务：文本模式（记事本）与 SQL 模式都该有；
/// 分析模式是逐单元语言、单元层结构属 1c，现在整篇笔记不开（与 `view/highlight` 同一口径）。
pub fn is_enabled(mode: EditorMode) -> bool {
    matches!(mode, EditorMode::Text | EditorMode::Sql)
}

/// 候选 → 内核形状（纯函数，可逐条断言）
///
/// `FoldRange::new` 对 `start_line <= end_line` 有断言，而 [`fold::spans`] 保证
/// `start_line < end_line`——转换这一层是唯一把两者接起来的窄口。
pub fn to_kernel(spans: Vec<FoldSpan>) -> Vec<FoldRange> {
    spans
        .into_iter()
        .map(|span| FoldRange::new(span.start_line, span.end_line))
        .collect()
}

/// 打开文档时喂一次候选（构造路径；此时 `Context<EditorState>` 直接可得）
pub fn install(state: &mut EditorState, text: &str, cx: &mut Context<EditorState>) {
    refresh(state, fold::spans(text), cx);
}

/// 文本变更后重喂候选（**内核不会替我们平移**，见模块文档的上表）
///
/// 传空 `Vec` 就是"没有可折的"：把 candidates 清掉，不报错也不降级说明——
/// 折叠候选是视图派生数据，没有它时编辑器仍然是完整可用的编辑器。
pub fn refresh(state: &mut EditorState, spans: Vec<FoldSpan>, cx: &mut Context<EditorState>) {
    #[cfg(test)]
    probe::record(spans.len());
    state.apply_highlighter_fold_candidates(to_kernel(spans), cx);
}

/// 测试探针：只为断言"接线真的跑了"（生产构建里不存在）
///
/// 内核没有公开的"折叠候选读口"（`display_map` 不在公开面上），所以窗口测试只能从
/// **喂的次数**确认路径走到了：本计数按线程（libtest 一个用例一个线程），不跨用例串扰。
#[cfg(test)]
pub(crate) mod probe {
    use std::cell::Cell;

    thread_local! {
        static REFRESHES: Cell<usize> = const { Cell::new(0) };
        static CANDIDATES: Cell<usize> = const { Cell::new(usize::MAX) };
    }

    pub(crate) fn record(candidates: usize) {
        REFRESHES.with(|count| count.set(count.get() + 1));
        CANDIDATES.with(|last| last.set(candidates));
    }

    /// （喂候选次数，最近一次候选数）；后者初值是 `usize::MAX`（可辨认的哨兵）
    pub(crate) fn read() -> (usize, usize) {
        (REFRESHES.with(Cell::get), CANDIDATES.with(Cell::get))
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{fold, is_enabled, to_kernel};
    use crate::model::EditorMode;

    #[test]
    fn folding_is_on_for_buffers_and_off_for_the_notebook() {
        assert!(is_enabled(EditorMode::Text));
        assert!(is_enabled(EditorMode::Sql));
        // 分析模式是逐单元结构（1c），整篇笔记不开
        assert!(!is_enabled(EditorMode::Analysis));
    }

    #[test]
    fn kernel_ranges_keep_the_same_lines() {
        let spans = fold::spans("SELECT (\n  1\n) AS x");
        assert_eq!(spans.len(), 1);
        let kernel = to_kernel(spans);
        assert_eq!(kernel.len(), 1);
        assert_eq!((kernel[0].start_line, kernel[0].end_line), (0, 2));
    }

    #[test]
    fn an_empty_candidate_list_is_a_valid_answer() {
        assert!(to_kernel(fold::spans("SELECT 1")).is_empty());
    }

    #[test]
    fn the_probe_records_what_was_fed_to_the_kernel() {
        // 探针本身也要有依据：喂两次后读到的次数与最近一次候选数都是真值
        let before = super::probe::read().0;
        super::probe::record(2);
        super::probe::record(0);
        let after = super::probe::read();
        assert_eq!(after.0, before + 2);
        assert_eq!(after.1, 0);
    }
}

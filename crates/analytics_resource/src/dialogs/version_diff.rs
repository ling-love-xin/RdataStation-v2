//! 版本历史 diff 对比对话框（原型 §4.3 扩展 · 只读查看器）。
//!
//! 数据在宿主取数线程备好：读两版文本 → `similar` 生成 unified patch → 回填 `VersionDiffSeed.patch`。
//! 这里只做 `DiffFile::parse` + `DiffState` + `Diff` 渲染 + Unified / Split 切换。
//!
//! **只读**：不提供任何还原 / 取回 / 编辑入口（文本编辑归编辑器，这里是"查看差异"）——
//! 与版本历史对话框"不做行级 diff（diff 归编辑器）"的决策不冲突：编辑器负责改，`Diff` 负责看。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::diff::{Diff, DiffFile, DiffHoverHighlight, DiffMode, DiffState};
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use crate::ui;

/// 打开 diff 对话框所需的全部信息（宿主取数线程备好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDiffSeed {
    /// 存档显示名（标题用）。
    pub name: String,
    /// 对比的旧版本号（版本历史里选的那一版）。
    pub old_version: i32,
    /// 当前版本号（新侧）。
    pub current_version: i32,
    /// unified diff patch 文本（`similar` 生成；`DiffFile::parse` 在这里解析）。
    pub patch: String,
}

/// 打开版本 diff 对比对话框（宿主入口）。
///
/// `on_close` 在对话框关闭后调用（宿主据此清掉"当前开着的那一个"的记账）。
pub fn open_version_diff_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: VersionDiffSeed,
    on_close: impl Fn(&mut App) + 'static,
) {
    let files = match DiffFile::parse(&seed.patch) {
        Ok(files) if !files.is_empty() => files,
        _ => {
            // patch 为空 / 解析失败：无可展示的差异（宿主已挡了非文本与超限），直接忽略。
            on_close(cx);
            return;
        }
    };
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let name = seed.name.clone();
    let old_version = seed.old_version;
    let current_version = seed.current_version;
    // `DiffState` 在开窗前建好：`open_dialog` 的 builder 是 `Fn`（每帧重建），在里面 `cx.new`
    // 就会每帧漏一个 `DiffState`（`dialogs/mod.rs` 纪律 1）。
    let state = cx.new(|cx| DiffState::new(files, cx));

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        // 模式切换行：统一 / 并排（只改显示，不碰任何数据）。按钮状态每帧从 `state.mode()` 读。
        let is_unified = state.read(cx).mode() == DiffMode::Unified;
        // builder 是 `Fn`（每帧重建）：把 `state` clone 到本地再 move 进按钮闭包，
        // 不能直接 move 外层捕获（否则 `Fn` 闭包无法重新 move）。
        let state_for_unified = state.clone();
        let state_for_split = state.clone();
        let mode_row = div()
            .h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("v{old_version} → v{current_version} 的差异（只读）")),
            )
            .child(
                div().h_flex().gap_1().child(
                    Button::new("diff-mode-unified")
                        .ghost()
                        .small()
                        .toggled(is_unified)
                        .debug_selector(|| "diff-mode-unified".to_string())
                        .label("统一")
                        .on_click(move |_, window, cx| {
                            state_for_unified.update(cx, |state, cx| {
                                state.set_mode(DiffMode::Unified, cx);
                            });
                            window.refresh();
                        }),
                )
                .child(
                    Button::new("diff-mode-split")
                        .ghost()
                        .small()
                        .toggled(!is_unified)
                        .debug_selector(|| "diff-mode-split".to_string())
                        .label("并排")
                        .on_click(move |_, window, cx| {
                            state_for_split.update(cx, |state, cx| {
                                state.set_mode(DiffMode::Split, cx);
                            });
                            window.refresh();
                        }),
                ),
            );

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(mode_row)
            // diff 区域：组件自带滚动与键盘导航，只给一个带高度上限的容器。
            .child(
                Diff::new(&state)
                    .line_number(true)
                    .syntax_highlight(true)
                    .hover_highlight(DiffHoverHighlight::Line)
                    .max_h(rems(ui::DIFF_LIST_MAX_HEIGHT)),
            );

        dialog
            .title(format!("{name} · v{old_version} → v{current_version}"))
            .w(cx.theme().font_size * ui::DIFF_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new().child(
                    Button::new("diff-close")
                        .with_variant(ButtonVariant::Secondary)
                        .small()
                        .debug_selector(|| "diff-close".to_string())
                        .label("关闭")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
            .on_close({
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_cancel(|_, _, _| true)
    });
}

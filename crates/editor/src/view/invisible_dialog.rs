//! 【2026-10-27】不可见字符配置对话框（状态栏「¶」段点击弹出）
//!
//! 场景：¶ 按钮点击弹出**双栏**面板——左栏 = 符号类别清单（每类一个 Switch，自定义该类别
//! 显示 / 隐藏），右栏 = 该类别的符号明细（码点范围说明）。勾选即时生效：回调宿主写文档
//! 位掩码并刷新内核标注（`EditorHostPanel::set_invisible_categories`）。
//!
//! 双栏语义（对应用户要求「哪些符号可以显示、哪些不用显示」）：左栏开关决定**是否标注**
//! 该类字符；右栏列出该类具体收录的符号。掩码 = bit0-3（零宽 / 特殊空格 / 双向 / 控制）。

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::*;

use crate::hidden::{self, HiddenCategory};
use crate::ui;

/// 面板状态：只存当前掩码（渲染读它；Switch 勾选即时更新 + 回调宿主）
pub struct InvisiblePanel {
    mask: u8,
}

/// 打开不可见字符配置对话框
///
/// `on_mask`：掩码变化时回调宿主（`EditorHostPanel::set_invisible_categories`）。
pub fn open_invisible_dialog(
    window: &mut Window,
    cx: &mut App,
    mask: u8,
    on_mask: impl Fn(u8, &mut Window, &mut App) + 'static,
) -> Entity<InvisiblePanel> {
    let on_mask = Rc::new(on_mask);
    let state = cx.new(|_| InvisiblePanel { mask });
    let entity = state.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let fg = theme.colors.foreground;

        // ── 左栏：类别清单（每类一行：名称 + 短述 + 右侧 Switch）──
        let left = div()
            .v_flex()
            .gap_1p5()
            .flex_1()
            .min_w_0()
            .children(hidden::all_categories().into_iter().map(|cat| {
                let (name, hint) = category_meta(cat);
                let on = entity.read(cx).mask & hidden::category_bit(cat) != 0;
                let e = entity.clone();
                let cb = on_mask.clone();
                let sw = Switch::new(format!("invisible-toggle-{:?}", cat))
                    .checked(on)
                    .on_click(move |_, window, cx| {
                        let cur = e.read(cx).mask;
                        let new = toggle_mask(cur, cat);
                        e.update(cx, |panel, cx| {
                            panel.mask = new;
                            cx.notify();
                        });
                        cb(new, window, cx);
                    });
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_1()
                    .child(
                        div()
                            .v_flex()
                            .min_w_0()
                            .child(div().text_sm().text_color(fg).child(name))
                            .child(div().text_xs().text_color(muted).child(hint)),
                    )
                    .child(sw)
            }));

        // ── 右栏：各类别的符号明细（码点范围说明）──
        let right = div()
            .v_flex()
            .gap_1p5()
            .w(rems(ui::INVISIBLE_DETAIL_WIDTH))
            .flex_none()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("符号明细（收录范围）"),
            )
            .children(
                hidden::all_categories()
                    .into_iter()
                    .map(|cat| {
                        let (name, _) = category_meta(cat);
                        let detail = category_detail(cat);
                        let on = entity.read(cx).mask & hidden::category_bit(cat) != 0;
                        div()
                            .v_flex()
                            .gap_0p5()
                            .px_2()
                            .py_1p5()
                            .rounded(rems(0.5))
                            .bg(if on {
                                theme.colors.list_active.opacity(0.4)
                            } else {
                                theme.colors.list
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if on { fg } else { muted })
                                    .child(format!("{name} · {}", if on { "已显示" } else { "已隐藏" })),
                            )
                            .child(div().text_xs().text_color(muted).child(detail))
                    }),
            );

        let body = div()
            .h_flex()
            .items_start()
            .gap_4()
            .w_full()
            .child(left)
            .child(right);
        let width = theme.font_size * ui::INVISIBLE_DIALOG_WIDTH;
        dialog.title("不可见字符").w(width).child(body)
    });

    state
}

/// 翻转某类别的显示位（纯函数，可穷举测试）
pub fn toggle_mask(mask: u8, cat: HiddenCategory) -> u8 {
    mask ^ hidden::category_bit(cat)
}

/// 类别展示名 + 短述（左栏每行）
fn category_meta(cat: HiddenCategory) -> (&'static str, &'static str) {
    match cat {
        HiddenCategory::ZeroWidth => ("零宽与连字控制", "零宽空格 / 连字 / BOM"),
        HiddenCategory::SpecialSpaces => ("特殊空格", "全角空格 / 各类 Unicode 空格"),
        HiddenCategory::Bidi => ("双向控制", "RTL 混淆代码"),
        HiddenCategory::Control => ("控制字符", "C0 / C1 控制字符"),
    }
}

/// 类别收录的符号明细（右栏）
fn category_detail(cat: HiddenCategory) -> &'static str {
    match cat {
        HiddenCategory::ZeroWidth => "ZWSP(U+200B) ZWNJ(U+200C) ZWJ(U+200D) 词连接符(U+2060) BOM(U+FEFF)",
        HiddenCategory::SpecialSpaces => "NBSP(U+00A0) 古爱尔兰空格(U+1680) 2000-200A 窄NBSP(202F) 中空格(205F) 全角空格(U+3000)",
        HiddenCategory::Bidi => "U+202A-202E / U+2066-2069（RTL 双向控制）",
        HiddenCategory::Control => "C0(U+0000-0008,000B,000C,000E-001F) DEL+C1(U+007F-009F)，排除 \\t\\n\\r",
    }
}

#[cfg(test)]
mod tests {
    // 注意：不要 `use super::*`——会把顶层 `gpui_kit::*` 的 `test` 宏带进来，递归展开崩溃
    use super::toggle_mask;
    use crate::hidden::{self, HiddenCategory};

    #[test]
    fn toggle_flips_the_category_bit() {
        assert_eq!(
            toggle_mask(0, HiddenCategory::ZeroWidth),
            hidden::category_bit(HiddenCategory::ZeroWidth)
        );
        let m = toggle_mask(hidden::category_bit(HiddenCategory::ZeroWidth), HiddenCategory::ZeroWidth);
        assert_eq!(m, 0);
    }

    #[test]
    fn mask_accumulates_distinct_categories() {
        let mut m = 0u8;
        for cat in hidden::all_categories() {
            m = toggle_mask(m, cat);
        }
        assert_eq!(m, hidden::FULL_MASK);
    }
}

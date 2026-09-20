//! 重命名对话框（原型 §3.2 的行右键「重命名…」与 §9 的 `F2`）。
//!
//! 只有**显示名**一件事，所以是个单输入的小对话框（与分组名对话框同一形状，
//! `dialogs/group.rs`）、共用同一个宽度常量。文件名与路径**不在这条路上**：
//! 显示名可改、`resources/` 下的物理路径归档时定死（原型 §1 原则 2）。
//!
//! 纪律与另几个对话框一致：**输入实体在开窗前建好**（builder 是 `Fn`）、
//! **校验从输入值推导**（每帧重算，改回去提示自然消失）、执行一律回宿主。

use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use super::archive::name_hint;
use crate::ui;

/// 开窗所需的初值：改的永远是**已存在**的那一条，故带 id 与当前名字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameSeed {
    pub id: String,
    /// 当前显示名（用于预填与「名字没变」判定）。
    pub name: String,
}

/// 提交的动作（执行一律回宿主）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameEvent {
    pub id: String,
    /// 已 trim 的新显示名。
    pub name: String,
}

/// 提交闸门（纯函数，带单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameGate {
    /// 可以提交。
    Ready,
    /// 名字没变：**不给提交但不算错误**（写一次库只为把「月报」改成「月报」没必要，
    /// 与「移动到分组」菜单里当前项置灰同一口径）。
    Unchanged,
    /// 挡住（空名等）：带一句为什么。
    Blocked(&'static str),
}

/// 判定这次重命名能不能提交（空名校验复用归档对话框的 [`name_hint`]，显示名口径只有一处）。
pub fn rename_gate(current: &str, value: &str) -> RenameGate {
    let trimmed = value.trim();
    if let Some(hint) = name_hint(trimmed) {
        return RenameGate::Blocked(hint);
    }
    if trimmed == current.trim() {
        return RenameGate::Unchanged;
    }
    RenameGate::Ready
}

/// 打开「重命名」对话框。
///
/// `name_input` 由调用方**在开窗前**建好并预填当前名字；`on_confirm` 只在可提交时回调
/// （失败由宿主在状态栏与面板提示里说）。
pub fn open_rename_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: RenameSeed,
    name_input: Entity<InputState>,
    on_confirm: impl Fn(RenameEvent, &mut Window, &mut App) + 'static,
) {
    let on_confirm: Rc<dyn Fn(RenameEvent, &mut Window, &mut App)> = Rc::new(on_confirm);
    /// 可提交时的说明：把「改的是什么、不改的是什么」说在前面。
    const HINT: &str = "只改显示名：resources/ 下的文件名与路径不变";

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let value = name_input.read(cx).value().trim().to_string();
        let (hint, is_error, can_submit) = match rename_gate(&seed.name, &value) {
            RenameGate::Ready => (HINT, false, true),
            RenameGate::Unchanged => ("名字没变", false, false),
            RenameGate::Blocked(hint) => (hint, true, false),
        };

        let submit = {
            let on_confirm = on_confirm.clone();
            let input = name_input.clone();
            let id = seed.id.clone();
            let current = seed.name.clone();
            move |window: &mut Window, cx: &mut App| {
                let name = input.read(cx).value().trim().to_string();
                if !matches!(rename_gate(&current, &name), RenameGate::Ready) {
                    return;
                }
                on_confirm(
                    RenameEvent {
                        id: id.clone(),
                        name,
                    },
                    window,
                    cx,
                );
            }
        };
        let submit_for_ok = submit.clone();

        dialog
            .title("重命名")
            .w(cx.theme().font_size * ui::NAME_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(Input::new(&name_input))
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(if is_error {
                                theme.colors.warning
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(hint),
                    ),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("archive-rename-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "archive-rename-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("archive-rename-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "archive-rename-ok".to_string())
                            .label("保存")
                            .disabled(!can_submit)
                            .on_click({
                                let submit = submit.clone();
                                move |_, window, cx| {
                                    submit(window, cx);
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            .on_ok(move |_, window, cx| {
                submit_for_ok(window, cx);
                true
            })
            .on_cancel(|_, _, _| true)
    });
}

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀）。
    use super::{RenameGate, rename_gate};

    /// 三种闸口：空名挡下、名字没变不给提交、真改了才放行（首尾空格不算改动）。
    #[test]
    fn gate_blocks_blank_and_unchanged_only() {
        assert_eq!(
            rename_gate("月报", "  "),
            RenameGate::Blocked("显示名不能为空")
        );
        assert_eq!(rename_gate("月报", "月报"), RenameGate::Unchanged);
        assert_eq!(rename_gate("月报", " 月报 "), RenameGate::Unchanged);
        assert_eq!(rename_gate("月报", "季度月报"), RenameGate::Ready);
        // 只改大小写 / 只改空格之间的字都算真改动。
        assert_eq!(rename_gate("dau.sql", "DAU.sql"), RenameGate::Ready);
    }
}

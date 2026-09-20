//! 「改一个显示字段」对话框（原型 §3.1 头部可编辑 + §3.2 行右键的「重命名…」与 `F2`）。
//!
//! 两个字段共用这一个形状（单输入 + 一行提示 + 保存 / 取消）：
//! - **显示名**（`F2` / 行右键 / 详情头部点名字）：空名挡下；文件名与路径**不在这条路上**；
//! - **别名**（详情头部点别名）：可留空——**留空就是清除**（别名是给人看的第二个叫法）。
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

/// 要改的是哪个字段（决定标题、占位符、闸门与提交的事件语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameField {
    /// 显示名（空名挡下）。
    DisplayName,
    /// 别名（可留空 = 清除）。
    Alias,
}

/// 开窗所需的初值：改的永远是**已存在**的那一条，故带 id 与当前值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameSeed {
    pub id: String,
    /// 该字段的当前值（显示名必非空；别名可能为空 = 没有别名）。
    pub name: String,
    pub field: RenameField,
}

/// 提交的动作（执行一律回宿主）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameEvent {
    pub id: String,
    /// 已 trim 的新值；别名为空串 = **清除别名**。
    pub name: String,
    pub field: RenameField,
}

/// 提交闸门（纯函数，带单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameGate {
    /// 可以提交。
    Ready,
    /// 值没变：**不给提交但不算错误**（写一次库只为把「月报」改成「月报」没必要，
    /// 与「移动到分组」菜单里当前项置灰同一口径）。
    Unchanged,
    /// 挡住（空名等）：带一句为什么。
    Blocked(&'static str),
}

impl RenameField {
    /// 对话框标题。
    fn title(&self) -> &'static str {
        match self {
            RenameField::DisplayName => "重命名",
            RenameField::Alias => "编辑别名",
        }
    }

    /// 输入框占位符。
    fn placeholder(&self) -> &'static str {
        match self {
            RenameField::DisplayName => "显示名",
            // 留空是**有意义的动作**（清除），所以占位符要说清它。
            RenameField::Alias => "别名（留空 = 清除）",
        }
    }

    /// 可提交时的说明：把「改的是什么、不改的是什么」说在前面。
    fn hint(&self) -> &'static str {
        match self {
            RenameField::DisplayName => "只改显示名：resources/ 下的文件名与路径不变",
            RenameField::Alias => "别名只是另一个叫法：不影响显示名、文件名与路径",
        }
    }
}

/// 判定这次编辑能不能提交（空名 / 空别名两种口径的分叉都在这里）。
pub fn rename_gate(field: RenameField, current: &str, value: &str) -> RenameGate {
    let trimmed = value.trim();
    match field {
        RenameField::DisplayName => {
            if let Some(hint) = name_hint(trimmed) {
                return RenameGate::Blocked(hint);
            }
            // 显示名的空名校验只此一处（归档表单也用 `name_hint`）。
            if trimmed == current.trim() {
                return RenameGate::Unchanged;
            }
            RenameGate::Ready
        }
        RenameField::Alias => {
            // 别名：**空是合法的**（= 清除），所以不走 `name_hint`。
            if trimmed == current.trim() {
                return RenameGate::Unchanged;
            }
            RenameGate::Ready
        }
    }
}

/// 打开「重命名 / 编辑别名」对话框。
///
/// `name_input` 由调用方**在开窗前**建好并预填当前值；`on_confirm` 只在可提交时回调
/// （失败由宿主在状态栏与面板提示里说）。
pub fn open_rename_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: RenameSeed,
    name_input: Entity<InputState>,
    on_confirm: impl Fn(RenameEvent, &mut Window, &mut App) + 'static,
) {
    let on_confirm: Rc<dyn Fn(RenameEvent, &mut Window, &mut App)> = Rc::new(on_confirm);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let value = name_input.read(cx).value().trim().to_string();
        let field = seed.field;
        let (hint, is_error, can_submit) = match rename_gate(field, &seed.name, &value) {
            RenameGate::Ready => (field.hint(), false, true),
            RenameGate::Unchanged => ("值没变", false, false),
            RenameGate::Blocked(hint) => (hint, true, false),
        };

        let submit = {
            let on_confirm = on_confirm.clone();
            let input = name_input.clone();
            let id = seed.id.clone();
            let current = seed.name.clone();
            move |window: &mut Window, cx: &mut App| {
                let name = input.read(cx).value().trim().to_string();
                if !matches!(rename_gate(field, &current, &name), RenameGate::Ready) {
                    return;
                }
                on_confirm(
                    RenameEvent {
                        id: id.clone(),
                        name,
                        field,
                    },
                    window,
                    cx,
                );
            }
        };
        let submit_for_ok = submit.clone();

        dialog
            .title(field.title())
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

/// 供宿主与测试共用的占位符（避免两处硬编码同一个字符串）。
pub fn rename_placeholder(field: RenameField) -> &'static str {
    field.placeholder()
}

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀）。
    use super::{RenameField, RenameGate, rename_gate};

    /// 显示名：空名挡下、名字没变不给提交、真改了才放行（首尾空格不算改动）。
    #[test]
    fn display_name_gate_blocks_blank_and_unchanged_only() {
        let field = RenameField::DisplayName;
        assert_eq!(
            rename_gate(field, "月报", "  "),
            RenameGate::Blocked("显示名不能为空")
        );
        assert_eq!(rename_gate(field, "月报", "月报"), RenameGate::Unchanged);
        assert_eq!(rename_gate(field, "月报", " 月报 "), RenameGate::Unchanged);
        assert_eq!(rename_gate(field, "月报", "季度月报"), RenameGate::Ready);
        // 只改大小写 / 只改空格之间的字都算真改动。
        assert_eq!(rename_gate(field, "dau.sql", "DAU.sql"), RenameGate::Ready);
    }

    /// 别名：**空是合法的**（清除），所以不走空名校验；其余与显示名同。
    #[test]
    fn alias_gate_allows_clearing_but_not_no_op() {
        let field = RenameField::Alias;
        assert_eq!(rename_gate(field, "", ""), RenameGate::Unchanged);
        assert_eq!(rename_gate(field, "", "月报"), RenameGate::Ready);
        // 有别名时清空 = 真改动（这是「清除别名」那条路）。
        assert_eq!(rename_gate(field, "月报", "  "), RenameGate::Ready);
        assert_eq!(rename_gate(field, "月报", "月报"), RenameGate::Unchanged);
        assert_eq!(rename_gate(field, "月报", " 月报 "), RenameGate::Unchanged);
    }
}

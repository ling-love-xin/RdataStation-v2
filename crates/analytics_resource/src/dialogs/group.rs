//! 分组对话框（原型 §2.4 的「分组头右键」与 §3.2 的「移动到分组」）。
//!
//! 只有两件事，所以只有一个文件两个入口：
//! - **新建 / 重命名分组**：开一个单输入的小对话框（`GroupNameEvent` 里区分）；
//! - **移动到分组**：不进对话框——行右键菜单里直接列出各分组（一次点击完成，
//!   多一级对话框反而多两步）。
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

use crate::ui;

/// 这个对话框在干嘛（决定标题、占位符与提交的动作）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupNameKind {
    /// 新建分组。
    Create,
    /// 重命名某个分组。
    Rename { id: String },
}

/// 提交的动作（执行一律回宿主）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupNameEvent {
    Create { name: String },
    Rename { id: String, name: String },
}

/// 打开「新建分组」/「重命名分组」对话框。
///
/// `name_input` 由调用方**在开窗前**建好（重命名时预填当前名字）。
/// `on_confirm` 只在名字非空时回调；回调后关窗（失败由宿主在状态栏与面板提示里说）。
pub fn open_group_name_dialog(
    window: &mut Window,
    cx: &mut App,
    kind: GroupNameKind,
    name_input: Entity<InputState>,
    on_confirm: impl Fn(GroupNameEvent, &mut Window, &mut App) + 'static,
) {
    let on_confirm: Rc<dyn Fn(GroupNameEvent, &mut Window, &mut App)> = Rc::new(on_confirm);
    let (title, placeholder, ok_label, hint) = match &kind {
        GroupNameKind::Create => (
            "新建分组",
            "分组名",
            "创建",
            "分组是单层的：没有子分组，也不影响存档本体",
        ),
        GroupNameKind::Rename { .. } => ("重命名分组", "分组名", "保存", "只改显示名：组里的存档不动"),
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let value = name_input.read(cx).value().trim().to_string();
        let error = value.is_empty().then_some("分组名不能为空");
        let can_submit = error.is_none();
        let _ = placeholder;

        let submit = {
            let on_confirm = on_confirm.clone();
            let input = name_input.clone();
            let kind = kind.clone();
            move |window: &mut Window, cx: &mut App| {
                let name = input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    return;
                }
                let event = match &kind {
                    GroupNameKind::Create => GroupNameEvent::Create { name },
                    GroupNameKind::Rename { id } => GroupNameEvent::Rename {
                        id: id.clone(),
                        name,
                    },
                };
                on_confirm(event, window, cx);
            }
        };
        let submit_for_ok = submit.clone();

        let mut body = div().v_flex().w_full().gap_2().child(Input::new(&name_input));
        body = body.child(
            div()
                .w_full()
                .text_xs()
                .text_color(if error.is_some() {
                    theme.colors.warning
                } else {
                    theme.colors.muted_foreground
                })
                .child(error.unwrap_or(hint)),
        );

        dialog
            .title(title)
            .w(cx.theme().font_size * ui::NAME_DIALOG_WIDTH)
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("group-name-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "group-name-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("group-name-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "group-name-ok".to_string())
                            .label(ok_label)
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
    use super::GroupNameKind;

    /// 两种形态的标题 / 占位符不同，而提交事件由 kind 决定（重命名要带 id）。
    #[test]
    fn kind_decides_the_shape_of_the_event() {
        assert_ne!(GroupNameKind::Create, GroupNameKind::Rename { id: "af_1".to_string() });
        let create = GroupNameKind::Create;
        let rename = GroupNameKind::Rename {
            id: "af_1".to_string(),
        };
        // 这里只钉住“两种形态不同源”：具体事件在 `open_group_name_dialog` 的提交闭包里构造，
        // 由窗口用例覆盖（见 `tests/dialog_window.rs`）。
        assert!(matches!(create, GroupNameKind::Create));
        assert!(matches!(rename, GroupNameKind::Rename { .. }));
    }
}

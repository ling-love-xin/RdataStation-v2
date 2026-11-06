//! 标签行尾「⋯」菜单：重命名与删除。

use super::*;

pub(super) fn tag_menu(
    state: TagDialogState,
    on_action: Rc<dyn Fn(TagDialogEvent, &mut Window, &mut App)>,
    tag_id: String,
    tag_name: String,
    can_edit: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu {
                move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
                    let rename_dispatch = {
                        let state = state.clone();
                        let on_action = on_action.clone();
                        let tag_id = tag_id.clone();
                        let tag_name = tag_name.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if !can_edit {
                                return;
                            }
                            // 输入实体在开窗前建好（builder 是 `Fn`，见模块头注释）。
                            let input = cx.new(|cx| {
                                InputState::new(window, cx).placeholder("标签名")
                            });
                            let value = tag_name.clone();
                            input.update(cx, |input, cx| input.set_value(value, window, cx));
                            let state = state.clone();
                            let on_action = on_action.clone();
                            let tag_id = tag_id.clone();
                            open_tag_rename_dialog(
                                window,
                                cx,
                                tag_id,
                                input,
                                move |id, name, window, cx| {
                                    state.set_busy(true);
                                    state.set_note(Some("正在重命名…".to_string()));
                                    on_action(TagDialogEvent::RenameTag { id, name }, window, cx);
                                },
                            );
                        }
                    };
                    let delete_dispatch = {
                        let state = state.clone();
                        let on_action = on_action.clone();
                        let tag_id = tag_id.clone();
                        let tag_name = tag_name.clone();
                        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                            if !can_edit {
                                return;
                            }
                            let state = state.clone();
                            let on_action = on_action.clone();
                            let tag_id = tag_id.clone();
                            let name = tag_name.clone();
                            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                                let state = state.clone();
                                let on_action = on_action.clone();
                                let tag_id = tag_id.clone();
                                alert
                                    .confirm()
                                    .title(format!("删除标签「{name}」？"))
                                    .description(
                                        "标签定义会软删并从列表消失，存档上的关联不再显示（存档本身不受影响；同名重建不会带回旧关联）。",
                                    )
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text("删除")
                                            .ok_variant(ButtonVariant::Danger)
                                            .show_cancel(true),
                                    )
                                    .on_ok(move |_, window, cx| {
                                        state.set_busy(true);
                                        state.set_note(Some("正在删除标签…".to_string()));
                                        on_action(
                                            TagDialogEvent::DeleteTag { id: tag_id.clone() },
                                            window,
                                            cx,
                                        );
                                        true
                                    })
                            });
                        }
                    };
                    let mut menu = menu;
                    if can_edit {
                        menu = menu.item(
                            PopupMenuItem::new("重命名…")
                                .disabled(!can_edit)
                                .on_click(rename_dispatch),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("删除")
                                .disabled(!can_edit)
                                .on_click(delete_dispatch),
                        );
                    }
                    menu
                }
}

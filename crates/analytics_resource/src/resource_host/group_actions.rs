//! 分组与重命名动作。

use super::*;

pub(super) fn request_create_group(host: &ResourceHost, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法新建分组", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能改分组", cx);
            return;
        }
        let backend = host.backend.clone();
        let read_only = host.read_only();
        // 输入实体在开窗前建好（builder 是 `Fn`，每帧重建）。
        let input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx).placeholder("分组名")
        });
        open_group_name_dialog(
            window,
            cx,
            GroupNameKind::Create,
            input,
            move |event, _window, cx| {
                let GroupNameEvent::Create { name } = event else {
                    return;
                };
                resource_jobs::enqueue_group_action(
                    root.clone(),
                    read_only,
                    resource_jobs::GroupJobAction::Create { name: name.clone() },
                );
                backend.refresh_resources(cx);
                say(&backend, format!("资产库：正在新建分组「{name}」…"), cx);
            },
        );
}

pub(super) fn request_rename_group(host: &ResourceHost, folder_id: &str, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法重命名分组", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能改分组", cx);
            return;
        }
        // 当前名字从面板快照取（分组字典就在里面）；取不到就空着让用户自己填。
        let current = host.group_name(folder_id, cx).unwrap_or_default();
        let backend = host.backend.clone();
        let read_only = host.read_only();
        let folder_id = folder_id.to_string();
        let input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx).placeholder("分组名")
        });
        input.update(cx, |input, cx| input.set_value(current, window, cx));
        open_group_name_dialog(
            window,
            cx,
            GroupNameKind::Rename {
                id: folder_id.clone(),
            },
            input,
            move |event, _window, cx| {
                let GroupNameEvent::Rename { id, name } = event else {
                    return;
                };
                resource_jobs::enqueue_group_action(
                    root.clone(),
                    read_only,
                    resource_jobs::GroupJobAction::Rename {
                        id,
                        name: name.clone(),
                    },
                );
                backend.refresh_resources(cx);
                say(&backend, format!("资产库：正在重命名分组为「{name}」…"), cx);
            },
        );
}

pub(super) fn request_rename(host: &ResourceHost, resource_id: &str, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法重命名", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能改显示名", cx);
            return;
        }
        // 当前名字从面板快照取（与 `request_rename_group` 同一处取法）：取不到就让输入框空着。
        let current = host.row_name(resource_id, cx).unwrap_or_default();
        let backend = host.backend.clone();
        let read_only = host.read_only();
        let resource_id = resource_id.to_string();
        let input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder(rename_placeholder(RenameField::DisplayName))
        });
        let prefilled = current.clone();
        input.update(cx, |input, cx| input.set_value(prefilled, window, cx));
        open_rename_dialog(
            window,
            cx,
            RenameSeed {
                id: resource_id.clone(),
                name: current.clone(),
                field: RenameField::DisplayName,
            },
            input,
            move |event, _window, cx| {
                resource_jobs::enqueue_rename(
                    root.clone(),
                    read_only,
                    event.id.clone(),
                    event.name.clone(),
                );
                backend.refresh_resources(cx);
                say(
                    &backend,
                    format!("资产库：正在把「{current}」改名为「{}」…", event.name),
                    cx,
                );
            },
        );
}

pub(super) fn request_edit_alias(host: &ResourceHost, resource_id: &str, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法编辑别名", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能改别名", cx);
            return;
        }
        // 当前别名从**详情**取（头部就展示它）；取不到（面板还没收到这条详情）就当空的。
        let current = host
            .row_alias(resource_id, cx)
            .unwrap_or_default()
            .unwrap_or_default();
        let backend = host.backend.clone();
        let read_only = host.read_only();
        let resource_id = resource_id.to_string();
        let input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder(rename_placeholder(RenameField::Alias))
        });
        let prefilled = current.clone();
        input.update(cx, |input, cx| input.set_value(prefilled, window, cx));
        open_rename_dialog(
            window,
            cx,
            RenameSeed {
                id: resource_id.clone(),
                name: current.clone(),
                field: RenameField::Alias,
            },
            input,
            move |event, _window, cx| {
                resource_jobs::enqueue_set_alias(
                    root.clone(),
                    read_only,
                    event.id.clone(),
                    event.name.clone(),
                );
                backend.refresh_resources(cx);
                say(&backend, "资产库：正在更新别名…", cx);
            },
        );
}

pub(super) fn request_delete_group(host: &ResourceHost, folder_id: &str, window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法删除分组", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能改分组", cx);
            return;
        }
        let name = host
            .group_name(folder_id, cx)
            .unwrap_or_else(|| "该分组".to_string());
        let backend = host.backend.clone();
        let read_only = host.read_only();
        let folder_id = folder_id.to_string();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let backend = backend.clone();
            let folder_id = folder_id.clone();
            // 外层是 `Fn`（弹框可能被重建）：`root` 要每帧各拿一份。
            let root = root.clone();
            alert
                .confirm()
                .title(format!("删除分组「{name}」？"))
                .description("组里的存档不会被删，它们会回到「未分组」。")
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .ok_text("删除")
                        .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                        .show_cancel(true),
                )
                .on_ok(move |_, _window, cx| {
                    resource_jobs::enqueue_group_action(
                        root.clone(),
                        read_only,
                        resource_jobs::GroupJobAction::Delete {
                            id: folder_id.clone(),
                        },
                    );
                    backend.refresh_resources(cx);
                    say(&backend, "资产库：正在删除分组…".to_string(), cx);
                    true
                })
        });
}

pub(super) fn request_move_to_group(host: &ResourceHost, resource_ids: &[String], folder_id: Option<&str>, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法移动", cx) else {
            return;
        };
        let scope = match resource_ids.len() {
            0 => return,
            1 => String::new(),
            count => format!("{count} 条存档"),
        };
        let target = match folder_id {
            Some(id) => host
                .group_name(id, cx)
                .unwrap_or_else(|| "该分组".to_string()),
            None => "未分组".to_string(),
        };
        resource_jobs::enqueue_group_action(
            root,
            host.read_only(),
            resource_jobs::GroupJobAction::Move {
                resource_ids: resource_ids.to_vec(),
                folder_id: folder_id.map(str::to_string),
            },
        );
        host.backend.refresh_resources(cx);
        host.notice(format!("资产库：正在把{scope}移到「{target}」…"), cx);
}

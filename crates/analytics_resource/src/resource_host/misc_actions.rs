//! 排序/折叠记忆、回收站入口、标签编辑与刷新。

use super::*;

pub(super) fn remember_sort(host: &ResourceHost, field: SortField, _order: SortOrder, cx: &mut App) {

        host.backend.set_default_resource_sort(field.key(), cx);
}

pub(super) fn remember_collapsed(host: &ResourceHost, collapsed_keys: &[String], cx: &mut App) {

        // 没有项目时没什么可记的（面板也在空态，不会有折叠动作）。
        let Some(root) = host.backend.project_root() else {
            return;
        };
        host.backend
            .set_collapsed_groups(&root.to_string_lossy(), collapsed_keys, cx);
}

pub(super) fn remember_default_group(host: &ResourceHost, folder_id: Option<&str>, cx: &mut App) {

        // 与 `remember_collapsed` 同一口径：没有项目就没什么可记的。
        let Some(root) = host.backend.project_root() else {
            return;
        };
        host.backend
            .set_default_group(&root.to_string_lossy(), folder_id, cx);
}

pub(super) fn request_open_trash(host: &ResourceHost, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法打开回收站", cx) else {
            return;
        };
        // 取数在后台线程（读 `.RSmeta/trash` 里每个条目的 manifest）；
        // 回来之后由侧栏轮询开窗（开窗要 `Window`，轮询任务里没有）。
        resource_jobs::enqueue_trash_list(root);
        host.notice("资产库：正在读取回收站…", cx);
}

pub(super) fn request_edit_tags(host: &ResourceHost, targets: &[TagTarget], _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法编辑标签", cx) else {
            return;
        };
        if targets.is_empty() {
            return;
        }
        // 取数在后台线程（标签词典 + 这批目标各自已挂的）；回来之后由侧栏轮询开窗。
        let count = targets.len();
        resource_jobs::enqueue_tag_list(root, targets.to_vec());
        host.notice(
            if count > 1 {
                format!("资产库：正在读取 {count} 项的标签…")
            } else {
                "资产库：正在读取标签…".to_string()
            },
            cx,
        );
}

pub(super) fn request_remove_tag(host: &ResourceHost, detail: &ArchiveDetail, tag_id: &str, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法去标签", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能改标签", cx);
            return;
        }
        // 作业在侧栏跑（回执与刷新同一条路）：去标签会改快照里的 chips 与筛选维，
        // 所以入队后立刻要一次刷新。
        resource_jobs::enqueue_tag_action(
            root,
            host.read_only(),
            vec![TagTarget {
                id: detail.id.clone(),
                name: detail.name.clone(),
            }],
            resource_jobs::TagJobAction::RemoveOne {
                tag_id: tag_id.to_string(),
            },
        );
        host.backend.refresh_resources(cx);
        host.notice(
            format!("资产库：正在去掉「{}」的一个标签…", detail.name),
            cx,
        );
}

pub(super) fn request_refresh(host: &ResourceHost, _window: &mut Window, cx: &mut App) {

        // 刷新不写回执：状态行的「加载中…」就是它的回执，再叠一条只是噪声。
        if host.backend.project_root().is_none() {
            host.pending("无法刷新", "：还没有打开项目", cx);
            return;
        }
        host.backend.refresh_resources(cx);
}

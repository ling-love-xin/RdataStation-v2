//! 删除与索引类动作：回收站、撤销、重建索引、打开资源目录。

use super::*;

pub(super) fn request_delete(host: &ResourceHost, targets: &[DeleteTarget], window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法移入回收站", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能移入回收站", cx);
            return;
        }
        if targets.is_empty() {
            return;
        }

        // 文案按归档粒度区分本体命运（§12.1.1 ③）：copy 搬副本进回收站；reference 本体
        // 留原位、只删登记行。混合时分组说清各有几项。
        let copy_count = targets
            .iter()
            .filter(|t| t.mode == PayloadMode::Copy)
            .count();
        let reference_count = targets.len() - copy_count;
        let description = match (copy_count, reference_count) {
            (_, 0) => "将删除 resources/ 副本与登记记录，之后可从回收站还原。".to_string(),
            (0, _) => "本体留在源位置不动，只删除登记记录，之后可从回收站还原登记行。".to_string(),
            _ => format!(
                "{copy_count} 项副本将删除 resources/ 副本与登记记录；{reference_count} 项引用本体留在源位置、只删除登记记录。之后均可从回收站还原。"
            ),
        };
        let title = match targets.len() {
            1 => "移入回收站？".to_string(),
            count => format!("移入回收站（{count} 项）？"),
        };

        let ids: Vec<String> = targets.iter().map(|t| t.id.clone()).collect();
        let backend = host.backend.clone();
        let read_only = host.read_only();
        // 回收站是**项目级**的（与草稿箱共用 `.RSmeta/trash`）：搬本体与软删登记行
        // 都在服务层一次完成，宿主不自己拼回收站路径（不做两套回收站）。
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let backend = backend.clone();
            let root = root.clone();
            let ids = ids.clone();
            alert
                .confirm()
                .title(title.clone())
                .description(description.clone())
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .ok_text("移入回收站")
                        .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                        .show_cancel(true),
                )
                .on_ok(move |_, _window, cx| {
                    resource_jobs::enqueue_trash(root.clone(), read_only, ids.clone());
                    backend.refresh_resources(cx);
                    say(&backend, "资产库：正在移入回收站…", cx);
                    true
                })
        });
}

pub(super) fn request_reveal(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        let Some(path) = host.payload_path(detail, cx) else {
            return;
        };
        // `opener::reveal` 会选中该文件（而不是只打开目录）；失败给回执而不是静默。
        if let Err(error) = opener::reveal(&path) {
            host.notice(format!("资产库：打开资源目录失败（{error}）"), cx);
        }
}

pub(super) fn request_copy_path(host: &ResourceHost, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {

        let Some(path) = host.payload_path(detail, cx) else {
            return;
        };
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            path.to_string_lossy().to_string(),
        ));
        host.notice("资产库：已复制本体路径", cx);
}

pub(super) fn request_undo_archive(host: &ResourceHost, undo: &ArchiveUndo, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法撤销", cx) else {
            return;
        };
        if host.read_only() {
            host.notice("资产库：项目为只读模式，不能撤销", cx);
            return;
        }
        // 凭据原样交给工作线程（面板只持有它，不认识服务层）。
        resource_jobs::enqueue_undo(root, host.read_only(), undo.clone());
        host.backend.refresh_resources(cx);
        host.notice("资产库：正在撤销…", cx);
}

pub(super) fn request_index_repair(host: &ResourceHost, _window: &mut Window, cx: &mut App) {

        // 扫描要逐个本体算 sha256（“指纹不匹配”那一档的代价），所以走后台线程；
        // 报告回来后再开对话框（开窗要 `Window`，轮询任务里没有）。
        let Some(root) = host.require_project("无法检查索引", cx) else {
            return;
        };
        resource_jobs::enqueue_index_scan(root);
        host.notice("资产库：正在检查索引…", cx);
}

pub(super) fn request_open_payload_dir(host: &ResourceHost, _window: &mut Window, cx: &mut App) {

        let Some(root) = host.require_project("无法打开资源目录", cx) else {
            return;
        };
        let dir = PayloadStore::new(root).resources_dir();
        // 目录由第一次归档创建：还没有存档时它不存在，**说清这一点**而不是报一个系统错误。
        if !dir.exists() {
            host.notice(
                format!(
                    "资产库：资源目录还不存在（{}）——归档第一个存档时会创建",
                    dir.display()
                ),
                cx,
            );
            return;
        }
        if let Err(error) = opener::open(&dir) {
            host.notice(format!("资产库：打开资源目录失败（{error}）"), cx);
        }
}

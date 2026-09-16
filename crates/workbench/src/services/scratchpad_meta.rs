//! 草稿执行回执 → 草稿元数据回写（M5 Phase C-2 后半）
//!
//! 编辑器把「哪份文档、用了哪个连接、成没成」留在 `EditorShared`（`ExecReceipt`）；
//! 这里每秒收一次，只对**落在草稿箱模块目录里**的文件写回 `file_meta`
//! （`last_connection_id` / `last_executed_at`）。
//!
//! 写的是元数据（`{项目}/.RSmeta/scratchpad/config.json`）：按架构 K1c，元数据级操作
//! 保持同步（一次小 JSON 写，微秒~毫秒级），不开后台任务也不必新起线程。
//! 写失败进状态栏提示，不静默。
//!
//! 边界：编辑器不认识草稿箱，这里也不改编辑器——回执是**加法**（结果仍归编辑区）；
//! 「哪些路径算草稿、同一草稿取哪条」是草稿箱的领域规则，落在
//! `ScratchpadStore::draft_targets`（crate 内、带单测）。

use std::path::PathBuf;

use editor::shared::EditorShared;
use gpui_kit::App;

use crate::panels::Shared;

/// 一拍：取回执 → 落到模块内的草稿 → 写回元数据。
///
/// 无回执时只花一次空 `Vec` 比较；未打开项目时回执直接丢弃（草稿箱此时不存在，不是错误）。
pub fn write_back(shared: &Shared, editor: &EditorShared, cx: &mut App) {
    let receipts = editor.drain_exec_receipts();
    if receipts.is_empty() {
        return;
    }

    // 只记成功的执行：失败的执行不该改变“上次用的连接”。文档路径在同一个只读借用里取完。
    let attempts: Vec<(PathBuf, Option<String>)> = {
        let service = editor.service();
        receipts
            .into_iter()
            .filter(|receipt| receipt.succeeded)
            .filter_map(|receipt| {
                service
                    .find(&receipt.document)
                    .and_then(|document| document.path().map(std::path::Path::to_path_buf))
                    .map(|path| (path, receipt.connection))
            })
            .collect()
    };
    if attempts.is_empty() {
        return;
    }

    let Ok((store, runtime)) = shared.scratchpad_store() else {
        // 未打开项目：草稿箱此时不存在，回执丢弃（不是错误）。
        return;
    };
    for (relative, connection) in store.draft_targets(attempts) {
        if let Err(error) = runtime.block_on(store.update_file_meta(&relative, connection)) {
            *shared.notice.borrow_mut() =
                Some(format!("草稿元数据写入失败（{relative}）: {error}"));
            shared.notify_host(cx);
        }
    }
}

//! 洞察接缝 · 历史与快照请求（自 jobs.rs 拆分，纯位移）

use super::*;

/// 读某列的历史（切到「历史」Tab 或缺载荷时补取）。
///
/// 读失败推整页错误态（与其他 Tab 的取数同形）：列表无从部分展示，
/// 而「为什么读不到」正是用户此刻要的答案。
pub fn request_history(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    column: String,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::column_history_view(project_root.as_deref(), &column)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(history) => panel.set_history(history, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}

/// 清理旧快照（删除**不可撤销**，所以面板上的入口要先过一道确认框）。
///
/// 保留天数由这里持有（`SNAPSHOT_RETENTION_DAYS`）：面板文案与实际切档取自同一个常量，
/// 且事件不带天数——否则哪天多一个入口，两边就可能各写一个数。
///
/// 失败只挂行内提示：列表与已有快照都还在，推整页错误态反而是吓人。
pub fn request_cleanup(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    column: String,
    cx: &mut App,
) {
    let days = crate::model::SNAPSHOT_RETENTION_DAYS;
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::cleanup_old_snapshots(project_root.as_deref(), &column, days)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(history) => panel.set_history(history, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_history_notice(info.message, cx);
            }
        });
    })
    .detach();
}

/// 对比某一版与最新一版（读两版正文再算差）。
///
/// 失败只挂**行内提示并放掉选中位**：列表本身是好的，推整页错误态会让人以为
/// 历史没了；而选中位若不放，界面上会留下「亮着但没有面板」的死状态。
pub fn request_version_compare(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    column: String,
    version_id: String,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::compare_column_snapshots(project_root.as_deref(), &column, &version_id)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(history) => panel.set_history(history, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_compare_notice(info.message, cx);
            }
        });
    })
    .detach();
}

/// 保存快照：重取领域画像 + 双写都在服务里，返回刷新后的历史。
///
/// 失败只挂**行内提示**（不是整页错误态）：保存失败时最要紧的是「已有的历史还在」，
/// 整页转错误态反而会让人以为快照丢了。
pub fn request_snapshot_save(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    temp_table: String,
    column: String,
    source_label: Option<String>,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::save_column_snapshot(
            project_root.as_deref(),
            &temp_table,
            &column,
            source_label.as_deref(),
        )
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(history) => panel.set_history(history, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_history_notice(info.message, cx);
            }
        });
    })
    .detach();
}
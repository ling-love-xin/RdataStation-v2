//! 洞察画像的后台执行（M8 Phase 1）。
//!
//! 为什么需要：列画像要抢 DuckDB 全局锁并逐条跑规则（多次往返），在 UI 线程 `block_on`
//! 会冻结界面。本模块把**阻塞段放到后台执行器**，跑完再把结果回填面板：
//! 面板只发 [`InsightEvent`]，本模块负责「解析目标 → 后台取数 → 回填四态」。
//!
//! 与 `mock_jobs` / `scratchpad_jobs` 的差别：那两个是**用户可取消的长任务**（要进度与取消），
//! 因此用「单一工作线程 + 结果槽 + 轮询」；洞察单次画像只是一次往返，用执行器短任务即可，
//! 不需要进度槽——失败与并发受限都由 [`InsightService::describe_error`] 给出可读文案。
//!
//! 线程边界：`Shared` 里的 `Rc<RefCell<…>>` 不可跨线程，所以**项目根在提交时解析成
//! 所有权数据**交给任务；后台闭包不碰任何宿主状态。

use std::path::PathBuf;

use gpui_kit::{App, Entity};
use insight::{InsightEvent, InsightService, InsightTarget, InsightView};

/// 一次列画像请求：后台执行所需的全部输入（均为所有权数据，因此可跨线程）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRequest {
    pub temp_table: String,
    pub column: String,
}

impl ProfileRequest {
    /// 从面板目标解析出请求；**当前不支持的形态返回 `None`**。
    ///
    /// Phase 1 只做列画像：表探查 / 多列 / 结构在后续期次落地，面板侧已按 Tab 给出期次提示，
    /// 这里就不再假装有行为。
    pub fn of(target: &InsightTarget) -> Option<Self> {
        match target {
            InsightTarget::Column {
                temp_table, column, ..
            } => Some(Self {
                temp_table: temp_table.clone(),
                column: column.clone(),
            }),
            InsightTarget::Table { .. }
            | InsightTarget::MultiColumn { .. }
            | InsightTarget::Schema { .. } => None,
        }
    }
}

/// 面板事件 → 执行动作（宿主侧的唯一入口）。
pub fn handle_event(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    event: &InsightEvent,
    cx: &mut App,
) {
    match event {
        InsightEvent::ProfileRequested { target } => {
            let Some(request) = ProfileRequest::of(target) else {
                return;
            };
            request_profile(view, project_root, request, cx);
        }
    }
}

/// 提交一次画像请求：后台执行 → 回填面板。
///
/// 回填只走面板的公开方法（`set_profile` / `set_error`），错误经 `describe_error` 转成
/// 「给人看的文案 + 是否可重试」——面板因此永远不接触 `CoreError`。
pub fn request_profile(
    view: &Entity<InsightView>,
    project_root: Option<PathBuf>,
    request: ProfileRequest,
    cx: &mut App,
) {
    let weak = view.downgrade();
    let task = cx.background_executor().spawn(async move {
        InsightService::profile_column_view(
            project_root.as_deref(),
            &request.temp_table,
            &request.column,
        )
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        // 面板可能已关闭：弱句柄升级失败就丢弃结果（不 panic）
        let _ = weak.update(cx, |panel, cx| match result {
            Ok(profile) => panel.set_profile(profile, cx),
            Err(err) => {
                let info = InsightService::describe_error(&err);
                panel.set_error(info.message, info.retryable, cx);
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::ProfileRequest;
    use insight::InsightTarget;

    #[test]
    fn column_target_maps_to_a_request() {
        let target = InsightTarget::Column {
            temp_table: "t_result_1".into(),
            column: "amount".into(),
            data_type: "DECIMAL(12,2)".into(),
        };
        assert_eq!(
            ProfileRequest::of(&target),
            Some(ProfileRequest {
                temp_table: "t_result_1".into(),
                column: "amount".into(),
            })
        );
    }

    #[test]
    fn later_phase_targets_are_not_pretended() {
        // Phase 1 只做列画像：其余目标没有生产者就不该被当成有行为
        for target in [
            InsightTarget::Table {
                temp_table: "t".into(),
                table_name: "orders".into(),
            },
            InsightTarget::MultiColumn {
                temp_table: "t".into(),
                columns: vec!["a".into()],
            },
            InsightTarget::Schema {
                conn_id: "G_1".into(),
                schema: None,
            },
        ] {
            assert_eq!(ProfileRequest::of(&target), None, "{target:?}");
        }
    }
}

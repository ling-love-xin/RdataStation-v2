//! 洞察的**事件接缝 + 后台取数**（M8 Phase 1）。
//!
//! # 为什么在特性 crate 内
//!
//! 耦合治理计划 `docs/architecture/layout/panels-coupling-plan.md` §5 定的目标形态是
//! 「特性 crate 内的 jobs + 面板 drain」。放这里还有一层理由：Phase 2 的「评估全表 + 进度」
//! 与 Phase 5 的快照保存都要用同一套后台形态，先用对位置就不用写两遍。
//! 宿主（workbench）侧因此只剩「提供项目根」这一行胶水（见 [`attach`]）。
//!
//! # 为什么需要后台
//!
//! 列画像要抢 DuckDB 全局锁并逐条跑规则（多次往返），在 UI 线程 `block_on` 会冻结界面。
//! 本模块把**阻塞段放到后台执行器**，跑完再把结果回填面板——面板只发 [`InsightEvent`]。
//!
//! # 线程边界
//!
//! `Shared` 里的 `Rc<RefCell<…>>` 不可跨线程，所以**项目根在提交时解析成所有权数据**；
//! 后台闭包不碰任何宿主状态。

use std::path::PathBuf;

use gpui_kit::{App, Context, Entity, Subscription};

use crate::insight_view::{InsightEvent, InsightView};
use crate::model::InsightTarget;
use crate::service::InsightService;

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

/// 把面板接上宿主：面板发 `ProfileRequested`，本模块在后台取数后回填。
///
/// 宿主侧只需一行，且**不必知道事件形状**：
/// ```ignore
/// let _sub = insight::jobs::attach(&panel, cx, move || shared.project_root());
/// ```
///
/// `project_root` 是**闭包而不是值**：每次请求时才解析（项目可能已切换），
/// 而解析结果（`PathBuf`）在提交任务前就脱离宿主状态，因此可以跨线程。
pub fn attach<H: 'static>(
    view: &Entity<InsightView>,
    host_cx: &mut Context<H>,
    project_root: impl Fn() -> Option<PathBuf> + 'static,
) -> Subscription {
    let panel = view.clone();
    host_cx.subscribe(view, move |_this, _emitter, event: &InsightEvent, cx| {
        let root = project_root();
        handle_event(&panel, root, event, cx);
    })
}

/// 面板事件 → 执行动作（宿主侧唯一入口；`attach` 内部也走这里）。
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
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use gpui_kit::{AppContext as _, Context, TestAppContext};

    use super::{ProfileRequest, attach};
    use crate::insight_view::InsightView;
    use crate::model::InsightTarget;

    /// 宿主替身：真实宿主只多持一个订阅句柄（`_sub`）
    struct TestHost;

    impl TestHost {
        fn new(_: &mut Context<Self>) -> Self {
            Self
        }
    }

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

    /// 宿主只写一行（`attach`），所以这一行的契约必须在本 crate 内验住：
    /// 面板一发目标，宿主提供的项目根就被取用、后台取数真的跑起来并回填。
    #[gpui_kit::test]
    fn attach_turns_a_panel_request_into_a_fetch(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let root = Arc::new(Mutex::new(Some(PathBuf::from("/tmp/rds-测试项目"))));
        let root_for_host = root.clone();
        let calls = Arc::new(Mutex::new(0usize));
        let calls_for_host = calls.clone();

        // 宿主实体必须被持有：仅靠 `Subscription` 不足以让订阅者存活（实体一旦释放，
        // 订阅就被剪掉）——这正好也是真实宿主的形态（面板实体活到窗口关）。
        let (host, panel, _sub) = cx.update(|cx| {
            let host = cx.new(TestHost::new);
            let panel = cx.new(InsightView::new);
            let sub = host.update(cx, |_host, host_cx| {
                attach(&panel, host_cx, move || {
                    *calls_for_host.lock().unwrap() += 1;
                    root_for_host.lock().unwrap().clone()
                })
            });
            (host, panel, sub)
        });
        let _host = host;

        // 目标不存在：走的是真实失败路径（DuckDB 内存库），因此能证明订阅确实把请求
        // 转成了取数并回填——而不是只验了「不 panic」。
        cx.update(|cx| {
            panel.update(cx, |panel, cx| {
                panel.set_target(
                    InsightTarget::Column {
                        temp_table: "t_insight_attach_absent".into(),
                        column: "amount".into(),
                        data_type: "INTEGER".into(),
                    },
                    cx,
                );
            });
        });
        cx.update(|_cx| {
            assert_eq!(*calls.lock().unwrap(), 1, "订阅应被触发（项目根提供者被问了一次）");
        });
        cx.run_until_parked();

        cx.update(|cx| {
            let state = panel.read(cx).state();
            assert!(
                state.is_error(),
                "订阅应把请求转成取数并回填错误态，实际：{state:?}"
            );
        });
    }
}

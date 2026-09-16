//! 右侧边栏面板：洞察 / Mock 生成 / 历史。
//!
//! 自 `panels.rs` 纯位移迁出（无语义改动）。Mock 与洞察的视图与状态分别由
//! `mock` / `insight` crate 自持，本面板只负责构造期创建实体、注入宿主桥与登记句柄。

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use crate::view::RightPanel;
use insight::InsightView;
use mock::mock_view::MockPanel;

use super::Shared;

/// 右侧边栏面板：洞察 / Mock 生成 / 历史。
///
/// Mock 面板视图由 mock crate 自持（`mock::mock_view::MockPanel`，与 project / settings 同例）；
/// 本面板只负责：**构造期**创建实体、注入宿主桥（`components::mock_host`）、登记句柄。
/// 洞察与历史仍为占位视图（各自模块轮次接入）。
pub struct RightSidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// Mock 面板实体（随面板构造期创建，无 I/O；视图与状态均在 mock crate）
    mock_panel: Entity<MockPanel>,
    /// 洞察面板实体（随面板构造期创建，无 I/O；视图与状态均在 insight crate）
    insight_panel: Entity<InsightView>,
    /// 洞察面板的取数请求订阅（仅持有）
    _insight_sub: Subscription,
}

impl RightSidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        // 构造期创建 Mock 面板实体（不触 I/O）：导航右键定向导入结构需要在事件路径
        // 拿到句柄，懒创建（首帧 render）会让「先右键、后面板尚未渲染」的路径丢目标。
        let host = crate::components::mock_host::build_host(&shared);
        let mock_panel = cx.new(|cx| MockPanel::new(host, cx));
        *shared.mock_panel.borrow_mut() = Some(mock_panel.downgrade());

        // 洞察面板：同样构造期创建 + 登记句柄（右键入口要在事件路径拿到它，
        // 懒创建会让「先右键、后面板尚未渲染」的路径丢目标）。
        let insight_panel = cx.new(|cx| InsightView::new(cx));
        *shared.insight_panel.borrow_mut() = Some(insight_panel.downgrade());
        // 事件接缝与取数都在 insight crate（`insight::jobs`）：面板发「请加载这个目标」，
        // 后台跑完回填。宿主只提供「项目根在哪」——`Shared` 不可跨线程，
        // 闭包在**提交时**才解析，结果以所有权数据交给任务。
        let root_shared = shared.clone();
        let _insight_sub = insight::jobs::attach(&insight_panel, cx, move || {
            root_shared
                .project
                .borrow()
                .as_ref()
                .map(|session| session.root.clone())
        });
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            mock_panel,
            insight_panel,
            _insight_sub,
        }
    }

    /// M8：洞察面板——视图与状态在 insight crate（`insight::InsightView`），
    /// 本面板只做「转发渲染」 + 同步项目开关。
    fn render_insight_panel(&mut self, cx: &mut Context<Self>) -> Div {
        // 项目可能随时被打开 / 切换 / 关闭：**渲染是权威同步点**（对齐 `apply_*_mode` 的口径），
        // 面板据此显示「未打开项目」提示（Phase 2 接入规则管理后也会约束 ⚙）。
        // `set_project_open` 只在值变化时 notify，不会触发渲染循环。
        let open = self.shared.project.borrow().is_some();
        self.insight_panel
            .update(cx, |panel, cx| panel.set_project_open(open, cx));
        let panel = self.insight_panel.clone();
        div().v_flex().size_full().min_h_0().child(panel)
    }

    /// M7：Mock 生成面板——视图与状态在 mock crate（`mock::mock_view::MockPanel`），
    /// 本面板只做「转发渲染」。
    fn render_mock_panel(&mut self, cx: &mut Context<Self>) -> Div {
        let panel = self.mock_panel.clone();
        let _ = cx;
        div().v_flex().size_full().min_h_0().child(panel)
    }

    fn render_history_placeholder(&self, fg: Hsla) -> Div {
        let mut panel = div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl_2()
            .pr_2()
            .pt_2()
            .pb_2()
            .child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("历史"),
            );
        let history = crate::services::query_history::load_history();
        if history.is_empty() {
            panel = panel.child(
                div()
                    .h_6()
                    .pl_2()
                    .pr_2()
                    .text_xs()
                    .text_color(fg)
                    .child("暂无历史记录"),
            );
        } else {
            for (i, sql) in history.iter().take(20).enumerate() {
                let preview: String = if sql.chars().count() > 42 {
                    let mut s: String = sql.chars().take(42).collect();
                    s.push('…');
                    s
                } else {
                    sql.clone()
                };
                panel = panel.child(
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "right-hist-{i}"
                        ))))
                        .h_6()
                        .pl_2()
                        .pr_2()
                        .text_xs()
                        .text_color(fg)
                        .child(preview),
                );
            }
        }
        panel
    }
}

impl EventEmitter<BasePanelEvent> for RightSidebarPanel {}

impl Focusable for RightSidebarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RightSidebarPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().colors.background;
        let active = self.shared.active_right.get();
        // 各分支自己取色：Mock 分支要 `&mut cx`（创建输入框 / 取实体句柄），
        // 不能先 `let fg = cx.theme()…` 把 `cx` 借出去。
        let content: Div = match active {
            RightPanel::Insight => self.render_insight_panel(cx),
            RightPanel::Mock => self.render_mock_panel(cx),
            RightPanel::History => {
                let fg = cx.theme().colors.foreground;
                self.render_history_placeholder(fg)
            }
        };
        div().v_flex().size_full().min_h_0().bg(bg).child(content)
    }
}

impl BasePanel for RightSidebarPanel {
    fn panel_name(&self) -> &'static str {
        "right_sidebar"
    }
}

impl ComponentPanel for RightSidebarPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.shared.active_right.get().label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(self.shared.active_right.get().label())
    }
}

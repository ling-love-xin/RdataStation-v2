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
use editor::view::history::HistoryView;
use insight::InsightView;
use mock::mock_view::MockPanel;

use analytics_resource::detail_view::{DetailActions, render_detail};

use super::{QueryRequest, Shared};

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
    /// 规则管理对话框的取数 / 写库请求订阅（仅持有）
    _rules_sub: Subscription,
    /// 历史面板实体（B8）：视图与状态在 editor crate，本面板只转发渲染 + 注入重放端口
    history_panel: Entity<HistoryView>,
}

impl RightSidebarPanel {
    pub fn new(shared: Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        // 规则管理对话框（Phase 2.3）：同一套后台形态，同一个项目根提供者。
        // 取数 / 写库 / 新建 / 打开规则文件都在 insight crate 的接缝里。
        let rules_shared = shared.clone();
        let _rules_sub = insight::jobs::attach_rules(&insight_panel, cx, move || {
            rules_shared
                .project
                .borrow()
                .as_ref()
                .map(|session| session.root.clone())
        });

        // 历史面板（B8）：视图与状态在 editor crate（数据读引擎的 `history_store`）。
        // 重放 = **在中央编辑器打开**（不替用户执行：写语句可能就在历史里），
        // 而“开文档”是宿主的事 —— 这里注入重放端口：入队请求 + 唤醒宿主 render 消费。
        let history_panel = cx.new(|cx| HistoryView::new(window, cx));
        {
            let shared = shared.clone();
            history_panel.update(cx, |view, _cx| {
                view.attach_replay(std::rc::Rc::new(move |item, app| {
                    shared.request_query(QueryRequest {
                        conn_id: item.conn_id.clone(),
                        sql: item.sql.clone(),
                        // 打开即就绪，但不替用户执行（历史里什么都有，包括写语句）
                        run: false,
                    });
                    shared.notify_host(app);
                }));
            });
        }
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            mock_panel,
            insight_panel,
            _insight_sub,
            _rules_sub,
            history_panel,
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

    /// M6：存档详情——视图在 `analytics_resource` crate（`detail_view::render_detail`），
    /// 数据取左侧资产库面板的选中项；本面板只做转发渲染与空态。
    ///
    /// 选中变化能刷到这里，靠**宿主侧的观察**（`init_workspace` 观察资产库面板实体 →
    /// 唤醒本面板）——面板之间不直接互相订阅，跨 crate 的视图也无从知道对方存在。
    fn render_archive_detail(&mut self, cx: &mut Context<Self>) -> Div {
        let detail = self
            .shared
            .resources_panel
            .borrow()
            .as_ref()
            .and_then(|panel| panel.upgrade())
            .and_then(|panel| panel.read(cx).selected_detail().cloned());
        match detail {
            Some(detail) => {
                // 动作接线：宿主端口在**这一层**注入（面板不认识服务层），
                // 项目只读随 `Shared` 实时取（不缓进快照——它是窗口级状态）。
                let read_only = self.shared.project_ui.borrow().read_only;
                let actions = DetailActions {
                    host: crate::components::resource_host::build_host(&self.shared),
                    read_only,
                };
                div()
                    .v_flex()
                    .size_full()
                    .min_h_0()
                    .child(render_detail(&detail, Some(actions), cx))
            }
            None => self.render_archive_detail_empty(cx),
        }
    }

    /// 详情空态：说的是"去哪儿选"，不是一句"无数据"。
    fn render_archive_detail_empty(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(div().text_sm().text_color(muted).child("未选中存档"))
            .child(
                div()
                    .px_4()
                    .text_xs()
                    .text_color(muted)
                    .child("在左侧「资产库」选中一行，这里显示它的归档凭证"),
            )
    }

    /// B8：历史面板——视图与状态在 editor crate（`editor::view::history::HistoryView`），
    /// 本面板只做「转发渲染」。重放端口已在构造期注入。
    fn render_history_panel(&mut self, cx: &mut Context<Self>) -> Div {
        let panel = self.history_panel.clone();
        let _ = cx;
        div().v_flex().size_full().min_h_0().child(panel)
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
            RightPanel::History => self.render_history_panel(cx),
            RightPanel::Archive => self.render_archive_detail(cx),
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

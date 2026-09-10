//! 工作台 Dock 面板（Round 20）
//!
//! DockArea 布局系统接入后，原侧边栏/内容区拆为独立面板实体：
//! - `SidebarPanel`：按活动工具渲染连接列表 / 导航树 / 资源 / 设置；
//! - `EditorPanel`：中央内容区（连接详情 + 新建连接）；
//! - `Shared`：面板与工作台之间共享的状态（工具 / 选中连接 / 连接数据 / 通知文案）。
//!
//! 面板只实现 GPUI 面板协议（base `Panel` + component `Panel`），不承载业务逻辑；
//! 数据仍来自 `ConnectionItem::sample()`（占位），下一轮接 ConnectionService。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::EventEmitter;
use gpui_kit::*;

use crate::components::connection_dialog;

use crate::services::db_navigator::NavTable;
use crate::services::query_runner::QueryOutput;
use crate::view::{ConnectionItem, LeftPanel, RightPanel, SidebarMode};

/// 面板与工作台共享的状态。
#[derive(Clone)]
pub struct Shared {
    pub active_left: Rc<Cell<LeftPanel>>,
    pub active_right: Rc<Cell<RightPanel>>,
    /// 左右边栏三模式（展开 / 收起 / 完全隐藏），互不影响。
    pub left_mode: Rc<Cell<SidebarMode>>,
    pub right_mode: Rc<Cell<SidebarMode>>,
    /// 完全隐藏前的模式快照：状态栏独立开关恢复时还原，避免丢失隐藏前状态。
    pub left_mode_before_hidden: Rc<Cell<SidebarMode>>,
    pub right_mode_before_hidden: Rc<Cell<SidebarMode>>,
    /// Quick Open 弹层开关（Ctrl+P / 标题栏搜索条）。
    pub quick_open: Rc<Cell<bool>>,
    /// 设置面板开关（活动栏底部 ⚙ / OpenSettings 命令）。
    pub settings_open: Rc<Cell<bool>>,
    pub selected: Rc<Cell<Option<usize>>>,
    pub connections: Rc<RefCell<Vec<ConnectionItem>>>,
    pub notice: Rc<RefCell<Option<String>>>,
    /// Round 25：数据库导航缓存（哪个连接加载的 + 表→列树）。
    pub nav_for: Rc<RefCell<Option<String>>>,
    pub nav_tables: Rc<RefCell<Vec<NavTable>>>,
    /// Round 30：SQL 结果归属（哪个连接执行的，切换连接即失效）。
    pub sql_for: Rc<RefCell<Option<String>>>,
    /// 编辑请求（侧边栏「编辑」→ EditorPanel 渲染时消费并打开对话框）。
    pub open_edit: Rc<RefCell<Option<String>>>,
}

impl Shared {
    /// 用真实连接数据初始化共享状态（连接为空时 selected=None，进入空态 UI）。
    pub fn with_connections(connections: Vec<ConnectionItem>, notice: Option<String>) -> Self {
        Self {
            active_left: Rc::new(Cell::new(LeftPanel::Database)),
            active_right: Rc::new(Cell::new(RightPanel::Insight)),
            left_mode: Rc::new(Cell::new(SidebarMode::Expanded)),
            right_mode: Rc::new(Cell::new(SidebarMode::Collapsed)),
            left_mode_before_hidden: Rc::new(Cell::new(SidebarMode::Expanded)),
            right_mode_before_hidden: Rc::new(Cell::new(SidebarMode::Expanded)),
            quick_open: Rc::new(Cell::new(false)),
            settings_open: Rc::new(Cell::new(false)),
            selected: Rc::new(Cell::new(if connections.is_empty() {
                None
            } else {
                Some(0)
            })),
            connections: Rc::new(RefCell::new(connections)),
            notice: Rc::new(RefCell::new(notice)),
            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
            sql_for: Rc::new(RefCell::new(None)),
            open_edit: Rc::new(RefCell::new(None)),
        }
    }

    pub fn new() -> Self {
        Self::with_connections(Vec::new(), None)
    }

    /// 当前选中连接（克隆，避免长时间持有 RefCell 借用）。
    pub fn selected_connection(&self) -> Option<ConnectionItem> {
        let conns = self.connections.borrow();
        self.selected.get().and_then(|i| conns.get(i)).cloned()
    }
}

/// 侧边栏面板发出的事件（由工作台订阅）。
#[derive(Clone, Debug)]
pub enum SidebarEvent {
    /// 用户点击了连接条目。
    SelectConnection(usize),
    /// 用户点击了连接「编辑」。
    EditConnection(String),
}

/// 侧边栏面板：按活动工具渲染内容。
pub struct SidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl SidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
        }
    }

    /// 连接列表（可选中，点击切换高亮并通知工作台）。
    fn render_connection_list(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let selected = self.shared.selected.get();
        let mut list = div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .pt(px(4.))
            .pb(px(4.))
            .pl(px(4.))
            .pr(px(4.))
            .gap_1();

        if self.shared.connections.borrow().is_empty() {
            let notice = self.shared.notice.borrow().clone();
            return div()
                .v_flex()
                .w_full()
                .h_full()
                .items_center()
                .pt(px(24.))
                .pl(px(12.))
                .pr(px(12.))
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(
                    notice.unwrap_or_else(|| "暂无连接，请先在「数据源连接」中创建。".to_string()),
                );
        }

        for (idx, item) in self.shared.connections.borrow().iter().enumerate() {
            let entity = cx.entity();
            let item = item.clone();
            let is_selected = selected == Some(idx);
            let edit_conn = {
                let shared = self.shared.clone();
                let entity = entity.clone();
                let cid = item.id.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    *shared.open_edit.borrow_mut() = Some(cid.clone());
                    entity.update(app, |_, cx| {
                        cx.emit(SidebarEvent::EditConnection(cid.clone()));
                    });
                }
            };
            list = list.child(
                div()
                    .id(format!("conn-{}", idx))
                    .h_flex()
                    .items_center()
                    .w_full()
                    .h(px(28.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(is_selected, |this| this.bg(theme.colors.list_active))
                    .on_click(move |_, _, app| {
                        entity.update(app, |_, cx| cx.emit(SidebarEvent::SelectConnection(idx)));
                    })
                    .child(div().w(px(8.)).h(px(8.)).flex_none().rounded_full().bg(
                        if item.connected {
                            theme.colors.success
                        } else {
                            theme.colors.muted
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(if is_selected {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(item.name),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(item.driver),
                    )
                    .child(
                        div()
                            .id(format!("conn-edit-{}", idx))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .px_1()
                            .child("编辑")
                            .on_click(edit_conn),
                    ),
            );
        }
        list
    }

    fn render_navigation_placeholder(&self, fg: Hsla) -> Div {
        let mut tree = div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.));
        for (depth, text) in [
            (0, "项目：营销分析"),
            (1, "数据源：本地 MySQL"),
            (2, "schema：analytics"),
            (3, "表：orders"),
            (3, "表：customers"),
            (2, "schema：staging"),
            (1, "数据源：生产 PG"),
            (2, "schema：public"),
        ] {
            tree = tree.child(
                div()
                    .h_flex()
                    .items_center()
                    .h(px(24.))
                    .rounded_md()
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child(div().w(px(depth as f32 * 14.0)).flex_none())
                    .child(text),
            );
        }
        tree
    }

    fn render_resources_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("分析资源（下一轮接入）"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 数据源连接引用"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· DuckDB 分析表"),
            )
    }

    fn render_draft_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("草稿箱"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 草稿（暂未接入）"),
            )
    }

    fn render_plugin_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("插件"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 插件市场（下一轮接入）"),
            )
    }
}

impl EventEmitter<BasePanelEvent> for SidebarPanel {}

impl EventEmitter<SidebarEvent> for SidebarPanel {}

impl Focusable for SidebarPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SidebarPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().colors.background;
        let fg = cx.theme().colors.foreground;
        let active = self.shared.active_left.get();
        let content: Div = match active {
            LeftPanel::Draft => self.render_draft_placeholder(fg),
            LeftPanel::Database => {
                // 数据库导航 = 数据源连接列表（保留连接管理入口）+ 导航树占位。
                let mut panel = self.render_connection_list(cx);
                panel = panel.child(self.render_navigation_placeholder(fg));
                panel
            }
            LeftPanel::Resources => self.render_resources_placeholder(fg),
            LeftPanel::Plugin => self.render_plugin_placeholder(fg),
        };
        div().v_flex().size_full().min_h_0().bg(bg).child(content)
    }
}

impl BasePanel for SidebarPanel {
    fn panel_name(&self) -> &'static str {
        "sidebar"
    }
}

impl ComponentPanel for SidebarPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some(self.shared.active_left.get().label().into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child(self.shared.active_left.get().label())
    }
}

/// 中央内容区面板。
pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    // Phase A：新建连接对话框状态（懒创建，open_dialog 由"新建连接"按钮触发）。
    dialog: Option<connection_dialog::ConnectionDialogState>,
    // Round 26：SQL 查询区（受控输入 + 结果集）。
    sql_textarea: Option<Entity<TextareaState>>,
    query_result: Rc<RefCell<Option<QueryOutput>>>,
    // Round 29：SQL 历史（最新在前，跨会话持久化）。
    sql_history: Rc<RefCell<Vec<String>>>,
}

impl EditorPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            dialog: None,
            sql_textarea: None,
            query_result: Rc::new(RefCell::new(None)),
            sql_history: Rc::new(RefCell::new(crate::services::query_history::load_history())),
        }
    }
}

impl EventEmitter<BasePanelEvent> for EditorPanel {}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 受控输入懒创建（render 首次初始化，需要 window）；SQL 查询区保留。
        if self.sql_textarea.is_none() {
            self.sql_textarea = Some(cx.new(|cx| TextareaState::new(window, cx)));
        }
        if self.dialog.is_none() {
            self.dialog = Some(connection_dialog::ConnectionDialogState::new(window, cx));
        }

        // 消费侧边栏「编辑」请求（open_edit 置位后在此打开对话框）。
        if let Some(cid) = self.shared.open_edit.borrow_mut().take() {
            if let Some(dialog) = self.dialog.as_ref() {
                dialog.open(cx.entity(), self.shared.clone(), Some(cid), window, cx);
            }
        }

        let theme = cx.theme();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        let mut content = div()
            .v_flex()
            .size_full()
            .pt(px(16.))
            .pl(px(16.))
            .pr(px(16.))
            .gap_3()
            .bg(theme.colors.background)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.colors.foreground)
                            .child("数据源连接"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("选中连接查看真实元数据；本地加速（DuckDB Secret）将在连接建立后自动注册"),
                    ),
            );

        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
            let shared = self.shared.clone();
            let entity = entity.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();
            let status = if item.connected {
                "已连接"
            } else {
                "未连接"
            };
            let status_color = if item.connected {
                theme.colors.success
            } else {
                theme.colors.muted
            };
            let fields: Vec<(&'static str, String)> = vec![
                ("名称", item.name.clone()),
                ("驱动", item.driver.clone()),
                ("主机", item.host.clone().unwrap_or_else(|| "-".to_string())),
                (
                    "端口",
                    item.port
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "数据库",
                    item.database.clone().unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "Schema",
                    item.schema.clone().unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "DuckDB 联邦",
                    if item.use_duckdb_fed {
                        "开启（本地加速）".to_string()
                    } else {
                        "关闭".to_string()
                    },
                ),
                (
                    "描述",
                    item.description.clone().unwrap_or_else(|| "-".to_string()),
                ),
                ("创建时间", item.created_at.clone()),
                ("更新时间", item.updated_at.clone()),
            ];

            let mut rows = div().v_flex().gap_1();
            for (label, value) in fields {
                rows = rows.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w(px(96.))
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.colors.foreground)
                                .child(value),
                        ),
                );
            }
            content = content.child(
                div().v_flex().gap_2().w_full().rounded_md().pl(px(12.)).pr(px(12.)).pt(px(10.)).pb(px(10.))
                    .border_1().border_color(theme.colors.border)
                    .child(
                        div().h_flex().items_center().gap_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(item.name))
                            .child(
                                div().h_flex().items_center().gap_1()
                                    .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(status_color))
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child(status)),
                            ),
                    )
                    .child(rows)
                    .child(
                        div().h_flex().items_center().gap_2().mt(px(8.))
                            .child(
                                div()
                                    .id("delete-connection")
                                    .cursor_pointer()
                                    .on_click(move |_, _, app| {
                                        match crate::services::workspace_loader::delete_connection(&selected_id) {
                                            Ok(()) => {
                                                let (items, _) =
                                                    crate::services::workspace_loader::load_persisted_connections();
                                                *shared.connections.borrow_mut() = items;
                                                shared.selected.set(None);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("连接「{}」已删除", selected_name));
                                            }
                                            Err(e) => {
                                                *shared.notice.borrow_mut() = Some(format!("删除失败: {}", e));
                                            }
                                        }
                                        entity.update(app, |_, cx| cx.notify());
                                    })
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.colors.danger)
                                            .child("删除连接"),
                                    ),
                            ),
                    )
                    .child(
                        div().v_flex().gap_1().mt(px(8.)).pt(px(8.))
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("数据库导航（DuckDB 分析库）")),
                    ),
            );
        }

        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                if self.shared.nav_for.borrow().as_deref() != Some(item.id.as_str()) {
                    let dir = crate::services::workspace_loader::default_global_dir();
                    let path = dir.join("global.duckdb");
                    let tree = crate::services::db_navigator::load_navigator_tree(&path)
                        .unwrap_or_default();
                    *self.shared.nav_tables.borrow_mut() = tree;
                    *self.shared.nav_for.borrow_mut() = Some(item.id.clone());
                }
                let nav = self.shared.nav_tables.borrow();
                let mut nav_content = div().v_flex().gap_1().mt(px(6.));
                if nav.is_empty() {
                    nav_content = nav_content.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("分析库暂无表——运行 seed_demo 或导入数据"),
                    );
                } else {
                    for table in nav.iter() {
                        nav_content =
                            nav_content.child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(div().h_flex().items_center().gap_2().child(
                                        div().text_xs().font_weight(FontWeight::MEDIUM).child(
                                            format!("{}（{} 列）", table.name, table.columns.len()),
                                        ),
                                    ))
                                    .child(div().v_flex().gap_1().pl(px(12.)).children(
                                        table.columns.iter().map(|col| {
                                            div()
                                                .h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .w(px(130.))
                                                        .flex_none()
                                                        .text_xs()
                                                        .child(col.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_xs()
                                                        .text_color(theme.colors.muted_foreground)
                                                        .child(col.data_type.clone()),
                                                )
                                                .child(if col.is_primary_key {
                                                    div()
                                                        .text_xs()
                                                        .text_color(theme.colors.info)
                                                        .child("PK")
                                                } else {
                                                    div().text_xs().child("")
                                                })
                                        }),
                                    )),
                            );
                    }
                }
                content = content.child(nav_content);
            }
        }

        content = content.child(div().h(px(24.)));

        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                let sql_state = self.sql_textarea.clone().expect("lazy init");
                let sql_state_hist = sql_state.clone();
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let shared_view = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
                let entity_hist = entity.clone();
                let conn_id = item.id.clone();

                let mut sql_ui = div()
                    .v_flex()
                    .gap_1()
                    .mt(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("SQL 查询（DuckDB 分析库）"),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .child(Textarea::new(&sql_state).h(px(96.)))
                            .child(div().h_flex().justify_end().w_full().child(
                                Button::new("run-sql").secondary().label("执行").on_click(
                                    move |_, _, app| {
                                        let sql = sql_state.read(app).value().to_string();
                                        let dir =
                                            crate::services::workspace_loader::default_global_dir();
                                        let path = dir.join("global.duckdb");
                                        let ok = match crate::services::query_runner::execute_sql(
                                            &path, &sql,
                                        ) {
                                            Ok(out) => {
                                                let n = out.row_count;
                                                *qr_closure.borrow_mut() = Some(out);
                                                *shared.sql_for.borrow_mut() =
                                                    Some(conn_id.clone());
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", n));
                                                true
                                            }
                                            Err(e) => {
                                                *qr_closure.borrow_mut() = None;
                                                *shared.sql_for.borrow_mut() = None;
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询失败: {}", e));
                                                false
                                            }
                                        };
                                        if ok {
                                            entity.update(app, |this, cx| {
                                                if let Ok(hist) =
                                                    crate::services::query_history::append_history(
                                                        &sql,
                                                    )
                                                {
                                                    *this.sql_history.borrow_mut() = hist;
                                                }
                                                cx.notify();
                                            });
                                        } else {
                                            entity.update(app, |_, cx| cx.notify());
                                        }
                                    },
                                ),
                            )),
                    );

                // Round 29：SQL 历史——点击回填到编辑器（最新在前，截断预览）。
                {
                    let history = self.sql_history.borrow();
                    if !history.is_empty() {
                        let mut hist_ui = div().v_flex().gap_1().mt(px(4.));
                        hist_ui = hist_ui.child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.colors.muted_foreground)
                                .child("历史"),
                        );
                        for (i, sql) in history.iter().enumerate() {
                            let preview: String = if sql.chars().count() > 42 {
                                let mut s: String = sql.chars().take(42).collect();
                                s.push('…');
                                s
                            } else {
                                sql.clone()
                            };
                            let sql_clone = sql.clone();
                            let st = sql_state_hist.clone();
                            let e_hist = entity_hist.clone();
                            hist_ui = hist_ui.child(
                                div()
                                    .id(ElementId::Name(SharedString::from(format!("hist-{i}"))))
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .cursor_pointer()
                                    .child(preview)
                                    .on_click(move |_, window, app| {
                                        st.update(app, |s, cx| {
                                            s.set_value(sql_clone.clone(), window, cx)
                                        });
                                        e_hist.update(app, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        sql_ui = sql_ui.child(hist_ui);
                    }
                }

                let result_visible =
                    shared_view.sql_for.borrow().as_deref() == Some(item.id.as_str());
                let result = if result_visible {
                    query_result.borrow().clone()
                } else {
                    None
                };
                if let Some(out) = result {
                    if out.columns.is_empty() {
                        sql_ui = sql_ui.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("无结果（DDL/无返回行）"),
                        );
                    } else {
                        let mut table = div().v_flex().gap_1();
                        // 列头
                        let mut head = div().h_flex().gap_2();
                        for col in &out.columns {
                            head = head.child(
                                div()
                                    .w(px(150.))
                                    .flex_none()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(col.clone()),
                            );
                        }
                        table = table.child(head);
                        // 行
                        for row in &out.rows {
                            let mut row_div = div().h_flex().gap_2();
                            for v in row {
                                row_div = row_div.child(
                                    div()
                                        .w(px(150.))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(v.clone()),
                                );
                            }
                            table = table.child(row_div);
                        }
                        sql_ui = sql_ui.child(table);

                        // Round 27：导出 CSV——结果落盘到全局目录 results/ 并按时间戳命名。
                        let out_clone = out.clone();
                        sql_ui = sql_ui.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(format!("共 {} 行", out.rows.len())),
                                )
                                .child(
                                    Button::new("export-csv")
                                        .ghost()
                                        .label("导出 CSV")
                                        .on_click(move |_, _, app| {
                                            match crate::services::query_export::export_to_default(
                                                &out_clone,
                                            ) {
                                                Ok(path) => {
                                                    *shared_export.notice.borrow_mut() =
                                                        Some(format!("已导出: {}", path.display()));
                                                }
                                                Err(e) => {
                                                    *shared_export.notice.borrow_mut() = Some(e);
                                                }
                                            }
                                            entity_export.update(app, |_, cx| cx.notify());
                                        }),
                                ),
                        );
                    }
                }
                sql_ui = sql_ui.child(
                    div().v_flex().gap_1().child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Mock 数据（生成测试数据）"),
                    ),
                );

                content = content.child(sql_ui);
            }
        }

        content = content.child(
            Button::new("mock-generate-btn")
                .primary()
                .label("生成 Mock")
                .on_click(|_, _, _| {}),
        );

        content = content.child(
            Button::new("new-connection")
                .primary()
                .icon(IconName::Plus)
                .label("新建连接")
                .on_click({
                    let entity = entity.clone();
                    move |_, window, app| {
                        entity.update(app, |editor, cx| {
                            if let Some(dialog) = editor.dialog.as_ref() {
                                dialog.open(cx.entity(), editor.shared.clone(), None, window, cx);
                            }
                            cx.notify();
                        });
                    }
                }),
        );

        if notice.is_some() {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.info)
                    .child(notice.unwrap()),
            );
        }

        content
    }
}

impl BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "editor"
    }
}

impl ComponentPanel for EditorPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("工作台".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child("工作台")
    }
}

/// 右侧边栏面板：洞察 / Mock 生成 / 历史（占位视图，业务下一轮接入）。
pub struct RightSidebarPanel {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl RightSidebarPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_insight_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("洞察"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 库/表/列画像（占位）"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 数据质量建议（占位）"),
            )
    }

    fn render_mock_placeholder(&self, fg: Hsla) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("Mock 生成"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 表结构模板（占位）"),
            )
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .text_color(fg)
                    .child("· 生成任务（占位）"),
            )
    }

    fn render_history_placeholder(&self, fg: Hsla) -> Div {
        let mut panel = div()
            .v_flex()
            .w_full()
            .gap_1()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("历史"),
            );
        let history = crate::services::query_history::load_history();
        if history.is_empty() {
            panel = panel.child(
                div()
                    .h(px(24.))
                    .pl(px(8.))
                    .pr(px(8.))
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
                        .h(px(24.))
                        .pl(px(8.))
                        .pr(px(8.))
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
        let fg = cx.theme().colors.foreground;
        let active = self.shared.active_right.get();
        let content: Div = match active {
            RightPanel::Insight => self.render_insight_placeholder(fg),
            RightPanel::Mock => self.render_mock_placeholder(fg),
            RightPanel::History => self.render_history_placeholder(fg),
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

//! 中央内容区面板：SQL 草稿区 / 查询结果 / 属性面板宿主 / 连接对话框宿主。
//!
//! 自 `panels.rs` 纯位移迁出（无语义改动）。
//!
//! 迁移去向（`docs/architecture/editor/editor-architecture.md` §3.3）：SQL 区块与执行链
//! 最终迁入 `crates/editor`；**属性面板宿主与连接对话框宿主保留在 workbench**——
//! 后者是宿主层职责（对话框层挂在 `WorkbenchView::render`），因此本模块是长期归属，
//! 不是过渡文件。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::EventEmitter;
use gpui_kit::base::{StyledExt, h_resizable, resizable_panel};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, IconName};
use gpui_kit::*;

use crate::components::connection_dialog;

use crate::services::db_navigator::NavTable;
use database::nav_jobs;
use scratchpad::jobs as scratchpad_jobs;

use crate::ui;

use super::shared::QueryRequest;
use super::Shared;
// 跨模块：导航拖拽载荷（落点转成「打开查询」请求，见 `Shared::request_query`）/ 属性面板状态。
use database::model::PropertyRequest;
use database::nav_view::NavDragPayload;
use database::property_panel::PropertyState;
use scratchpad::{ScratchpadSearchView, scratchpad_view::render_scratchpad_search_pane};

/// 中央内容区面板。
pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    // Phase A：新建连接对话框状态（懒创建，open_dialog 由"新建连接"按钮触发）。
    // `Rc` 包装：对话框内部（暂存列表 / 管理器等）需把状态句柄 clone 进回调。
    dialog: Option<Rc<connection_dialog::ConnectionDialogState>>,
    /// 连接对话框项目下拉确认订阅（由 `ensure_dialog_subscription` 持有；句柄释放即取消）。
    _dialog_sub: Option<Subscription>,
    // Phase B：右侧停靠属性面板状态。
    property: Rc<RefCell<PropertyState>>,
    /// 属性面板宽度（rem 倍率；拖拽时更新，关闭时持久化到 settings.json）。
    property_width: Rc<Cell<f32>>,
    /// 正在轮询属性加载结果的后台任务。
    props_pump: RefCell<Option<Task<()>>>,
    /// M5 草稿箱内容搜索的「替换为」输入框（懒创建，有搜索结果时才建）。
    scratchpad_replace: Option<Entity<InputState>>,
    _scratchpad_replace_sub: Option<Subscription>,
    /// Round 25：连接详情卡「分析库元数据树」缓存（哪个连接加载的 + 表列表）。
    ///
    /// 宿主 / 项目宿主 / Mock 宿主**不直接读写这里**：它们只递增
    /// `Shared::nav_cache_epoch` 递失效信号，本面板在 `sync_shared_epochs` 消费。
    nav_for: Rc<RefCell<Option<String>>>,
    nav_tables: Rc<RefCell<Vec<NavTable>>>,
    /// 已消费的失效戳（与 `Shared` 逐帧对比，不等则丢弃自持缓存）。
    nav_cache_epoch: u64,
    /// Phase B：属性面板请求（导航双击对象 / F4 走 `EditorBridge::show_properties`）。
    property_target: Rc<RefCell<Option<PropertyRequest>>>,
    /// M5：草稿箱内容搜索结果（草稿箱经端口投递，见 `set_scratchpad_search`）。
    ///
    /// 展示归编辑区（结果渲染在中央区）；草稿箱自己只记搜索参数，不回读这份数据。
    scratchpad_search: Rc<RefCell<Option<ScratchpadSearchView>>>,
    /// 分析库元数据树是否正在后台加载（避免 render 每帧重复入队）。
    nav_tree_loading: Cell<bool>,
}

impl EditorPanel {
    /// 消费导航拖拽（表 / 视图 → 中央编辑器）。
    ///
    /// B12 后不再往“编辑区自己的 SQL 框”里写：拖拽结果与右键「查看数据」同口径，
    /// 都变成一条「打开查询」请求（`Shared::request_query`，宿主 render 消费）；
    /// 追加语义由 `Shared::append_sql` 保证，尾随空格避免连续拖多个表时粘成一个标识符。
    fn send_nav_drag(&self, qualified: &str, cx: &mut Context<Self>) {
        self.shared.request_query(QueryRequest {
            conn_id: self
                .shared
                .selected_connection()
                .map(|item| item.id.clone()),
            sql: format!("{qualified} "),
            run: false,
        });
        self.shared.notify_host(cx);
        cx.notify();
    }

    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            dialog: None,
            _dialog_sub: None,
            property: Rc::new(RefCell::new(PropertyState::default())),
            property_width: Rc::new(Cell::new(
                settings::load_settings().navigator.property_panel_width,
            )),
            props_pump: RefCell::new(None),
            scratchpad_replace: None,
            _scratchpad_replace_sub: None,
            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
            nav_cache_epoch: 0,
            property_target: Rc::new(RefCell::new(None)),
            scratchpad_search: Rc::new(RefCell::new(None)),
            nav_tree_loading: Cell::new(false),
        }
    }

    /// 消费宿主侧的失效信号（宿主只递增 `Shared` 的 epoch，不碰本面板数据）。
    fn sync_shared_epochs(&mut self) {
        let nav = self.shared.nav_cache_epoch.get();
        if nav != self.nav_cache_epoch {
            self.nav_cache_epoch = nav;
            *self.nav_for.borrow_mut() = None;
            self.nav_tables.borrow_mut().clear();
        }
    }

    /// 当前连接的分析库表名快照（宿主 Quick Open 的「表」分组用）。
    pub fn nav_table_names(&self) -> Vec<String> {
        self.nav_tables
            .borrow()
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    /// 懒创建草稿箱搜索结果的「替换为」输入框（有结果时才建）并订阅回车执行。
    fn ensure_scratchpad_replace_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scratchpad_replace.is_some() || self.scratchpad_search.borrow().is_none() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("替换为…"));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _e, ev: &InputEvent, _w, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.replace_scratchpad_all(cx),
                _ => {}
            },
        );
        self.scratchpad_replace = Some(input);
        self._scratchpad_replace_sub = Some(sub);
    }

    /// 在草稿箱内容搜索结果上「全部替换」：入队后台（替写 + 重搜一次任务完成）。
    ///
    /// 命中文件列表由任务内部从「先搜一遍」得出，与侧栏搜索开关语义完全一致；
    /// 结果（替换汇总 + 刷新后的结果视图）由侧栏轮询印回填到 `Shared`。
    fn replace_scratchpad_all(&mut self, cx: &mut Context<Self>) {
        if self.shared.project_ui.borrow().read_only {
            *self.shared.notice.borrow_mut() = Some("只读模式：不允许替换草稿内容".to_string());
            cx.notify();
            return;
        }
        let replacement = self
            .scratchpad_replace
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default();
        if replacement.trim().is_empty() {
            *self.shared.notice.borrow_mut() = Some("请先输入替换内容".to_string());
            cx.notify();
            return;
        }
        let (query, is_regex, case_sensitive) = {
            let search = self.scratchpad_search.borrow();
            match search.as_ref() {
                Some(s) => (s.query.clone(), s.is_regex, s.case_sensitive),
                None => return,
            }
        };
        let Some(root) = self.shared.project_root() else {
            *self.shared.notice.borrow_mut() = Some("未打开项目".to_string());
            cx.notify();
            return;
        };
        scratchpad_jobs::enqueue_replace_all(
            &root,
            &query,
            &replacement,
            case_sensitive,
            is_regex,
        );
        *self.shared.notice.borrow_mut() = Some("替换中…".to_string());
        // 结果由侧栏的轮询印回填：请求它确保还在跑。
        self.shared.ensure_scratchpad_pump(cx);
        self.shared.notify_host(cx);
        cx.notify();
    }

    /// 连接对话框状态（懒创建：首次 `request_*` 时建立；宿主 / 测试只读访问）。
    pub fn dialog_state(&self) -> Option<Rc<connection_dialog::ConnectionDialogState>> {
        self.dialog.clone()
    }

    /// 打开「新建数据源连接」对话框（编辑区按钮入口）。
    ///
    /// 打开后必须通知宿主重绘：对话框层挂在 `WorkbenchView::render` 上，
    /// 而 `Root` 的 notify 不会让子视图重建元素树。
    pub fn request_new_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
            // 新状态 → 旧订阅（若有）失去意义，重建。
            self._dialog_sub = None;
        }
        let dialog = self.dialog.clone().expect("dialog initialized");
        // 重新打开：重置元数据标记，强制下一次渲染重新拉取引用 / 类型 / 驱动目录。
        dialog.meta_refreshed.set(false);
        self.ensure_dialog_subscription(&dialog, window, cx);
        dialog.open(cx.entity(), self.shared.clone(), None, window, cx);
        self.shared.notify_host(cx);
    }

    /// 打开「编辑数据源连接」对话框（侧边栏「编辑」入口）。
    pub fn request_edit_connection(
        &mut self,
        conn_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
            self._dialog_sub = None;
        }
        let dialog = self.dialog.clone().expect("dialog initialized");
        // 重新打开：重置元数据标记，强制下一次渲染重新拉取（引用 / 类型 / 驱动目录）。
        dialog.meta_refreshed.set(false);
        self.ensure_dialog_subscription(&dialog, window, cx);
        dialog.open(cx.entity(), self.shared.clone(), Some(conn_id), window, cx);
        self.shared.notify_host(cx);
    }

    /// 确保项目下拉确认订阅已建立（只建一次；订阅句柄由本面板持有）。
    ///
    /// 订阅建立放在面板入口而非 `ConnectionDialogState::open`：`open` 在面板 `update`
    /// 上下文内被调用，在那里再 `update` 面板会触发重入 panic。
    fn ensure_dialog_subscription(
        &mut self,
        dialog: &Rc<connection_dialog::ConnectionDialogState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._dialog_sub.is_none() {
            self._dialog_sub = Some(dialog.subscribe_project_confirm(&self.shared, window, cx));
        }
    }

    /// 后台加载分析库元数据树（连接详情卡用）。
    ///
    /// `load_navigator_tree` 会打开 DuckDB 文件（同步读盘），原先在 render 内直接调用——
    /// 选中联邦连接的那一帧会阻塞；现改为后台执行 + 回填，render 只读缓存。
    fn ensure_analysis_tree(&self, conn_id: String, cx: &mut Context<Self>) {
        if self.nav_tree_loading.get() {
            return;
        }
        self.nav_tree_loading.set(true);
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let path = crate::services::workspace_loader::global_analysis_db_path();
        cx.spawn(async move |_this, cx| {
            let tree = executor
                .spawn(async move {
                    crate::services::db_navigator::load_navigator_tree(&path).unwrap_or_default()
                })
                .await;
            let _ = weak.update(cx, |panel, cx| {
                *panel.nav_tables.borrow_mut() = tree;
                *panel.nav_for.borrow_mut() = Some(conn_id);
                panel.nav_tree_loading.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    /// 启动属性结果轮询（已有存活任务时不重复启动）。
    fn ensure_props_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.props_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let results = nav_jobs::drain_props_results();
                let had = !results.is_empty();
                if had
                    && weak
                        .update(cx, |this, cx| this.apply_props_results(results, cx))
                        .is_err()
                {
                    return;
                }
                if !nav_jobs::has_pending_props() {
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !nav_jobs::has_pending_props() {
                        break;
                    }
                }
            }
        });
        *self.props_pump.borrow_mut() = Some(task);
    }

    /// 投递草稿箱内容搜索结果（`EditorBridge::show_search_results`；`None` = 清空）。
    pub(super) fn set_scratchpad_search(
        &mut self,
        view: Option<ScratchpadSearchView>,
        cx: &mut Context<Self>,
    ) {
        *self.scratchpad_search.borrow_mut() = view;
        cx.notify();
    }

    /// 请求属性面板数据（`EditorBridge::show_properties`）：**入队与轮询都在事件路径**，
    /// 渲染只读 `PropertyState`（不再在 render 内入队）。
    pub fn request_properties(&self, request: PropertyRequest, cx: &mut Context<Self>) {
        let key = format!(
            "{}|{:?}|{}|{:?}",
            request.property.conn_id, request.property.kind, request.property.name, request.property.parent
        );
        {
            let mut st = self.property.borrow_mut();
            st.loaded_for = Some(key.clone());
            st.props = None;
            st.error = None;
            st.loading = true;
        }
        // 驱动目录已缓存（随组织数据一次加载）：把「数据库类型 + 驱动友好名」一并传给属性面板。
        let (driver_display, db_type) = {
            let catalog = self.shared.driver_catalog.borrow();
            match catalog.get(&request.driver) {
                Some(meta) => (
                    format!("{} · {}", meta.name, request.driver),
                    Some(meta.type_id.clone()),
                ),
                None => (request.driver.clone(), None),
            }
        };
        nav_jobs::enqueue_properties(
            &key,
            request.property.clone(),
            &request.conn_label,
            &driver_display,
            db_type.as_deref(),
        );
        self.ensure_props_pump(cx);
        *self.property_target.borrow_mut() = Some(request);
        cx.notify();
    }

    /// 回填属性加载结果（主线程；key 不匹配的过期结果丢弃）。
    fn apply_props_results(&mut self, results: Vec<nav_jobs::PropsResult>, cx: &mut Context<Self>) {
        for r in results {
            let mut st = self.property.borrow_mut();
            if st.loaded_for.as_deref() != Some(r.key.as_str()) {
                continue;
            }
            st.loading = false;
            match r.result {
                Ok(p) => {
                    st.props = Some(p);
                    st.error = None;
                }
                Err(e) => {
                    st.props = None;
                    st.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    /// 右侧停靠属性面板（DBeaver 式：属性网格 + 子实体表格）。
    fn render_property_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(target) = self.property_target.borrow().clone() else {
            return div();
        };
        // 数据加载与 loading 置位在事件路径（`request_properties`）：渲染只读 `PropertyState`。

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.background;
        let tabbar = cx.theme().colors.tab_bar;
        let danger = cx.theme().colors.danger;

        let st = self.property.borrow();
        let title = st
            .props
            .as_ref()
            .map(|p| p.title.clone())
            .unwrap_or_else(|| target.property.name.clone());
        let object_type = st
            .props
            .as_ref()
            .map(|p| p.object_type.clone())
            .unwrap_or_default();

        let mut body = div().v_flex().w_full().gap_1();
        if st.loading {
            body = body.child(div().text_xs().text_color(muted).child("加载中…"));
        }
        if let Some(err) = &st.error {
            body = body.child(div().text_xs().text_color(danger).child(err.clone()));
        }
        if let Some(props) = &st.props {
            for row in &props.properties {
                body = body.child(
                    div()
                        .h_flex()
                        .items_start()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .w(rems(4.5))
                                .flex_none()
                                .text_color(muted)
                                .child(row.label.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(fg)
                                .child(row.value.clone()),
                        ),
                );
            }
            for section in &props.sections {
                body = body.child(
                    div()
                        .pt_2()
                        .pb_1()
                        .w_full()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(section.label.clone()),
                );
                body = body.child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .text_color(muted)
                        .children(
                            section
                                .table
                                .headers
                                .iter()
                                .map(|h| div().flex_1().min_w_0().child(h.clone())),
                        ),
                );
                for data_row in &section.table.rows {
                    body = body.child(
                        div()
                            .h_flex()
                            .w_full()
                            .gap_2()
                            .text_xs()
                            .text_color(fg)
                            .children(
                                data_row
                                    .iter()
                                    .map(|c| div().flex_1().min_w_0().child(c.clone())),
                            ),
                    );
                }
            }
        }

        div()
            .v_flex()
            .w_full()
            .h_full()
            .flex_none()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .h(rems(2.125))
                    .px_3()
                    .border_1()
                    .border_color(border)
                    .bg(tabbar)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child(title),
                    )
                    .child(div().text_xs().text_color(muted).child(object_type))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("property-close")
                            .px_1()
                            .text_xs()
                            .text_color(muted)
                            .cursor_pointer()
                            .child("关闭")
                            .on_click({
                                let entity = cx.entity();
                                let width = self.property_width.clone();
                                let target = self.property_target.clone();
                                move |_, _, app: &mut App| {
                                    *target.borrow_mut() = None;
                                    // 关闭时持久化拖拽宽度（拖拽过程不写盘）。
                                    settings::SettingsService::set_property_panel_width(
                                        width.get(),
                                        app,
                                    );
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .id("property-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px_3()
                    .py_2()
                    .overflow_y_scroll()
                    .child(body),
            )
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
        // 宿主失效信号（切换连接 / 项目 / Mock 落库）在渲染入口消费：只读 Shared，不写别家数据。
        self.sync_shared_epochs();
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
        }

        // 「编辑连接」/「新建连接」改由 `EditorBridge` 在事件路径直接调用（见 `shared.rs`）。
        // 导航的 SQL（查看数据 / 生成 SQL / 拖拽）在 B12 后不再落到本面板：它们走
        // `Shared::request_query`，由宿主开一份编辑器文档（真正的 SQL 编辑器在那里）。
        // M5 草稿箱搜索结果：替换输入框懒创建（须在 `let theme = cx.theme()` 之前）。
        self.ensure_scratchpad_replace_input(window, cx);

        // Round 25：分析库元数据树按需后台加载（同样必须在 `let theme = cx.theme()` 之前：
        // theme 借了 `cx`，之后再要 `&mut Context` 会 E0502）。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed && self.nav_for.borrow().as_deref() != Some(item.id.as_str()) {
                self.ensure_analysis_tree(item.id.clone(), cx);
            }
        }

        let theme = cx.theme();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        let mut content = div()
            .v_flex()
            .size_full()
            .pt_4()
            .pl_4()
            .pr_4()
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
                                .w_24()
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
                div().v_flex().gap_2().w_full().rounded_md().pl_3().pr_3().pt_2p5().pb_2p5()
                    .border_1().border_color(theme.colors.border)
                    .child(
                        div().h_flex().items_center().gap_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(item.name))
                            .child(
                                div().h_flex().items_center().gap_1()
                                    .child(div().w_2().h_2().rounded_full().bg(status_color))
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child(status)),
                            ),
                    )
                    .child(rows)
                    .child(
                        div().h_flex().items_center().gap_2().mt_2()
                            .child(
                                div()
                                    .id("delete-connection")
                                    .cursor_pointer()
                                    .on_click(move |_, _, app| {
                                        // 项目作用域删除需要当前项目根（未打开项目时传 None）。
                                        let project_root = shared
                                            .project
                                            .borrow()
                                            .as_ref()
                                            .map(|p| p.root.to_string_lossy().to_string());
                                        match crate::services::workspace_loader::delete_connection(
                                            &selected_id,
                                            project_root.as_deref(),
                                        ) {
                                            Ok(()) => {
                                                let (items, _) =
                                                    crate::services::workspace_loader::load_connections_for_scope(
                                                        project_root
                                                            .as_deref()
                                                            .map(std::path::Path::new),
                                                    );
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
                        div().v_flex().gap_1().mt_2().pt_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child("数据库导航（DuckDB 分析库）")),
                    ),
            );
        }

        // Round 25：数据库导航区——分析库元数据树由上文按需后台加载，这里只读缓存。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                let nav = self.nav_tables.borrow();
                let mut nav_content = div().v_flex().gap_1().mt_1p5();
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
                                    .child(div().v_flex().gap_1().pl_3().children(
                                        table.columns.iter().map(|col| {
                                            div()
                                                .h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .w(rems(8.125))
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

        content = content.child(div().h_6());

        // M7：Mock 生成入口的标题（原先挂在 SQL 区里；B12 删掉 SQL 区后独立成块）。
        content = content.child(
            div().v_flex().gap_1().child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Mock 数据（生成测试数据）"),
            ),
        );

        // M5 草稿箱内容搜索结果（原型 §4.3/§4.5：结果与替换栏落中央编辑区）。
        {
            let search = self.scratchpad_search.borrow();
            if let Some(search) = search.as_ref() {
                let shared = self.shared.clone();
                let search_state = self.scratchpad_search.clone();
                let replace_input = self.scratchpad_replace.clone();
                let replace_filled = replace_input
                    .as_ref()
                    .map(|i| !i.read(cx).value().trim().is_empty())
                    .unwrap_or(false);
                let clear = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        *search_state.borrow_mut() = None;
                        shared.notify_host(app);
                        entity.update(app, |_, cx| cx.notify());
                    }
                };
                let replace_all = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| this.replace_scratchpad_all(cx));
                    }
                };
                // 借用顺序：`theme` 已从 cx 借出，这里不再调用 `entity.update(cx, …)`。
                let match_bg = settings::product_tokens::get(cx).search_match_background(theme);
                content = content.child(render_scratchpad_search_pane(
                    search,
                    theme,
                    match_bg,
                    replace_input.as_ref(),
                    replace_filled,
                    clear,
                    replace_all,
                ));
            }
        }

        content = content.child(
            Button::new("mock-generate-btn")
                .primary()
                .label("生成 Mock")
                .on_click({
                    let shared = self.shared.clone();
                    move |_, _, app| {
                        // 单一入口：目标表 / 行数 / 列配置都在右 Dock 的 Mock 面板里确定。
                        shared.open_mock_panel(None, app);
                    }
                }),
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
                            editor.request_new_connection(window, cx);
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

        // 兜底落点：拖到本面板任意位置都变成「把限定名送进编辑器」的请求。
        // 真正的 SQL 编辑器在中央 Dock（`crates/editor`），落点只是把意图转成请求。
        content = content.on_drop({
            let entity = entity.clone();
            move |payload: &NavDragPayload, _window, app| {
                let qualified = payload.qualified.clone();
                entity.update(app, |this, cx| this.send_nav_drag(&qualified, cx));
            }
        });

        // `h_flex()` 默认交叉轴居中：不写 items_stretch，内容列会按内容高度被竖直居中，
        // 高于面板的部分上下同时被裁。
        let mut root = div().h_flex().items_stretch().size_full();
        if self.property_target.borrow().is_some() {
            // 属性面板停靠右侧，可拖拽调宽（宽度记忆到 settings.json）。
            let font_size = theme.font_size;
            let width = font_size * self.property_width.get();
            let property = self.render_property_panel(cx);
            let width_cell = self.property_width.clone();
            root = root.child(
                h_resizable("editor-property-split")
                    .child(resizable_panel().child(div().flex_1().min_w_0().child(content)))
                    .child(
                        resizable_panel()
                            .size(width)
                            .size_range(
                                font_size * ui::PROPERTY_PANEL_MIN_WIDTH
                                    ..font_size * ui::PROPERTY_PANEL_MAX_WIDTH,
                            )
                            .flex_none()
                            .child(property),
                    )
                    .on_resize(move |state, _window, app| {
                        let sizes = state.read(app).sizes();
                        if let Some(last) = sizes.last() {
                            width_cell.set(last.as_f32() / app.theme().font_size.as_f32());
                        }
                    }),
            );
        } else {
            root = root.child(div().flex_1().min_w_0().child(content));
        }
        root
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

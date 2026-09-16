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
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, IconName};
use gpui_kit::*;

use crate::components::connection_dialog;

use crate::services::db_navigator::NavTable;
use crate::services::nav_jobs;
use crate::services::scratchpad_jobs;

use crate::services::query_runner::QueryOutput;
use crate::ui;

use super::Shared;
// 跨模块：导航拖拽载荷 / 属性面板状态 / 草稿追加（编辑区落点）。
use super::nav::{NavDragPayload, PropertyRequest, PropertyState, nav_draft_append};
use super::scratchpad_panel::{ScratchpadSearchView, render_scratchpad_search_pane};

/// 导航拖拽落点语义（`EditorPanel::apply_nav_drag`）。
///
/// SQL 区当前仅在 `use_duckdb_fed` 连接下可见（架构 §11#16），落到编辑区
/// 其它位置时只能“盲写”草稿，因此两种落点各有一条路径并附带通知。
#[derive(Clone, Copy, PartialEq, Eq)]
enum NavDropMode {
    /// 落在 SQL 区：插入光标处（未聚焦时退化为追加）。
    AtCursor,
    /// 落在编辑区其它位置：追加到草稿末尾。
    Append,
}

/// 中央内容区面板。
pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    // Phase A：新建连接对话框状态（懒创建，open_dialog 由"新建连接"按钮触发）。
    // `Rc` 包装：对话框内部（暂存列表 / 管理器等）需把状态句柄 clone 进回调。
    dialog: Option<Rc<connection_dialog::ConnectionDialogState>>,
    // Round 26：SQL 查询区（受控输入 + 结果集）。
    sql_textarea: Option<Entity<TextareaState>>,
    query_result: Rc<RefCell<Option<QueryOutput>>>,
    // Round 29：SQL 历史（最新在前，跨会话持久化）。
    sql_history: Rc<RefCell<Vec<String>>>,
    /// 最近一次成功执行的 SQL（用于判断编辑区是否有未保存草稿）。
    last_executed: Rc<RefCell<String>>,
    /// SQL 输入订阅句柄（Change 事件 → 脏状态）。
    _sql_sub: Option<Subscription>,
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
    /// Round 30：当前 SQL 结果归属的连接 ID（失效信号走 `Shared::result_epoch`）。
    sql_for: Rc<RefCell<Option<String>>>,
    /// 已消费的两个失效戳（与 `Shared` 逐帧对比，不等则丢弃自持缓存）。
    nav_cache_epoch: u64,
    result_epoch: u64,
    /// Phase B：属性面板请求（导航双击对象 / F4 走 `EditorBridge::show_properties`）。
    property_target: Rc<RefCell<Option<PropertyRequest>>>,
    /// 待注入草稿的 SQL（`EditorBridge::insert_sql` 写入；下一次渲染消费）。
    ///
    /// `TextareaState::set_value` 需要 `Window`，而「生成 SQL」的排空路径拿不到窗口，
    /// 因此入自有缓冲——写的是编辑区私有状态，不涉及跨模块字段。
    pending_sql: RefCell<Option<String>>,
    /// M5：草稿箱内容搜索结果（草稿箱经端口投递，见 `set_scratchpad_search`）。
    ///
    /// 展示归编辑区（结果渲染在中央区）；草稿箱自己只记搜索参数，不回读这份数据。
    scratchpad_search: Rc<RefCell<Option<ScratchpadSearchView>>>,
    /// 分析库元数据树是否正在后台加载（避免 render 每帧重复入队）。
    nav_tree_loading: Cell<bool>,
}

impl EditorPanel {
    /// 消费导航拖拽（表 / 视图 → SQL 草稿）。
    ///
    /// 插入语义：
    /// - SQL 区可见**且已聚焦**时插到光标处（尊重选区，由 `TextareaState::insert` 完成）；
    /// - 其余情况一律追加到末尾——未聚焦的输入光标停在 0，直接插入会把表名顶到
    ///   用户语句前面；与右键「查看数据」同策略走 `set_value`（不发事件，手动同步
    ///   `editor_dirty` / `editor_sql`，避免在事件路径上重入 `InputEvent`）。
    ///
    /// 两种落点都聚焦 SQL 区，方便用户接着写。SQL 区不可见时给一条通知：
    /// 拖拽已生效，只是没有可见的 SQL 区。
    fn apply_nav_drag(
        &mut self,
        qualified: &str,
        mode: NavDropMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ta) = self.sql_textarea.clone() else {
            return;
        };
        let sql_visible = self
            .shared
            .selected_connection()
            .is_some_and(|c| c.use_duckdb_fed);
        let focused = ta.read(cx).focus_handle(cx).is_focused(window);
        // 尾随空格：连续拖多个表时不会粘成一个标识符。
        let snippet = format!("{qualified} ");

        if sql_visible && mode == NavDropMode::AtCursor && focused {
            ta.update(cx, |s, cx| s.insert(snippet.clone(), window, cx));
        } else {
            ta.update(cx, |s, cx| {
                let combined = nav_draft_append(&s.value(), &snippet);
                s.set_value(combined, window, cx);
            });
        }
        ta.update(cx, |s, cx| s.focus(window, cx));

        let value = ta.read(cx).value().to_string();
        self.shared.editor_dirty.set(!value.trim().is_empty());
        *self.shared.editor_sql.borrow_mut() = value;
        if !sql_visible {
            *self.shared.notice.borrow_mut() = Some(format!(
                "已把 {qualified} 送入 SQL 草稿（当前连接未启用 DuckDB 联邦，SQL 区未显示）"
            ));
        }
        cx.notify();
    }

    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            dialog: None,
            sql_textarea: None,
            query_result: Rc::new(RefCell::new(None)),
            sql_history: Rc::new(RefCell::new(crate::services::query_history::load_history())),
            last_executed: Rc::new(RefCell::new(String::new())),
            _sql_sub: None,
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
            sql_for: Rc::new(RefCell::new(None)),
            nav_cache_epoch: 0,
            result_epoch: 0,
            property_target: Rc::new(RefCell::new(None)),
            pending_sql: RefCell::new(None),
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
        let res = self.shared.result_epoch.get();
        if res != self.result_epoch {
            self.result_epoch = res;
            *self.sql_for.borrow_mut() = None;
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

    /// 清空 SQL 编辑区（保存 / 放弃未保存草稿后由宿主命令调用；在事件上下文中执行，非 render）。
    pub fn clear_sql(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ta) = &self.sql_textarea {
            ta.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.last_executed.borrow_mut().clear();
        self.shared.editor_dirty.set(false);
        self.shared.editor_sql.borrow_mut().clear();
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

    /// 把导航注入的 SQL 排进草稿缓冲（`EditorBridge::insert_sql`）。
    ///
    /// 渲染期统一消费（见 `render`）：`set_value` 需要 `Window`，事件路径取不到。
    pub fn insert_sql(&self, sql: String) {
        *self.pending_sql.borrow_mut() = Some(sql);
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
        // 受控输入懒创建（render 首次初始化，需要 window）；SQL 查询区保留。
        if self.sql_textarea.is_none() {
            let ta = cx.new(|cx| TextareaState::new(window, cx));
            // M1：脏状态由输入事件驱动（render 只读状态，不做写副作用——遵循编码指南）。
            let shared = self.shared.clone();
            let last_executed = self.last_executed.clone();
            let ta_for_read = ta.clone();
            let sub = cx.subscribe_in(
                &ta,
                window,
                move |_this, _ta, ev: &InputEvent, _window, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    let val = ta_for_read.read(cx).value().to_string();
                    let dirty = !val.trim().is_empty() && val != *last_executed.borrow();
                    shared.editor_dirty.set(dirty);
                    *shared.editor_sql.borrow_mut() = val;
                },
            );
            self._sql_sub = Some(sub);
            self.sql_textarea = Some(ta);
        }
        if self.dialog.is_none() {
            self.dialog = Some(Rc::new(connection_dialog::ConnectionDialogState::new(
                window, cx,
            )));
        }

        // 「编辑连接」/「新建连接」改由 `EditorBridge` 在事件路径直接调用（见 `shared.rs`）。
        // 消费端口注入的 SQL：只读编辑区私有缓冲（`set_value` 需要 window，故延到渲染期）。
        let pending_sql = self.pending_sql.borrow_mut().take();
        if let Some(sql) = pending_sql {
            if let Some(ta) = &self.sql_textarea {
                let current = ta.read(cx).value().to_string();
                let combined = if current.trim().is_empty() {
                    sql
                } else {
                    format!("{}\n{}", current.trim_end(), sql)
                };
                ta.update(cx, |s, cx| s.set_value(combined.clone(), window, cx));
                self.shared.editor_dirty.set(true);
                *self.shared.editor_sql.borrow_mut() = combined;
            }
        }

        // 消费导航右键「新建查询 / 查看数据」注入的 SQL：追加到当前草稿后。
        // 用 `set_value`（不发事件）并手动同步 dirty / editor_sql，与 `clear_sql` 同策略，
        // 避免在 render 内同步派发 InputEvent 引发重入。
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

        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                let sql_state = self.sql_textarea.clone().expect("lazy init");
                let sql_state_hist = sql_state.clone();
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let sql_for_view = self.sql_for.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
                let entity_hist = entity.clone();
                let conn_id = item.id.clone();
                let last_exec = self.last_executed.clone();
                let sql_for = self.sql_for.clone();

                let mut sql_ui = div()
                    .v_flex()
                    .gap_1()
                    .mt_2()
                    .pt_2()
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
                            // 拖拽落点（表 / 视图 → 限定名）：包一层带描边的容器，
                            // 拖入时高亮，作为「插到光标处」的可见 affordance。
                            .child(
                                div()
                                    .w_full()
                                    .p_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .drag_over::<NavDragPayload>(|style, _, _, cx| {
                                        style
                                            .bg(cx.theme().colors.list_hover)
                                            .border_color(cx.theme().colors.primary)
                                    })
                                    .on_drop({
                                        let entity = entity.clone();
                                        move |payload: &NavDragPayload, window, app| {
                                            let qualified = payload.qualified.clone();
                                            entity.update(app, |this, cx| {
                                                this.apply_nav_drag(
                                                    &qualified,
                                                    NavDropMode::AtCursor,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    })
                                    .child(Textarea::new(&sql_state).h_24()),
                            )
                            .child(div().h_flex().justify_end().w_full().child(
                                Button::new("run-sql").secondary().label("执行").on_click(
                                    move |_, _, app| {
                                        // M1：只读打开时禁止执行（禁止写分析库）。
                                        if shared.project_ui.borrow().read_only {
                                            *shared.notice.borrow_mut() =
                                                Some("只读模式：不允许执行 SQL".to_string());
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let sql = sql_state.read(app).value().to_string();
                                        let path =
                                            crate::services::workspace_loader::global_analysis_db_path();
                                        let ok = match crate::services::query_runner::execute_sql(
                                            &path, &sql,
                                        ) {
                                            Ok(out) => {
                                                let n = out.row_count;
                                                *qr_closure.borrow_mut() = Some(out);
                                                *last_exec.borrow_mut() = sql.clone();
                                                *sql_for.borrow_mut() =
                                                    Some(conn_id.clone());
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", n));
                                                true
                                            }
                                            Err(e) => {
                                                *qr_closure.borrow_mut() = None;
                                                *sql_for.borrow_mut() = None;
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
                        let mut hist_ui = div().v_flex().gap_1().mt_1();
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
                    sql_for_view.borrow().as_deref() == Some(item.id.as_str());
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
                                    .w(rems(9.375))
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
                                        .w(rems(9.375))
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

        // 兜底落点：拖到编辑区任意位置都送进 SQL 草稿（追加），避免「拖了但没落点」。
        // SQL 区有自己的落点（插到光标处），其 `on_drop` 会 `stop_propagation`，不会重复触发。
        content = content.on_drop({
            let entity = entity.clone();
            move |payload: &NavDragPayload, window, app| {
                let qualified = payload.qualified.clone();
                entity.update(app, |this, cx| {
                    this.apply_nav_drag(&qualified, NavDropMode::Append, window, cx);
                });
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

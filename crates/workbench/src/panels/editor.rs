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
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, IconName, Size, Sizable as _, Theme};
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
use database::property_panel::PropertyRegistryImpl;
use database::table_data::{DEFAULT_PAGE_LIMIT, TableDataRequest};
use property::{ObjectProperties, PageId, PropertyPage, PropertyRegistry as _, PropertyRow, PropertyValueKind, SupportState};
use property::PropertyState;
use scratchpad::{
    ScratchpadDiffView, ScratchpadSearchView,
    scratchpad_view::{render_scratchpad_diff_pane, render_scratchpad_search_pane},
};

/// 属性行内编辑态（DBeaver 属性编辑：点击可编辑值 → 行内编辑控件）。
#[derive(Clone)]
struct PropertyEdit {
    row_id: String,
    input: Entity<InputState>,
}

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
    /// 属性行内编辑态（DBeaver 属性编辑；None = 未在编辑）。
    property_edit: Rc<RefCell<Option<PropertyEdit>>>,
    /// M5：草稿箱内容搜索结果（草稿箱经端口投递，见 `set_scratchpad_search`）。
    ///
    /// 展示归编辑区（结果渲染在中央区）；草稿箱自己只记搜索参数，不回读这份数据。
    scratchpad_search: Rc<RefCell<Option<ScratchpadSearchView>>>,
    /// M5：草稿箱冲突 Diff（原型 §4.5「查看差异」→ 落中央编辑区；消解动作在左侧冲突条上）。
    scratchpad_diff: Rc<RefCell<Option<ScratchpadDiffView>>>,
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
            // 属性面板宽度走 `SettingsService`（唯一读路径，K1）；`app` 已在开窗前 init。
            property_width: Rc::new(Cell::new(settings::SettingsService::property_panel_width(
                cx,
            ))),
            props_pump: RefCell::new(None),
            scratchpad_replace: None,
            _scratchpad_replace_sub: None,
            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
            nav_cache_epoch: 0,
            property_target: Rc::new(RefCell::new(None)),
            property_edit: Rc::new(RefCell::new(None)),
            scratchpad_search: Rc::new(RefCell::new(None)),
            scratchpad_diff: Rc::new(RefCell::new(None)),
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

    /// 投递草稿箱冲突 Diff（`EditorBridge::show_diff`；`None` = 关闭）。
    pub(super) fn set_scratchpad_diff(
        &mut self,
        view: Option<ScratchpadDiffView>,
        cx: &mut Context<Self>,
    ) {
        *self.scratchpad_diff.borrow_mut() = view;
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
                    // 类型行显示**目录名**（`data_source_types.name`）优先，取不到才回退类型 id
                    Some(
                        meta.type_name
                            .clone()
                            .filter(|n| !n.trim().is_empty())
                            .unwrap_or_else(|| meta.type_id.clone()),
                    ),
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
            request.data.clone(),
        );
        self.ensure_props_pump(cx);
        *self.property_target.borrow_mut() = Some(request);
        cx.notify();
    }

    /// 以新的数据页参数重新请求当前对象（「数据」页签翻页 / 排序用）。
    ///
    /// 属性与数据同一次往返：属性走 L2 缓存，数据按 `offset` / `order_by` 重查。
    fn reload_properties_data(&self, data: TableDataRequest, cx: &mut Context<Self>) {
        let Some(mut req) = self.property_target.borrow().clone() else {
            return;
        };
        req.data = Some(data);
        self.request_properties(req, cx);
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

    /// 进入属性行内编辑（DBeaver 属性编辑：点击可编辑值 → 行内输入控件）。
    fn begin_property_edit(
        &self,
        row_id: String,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut st = InputState::new(window, cx);
            st.set_value(initial, window, cx);
            st
        });
        *self.property_edit.borrow_mut() = Some(PropertyEdit { row_id, input });
        cx.notify();
    }

    /// 取消编辑（丢弃草稿）。
    fn cancel_property_edit(&self, cx: &mut Context<Self>) {
        *self.property_edit.borrow_mut() = None;
        cx.notify();
    }

    /// 提交编辑：读输入 → 后台执行 ALTER（`apply_property_edit`）→ 成功重载面板、
    /// 失败在面板内红字提示。
    fn submit_property_edit(&self, cx: &mut Context<Self>) {
        let Some(edit) = self.property_edit.borrow().clone() else {
            return;
        };
        let Some(target) = self.property_target.borrow().clone() else {
            return;
        };
        let value = edit.input.read(cx).value().to_string();
        *self.property_edit.borrow_mut() = None;
        self.run_property_edit(target, edit.row_id, value, cx);
    }

    /// 布尔属性直接切换提交（可空等；DBeaver「编辑即生效」）。
    fn toggle_property_bool(&self, row_id: String, new_value: bool, cx: &mut Context<Self>) {
        let Some(target) = self.property_target.borrow().clone() else {
            return;
        };
        let value = if new_value { "true" } else { "false" }.to_string();
        self.run_property_edit(target, row_id, value, cx);
    }

    /// 后台执行一次属性编辑（公共路径：文本提交与布尔切换共用）。
    fn run_property_edit(
        &self,
        target: PropertyRequest,
        row_id: String,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let metadata =
            database::metadata_service::MetadataService::new(engine::get_connection_manager().clone());
        // async move 按值捕获：把传给后台的字段先拆出来，`target` 保持完整供回填闭包用。
        let prop = target.property.clone();
        let driver = target.driver.clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move {
                    database::property_panel::apply_property_edit(
                        &metadata,
                        &prop,
                        &driver,
                        &row_id,
                        &value,
                    )
                    .await
                })
                .await;
            let _ = weak.update(cx, |panel, cx| {
                match result {
                    Ok(_sql) => {
                        // 成功：重载面板（列结构可能变化——重命名 / 可空等）。
                        panel.request_properties(target, cx);
                        // 树节点刷新依赖导航刷新事件（列名变更场景），下一轮接。
                    }
                    Err(e) => {
                        let mut st = panel.property.borrow_mut();
                        st.error = Some(format!("编辑失败：{e}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 右侧停靠属性面板（DBeaver 式：tab 页签 + 描述驱动渲染）。
    ///
    /// 页集合来自 [`PropertyRegistry`]（`PropertyRegistryImpl`），渲染只遍历页描述，
    /// 不按 `PropertyKind` 写分支；暂不支持页（权限 / 依赖）先绘制、置灰标注
    /// （`docs/architecture/property/property-module-spec.md` §8）。
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

        // 页集合（描述驱动）：只依赖对象类型，顺序即 tab 顺序。
        let pages = PropertyRegistryImpl.pages_for(target.property.kind);
        let active = st.active_page;
        let active_index = pages.iter().position(|p| p.id == active).unwrap_or(0);

        let theme = cx.theme();
        let mut body = div().v_flex().w_full().gap_1();
        if st.loading {
            body = body.child(div().text_xs().text_color(muted).child("加载中…"));
        }
        if let Some(err) = &st.error {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(err.clone()),
            );
        }
        if let Some(props) = &st.props {
            body = body.child(self.render_property_page_body(
                cx.entity(),
                active,
                &pages,
                props,
                theme,
            ));
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
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        TabBar::new("property-tabs")
                            .underline()
                            .with_size(Size::Small)
                            .selected_index(active_index)
                            .children(
                                pages.iter().map(|p| Tab::new().label(p.title.clone())),
                            )
                            .on_click({
                                let entity = cx.entity();
                                let property = self.property.clone();
                                let target = self.property_target.clone();
                                let pages = pages.clone();
                                move |ix, _window, app| {
                                    if let Some(page) = pages.get(*ix) {
                                        property.borrow_mut().active_page = page.id;
                                        entity.update(app, |this, cx| {
                                            // 「数据」页签懒加载：切到数据页且未请求过数据时，
                                            // 自动带默认分页参数重载（首屏 200 行）。
                                            if page.id == PageId::Data {
                                                let has_data = this
                                                    .property
                                                    .borrow()
                                                    .props
                                                    .as_ref()
                                                    .map(|p| p.data.is_some())
                                                    .unwrap_or(false);
                                                if !has_data {
                                                    if let Some(req) = target.borrow().clone() {
                                                        let mut req = req;
                                                        req.data =
                                                            Some(TableDataRequest::default());
                                                        this.request_properties(req, cx);
                                                        return;
                                                    }
                                                }
                                            }
                                            cx.notify();
                                        });
                                    }
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

    /// 当前页内容（描述驱动；Unsupported 页给灰置占位，不发起任何内省查询）。
    ///
    /// `entity` 供「数据」页的翻页 / 排序 on_click 闭包在事件路径重发请求（渲染零 I/O）。
    fn render_property_page_body(
        &self,
        entity: gpui::Entity<EditorPanel>,
        page_id: PageId,
        pages: &[PropertyPage],
        props: &ObjectProperties,
        theme: &Theme,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let Some(page) = pages.iter().find(|p| p.id == page_id) else {
            return div();
        };
        let mut body = div().v_flex().w_full().gap_1();
        if page.state == SupportState::Unsupported {
            // 能力未实现：先绘制、置灰标注（不发起任何内省查询）。
            body = body.child(
                div()
                    .w_full()
                    .py_2()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("「{}」查看与操作暂不支持（驱动能力未实现）", page.title)),
            );
            return body;
        }
        match page_id {
            PageId::General => {
                // DBeaver 分组渲染：按 `category` 分组（`None` 归默认组，不显组标题）。
                // 本轮提供者 category 全 None；结构已支持多组（后续「高级」等分组接入）。
                for row in &props.properties {
                    body = body.child(self.render_property_row(entity.clone(), row, theme));
                }
            }
            PageId::Columns
            | PageId::Indexes
            | PageId::Constraints
            | PageId::Ddl
            | PageId::Source => {
                if let Some(section) = props.sections.iter().find(|s| s.id == page_id) {
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
                } else {
                    body = body.child(div().text_xs().text_color(muted).child("（无）"));
                }
            }
            PageId::Privileges | PageId::Dependencies => {
                // 正常被 Unsupported 分支拦住；兜底灰置。
                body = body.child(div().text_xs().text_color(muted).child("（暂不可用）"));
            }
            PageId::Data => {
                body = self.render_data_page(entity, page, props, theme);
            }
        }
        body
    }

    /// 「数据」页签：只读结果网格（行号 + 列名 + 单元格，NULL 置灰）+ 分页 / 排序。
    ///
    /// 数据来自 [`ObjectProperties::data`]（`rds-database::table_data` 分页抓取）；
    /// 排序 / 翻页走事件路径（`reload_properties_data`），渲染零 I/O。
    fn render_data_page(
        &self,
        entity: gpui::Entity<EditorPanel>,
        page: &PropertyPage,
        props: &ObjectProperties,
        theme: &Theme,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        let mut body = div().v_flex().w_full().gap_1();
        let Some(data) = props.data.as_ref() else {
            body = body.child(div().text_xs().text_color(muted).child("（未加载）"));
            return body;
        };
        if data.columns.is_empty() {
            body = body.child(div().text_xs().text_color(muted).child("（无）"));
            return body;
        }

        // 当前请求参数（翻页 / 排序基线）。
        let current = self
            .property_target
            .borrow()
            .clone()
            .and_then(|r| r.data)
            .unwrap_or_default();
        let current_offset = current.offset;
        let current_limit = if current.limit == 0 { DEFAULT_PAGE_LIMIT } else { current.limit };

        // 表头：行号槽 + 列名（可点击排序——DBeaver 式）。
        let mut header = div()
            .h_flex()
            .w_full()
            .gap_2()
            .pt_1()
            .pb_1()
            .text_xs()
            .text_color(muted);
        header = header.child(div().w(rems(2.0)).flex_none().child("#"));
        for (ix, col) in data.columns.iter().enumerate() {
            let sorted = current
                .order_by
                .iter()
                .find(|(c, _)| c == &col.name)
                .map(|(_, asc)| if *asc { " ↑" } else { " ↓" })
                .unwrap_or("");
            let mut cell = div()
                .id(SharedString::from(format!("data-col-{}-{ix}", col.name)))
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .child(format!("{}{}", col.name, sorted));
            let entity = entity.clone();
            let col_name = col.name.clone();
            let current = current.clone();
            cell = cell.on_click(move |_ev, _window, app| {
                // 点击列头：同列切向 → 升序 → 降序（循环），回到第一页。
                let asc = match current.order_by.iter().find(|(c, _)| c == &col_name) {
                    Some((_, true)) => false,
                    Some((_, false)) => true,
                    None => true,
                };
                let mut order_by = current
                    .order_by
                    .iter()
                    .filter(|(c, _)| c != &col_name)
                    .cloned()
                    .collect::<Vec<_>>();
                order_by.push((col_name.clone(), asc));
                entity.update(app, |this, cx| {
                    this.reload_properties_data(TableDataRequest {
                        offset: 0,
                        limit: current.limit,
                        order_by,
                    }, cx);
                });
            });
            header = header.child(cell);
        }
        body = body.child(header);

        // 数据行。
        for (i, row) in data.rows.iter().enumerate() {
            let row_no = data.offset + i + 1;
            let mut line = div()
                .h_flex()
                .w_full()
                .gap_2()
                .text_xs()
                .border_b_1()
                .border_color(border)
                .text_color(fg);
            line = line
                .child(div().w(rems(2.0)).flex_none().text_color(muted).child(row_no.to_string()));
            for cell in row {
                line = match cell {
                    Some(v) => line.child(div().flex_1().min_w_0().py_px().child(v.clone())),
                    None => line.child(
                        div().flex_1().min_w_0().py_px().text_color(muted).child("NULL"),
                    ),
                };
            }
            body = body.child(line);
        }

        // 页脚：行数 + 顺序提示 + 分页按钮。
        let total_text = data
            .total
            .map(|t| format!("共 {t} 行"))
            .unwrap_or_else(|| "行数未知".to_string());
        let mut footer = div()
            .h_flex()
            .items_center()
            .gap_2()
            .pt_2()
            .text_xs()
            .text_color(muted)
            .child(div().child(total_text));
        if !data.is_ordered {
            footer = footer.child(div().child("（本页无定义顺序）"));
        }
        if current_offset > 0 {
            let entity = entity.clone();
            let current = current.clone();
            footer = footer.child(
                Button::new("data-prev")
                    .label("‹ 上一页")
                    .with_size(Size::Small)
                    .on_click(move |_, _, app| {
                        let new_offset = current_offset.saturating_sub(current_limit);
                        entity.update(app, |this, cx| {
                            let mut req = current.clone();
                            req.offset = new_offset;
                            this.reload_properties_data(req, cx);
                        });
                    }),
            );
        }
        if data.has_more {
            let entity = entity.clone();
            let current = current.clone();
            footer = footer.child(
                Button::new("data-next")
                    .label("下一页 ›")
                    .with_size(Size::Small)
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| {
                            let mut req = current.clone();
                            req.offset = current_offset + current_limit;
                            this.reload_properties_data(req, cx);
                        });
                    }),
            );
        }
        body = body.child(footer);
        let _ = page;
        body
    }

    /// 属性网格行渲染（DBeaver 显示逻辑：只读 → 文本；可编辑 → 编辑控件）。
    ///
    /// - `Boolean` 形态 → Switch（点击即切换提交）；
    /// - `Text / LongText` 形态 → 未编辑时「值 + 编辑按钮」，编辑中 → 输入框 + 保存 / 取消。
    fn render_property_row(
        &self,
        entity: gpui::Entity<EditorPanel>,
        row: &PropertyRow,
        theme: &Theme,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let label = div()
            .w(rems(4.5))
            .flex_none()
            .text_xs()
            .text_color(muted)
            .child(row.label.clone());

        if !row.editable {
            // 只读：文本行（DBeaver 只读属性）。
            return div()
                .h_flex()
                .items_start()
                .w_full()
                .gap_2()
                .text_xs()
                .child(label)
                .child(div().flex_1().min_w_0().text_color(fg).child(row.value.clone()));
        }

        match row.value_kind {
            PropertyValueKind::Boolean => {
                // 布尔：Switch（DBeaver 布尔属性编辑即生效；回调收到切换后的新值）。
                let checked = row.value == "true";
                let entity = entity.clone();
                let rid = row.id.clone();
                div()
                    .h_flex()
                    .items_center()
                    .w_full()
                    .gap_2()
                    .text_xs()
                    .child(label)
                    .child(
                        div().flex_1().min_w_0().child(
                            Switch::new(format!("prop-bool-{}", rid))
                                .checked(checked)
                                .on_click(move |new_val, _, cx| {
                                    entity.update(cx, |panel, cx| {
                                        panel.toggle_property_bool(rid.clone(), *new_val, cx);
                                    });
                                }),
                        ),
                    )
            }
            _ => {
                let editing = self
                    .property_edit
                    .borrow()
                    .as_ref()
                    .map(|e| e.row_id == row.id)
                    .unwrap_or(false);
                if editing {
                    // 编辑中：输入框 + 保存 / 取消。
                    let edit = self.property_edit.borrow().clone().expect("编辑态已检查");
                    let save_entity = entity.clone();
                    let cancel_entity = entity.clone();
                    let input = Input::new(&edit.input).id(format!("prop-edit-{}", row.id)).w_full();
                    div()
                        .h_flex()
                        .items_start()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .child(label)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .v_flex()
                                .gap_1()
                                .child(input)
                                .child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Button::new(format!("prop-save-{}", row.id))
                                                .label("保存")
                                                .with_size(Size::XSmall)
                                                .on_click(move |_, _, app| {
                                                    save_entity.update(app, |panel, cx| {
                                                        panel.submit_property_edit(cx);
                                                    });
                                                }),
                                        )
                                        .child(
                                            Button::new(format!("prop-cancel-{}", row.id))
                                                .label("取消")
                                                .with_size(Size::XSmall)
                                                .on_click(move |_, _, app| {
                                                    cancel_entity.update(app, |panel, cx| {
                                                        panel.cancel_property_edit(cx);
                                                    });
                                                }),
                                        ),
                                ),
                        )
                } else {
                    // 未编辑：值 + 编辑按钮（DBeaver 点击可编辑值进入编辑）。
                    let entity = entity.clone();
                    let rid = row.id.clone();
                    let initial = row.value.clone();
                    div()
                        .h_flex()
                        .items_start()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .child(label)
                        .child(div().flex_1().min_w_0().text_color(fg).child(row.value.clone()))
                        .child(
                            Button::new(format!("prop-edit-btn-{}", row.id))
                                .label("编辑")
                                .with_size(Size::XSmall)
                                .on_click(move |_, window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.begin_property_edit(
                                            rid.clone(),
                                            initial.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                }
            }
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
                // 点命中 → 在编辑器里打开该草稿（相对路径 → 模块内绝对路径；跳行仍待编辑器端口）。
                let open_hit: Rc<dyn Fn(String, &mut gpui_kit::Window, &mut App)> = {
                    let shared = self.shared.clone();
                    Rc::new(move |file: String, _window: &mut gpui_kit::Window, _app: &mut App| {
                        if let Some(root) = shared.project_root() {
                            let abs = root
                                .join(scratchpad::MODULE_DIR_NAME)
                                .join(file.as_str());
                            shared.request_open_in_editor(abs);
                        }
                    })
                };
                content = content.child(render_scratchpad_search_pane(
                    search,
                    theme,
                    match_bg,
                    replace_input.as_ref(),
                    replace_filled,
                    open_hit,
                    clear,
                    replace_all,
                ));
            }
        }

        // M5 草稿箱冲突 Diff（原型 §4.5：冲突时「查看差异」→ 落中央编辑区）。
        {
            let diff = self.scratchpad_diff.borrow();
            if let Some(diff) = diff.as_ref() {
                let shared = self.shared.clone();
                let diff_state = self.scratchpad_diff.clone();
                let entity = entity.clone();
                let clear = move |_: &gpui_kit::ClickEvent,
                                  _: &mut gpui_kit::Window,
                                  app: &mut App| {
                    *diff_state.borrow_mut() = None;
                    shared.notify_host(app);
                    entity.update(app, |_, cx| cx.notify());
                };
                content = content.child(render_scratchpad_diff_pane(diff, theme, clear));
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

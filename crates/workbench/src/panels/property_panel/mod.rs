//! 属性面板（中央 Dock 独立 tab，DBeaver 式属性编辑器）。
//!
//! 双击导航对象 / 键盘 F4 → 打开/聚焦本 tab 并加载对象属性（与编辑器同级别，
//! 不再内嵌在 EditorPanel 右侧分割）。单实例复用：切对象即替换 tab 内容，
//! tab 标题跟随当前对象名（`ComponentPanel::title` 动态渲染）。
//!
//! 数据链路：`Shared::show_properties` → 宿主装配的 `open_property` 回调
//! （创建/聚焦面板）→ `PropertyHostPanel::show_properties` 入队加载
//! （`nav_jobs::enqueue_properties`）→ 轮询回填 `PropertyState` → 渲染只读。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::EventEmitter;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Shared;
use database::model::PropertyRequest;
use database::nav_jobs;
use database::table_data::{DEFAULT_PAGE_LIMIT, TableDataRequest};
use property::PropertyState;
use property::{ObjectProperties, PageId, PropertyRow, PropertySection, PropertyValueKind};
use workbench_shell::{tree, ui};

mod cells;
mod data_page;
mod render;
mod sections;

/// 属性行内编辑态（DBeaver 属性编辑：点击可编辑值 → 行内编辑控件）。
#[derive(Clone)]
struct PropertyEdit {
    row_id: String,
    input: Entity<InputState>,
}

/// 属性面板宿主：中央 tab 内单实例，渲染只读 `PropertyState`。
pub struct PropertyHostPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`BasePanel::on_added_to` 注入；重复打开时用于聚焦自身）。
    group: Option<WeakEntity<TabGroup>>,
    /// 属性加载结果（provider 组装；`request_properties` 置 loading，轮询回填）。
    property: Rc<RefCell<PropertyState>>,
    /// 正在轮询属性加载结果的后台任务。
    props_pump: RefCell<Option<Task<()>>>,
    /// 当前对象请求（翻页 / 排序 / 编辑的目标基线）。
    property_target: Rc<RefCell<Option<PropertyRequest>>>,
    /// 属性行内编辑态（`None` = 未在编辑）。
    property_edit: Rc<RefCell<Option<PropertyEdit>>>,
    /// 子项区当前选中的类别索引（「子 tab + 内容」左右结构；切对象时重置 0）。
    active_category: Cell<usize>,
    /// Select 常驻下拉：已挂订阅的行 id（渲染时按业务键查重，避免重复订阅）。
    select_sub_keys: RefCell<HashSet<String>>,
    /// Select 订阅句柄（保活；切对象时 clear 注销，避免跨对象残留回调）。
    select_subs: RefCell<Vec<Subscription>>,
    /// 顶部 filter 搜索框文本（DBeaver 属性面板右上角搜索：实时过滤属性名/值）。
    filter: Rc<RefCell<String>>,
    /// 搜索框 InputState（渲染时惰性创建；切对象时不清空——filter 是视图态）。
    filter_input: RefCell<Option<Entity<InputState>>>,
    /// 搜索框 Change 事件订阅（保活；首次渲染时挂）。
    filter_sub: RefCell<Option<Subscription>>,
    /// 未提交的草稿改动（DBeaver：编辑 → dirty → Apply 才发 ALTER）。
    /// (row_id, new_value)；切对象时清空。
    dirty: Rc<RefCell<Vec<(String, String)>>>,
}

impl PropertyHostPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            group: None,
            property: Rc::new(RefCell::new(PropertyState::default())),
            props_pump: RefCell::new(None),
            property_target: Rc::new(RefCell::new(None)),
            property_edit: Rc::new(RefCell::new(None)),
            active_category: Cell::new(0),
            select_sub_keys: RefCell::new(HashSet::new()),
            select_subs: RefCell::new(Vec::new()),
            filter: Rc::new(RefCell::new(String::new())),
            filter_input: RefCell::new(None),
            filter_sub: RefCell::new(None),
            dirty: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// 请求属性面板数据（`Shared::show_properties` 分发）：**入队与轮询都在事件路径**，
    /// 渲染只读 `PropertyState`。
    pub fn show_properties(&self, request: PropertyRequest, cx: &mut Context<Self>) {
        // 切对象时清空草稿（不同对象的 dirty 不跨对象保留）。
        *self.dirty.borrow_mut() = Vec::new();
        let key = format!(
            "{}|{:?}|{}|{:?}",
            request.property.conn_id,
            request.property.kind,
            request.property.name,
            request.property.parent
        );
        // 驱动目录已缓存（随组织数据一次加载）：补 type_id（属性源分派键）+
        // 组装「驱动友好名 + 类型展示名」（视图层只给驱动 id）。
        let mut request = request;
        let (driver_display, type_id, db_type) = {
            let catalog = self.shared.driver_catalog.borrow();
            match catalog.get(&request.driver) {
                Some(meta) => {
                    if request.type_id.is_none() {
                        request.type_id = Some(meta.type_id.clone());
                    }
                    (
                        format!("{} · {}", meta.name, request.driver),
                        request.type_id.clone(),
                        // 类型行显示**目录名**（`data_source_types.name`）优先，取不到才回退类型 id
                        Some(
                            meta.type_name
                                .clone()
                                .filter(|n| !n.trim().is_empty())
                                .unwrap_or_else(|| meta.type_id.clone()),
                        ),
                    )
                }
                None => (request.driver.clone(), request.type_id.clone(), None),
            }
        };
        {
            let mut st = self.property.borrow_mut();
            st.loaded_for = Some(key.clone());
            st.props = None;
            st.error = None;
            st.loading = true;
        }
        // 切对象回到第一子项（子项区「子 tab + 内容」左栏选中态归零）。
        self.active_category.set(0);
        // 切对象：Select 常驻下拉的旧订阅注销（行 id 随属性集重建；旧 state 由
        // `use_keyed_state` 在元素消失后自动回收，无需显式释放）。
        self.select_sub_keys.borrow_mut().clear();
        self.select_subs.borrow_mut().clear();
        nav_jobs::enqueue_properties(
            &key,
            request.property.clone(),
            &request.conn_label,
            &driver_display,
            type_id.as_deref(),
            db_type.as_deref(),
            request.data.clone(),
        );
        self.ensure_props_pump(cx);
        *self.property_target.borrow_mut() = Some(request);
        cx.notify();
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

    /// 以新的数据页参数重新请求当前对象（「数据」页签翻页 / 排序用）。
    ///
    /// 属性与数据同一次往返：属性走 L2 缓存，数据按 `offset` / `order_by` 重查。
    fn reload_properties_data(&self, data: TableDataRequest, cx: &mut Context<Self>) {
        let Some(mut req) = self.property_target.borrow().clone() else {
            return;
        };
        req.data = Some(data);
        self.show_properties(req, cx);
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

    /// 布尔属性切换：写入 dirty 草稿（DBeaver：编辑即标记 dirty，Apply 才发 ALTER）。
    fn toggle_property_bool(&self, row_id: String, new_value: bool, cx: &mut Context<Self>) {
        let value = if new_value { "true" } else { "false" }.to_string();
        let mut dirty = self.dirty.borrow_mut();
        // upsert：同 row_id 覆盖旧草稿。
        if let Some(slot) = dirty.iter_mut().find(|(rid, _)| *rid == row_id) {
            slot.1 = value;
        } else {
            dirty.push((row_id, value));
        }
        cx.notify();
    }

    /// Apply：遍历 dirty 草稿逐个发 ALTER，全部成功后清空 dirty + 重载。
    fn apply_dirty(&self, cx: &mut Context<Self>) {
        let dirty: Vec<(String, String)> = self.dirty.borrow().clone();
        if dirty.is_empty() {
            return;
        }
        let Some(target) = self.property_target.borrow().clone() else {
            return;
        };
        // 逐个串行执行（避免并发 ALTER 冲突）；全部完成后统一清空 dirty + 重载。
        for (row_id, value) in dirty {
            self.run_property_edit(target.clone(), row_id, value, cx);
        }
        *self.dirty.borrow_mut() = Vec::new();
        cx.notify();
    }

    /// Revert：丢弃全部草稿改动，重载面板恢复原始值。
    fn revert_dirty(&self, cx: &mut Context<Self>) {
        *self.dirty.borrow_mut() = Vec::new();
        // 重载面板拉回服务端最新值。
        if let Some(target) = self.property_target.borrow().clone() {
            self.show_properties(target, cx);
        }
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
        let metadata = database::metadata_service::MetadataService::new(
            engine::get_connection_manager().clone(),
        );
        // async move 按值捕获：把传给后台的字段先拆出来，`target` 保持完整供回填闭包用。
        let prop = target.property.clone();
        let driver = target.driver.clone();
        let type_id = target.type_id.clone();
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move {
                    database::property_panel::apply_property_edit(
                        &metadata,
                        &prop,
                        &driver,
                        type_id.as_deref(),
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
                        panel.show_properties(target, cx);
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
}

impl EventEmitter<PanelEvent> for PropertyHostPanel {}

impl Focusable for PropertyHostPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PropertyHostPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 先渲染（可变借用 cx），再取色（不可变借用）——顺序反过来会 E0502。
        let panel = self.render_property_panel(window, cx);
        let bg = cx.theme().colors.background;
        panel
            .id("property-host-panel")
            .role(gpui::Role::Group)
            .test_support()
            .bg(bg)
            .size_full()
    }
}

impl BasePanel for PropertyHostPanel {
    fn panel_name(&self) -> &'static str {
        "property"
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }
}

impl ComponentPanel for PropertyHostPanel {
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.tab_label(cx).into())
    }

    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = self.tab_label(cx);
        div().text_sm().font_weight(FontWeight::MEDIUM).child(label)
    }
}

impl PropertyHostPanel {
    /// tab 标题：当前对象名（未选择时回退「属性」）。
    fn tab_label(&self, _cx: &App) -> String {
        let dirty = !self.dirty.borrow().is_empty();
        let title = if let Some(target) = self.property_target.borrow().as_ref() {
            self.property
                .borrow()
                .props
                .as_ref()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| target.property.name.clone())
        } else {
            String::new()
        };
        let base = if !title.is_empty() { title } else { "属性".to_string() };
        if dirty { format!("*{}", base) } else { base }
    }
}

/// 把属性 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**：`TabGroup` 切 tab 时会回读面板实体（`panel_name` /
/// `focus_handle`），若在 `update_entity(PropertyHostPanel)` 的闭包里调用，gpui 会以
/// `cannot read … while it is already being updated` panic（double lease）。
pub fn focus_property_tab(panel: &Entity<PropertyHostPanel>, window: &mut Window, cx: &mut App) {
    let (focus_handle, group) = {
        let view = panel.read(cx);
        (view.focus_handle.clone(), view.group.clone())
    };
    window.focus(&focus_handle, cx);
    let Some(group) = group else {
        return;
    };
    let me = panel.entity_id();
    group
        .update(cx, |group, cx| {
            let ix = group
                .panels()
                .iter()
                .position(|p| p.view().entity_id() == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        })
        .ok();
}

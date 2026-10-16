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
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Size, Sizable as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Shared;
use database::model::PropertyRequest;
use database::nav_jobs;
use database::table_data::{DEFAULT_PAGE_LIMIT, TableDataRequest};
use property::{ObjectProperties, PageId, PropertyRow, PropertySection, PropertyValueKind};
use property::PropertyState;
use workbench_shell::{tree, ui};

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
        }
    }

    /// 请求属性面板数据（`Shared::show_properties` 分发）：**入队与轮询都在事件路径**，
    /// 渲染只读 `PropertyState`。
    pub fn show_properties(&self, request: PropertyRequest, cx: &mut Context<Self>) {
        let key = format!(
            "{}|{:?}|{}|{:?}",
            request.property.conn_id, request.property.kind, request.property.name, request.property.parent
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
                None => (
                    request.driver.clone(),
                    request.type_id.clone(),
                    None,
                ),
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

    /// 属性面板（DBeaver 式：tab 页签 + 描述驱动渲染）。
    ///
    /// 页集合来自 [`PropertyRegistry`]（`PropertyRegistryImpl`），渲染只遍历页描述，
    /// 不按 `PropertyKind` 写分支；暂不支持页（权限 / 依赖）先绘制、置灰标注
    /// （`docs/architecture/property/property-module-spec.md` §8）。
    fn render_property_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let Some(target) = self.property_target.borrow().clone() else {
            let muted = cx.theme().colors.muted_foreground;
            return div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(muted)
                .child("未选择对象：双击导航树中的连接 / 数据库 / 表等对象查看属性");
        };

        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;

        let st = self.property.borrow();

        // clone 主题对象：渲染子树需要同时拿 `&Theme` 与 `&mut cx`（`cx.theme()` 会一直
        // 借用 cx，直接传会 E0502；Theme 是轻量值类型，clone 成本可忽略）。
        let theme = cx.theme().clone();
        let entity = cx.entity();
        // 表 / 视图两页签【属性 | 数据】；其余对象为单属性视图（无页签条）。
        let is_tabbed = matches!(
            target.property.kind,
            property::PropertyKind::Table | property::PropertyKind::View
        );
        let active = st.active_page;
        let tabs: Vec<(PageId, &str)> = if is_tabbed {
            vec![(PageId::General, "属性"), (PageId::Data, "数据")]
        } else {
            vec![(PageId::General, "属性")]
        };
        let active_index = tabs.iter().position(|(id, _)| *id == active).unwrap_or(0);

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
            if active == PageId::Data {
                body = body.child(self.render_data_page(entity.clone(), props, &theme));
            } else {
                body = body.child(self.render_property_view(entity.clone(), props, &theme, window, cx));
            }
        }

        let mut tab_bar = div().h_flex().items_center().gap_1().w_full().border_b_1().border_color(border);
        if is_tabbed {
            tab_bar = tab_bar.child(
                TabBar::new("property-tabs")
                    .underline()
                    .with_size(Size::Small)
                    .selected_index(active_index)
                    .children(tabs.iter().map(|(_, label)| Tab::new().label(label.to_string())))
                    .on_click({
                        let entity = cx.entity();
                        let property = self.property.clone();
                        let target = self.property_target.clone();
                        let tabs = tabs.clone();
                        move |ix, _window, app| {
                            if let Some((page_id, _)) = tabs.get(*ix) {
                                property.borrow_mut().active_page = *page_id;
                                entity.update(app, |this, cx| {
                                    // 「数据」页签懒加载：切到数据页且未请求过数据时，
                                    // 自动带默认分页参数重载（首屏 200 行）。
                                    if *page_id == PageId::Data {
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
                                                req.data = Some(TableDataRequest::default());
                                                this.show_properties(req, cx);
                                                return;
                                            }
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        }
                    }),
            );
        }

        div()
            .v_flex()
            .w_full()
            .h_full()
            .flex_none()
            .child(tab_bar)
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

    /// 属性视图（三部分布局，DBeaver 之上加「洞察总览」——见 §16 设计）：
    /// ① 洞察总览（运行时 / 统计事实，provider 组装）
    /// ② General 双栏网格（定义属性；行数多时自动三栏，增加面积使用率）
    /// ③ 子项类别列表（驱动能力掩码；灰置类别先绘制、标注「暂不可用」）
    ///
    /// 大区块统一「卡片语言」：`border + rounded + sidebar 底 + 标题头`（与洞察卡一致，
    /// 连接编辑器 / 洞察 / Mock 模块同规范，见 `property-module-spec.md` §17）。
    fn render_property_view(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = div().v_flex().w_full().gap_3();
        body = body.child(self.render_overview_section(props, theme));
        body = body.child(self.render_general_grid(entity.clone(), props, theme, window, cx));
        body = body.child(self.render_category_list(entity.clone(), props, theme, cx));
        body
    }

    /// 卡片头：标题 + 可选尾注（规范：`text_xs` 中字重标题 + `muted` 尾注）。
    fn card_header(&self, title: &str, note: Option<&str>, theme: &Theme) -> Div {
        let mut head = div()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child(title.to_string()),
            );
        if let Some(note) = note {
            head = head.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_right()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(note.to_string()),
            );
        }
        head
    }

    /// ① 洞察总览：标题 + 状态点 + meta 芯片 + 指标卡 + 关键信息 + 口径注脚。
    fn render_overview_section(&self, props: &ObjectProperties, theme: &Theme) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;
        let ov = &props.overview;
        let status_color = match ov.status {
            property::OverviewStatus::Connected => theme.colors.success,
            property::OverviewStatus::Disconnected => theme.colors.danger,
            property::OverviewStatus::Idle => muted,
        };

        let mut section = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(border)
            .rounded_md()
            .bg(theme.colors.sidebar);

        // 头部：标题 + 状态点 + 状态文案 + 子标题。
        let mut head = div().h_flex().items_center().gap_2().w_full();
        head = head
            .child(
                div()
                    .w(rems(ui::PROPERTY_STATUS_DOT_SIZE))
                    .h(rems(ui::PROPERTY_STATUS_DOT_SIZE))
                    .flex_none()
                    .rounded_full()
                    .bg(status_color),
            )
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(fg).child(ov.title.clone()))
            .child(div().text_xs().text_color(muted).child(ov.status_text.clone()))
            .child(div().flex_1())
            .child(div().text_xs().text_color(muted).child(ov.subtitle.clone()));

        // meta 芯片（驱动 / 作用域 / 分组 / 标签——RDS 连接模型独有信息）。
        let mut chips = div().h_flex().items_center().gap_1().w_full().flex_wrap();
        for (label, value) in &ov.meta {
            chips = chips.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_px()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .text_xs()
                    .child(div().text_color(muted).child(label.clone()))
                    .child(div().text_color(fg).child(value.clone())),
            );
        }

        // 指标卡（横排）。
        let mut cards = div().h_flex().items_stretch().gap_2().w_full().flex_wrap();
        for c in &ov.cards {
            cards = cards.child(
                div()
                    .v_flex()
                    .gap_1()
                    .min_w(rems(ui::PROPERTY_CARD_MIN_WIDTH))
                    .flex_1()
                    .p_2()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .bg(theme.colors.background)
                    .child(div().text_xs().text_color(muted).child(c.label.clone()))
                    .child(
                        div()
                            .h_flex()
                            .items_baseline()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(fg)
                                    .child(c.value.clone()),
                            )
                            .child(div().text_xs().text_color(muted).child(c.unit.clone())),
                    )
                    .when_some(c.delta.clone(), |el, delta| {
                        el.child(div().text_xs().text_color(muted).child(delta))
                    }),
            );
        }

        // 关键信息（键值行）。
        let mut kv = div().v_flex().w_full().gap_px().text_xs();
        for (label, value) in &ov.kv {
            kv = kv.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(div().w(rems(ui::PROPERTY_KEY_LABEL_WIDTH)).flex_none().text_color(muted).child(label.clone()))
                    .child(div().flex_1().min_w_0().text_color(fg).child(value.clone())),
            );
        }

        section = section.child(head).child(chips);
        if !ov.cards.is_empty() {
            section = section.child(cards);
        }
        if !ov.kv.is_empty() {
            section = section.child(kv);
        }
        if !ov.note.is_empty() {
            section = section.child(div().text_xs().text_color(muted).child(ov.note.clone()));
        }
        section
    }

    /// ② General 双栏网格（DBeaver General 扁平紧凑 + 面积使用率；行数 > 8 自动三栏）。
    /// 大区块卡片化：与洞察卡同一「边框 + 圆角 + 侧栏底」语言。
    fn render_general_grid(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = theme.colors.muted_foreground;
        let rows = &props.properties;
        let mut section = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(theme.colors.border)
            .rounded_md()
            .bg(theme.colors.sidebar);
        section = section.child(self.card_header("通用", Some("定义属性"), theme));
        if rows.is_empty() {
            section = section.child(div().text_xs().text_color(muted).child("（无属性）"));
            return section;
        }
        let cols = if rows.len() > 8 { 3 } else { 2 };
        for chunk in rows.chunks(cols) {
            let mut line = div().h_flex().items_start().w_full().gap_2();
            for row in chunk {
                line = line.child(self.render_property_cell(entity.clone(), row, theme, window, cx));
            }
            line = line.child(div().flex_1().min_w_0());
            section = section.child(line);
        }
        section
    }

    /// ③ 子项类别区（DBeaver 表属性页形态：**子 tab + 内容**）。
    ///
    /// 多类别 → 左侧竖排子标签栏（名称 + 行数徽标 + 灰置态）+ 右侧内容区，
    /// 点击左栏切换右侧表格；单类别 / 无类别 → 退化为直接堆叠（不显示左栏）。
    /// 整体卡片化（与洞察 / General 同语言）。
    fn render_category_list(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut section = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(theme.colors.border)
            .rounded_md()
            .bg(theme.colors.sidebar);
        if props.sections.is_empty() {
            section = section.child(self.card_header("子项", Some("（无）"), theme));
            return section;
        }
        section = section.child(self.card_header("子项", None, theme));
        if props.sections.len() == 1 {
            section = section.child(self.render_category_section(&props.sections[0], theme));
            return section;
        }

        // 多类别：左栏子标签 + 右栏内容（当前选中越界保护）。
        let active = self.active_category.get().min(props.sections.len() - 1);
        let mut body = div().h_flex().items_stretch().w_full().gap_2();
        // 左栏：竖排子标签（固定宽，类别切换）。
        let mut rail = div()
            .v_flex()
            .w(rems(ui::PROPERTY_CAT_RAIL_WIDTH))
            .flex_none()
            .border_1()
            .border_color(theme.colors.border)
            .rounded_md()
            .overflow_y_scrollbar();
        for (ix, s) in props.sections.iter().enumerate() {
            rail = rail.child(self.render_category_tab(entity.clone(), s, ix, ix == active, theme, cx));
        }
        // 右栏：选中类别内容。
        let content = div().flex_1().min_w_0().child(self.render_category_section(&props.sections[active], theme));
        body = body.child(rail).child(content);
        section = section.child(body);
        section
    }

    /// 左栏单个子标签（「子 tab + 内容」的 tab 本体）：
    /// 选中态 = `Button` toggled 底色 + 左侧 2px 激活条（与导航树行同原语）；
    /// 灰置态 = custom 变体置灰文字 + 「暂不可用」尾注，仍可点（切过去看占位说明）。
    ///
    /// 2026-10-15 由自绘可点行迁移到语义 `Button`：拿到 hover / 焦点 / 键盘（Enter/Space
    /// 在 KeyUp 激活）；计数徽标 / 尾注外置行尾（不并入按钮命中区，保持右对齐）。
    fn render_category_tab(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        s: &PropertySection,
        ix: usize,
        selected: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = theme.colors.muted_foreground;
        let active_border = theme.colors.sidebar_accent;
        let count = s.table.rows.len();

        // 按钮本体：id 用类别名（业务键，provider 增删类别时焦点 / hover 态不串位）。
        let mut btn = Button::new(format!("property-cat-{}", s.label))
            .ghost()
            .toggled(selected)
            .small()
            .flex_1()
            .min_w_0()
            .label(s.label.clone());
        // 灰置类别：`Button` 无公开前景色 setter，走 `ButtonCustomVariant` 保持置灰文字
        //（hover / active 底色与可用行同一套，避免“灰置行点了没反馈”）。
        if !s.enabled {
            btn = btn.custom(
                ButtonCustomVariant::new(&**cx)
                    .foreground(muted)
                    .hover(theme.colors.list_hover)
                    .active(theme.colors.list_active),
            );
        }
        let mut tab = div()
            .relative()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .h(rems(ui::PROPERTY_CAT_ROW_HEIGHT))
            .px_1()
            .when(selected, |el| el.child(tree::active_bar(active_border)))
            .child(
                btn.on_click(move |_, _, app| {
                    entity.update(app, |this, cx| {
                        this.active_category.set(ix);
                        cx.notify();
                    });
                }),
            );
        if !s.enabled {
            tab = tab.child(div().flex_none().text_xs().text_color(muted).child("暂不可用"));
        } else {
            tab = tab.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(count.to_string()),
            );
        }
        tab
    }

    /// 单个子项类别：可用 → 标题 + 表头 + 行；灰置 → 标题置灰 + 「暂不可用」占位。
    fn render_category_section(
        &self,
        s: &PropertySection,
        theme: &Theme,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        if !s.enabled {
            // 能力未实现：先绘制、置灰标注（不发起任何内省查询）。
            return div()
                .v_flex()
                .w_full()
                .gap_1()
                .p_2()
                .rounded_md()
                .bg(theme.colors.background)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(s.label.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("（暂不可用）"),
                        ),
                );
        }

        let mut section = div()
            .v_flex()
            .w_full()
            .gap_1()
            .p_2()
            .rounded_md()
            .bg(theme.colors.background);
        section = section.child(
            div()
                .pt_1()
                .pb_1()
                .w_full()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(fg)
                .child(s.label.clone()),
        );
        if s.table.headers.is_empty() {
            section = section.child(div().text_xs().text_color(muted).child("（无）"));
            return section;
        }
        section = section.child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .pb_1()
                .border_b_1()
                .border_color(border)
                .text_xs()
                .text_color(muted)
                .children(
                    s.table
                        .headers
                        .iter()
                        .map(|h| div().flex_1().min_w_0().child(h.clone())),
                ),
        );
        for data_row in &s.table.rows {
            section = section.child(
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
        section
    }

    /// 「数据」页签：只读结果网格（行号 + 列名 + 单元格，NULL 置灰）+ 分页 / 排序。
    ///
    /// 数据来自 [`ObjectProperties::data`]（`rds-database::table_data` 分页抓取）；
    /// 排序 / 翻页走事件路径（`reload_properties_data`），渲染零 I/O。
    fn render_data_page(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        props: &ObjectProperties,
        theme: &Theme,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        // 与洞察 / General / 子项同卡片语言：侧栏底 + 边框 + 标题头。
        let mut body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(border)
            .rounded_md()
            .bg(theme.colors.sidebar)
            .child(self.card_header("数据", None, theme));
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
        header = header.child(div().w(rems(ui::PROPERTY_ROW_NUMBER_WIDTH)).flex_none().child("#"));
        for (ix, col) in data.columns.iter().enumerate() {
            let sorted = current
                .order_by
                .iter()
                .find(|(c, _)| c == &col.name)
                .map(|(_, asc)| if *asc { " ↑" } else { " ↓" })
                .unwrap_or("");
            let entity = entity.clone();
            let col_name = col.name.clone();
            let current = current.clone();
            header = header.child(
                // 列头排序迁移到语义 `Button`（2026-10-15）：拿到 hover / 焦点 / 键盘，
                // 不再自绘可点 div；id 保留业务键（列名 + 下标，与行渲染一致）。
                Button::new(SharedString::from(format!("data-col-{}-{ix}", col.name)))
                    .ghost()
                    .xsmall()
                    .flex_1()
                    .min_w_0()
                    .label(format!("{}{}", col.name, sorted))
                    .on_click(move |_ev, _window, app| {
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
                    }),
            );
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
                .child(div().w(rems(ui::PROPERTY_ROW_NUMBER_WIDTH)).flex_none().text_color(muted).child(row_no.to_string()));
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
        body
    }

    /// 属性网格单元（General 双栏 / 三栏网格的一格：label 上 / 值下，紧凑竖排）。
    ///
    /// 编辑逻辑与旧整行版本一致：Boolean → Switch（点击即切换提交）；
    /// Select → 常驻下拉（选中即提交，见下方分支）；Text / LongText →
    /// 未编辑时「值 + 编辑按钮」，编辑中 → 输入框 + 保存 / 取消。
    fn render_property_cell(
        &self,
        entity: gpui::Entity<PropertyHostPanel>,
        row: &PropertyRow,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let label = div().text_xs().text_color(muted).child(row.label.clone());

        if !row.editable {
            // 只读：文本（DBeaver 只读属性）。
            return div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .text_xs()
                .child(label)
                .child(div().w_full().min_w_0().text_color(fg).child(row.value.clone()));
        }

        match row.value_kind {
            PropertyValueKind::Boolean => {
                // 布尔：Switch（DBeaver 布尔属性编辑即生效；回调收到切换后的新值）。
                let checked = row.value == "true";
                let entity = entity.clone();
                let rid = row.id.clone();
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .text_xs()
                    .child(label)
                    .child(
                        div()
                            .w_full()
                            .child(
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
            PropertyValueKind::Select if row.select_options.is_empty() => {
                // 空候选回退文本展示（provider 未给候选时不做空下拉）。
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .text_xs()
                    .child(label)
                    .child(div().w_full().min_w_0().text_color(fg).child(row.value.clone()))
            }
            PropertyValueKind::Select => {
                // 常驻下拉（不经编辑按钮，选中即提交）：`use_keyed_state` 在渲染管线内
                // 惰性创建 SelectState（元素消失自动回收，无需手工清理）；Confirm 走公共
                // `run_property_edit` 后台 ALTER 路径（与 Boolean / 文本同一提交链）。
                let row_id = row.id.clone();
                let options = row.select_options.clone();
                let state = window.use_keyed_state(
                    format!("prop-select-{row_id}"),
                    cx,
                    move |window, cx| {
                        SelectState::new(SearchableVec::new(options), None, window, cx)
                    },
                );
                // 首次渲染该行挂订阅（按业务键查重，切对象时在 show_properties 注销）。
                if !self.select_sub_keys.borrow().contains(&row_id) {
                    self.select_sub_keys.borrow_mut().insert(row_id.clone());
                    let sub = cx.subscribe_in(
                        &state,
                        window,
                        move |panel, _state, event: &SelectEvent<SearchableVec<String>>, _window, cx| {
                            if let SelectEvent::Confirm(Some(value)) = event {
                                if let Some(target) = panel.property_target.borrow().clone() {
                                    panel.run_property_edit(target, row_id.clone(), value.to_string(), cx);
                                }
                            }
                        },
                    );
                    self.select_subs.borrow_mut().push(sub);
                }
                // 渲染同步：模型值（编辑重载后变化）→ 控件选中；`set_selected_value` 不触发
                // Confirm（程序化写入不发事件），不会造成重复提交。
                if state.read(cx).selected_value().map(|v| v.as_str()) != Some(row.value.as_str()) {
                    let value = row.value.clone();
                    state.update(cx, |s, cx| {
                        s.set_selected_value(&value, window, cx);
                    });
                }
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .text_xs()
                    .child(label)
                    .child(
                        div()
                            .w_full()
                            .child(Select::new(&state).w_full().placeholder("选择…")),
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
                        .v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .text_xs()
                        .child(label)
                        .child(
                            div()
                                .w_full()
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
                        .v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .text_xs()
                        .child(label)
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
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
                                ),
                        )
                }
            }
        }
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
        panel.id("property-host-panel").role(gpui::Role::Group).test_support().bg(bg).size_full()
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
        if let Some(target) = self.property_target.borrow().as_ref() {
            let title = self
                .property
                .borrow()
                .props
                .as_ref()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| target.property.name.clone());
            if !title.is_empty() {
                return title;
            }
        }
        "属性".to_string()
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

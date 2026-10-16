//! 元数据搜索结果标签页（「搜索: query」，中央 Dock 独立 tab，VSCode 全局搜索结果形态）。
//!
//! 与 `search_results.rs`（草稿箱内容搜索）**同形态、不同搜索源**：
//! 本面板搜的是**连接级元数据缓存**（`nav_jobs::SearchIndex`，名称中缀 / 内容 FTS），
//! 不是草稿箱文件内容。数据链路（2026-10-14 P0）：
//!
//! ```text
//! 面板顶部搜索栏 Enter → nav_jobs::enqueue_search(SearchConsumer::Panel, …)
//!   → 后台并行搜全部可见连接的缓存
//!   → 面板自持后台泵（60ms 轮询）drain_search_results(Panel)
//!   → Shared::publish_metadata_search（全局态 + notify_host）
//!   → WorkbenchView::render 检测有结果且面板未建 → cx.defer_in 建面板并聚焦
//!   → 面板每次渲染从 Shared::metadata_search 读最新结果
//! ```
//!
//! 多开语义（与 `search_results.rs` 一致）：每次**不同查询词**的搜索结果新建独立 tab；
//! 面板内「关闭」→ 清空结果；tab ✕ 由 dock 的 Close 事件移除面板。
//!
//! P2（本批）：命中行「单击 = 看属性」（`Shared::show_properties`）；可定位的行摆
//! 「定位」按钮（判据 `database::nav_view::search_hit_locatable`，与导航结果行同一处）
//! → `Shared::request_reveal` 在导航树中定位。

use std::rc::Rc;
use std::time::Duration;

use database::model::{property_ref_of, PropertyRequest};
use database::nav_jobs::{self, SearchConsumer, SearchHit, SearchKind, SearchTarget};
use database::nav_view::search_hit_locatable;
use engine::ObjectRef;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::Theme;
use gpui_kit::component::{ActiveTheme, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::EventEmitter;
use gpui_kit::*;

use crate::panels::Shared;
use crate::view::{LeftPanel, SidebarMode};

/// 一次元数据搜索的全局数据源（面板每次渲染从这里读）。
#[derive(Clone, Debug, Default)]
pub struct MetadataSearchView {
    /// 本次搜索词（回填方据此丢弃过期批次；tab 标题「搜索: query」）。
    pub query: String,
    /// 实际搜了的连接数（有缓存的那些；无缓存的连接不建文件、直接跳过）。
    pub searched: usize,
    /// 命中（已按连接分组，连接信息在每条的 `conn_id` / `conn_label` 上）。
    pub hits: Vec<SearchHit>,
}

/// 元数据搜索宿主：中央 tab 内单实例，渲染只读 `Shared::metadata_search`。
///
/// 面板自持结果回填泵（`cx.spawn` 60ms 轮询，关标签即销毁）；顶部搜索栏可改词
/// 独立查询（与侧栏 / Quick Open 不联动）。
pub struct MetadataSearchHostPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`BasePanel::on_added_to` 注入；重复打开时用于聚焦自身）。
    group: Option<gpui::WeakEntity<TabGroup>>,
    /// 顶部搜索栏输入框（懒建；独立查询用）。
    panel_input: Option<Entity<InputState>>,
    panel_sub: Option<Subscription>,
    /// 顶部搜索栏开关（Aa/ab/.*；第一版只对内容档生效，见 `nav_jobs::SearchKind`）。
    panel_case: bool,
    panel_regex: bool,
    panel_word: bool,
    /// 本 tab 的查询词（新建时捕获；多开语义：与全局最新一致才实时，否则用快照）。
    query: Option<String>,
    /// 本 tab 创建时刻的结果快照（旧 tab 不随新搜索变化）。
    snapshot: Option<MetadataSearchView>,
    /// 结果回填泵（60ms 轮询 `drain_search_results(Panel)` → `Shared`）。
    pump: Option<Task<()>>,
}

impl MetadataSearchHostPanel {
    pub fn new(shared: Shared, cx: &mut gpui::Context<Self>) -> Self {
        let (query, snapshot) = {
            let s = shared.metadata_search.borrow().clone();
            (s.as_ref().map(|v| v.query.clone()), s)
        };
        let mut this = Self {
            shared,
            focus_handle: cx.focus_handle(),
            group: None,
            panel_input: None,
            panel_sub: None,
            panel_case: false,
            panel_regex: false,
            panel_word: false,
            query,
            snapshot,
            pump: None,
        };
        this.ensure_pump(cx);
        this
    }

    /// 当前搜索结果查询词（tab 标题「搜索: query」；无结果回退「搜索」）。
    fn tab_label(&self, _cx: &App) -> String {
        self.query
            .as_ref()
            .map(|q| format!("搜索: {}", q))
            .unwrap_or_else(|| "搜索".to_string())
    }

    /// 本 tab 的查询词（WorkbenchView 多开判定：全局最新查询与任一存活面板相同则聚焦复用，否则新建）。
    pub fn query(&self) -> Option<String> {
        self.query.clone()
    }

    /// 启动结果回填泵（构造时一次；面板销毁即退出）。
    fn ensure_pump(&mut self, cx: &mut gpui::Context<Self>) {
        if self.pump.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| loop {
            match weak.update(cx, |this, cx| this.pump_results(cx)) {
                Ok(()) => {}
                // 视图已销毁：退出任务（否则会永远空转）
                Err(_) => return,
            }
            executor
                .timer(Duration::from_millis(60))
                .await;
        });
        self.pump = Some(task);
    }

    /// 取走本消费方的搜索结果 → 写全局态（过期批次丢弃；不碰 UI，只置脏标记由宿主重绘）。
    fn pump_results(&mut self, cx: &mut gpui::Context<Self>) {
        let results = nav_jobs::drain_search_results(SearchConsumer::Panel);
        if results.is_empty() {
            return;
        }
        for result in results {
            // 过期批次：词已经被改了（收尾交给当前词的那一批）
            if Some(&result.query) != self.query.as_ref() {
                continue;
            }
            self.shared.publish_metadata_search(
                Some(MetadataSearchView {
                    query: result.query,
                    searched: result.searched,
                    hits: result.hits,
                }),
                cx,
            );
        }
    }

    /// 顶部搜索栏独立查询：读面板自持输入 + 开关 → `enqueue_search(Panel)` →
    /// 面板泵回填 `Shared::metadata_search` → 本面板重绘（不联动侧栏 / Quick Open）。
    fn run_panel_search(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let query = self
            .panel_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            return;
        }
        // 多开：顶部查询词更新到本 tab（回填后全局 query 与面板一致 → 实时显示本 tab）。
        self.query = Some(query.clone());
        // 第一版只接内容档（FTS，注释 / 数据类型）；名称档与 Aa/ab/.* 开关语义留第二版（见 P0 拍板）。
        let kind = SearchKind::FullText;
        let targets: Vec<SearchTarget> = self
            .shared
            .connections
            .borrow()
            .iter()
            .map(|c| SearchTarget {
                conn_id: c.id.clone(),
                label: c.name.clone(),
                driver: c.driver.clone(),
            })
            .collect();
        let project_root = self
            .shared
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        nav_jobs::enqueue_search(
            SearchConsumer::Panel,
            kind,
            &query,
            project_root.as_deref(),
            targets,
        );
        self.shared.notify_host(cx);
        let _ = window;
    }
}

impl EventEmitter<PanelEvent> for MetadataSearchHostPanel {}

impl Focusable for MetadataSearchHostPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MetadataSearchHostPanel {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        // 多开语义：本 tab 查询词与全局最新一致 → 实时显示；否则显示创建时快照。
        let mut search = self.snapshot.clone();
        if let Some(s) = self.shared.metadata_search.borrow().clone() {
            if Some(&s.query) == self.query.as_ref() {
                search = Some(s);
            }
        }
        let Some(search) = search else {
            return div()
                .id("metadata-search-host").role(gpui::Role::Group).test_support()
                .size_full()
                .bg(theme.colors.background)
                .v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child("搜索"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("在顶部搜索框输入关键词回车，结果会显示在这里"),
                );
        };

        // ── 顶部搜索栏（输入框 + Aa/ab/.* 开关 + 序号 badge + ☰）──
        // 输入框懒建：首次取当前 query 为初值，之后为用户自主编辑区（不跟随任何入口）。
        if self.panel_input.is_none() {
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder("搜索元数据");
                if !search.query.is_empty() {
                    state.set_value(search.query.clone(), window, cx);
                }
                state
            });
            let sub = cx.subscribe_in(
                &input,
                window,
                |this, _e, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. }) {
                        this.run_panel_search(window, cx);
                    }
                },
            );
            self.panel_input = Some(input);
            self.panel_sub = Some(sub);
        }
        let panel_input = self.panel_input.clone();
        let (panel_case, panel_regex, panel_word) = (self.panel_case, self.panel_regex, self.panel_word);
        let entity = cx.entity();
        let toggle_panel_case = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.panel_case = !this.panel_case;
                    cx.notify();
                });
            }
        };
        let toggle_panel_regex = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.panel_regex = !this.panel_regex;
                    cx.notify();
                });
            }
        };
        let toggle_panel_word = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.panel_word = !this.panel_word;
                    cx.notify();
                });
            }
        };
        let word_btn = div()
            .id("md-es-word")
            .h_flex()
            .items_center()
            .justify_center()
            .px_1()
            .h_5()
            .rounded_sm()
            .cursor_pointer()
            .text_xs()
            .text_color(if panel_word { fg } else { muted })
            .when(panel_word, |this| this.bg(theme.colors.list_active))
            .when(!panel_word, move |s| s.hover(move |s| s.bg(theme.colors.list_hover)))
            .child("ab")
            .tooltip(|window, cx| {
                Tooltip::element(move |_, _| {
                    div().text_xs().child("全字匹配（第二版生效）")
                })
                .build(window, cx)
            })
            .on_click(toggle_panel_word);
        let case_btn = div()
            .id("md-es-case")
            .h_flex()
            .items_center()
            .justify_center()
            .px_1()
            .h_5()
            .rounded_sm()
            .cursor_pointer()
            .text_xs()
            .text_color(if panel_case { fg } else { muted })
            .when(panel_case, |this| this.bg(theme.colors.list_active))
            .when(!panel_case, move |s| s.hover(move |s| s.bg(theme.colors.list_hover)))
            .child(IconName::CaseSensitive)
            .tooltip(|window, cx| {
                Tooltip::element(move |_, _| {
                    div().text_xs().child("区分大小写（第二版生效）")
                })
                .build(window, cx)
            })
            .on_click(toggle_panel_case);
        let regex_btn = div()
            .id("md-es-regex")
            .h_flex()
            .items_center()
            .justify_center()
            .px_1()
            .h_5()
            .rounded_sm()
            .cursor_pointer()
            .text_xs()
            .text_color(if panel_regex { fg } else { muted })
            .when(panel_regex, |this| this.bg(theme.colors.list_active))
            .when(!panel_regex, move |s| s.hover(move |s| s.bg(theme.colors.list_hover)))
            .child(".*")
            .tooltip(|window, cx| {
                Tooltip::element(move |_, _| {
                    div().text_xs().child("使用正则表达式（第二版生效）")
                })
                .build(window, cx)
            })
            .on_click(toggle_panel_regex);
        let mut es_bar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .py_1()
            .border_b_1()
            .border_color(border);
        if let Some(input) = panel_input {
            es_bar = es_bar.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&input).w_full()),
            );
            es_bar = es_bar
                .child(case_btn)
                .child(word_btn)
                .child(regex_btn);
        } else {
            es_bar = es_bar.child(div().flex_1());
        }
        es_bar = es_bar
            // 命中数 badge（第一版不做滚动跟踪）。
            .child(
                div()
                    .id("md-es-badge")
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .text_xs()
                    .text_color(fg)
                    .bg(theme.colors.primary)
                    .child(search.hits.len().to_string()),
            )
            .child(
                Button::new("md-es-menu")
                    .ghost()
                    .small()
                    .icon(IconName::EllipsisVertical)
                    .tooltip("列表菜单：切换视图布局 / 导出结果（后续）")
                    .on_click(|_, _, _| {}),
            );

        let shared = self.shared.clone();
        // 点命中 → 打开属性面板（请求带全定位信息；与导航搜索结果同一去向）。
        let open_hit: Rc<dyn Fn(SearchHit, &mut Window, &mut App)> = {
            let shared = shared.clone();
            Rc::new(
                move |hit: SearchHit, window: &mut Window, app: &mut App| {
                    let Some(object) = ObjectRef::from_index_hit(
                        &hit.conn_id,
                        &hit.object_type,
                        &hit.object_name,
                        hit.parent_name.as_deref(),
                        hit.catalog.as_deref(),
                        hit.schema.as_deref(),
                    ) else {
                        return;
                    };
                    let request = PropertyRequest {
                        property: property_ref_of(&object),
                        conn_label: hit.conn_label.clone(),
                        driver: hit.driver.clone(),
                        type_id: None,
                        data: None,
                    };
                    shared.show_properties(request, window, app);
                },
            )
        };
        // 「在树中定位」→ 切左面板到数据源 + `Shared::request_reveal`（与 Quick Open 同口径）。
        let locate_hit: Rc<dyn Fn(SearchHit, &mut App)> = {
            let shared = shared.clone();
            Rc::new(
                move |hit: SearchHit, app: &mut App| {
                    let Some(object) = ObjectRef::from_index_hit(
                        &hit.conn_id,
                        &hit.object_type,
                        &hit.object_name,
                        hit.parent_name.as_deref(),
                        hit.catalog.as_deref(),
                        hit.schema.as_deref(),
                    ) else {
                        return;
                    };
                    shared.active_left.set(LeftPanel::Database);
                    shared.left_mode.set(SidebarMode::Expanded);
                    shared.request_reveal(object);
                    let _ = app;
                },
            )
        };
        // 面板内「关闭」→ 清空结果（面板显示空态）；彻底关标签走 tab ✕。
        let clear = {
            let shared = self.shared.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, app: &mut App| {
                shared.publish_metadata_search(None, app);
            }
        };
        let pending = nav_jobs::has_pending_search(SearchConsumer::Panel);

        let pane = render_metadata_search_pane(&search, &theme, &open_hit, &locate_hit, pending);
        div()
            .id("metadata-search-host").role(gpui::Role::Group).test_support()
            .size_full()
            .bg(theme.colors.background)
            .v_flex()
            .child(es_bar)
            .child(div().flex_1().min_h_0().child(pane))
            .child(render_footer(&search, &theme, pending, clear))
    }
}

/// 结果主体：连接分组 → 命中行；无命中给空态文案。
fn render_metadata_search_pane(
    search: &MetadataSearchView,
    theme: &Theme,
    open_hit: &Rc<dyn Fn(SearchHit, &mut Window, &mut App)>,
    locate_hit: &Rc<dyn Fn(SearchHit, &mut App)>,
    pending: bool,
) -> impl IntoElement {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let hover = theme.colors.list_hover;

    if search.hits.is_empty() {
        return div()
            .id("md-pane")
            .flex_1()
            .min_h_0()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(if pending { fg } else { muted })
                    .child(if pending {
                        format!("索引搜索：{}（搜索中…）", search.query)
                    } else {
                        format!("索引搜索：{}（无命中；已搜 {} 个有缓存的连接）", search.query, search.searched)
                    }),
            )
            .into_any_element();
    }

    // 按连接分组（保序：首次出现顺序即连接顺序）。
    let mut groups: Vec<(String, String, Vec<&SearchHit>)> = Vec::new();
    for hit in &search.hits {
        match groups.iter_mut().find(|(conn_id, _, _)| conn_id == &hit.conn_id) {
            Some((_, _, rows)) => rows.push(hit),
            None => groups.push((hit.conn_id.clone(), hit.conn_label.clone(), vec![hit])),
        }
    }

    let body = div()
        .id("md-pane")
        .flex_1()
        .min_h_0()
        .size_full()
        .overflow_y_scrollbar()
        .py_1();
    let mut body = body;
    for (conn_id, conn_label, rows) in groups {
        let header = div()
            .id(SharedString::from(format!("md-group-{conn_id}")))
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_0p5()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(fg)
            .child(conn_label.clone())
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(rows.len().to_string()),
            );
        body = body.child(header);
        for (row_ix, hit) in rows.iter().enumerate() {
            let hit_for_open = (*hit).clone();
            let hit_for_locate = (*hit).clone();
            let open_hit = open_hit.clone();
            let locate_hit = locate_hit.clone();
            let kind_label = engine::ObjectKind::parse_index_str(&hit.object_type)
                .map(|k| k.label())
                .unwrap_or("对象");
            let scope = match (&hit.schema, &hit.parent_name) {
                (Some(schema), Some(parent)) => format!("{schema}.{parent}"),
                (Some(schema), None) => schema.clone(),
                (None, Some(parent)) => parent.clone(),
                (None, None) => String::new(),
            };
            let mut row = div()
                .id(SharedString::from(format!(
                    "md-hit-{}-{}-{}",
                    conn_id, row_ix, hit.object_name
                )))
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .px_2()
                .h_6()
                .hover(|s| s.bg(hover))
                .cursor_pointer()
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(kind_label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(fg)
                        .text_ellipsis()
                        .child(hit.object_name.clone()),
                )
                .child(
                    div()
                        .flex_shrink_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(muted)
                        .text_ellipsis()
                        .child(scope),
                );
            // 内容档命中带 snippet（为什么命中）——第一版只接内容档，展示归属 + 片段。
            if let Some(snippet) = hit.snippet.as_ref() {
                row = row.child(
                    div()
                        .flex_shrink_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(muted)
                        .text_ellipsis()
                        .child(snippet.clone()),
                );
            }
            // 「定位」只在**真能定位**时摆出（判据与状态机同一处：`search_hit_locatable`）——
            // 摆了却点了没反应，比不摆更伤。
            if search_hit_locatable(hit) {
                row = row.child(
                    div()
                        .id(format!("md-locate-{conn_id}-{row_ix}"))
                        .flex_none()
                        .h_5()
                        .px_1p5()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .text_xs()
                        .text_color(theme.colors.primary)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .on_click(move |_, _, app| {
                            // 行本身的单击是「看属性」：点「定位」不该同时把属性面板也推出来。
                            app.stop_propagation();
                            locate_hit(hit_for_locate.clone(), app);
                        })
                        .child("定位"),
                );
            }
            let hit_for_open = hit_for_open.clone();
            let open_hit = open_hit.clone();
            row = row.on_click(move |_, window, app| {
                open_hit(hit_for_open.clone(), window, app);
            });
            body = body.child(row);
        }
    }
    body.into_any_element()
}

/// 底部状态行：搜索中 / 已搜连接数 + 关闭按钮。
fn render_footer(
    search: &MetadataSearchView,
    theme: &Theme,
    pending: bool,
    clear: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let fg = theme.colors.foreground;
    let muted = theme.colors.muted_foreground;
    let border = theme.colors.border;
    let status = if pending {
        "搜索中…".to_string()
    } else {
        format!("{} 条命中 · 覆盖 {} 个连接", search.hits.len(), search.searched)
    };
    div()
        .id("md-footer")
        .h_flex()
        .items_center()
        .justify_between()
        .px_2()
        .h_6()
        .border_t_1()
        .border_color(border)
        .child(div().text_xs().text_color(if pending { fg } else { muted }).child(status))
        .child(
            Button::new("md-clear")
                .ghost()
                .small()
                .label("关闭")
                .tooltip("关闭结果（保留标签页）")
                .on_click(clear),
        )
}

impl BasePanel for MetadataSearchHostPanel {
    fn panel_name(&self) -> &'static str {
        "metadata-search"
    }

    fn on_added_to(
        &mut self,
        group: gpui::WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        self.group = Some(group);
    }

    /// 面板被 dock 移除（tab ✕ 关闭 / 布局替换）→ 置「已关闭」标志：
    /// 关标签**保留结果**（`Shared::metadata_search` 不清空），`WorkbenchView::render`
    /// 据此不再自动重建；再次搜索时 `publish_metadata_search(Some)` 清标志 → 重建并聚焦。
    fn on_removed(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) {
        *self.shared.metadata_search_dismissed.borrow_mut() = true;
    }
}

impl ComponentPanel for MetadataSearchHostPanel {
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.tab_label(cx).into())
    }

    fn title(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let label = self.tab_label(cx);
        div().text_sm().font_weight(FontWeight::MEDIUM).child(label)
    }
}

/// 把元数据搜索 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// 与 `focus_search_tab` 同因：必须在**实体更新之外**调用（`TabGroup` 切 tab 会回读
/// 面板实体，在更新闭包内调用会 double lease panic）。选中按面板 `entity_id` 定位。
pub fn focus_metadata_search_tab(
    panel: &gpui::Entity<MetadataSearchHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
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

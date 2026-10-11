//! 草稿箱内容搜索结果标签页（「搜索: query」，VS Code 全局搜索结果标签页形态）。
//!
//! 数据链路（2026-10-10 重构，VSCode 对齐）：
//! `ScratchpadHost::show_search_results` → `Shared::show_scratchpad_search`
//! （**不再**投递给数据源连接面板）→ `Shared::scratchpad_search` 存全局状态 +
//! `notify_host` → `WorkbenchView::render` 检测有结果且本面板未建 →
//! `cx.defer_in` 建面板并加入中央 tab 组（`tab_name = "搜索: query"`）→
//! 面板每次渲染从 `Shared::scratchpad_search` 读最新结果。
//!
//! 与属性面板同例：面板实体只建一次（`Shared::search_results_panel` 弱句柄），
//! 重复搜索结果只聚焦 + 重绘，不重建。面板内「关闭」→ 清空结果（面板显示空态）；
//! tab ✕ 由 dock 的 Close 事件移除面板。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::EventEmitter;
use gpui_kit::*;

use super::shared::Shared;

/// 搜索结果宿主：中央 tab 内单实例，渲染只读 `Shared::scratchpad_search`。
///
/// 2026-10-11（C1，用户裁决「不联动、可独立查询」）：顶部新增搜索栏（可改词输入框 +
/// Aa/ab/.* 开关 + 序号 badge + ☰），输入框 Enter 发起**独立查询**
/// （`scratchpad::jobs::enqueue_search`，结果仍走侧栏 pump → `Shared::scratchpad_search`
/// 回填 → 面板重绘）；输入框只首次取当前 query 为初值，之后不跟随侧栏。
pub struct SearchResultsHostPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`BasePanel::on_added_to` 注入；重复打开时用于聚焦自身）。
    group: Option<gpui::WeakEntity<TabGroup>>,
    /// 顶部搜索栏输入框（懒建；独立查询用，不联动侧栏）。
    panel_input: Option<Entity<InputState>>,
    panel_sub: Option<Subscription>,
    /// 顶部搜索栏开关（Aa/ab/.*）。
    panel_case: bool,
    panel_regex: bool,
    panel_word: bool,
}

impl SearchResultsHostPanel {
    pub fn new(shared: Shared, cx: &mut gpui::Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            group: None,
            panel_input: None,
            panel_sub: None,
            panel_case: false,
            panel_regex: false,
            panel_word: false,
        }
    }

    /// 当前搜索结果查询词（tab 标题「搜索: query」；无结果回退「搜索」）。
    fn tab_label(&self, _cx: &App) -> String {
        self.shared
            .scratchpad_search
            .borrow()
            .as_ref()
            .map(|s| format!("搜索: {}", s.query.clone()))
            .unwrap_or_else(|| "搜索".to_string())
    }
}

impl EventEmitter<PanelEvent> for SearchResultsHostPanel {}

impl Focusable for SearchResultsHostPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SearchResultsHostPanel {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        // 无结果（面板打开但搜索被清空）：空态占位。
        let search = self.shared.scratchpad_search.borrow().clone();
        let Some(search) = search else {
            return div()
                .id("search-results-host")
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
                        .child("在草稿箱搜索框输入关键词回车，结果会显示在这里"),
                );
        };

        // ── C1 顶部搜索栏（原型 es-bar，2026-10-11 用户裁决「不联动、可独立查询」）──
        // 输入框懒建：首次取当前 query 为初值，之后为用户自主编辑区（不跟随侧栏）；
        // Enter 发起独立查询（`scratchpad::jobs::enqueue_search` → 侧栏 pump → Shared 回填）。
        if self.panel_input.is_none() {
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder("搜索");
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
            .id("sp-es-word")
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
                    div().text_xs().child("全字匹配（ab 开关：只匹配完整单词）")
                })
                .build(window, cx)
            })
            .on_click(toggle_panel_word);
        let case_btn = div()
            .id("sp-es-case")
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
                    div().text_xs().child("区分大小写（Aa 开关）")
                })
                .build(window, cx)
            })
            .on_click(toggle_panel_case);
        let regex_btn = div()
            .id("sp-es-regex")
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
                    div().text_xs().child("使用正则表达式（如 \\d+ 匹配数字）")
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
            // 原型 es-bar（2026-10-11 用户对比 + 2026-10-11 全面对齐）：输入框 + **框外**
            // Aa/ab/.* 开关同行（VSCode 形态；开关不嵌输入框内）——与侧栏搜索行（框内）刻意
            // 区分。顺序按原型 Aa/ab/.*（case 先；2026-10-11 对齐侧栏）。
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
            // 当前匹配序号 badge（原型 es-bar；滚动跟踪后续）。
            .child(
                div()
                    .id("sp-es-badge")
                    .flex_none()
                    .px_1()
                    .rounded_sm()
                    .text_xs()
                    .text_color(fg)
                    .bg(theme.colors.primary)
                    .child("1"),
            )
            .child(
                Button::new("sp-es-menu")
                    .ghost()
                    .small()
                    .icon(IconName::EllipsisVertical)
                    .tooltip("列表菜单：切换视图布局 / 导出结果（后续）")
                    .on_click(|_, _, _| {}),
            );

        let shared = self.shared.clone();
        // 点命中 → 在编辑器里打开该草稿并跳到命中行（相对路径 → 模块内绝对路径）。
        let open_hit: Rc<dyn Fn(
            scratchpad::ScratchpadSearchHitTarget,
            &mut Window,
            &mut App,
        )> = {
            Rc::new(
                move |target: scratchpad::ScratchpadSearchHitTarget,
                      _window: &mut Window,
                      _app: &mut App| {
                    if let Some(root) = shared.project_root() {
                        let abs = root
                            .join(scratchpad::MODULE_DIR_NAME)
                            .join(target.file.as_str());
                        shared.request_open_in_editor_at(
                            abs,
                            crate::panels::shared::RevealTarget {
                                line: target.line,
                                span: target.span,
                            },
                        );
                    }
                },
            )
        };
        // 面板内「关闭」→ 清空结果（面板显示空态）；彻底关标签走 tab ✕。
        let clear = {
            let shared = self.shared.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut Window, app: &mut App| {
                *shared.scratchpad_search.borrow_mut() = None;
                shared.notify_host(app);
            }
        };
        let match_bg = settings::product_tokens::get(cx).search_match_background(&theme);

        let pane = scratchpad::scratchpad_view::render_scratchpad_search_pane(
            &search,
            &theme,
            match_bg,
            |name| scratchpad::scratchpad_view::scratchpad_type_icon(name).to_string(),
            open_hit,
            clear,
        );
        div()
            .id("search-results-host")
            .size_full()
            .bg(theme.colors.background)
            .v_flex()
            .child(es_bar)
            .child(div().flex_1().min_h_0().child(pane))
    }
}

impl SearchResultsHostPanel {
    /// C1 顶部搜索栏独立查询：读面板自持输入 + 开关 → `enqueue_search` →
    /// 侧栏 pump 回填 `Shared::scratchpad_search` → 本面板重绘（不联动侧栏输入框）。
    fn run_panel_search(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let query = self
            .panel_input
            .as_ref()
            .map(|i| i.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            return;
        }
        let (case, regex, word) = (self.panel_case, self.panel_regex, self.panel_word);
        let Some(root) = self.shared.project_root() else {
            return;
        };
        scratchpad::jobs::enqueue_search(&root, &query, case, regex, word, None, None);
        self.shared.notify_host(cx);
        let _ = window;
    }
}

impl BasePanel for SearchResultsHostPanel {
    fn panel_name(&self) -> &'static str {
        "search-results"
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
    /// 关闭语义（2026-10-10 用户拍板）＝ 关标签页**保留结果**（`Shared::scratchpad_search`
    /// 不清空），`WorkbenchView::render` 据此不再自动重建；再次搜索时
    /// `show_scratchpad_search(Some)` 清标志 → 重建并聚焦。
    fn on_removed(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) {
        *self.shared.search_results_dismissed.borrow_mut() = true;
    }
}

impl ComponentPanel for SearchResultsHostPanel {
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(self.tab_label(cx).into())
    }

    fn title(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let label = self.tab_label(cx);
        div().text_sm().font_weight(FontWeight::MEDIUM).child(label)
    }
}

/// 把搜索 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**（与 `focus_property_tab` 同因：`TabGroup` 切 tab 会回读
/// 面板实体，在更新闭包内调用会 double lease panic）。选中按面板 `entity_id` 定位
///（`TabGroup::activate_item` 不存在，走 `panels()` 查找 + `select_tab`，同 property_panel）。
pub fn focus_search_tab(
    panel: &gpui::Entity<SearchResultsHostPanel>,
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

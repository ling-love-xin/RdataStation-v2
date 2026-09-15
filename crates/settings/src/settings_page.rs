//! rds-settings — 设置页（两栏：分节导航 + 内容区）。
//!
//! 形态与规格见 `docs/architecture/settings/settings-prototype-design.md`：
//! 标题行 → 搜索行 → 左分节导航（12.5rem）+ 右内容区（内部滚动）→ 底栏状态条。
//!
//! ## 三条硬约束（改这个文件前先读）
//!
//! 1. **行只能来自登记表**：分节用 `registry::sections()`、行用 `registry::page_rows()`。
//!    页面不自己排顺序、不自己加设置行——想加一项，先过
//!    `docs/architecture/settings/settings-architecture.md` §7.1 的准入五条。
//! 2. **写只走 `SettingsService::apply_by_key`**（唯一写入者）：页面不直接改 model，也不落盘。
//! 3. **render 是纯读路径**：不读盘、不写盘、不改状态；副作用只在事件路径（点击 / 输入）。
//!
//! 「缓存管理…」是**动作行**（不是设置项：没有 key / 默认值 / 生效方式），
//! 因此不在登记表里，由页面按节硬编码一行，副作用经宿主桥 `SettingsHost` 回调。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::IndexPath;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Size, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::model::Settings;
use crate::registry::{self, SettingKind, SettingSpec, SettingValue};
use crate::ui;
use crate::{SettingsService, value_by_key};

/// 宿主桥：页面的副作用由宿主提供。
///
/// 页面**不**依赖 engine / 具体对话框（依赖只向下）：缓存对话框要 engine（rusqlite +
/// 元数据缓存清理），引入它会把数据库依赖拖进视图 crate。
pub struct SettingsHost {
    /// 关闭设置页（宿主把 overlay 关掉并重绘）。
    pub on_close: Rc<dyn Fn(&mut App)>,
    /// 打开「缓存管理」对话框（宿主实现）。
    pub on_open_cache: Rc<dyn Fn(&mut Window, &mut App)>,
}

/// 设置页实体（宿主把本实体放在 overlay 层渲染）。
pub struct SettingsPage {
    /// 设置快照：每次写入后由 `SettingsService` 重读（不维护第二份真相）。
    settings: Settings,
    host: SettingsHost,
    /// 分节导航：hover / 选中 / 键盘由组件（`List`）负责。
    nav: Entity<ListState<SectionNav>>,
    /// 搜索框。
    query: Entity<InputState>,
    /// 当前搜索词（由 `InputEvent::Change` 维护；**不是**每帧去读输入框）。
    filter: String,
    /// 订阅句柄（仅持有；释放即取消）。
    _query_sub: Option<Subscription>,
}

impl SettingsPage {
    /// 创建页面。需要 `Window`：搜索框与列表状态都要窗口上下文。
    pub fn new(window: &mut Window, host: SettingsHost, cx: &mut Context<Self>) -> Self {
        let nav = {
            let page = cx.entity().downgrade();
            let delegate = SectionNav::new(registry::sections(), page);
            cx.new(|cx| ListState::new(delegate, window, cx))
        };
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设置…"));
        let sub = cx.subscribe_in(
            &query,
            window,
            |this: &mut Self, emitter, ev: &InputEvent, _window, cx| {
                if !matches!(ev, InputEvent::Change) {
                    return;
                }
                this.filter = emitter.read(cx).value().to_string();
                cx.notify();
            },
        );
        Self {
            settings: SettingsService::get(cx),
            host,
            nav,
            query,
            filter: String::new(),
            _query_sub: Some(sub),
        }
    }

    /// 重读设置快照（写入之后调用；页面只有这一个数据来源）。
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.settings = SettingsService::get(cx);
        cx.notify();
    }

    /// 某项当前值是否偏离默认值（决定「恢复默认」按钮出不出现）。
    fn is_changed(&self, spec: &SettingSpec) -> bool {
        match (
            value_by_key(&self.settings, spec.key),
            spec.default_value(),
        ) {
            (Some(current), Some(default)) => current != default,
            _ => false,
        }
    }

    // ===== 标题行 / 搜索行 / 底栏 =====

    fn render_header(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme().clone();
        div()
            .h_flex()
            .w_full()
            .h(rems(ui::HEADER_HEIGHT))
            .flex_none()
            .pl_3()
            .pr_2()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.colors.foreground)
                    .child("设置"),
            )
            .child(Icon::new(IconName::Settings).size_4())
            .child(
                Button::new("settings-page-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::CircleX)
                    .on_click({
                        let on_close = self.host.on_close.clone();
                        move |_, _window, app| on_close(app)
                    }),
            )
    }

    fn render_search_row(&self) -> Div {
        div()
            .w_full()
            .flex_none()
            .px_2p5()
            .py_1p5()
            .child(Input::new(&self.query))
    }

    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        StatusBar::new().left(
            div()
                .pl_3()
                .text_xs()
                .text_color(theme.colors.primary_foreground)
                .child("修改即时生效并持久化到 settings.json"),
        )
    }

    // ===== 左侧：分节导航 =====

    fn render_nav(&self, _cx: &Context<Self>) -> Div {
        div()
            .w(rems(ui::NAV_WIDTH))
            .flex_none()
            .h_full()
            .v_flex()
            .child(List::new(&self.nav).flex_1())
    }

    // ===== 右侧：内容区 =====

    /// 当前节的内容（节标题 + 分组卡 + 行）。
    fn render_section_body(&self, active: &'static str, cx: &Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let rows: Vec<&'static SettingSpec> = registry::page_rows()
            .filter(|s| s.section == active)
            .collect();
        let changed = rows.iter().filter(|s| self.is_changed(s)).count();
        let entity = cx.entity();

        let head = div()
            .h_flex()
            .w_full()
            .h(rems(ui::SECTION_HEAD_HEIGHT))
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child(registry::section_label(active).unwrap_or("设置")),
            )
            .when(changed > 0, |d| {
                d.child(
                    Button::new("settings-reset-section")
                        .ghost()
                        .xsmall()
                        .label(format!("重置本节（{changed}）"))
                        .on_click({
                            let entity = entity.clone();
                            move |_, window, app| {
                                let defaults: Vec<(&'static str, SettingValue)> =
                                    registry::page_rows()
                                        .filter(|s| s.section == active)
                                        .filter_map(|s| {
                                            s.default_value().map(|v| (s.key, v))
                                        })
                                        .collect();
                                let mut applied = 0usize;
                                for (key, default) in defaults {
                                    if SettingsService::apply_by_key(
                                        key,
                                        default,
                                        Some(&mut *window),
                                        app,
                                    ) {
                                        applied += 1;
                                    }
                                }
                                let _ = applied;
                                entity.update(app, |this, cx| this.reload(cx));
                            }
                        }),
                )
            });

        let last = rows.len().saturating_sub(1);
        let mut card = div()
            .v_flex()
            .w_full()
            .rounded(rems(ui::CARD_RADIUS))
            .bg(theme.colors.group_box)
            .overflow_hidden();
        for (i, spec) in rows.iter().enumerate() {
            card = card.child(self.render_row(spec, None, i == last, cx));
        }
        // 动作行（不是设置项，见文件头注释）：只在「数据源导航」节出现。
        if active == "navigator" {
            card = card.child(self.render_action_row(cx));
        }

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .p(rems(ui::CARD_PADDING))
            .child(head)
            .child(card)
    }

    /// 搜索结果（跨节）：每条是「节 › 行」+ 控件；没有命中则给空态。
    fn render_search_body(&self, query_lower: &str, cx: &Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let hits: Vec<&'static SettingSpec> = registry::page_rows()
            .filter(|s| matches_query(s, query_lower))
            .collect();
        if hits.is_empty() {
            return div()
                .v_flex()
                .w_full()
                .h_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    Icon::new(IconName::Info).text_color(theme.colors.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.colors.foreground)
                        .child("没有匹配的设置项"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("这里只列已登记的设置项；模块面板内的开关不在此列"),
                );
        }
        let last = hits.len().saturating_sub(1);
        let mut card = div()
            .v_flex()
            .w_full()
            .rounded(rems(ui::CARD_RADIUS))
            .bg(theme.colors.group_box)
            .overflow_hidden();
        for (i, spec) in hits.iter().enumerate() {
            card = card.child(self.render_row(spec, Some(spec.section_label), i == last, cx));
        }
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .p(rems(ui::CARD_PADDING))
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("{} 项匹配", hits.len())),
            )
            .child(card)
    }

    /// 一行设置：标签列（15rem）+ 说明行 + 控件列（右对齐）+ 恢复默认。
    ///
    /// `breadcrumb` 非空时用于搜索结果（标签列改显「节 › 行」）。
    fn render_row(
        &self,
        spec: &'static SettingSpec,
        breadcrumb: Option<&'static str>,
        is_last: bool,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme().clone();
        let current = value_by_key(&self.settings, spec.key);
        let changed = self.is_changed(spec);
        let label = match breadcrumb {
            Some(section) => format!("{section} › {}", spec.label),
            None => spec.label.to_string(),
        };

        // 恢复默认（仅在偏离默认值时出现）；默认值直接取自登记表，不另存一份。
        let reset = (changed)
            .then(|| spec.default_value())
            .flatten()
            .map(|default| {
                let entity = cx.entity();
                let key = spec.key;
                Button::new(ElementId::Name(
                    SharedString::from(format!("{}-reset", spec.key)),
                ))
                .ghost()
                .xsmall()
                .icon(IconName::RotateCw)
                .on_click(move |_, window, app| {
                    commit(&entity, key, default.clone(), window, app)
                })
                .into_any_element()
            });

        div()
            .h_flex()
            .w_full()
            .min_h(rems(ui::ROW_MIN_HEIGHT))
            .gap_3()
            .px_3()
            .py_2()
            .items_center()
            .when(!is_last, |d| {
                d.border_b(ui::HAIRLINE).border_color(theme.colors.border)
            })
            .child(
                div()
                    .v_flex()
                    .w(rems(ui::LABEL_WIDTH))
                    .flex_none()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.muted_foreground)
                            .child(spec.hint),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .items_center()
                    .justify_end()
                    .child(self.render_control(spec, current, cx))
                    .when_some(reset, |d, reset| d.child(reset)),
            )
    }

    /// 动作行（不是设置项）：跳转到宿主提供的副作用。
    fn render_action_row(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme().clone();
        div()
            .h_flex()
            .w_full()
            .min_h(rems(ui::ROW_MIN_HEIGHT))
            .gap_3()
            .px_3()
            .py_2()
            .items_center()
            .child(
                div()
                    .v_flex()
                    .w(rems(ui::LABEL_WIDTH))
                    .flex_none()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground)
                            .child("缓存"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.muted_foreground)
                            .child("动作行（不是设置项）：打开缓存对话框，清理元数据缓存"),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .justify_end()
                    .child(
                        Button::new("settings-open-cache")
                            .ghost()
                            .small()
                            .label("缓存管理…")
                            .on_click({
                                let on_open_cache = self.host.on_open_cache.clone();
                                move |_, window, app| on_open_cache(window, app)
                            }),
                    ),
            )
    }

    /// 控件：按登记表的形态选（开关 / 分段；数值必须是预设档）。
    fn render_control(
        &self,
        spec: &'static SettingSpec,
        current: Option<SettingValue>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let key = spec.key;
        match spec.kind {
            SettingKind::Bool => {
                let on = current.as_ref().and_then(SettingValue::as_bool).unwrap_or(false);
                Switch::new(key)
                    .checked(on)
                    .on_click(move |want: &bool, window, app| {
                        commit(&entity, key, SettingValue::Bool(*want), window, app)
                    })
                    .into_any_element()
            }
            SettingKind::BoolPair { on, off } => {
                let value = current.as_ref().and_then(SettingValue::as_bool).unwrap_or(false);
                segmented(
                    key,
                    vec![on, off],
                    if value { 0 } else { 1 },
                    move |ix, window, app| {
                        commit(&entity, key, SettingValue::Bool(ix == 0), window, app)
                    },
                )
            }
            SettingKind::Enum(options) => {
                let text = current
                    .as_ref()
                    .and_then(SettingValue::as_text)
                    .unwrap_or_default()
                    .to_string();
                let selected = options.iter().position(|(v, _)| *v == text).unwrap_or(0);
                segmented(
                    key,
                    options.iter().map(|(_, label)| *label).collect(),
                    selected,
                    move |ix, window, app| {
                        if let Some((value, _)) = options.get(ix) {
                            commit(
                                &entity,
                                key,
                                SettingValue::Text((*value).to_string()),
                                window,
                                app,
                            );
                        }
                    },
                )
            }
            SettingKind::Number => {
                let value = current.as_ref().and_then(SettingValue::as_number).unwrap_or(0.);
                let selected = spec
                    .presets
                    .iter()
                    .position(|(v, _)| *v as f64 == value)
                    .unwrap_or(0);
                segmented(
                    key,
                    spec.presets.iter().map(|(_, label)| *label).collect(),
                    selected,
                    move |ix, window, app| {
                        if let Some((v, _)) = spec.presets.get(ix) {
                            commit(&entity, key, SettingValue::Number(*v as f64), window, app);
                        }
                    },
                )
            }
            // 复合值不上页（`page_rows` 保证）；真出现了就明说，不画一个假控件。
            SettingKind::Composite => div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child("（复合值：请在对应面板内调整）")
                .into_any_element(),
        }
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let active = self.nav.read(cx).delegate().active_key();
        let filter = self.filter.trim().to_lowercase();
        let body = if filter.is_empty() {
            self.render_section_body(active, cx)
        } else {
            self.render_search_body(&filter, cx)
        };
        div()
            .v_flex()
            .w(rems(ui::PAGE_WIDTH))
            .h(rems(ui::PAGE_HEIGHT))
            .max_h_full()
            .rounded_lg()
            .bg(theme.colors.popover)
            .border_1()
            .border_color(theme.colors.border)
            .overflow_hidden()
            .child(self.render_header(cx))
            .child(div().w_full().h(ui::HAIRLINE).flex_none().bg(theme.colors.border))
            .child(self.render_search_row())
            .child(div().w_full().h(ui::HAIRLINE).flex_none().bg(theme.colors.border))
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.render_nav(cx))
                    .child(div().w(ui::HAIRLINE).h_full().flex_none().bg(theme.colors.border))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scrollbar()
                            .child(body),
                    ),
            )
            .child(self.render_footer(cx))
            .id(ElementId::Name("settings-page".into()))
            .debug_selector(|| "settings-page".to_string())
    }
}

// ===== 事件路径的写入（闭包共用） =====

/// 写入一项并刷新页面快照。
///
/// 注意：`Some(&mut *window)` 是对 `window` 的**重借用**——直接 `Some(window)` 会把
/// 可变引用移动进函数，后面的 `entity.update` 就用不成了。
fn commit(
    entity: &Entity<SettingsPage>,
    key: &'static str,
    value: SettingValue,
    window: &mut Window,
    app: &mut App,
) {
    if !SettingsService::apply_by_key(key, value, Some(&mut *window), app) {
        // 拒绝写入（未登记 / 形态不符）：不改快照，也不假装成功。
        return;
    }
    entity.update(app, |this, cx| this.reload(cx));
}

/// 搜索匹配（纯函数，便于单测）：**节名 / 标签 / 说明 / key** 任一命中即可。
fn matches_query(spec: &SettingSpec, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {} {}",
        spec.key, spec.label, spec.hint, spec.section_label
    );
    haystack.to_lowercase().contains(query_lower)
}

/// 分段控件（枚举 / 两态 / 数值预设档三种来源都汇到这里）。
fn segmented(
    key: &'static str,
    labels: Vec<&'static str>,
    selected: usize,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    TabBar::new(key)
        .segmented()
        // Small 档与原型 HTML 的分段控件等高
        .with_size(Size::Small)
        .flex_shrink_0()
        .selected_index(selected)
        .children(labels.into_iter().map(|label| Tab::new().label(label)))
        .on_click(move |ix: &usize, window, app| on_pick(*ix, window, app))
        .into_any_element()
}

// ===== 分节导航（List 组件：hover / 选中 / 键盘都由它给） =====

struct SectionNav {
    sections: Vec<(&'static str, &'static str)>,
    active: usize,
    page: WeakEntity<SettingsPage>,
}

impl SectionNav {
    fn new(sections: Vec<(&'static str, &'static str)>, page: WeakEntity<SettingsPage>) -> Self {
        Self {
            sections,
            active: 0,
            page,
        }
    }

    /// 当前节的 id（页面据此决定内容区渲染哪一节）。
    fn active_key(&self) -> &'static str {
        self.sections
            .get(self.active)
            .map(|(key, _)| *key)
            .unwrap_or("")
    }
}

impl ListDelegate for SectionNav {
    type Item = ListItem;

    /// 导航不做搜索（搜索在页面顶部，搜的是设置项而不是节名）。
    fn perform_search(
        &mut self,
        _query: &str,
        _window: &mut Window,
        _cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        Task::ready(())
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.sections.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let (_, label) = *self.sections.get(ix.row)?;
        let theme = cx.theme().clone();
        let active = self.active == ix.row;
        // 激活条：不激活时留同宽空位，避免文字左右跳动。
        let bar = div().flex_none().w(ui::ACTIVE_BAR).h(rems(1.0));
        let bar = if active {
            bar.bg(theme.colors.list_active_border)
        } else {
            bar
        };
        Some(
            ListItem::new(ix.row).child(
                div()
                    .h_flex()
                    .w_full()
                    .h(rems(ui::ROW_HEIGHT))
                    .gap_2()
                    .items_center()
                    .child(bar)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_ellipsis()
                            .text_color(if active {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(label),
                    ),
            ),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        if let Some(ix) = ix
            && self.active != ix.row
        {
            self.active = ix.row;
            // 页面据选中节重渲染内容区；页面不复存一份 active，避免两处漂移。
            // 页面已销毁（Err）时什么都不用做。
            let _ = self.page.update(cx, |_, cx| cx.notify());
        }
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let _ = self.page.update(cx, |_, cx| cx.notify());
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：逐项列举依赖（不通配导入，见 gpui-kit-dev skill「窗口测试」）
    use super::{SettingSpec, matches_query, value_by_key};
    use crate::model::Settings;
    use crate::registry::{self, KindTag};

    /// 搜索命中面：key / 标签 / 说明 / 节名任一命中；空词命中全部。
    #[test]
    fn search_covers_key_label_hint_and_section() {
        let spec: &'static SettingSpec = registry::spec("navigator.show_tags").expect("登记项");
        assert!(matches_query(spec, ""), "空词命中全部");
        assert!(matches_query(spec, "navigator.show_tags"), "key 可命中");
        assert!(matches_query(spec, "显示标签"), "标签可命中");
        assert!(matches_query(spec, "chip"), "说明可命中");
        assert!(matches_query(spec, "数据源导航"), "节名可命中");
        assert!(!matches_query(spec, "建连超时"), "无关词不命中");
    }

    /// 页面上出现的行 = 登记表里 `entry != Module` 的行（页面不得自加/自减）。
    #[test]
    fn page_rows_match_the_registry() {
        let expected = registry::REGISTRY
            .iter()
            .filter(|s| s.entry != registry::SettingEntry::Module)
            .count();
        assert_eq!(registry::page_rows().count(), expected);
        assert!(registry::page_rows().all(|s| s.kind.tag() != KindTag::Composite));
    }

    /// 「非默认值」判定与默认值来源一致：默认快照上没有任何一行显示「已修改」。
    #[test]
    fn default_snapshot_has_no_changed_rows() {
        let settings = Settings::default();
        for spec in registry::REGISTRY {
            let current = value_by_key(&settings, spec.key);
            let default = spec.default_value();
            match (current, default) {
                (Some(c), Some(d)) => assert_eq!(c, d, "`{}` 的默认快照应等于默认值", spec.key),
                (None, None) => {}
                (c, d) => panic!("`{}` 的读写覆盖不一致：{c:?} / {d:?}", spec.key),
            }
        }
    }
}

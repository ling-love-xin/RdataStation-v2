//! rds-settings — 字段适配：把 `registry` 的 `SettingKind` 映射为
//! gpui-kit `setting` 组件的 `SettingItem`。
//!
//! 行内控件复用 gpui-kit 组件（`Switch` / `TabBar::segmented`），经
//! `SettingField::<SharedString>::render` 嵌进设置骨架；每行 field 区追加
//! 「恢复默认」按钮（偏离默认值时出现），并给字段 `on_reset`，让页面级
//! reset-all 也能命中同一恢复路径。动作行 / 布局预览用 `SettingItem::Element`。
//!
//! 依赖只向下：只依赖 `settings` 自身 + gpui-kit，不引 engine / 对话框。
//! `render` 是纯读路径；写入只经 `bridge::write_scalar`（唯一写入路径）。

use std::rc::Rc;

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::setting::{RenderOptions, SettingField, SettingItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar, TabVariant};
use gpui_kit::component::{ActiveTheme, IconName, Sizable as _, Size, Selectable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::bridge;
use crate::registry::{SettingKind, SettingSpec, SettingValue};
use crate::settings_page::SettingsHost;
use crate::SettingsService;

/// 生成一个「值行」设置项（`Bool` / `BoolPair` / `Enum` / `Number`）。
///
/// 控件按 `spec.kind` 选（开关 / 分段预设档），读写经 `bridge` 统一接到
/// `apply_by_key`（唯一写入路径）；写后通知页面重读 + 通知宿主重应用。
pub fn value_item(
    spec: &'static SettingSpec,
    host: &SettingsHost,
    notify: &Rc<dyn Fn(&mut App)>,
) -> SettingItem {
    let notify_render = notify.clone();
    let host_render = host.on_settings_changed.clone();
    let notify_reset = notify.clone();
    let host_reset = host.on_settings_changed.clone();

    let field = SettingField::<SharedString>::render(move |options, window, cx| {
        let changed = bridge::is_changed(&SettingsService::get(cx), spec);
        let reset =
            changed.then(|| reset_button(spec, &notify_render, &host_render).into_any_element());
        let controls = div()
            .h_flex()
            .gap_2()
            .items_center()
            .child(render_control(
                spec,
                options,
                window,
                cx,
                &notify_render,
                &host_render,
            ))
            .when_some(reset, |row, btn| row.child(btn));
        // RdataStation vendor patch：标签栏样式实时预览（紧跟控件下方）。
        if spec.key == "appearance.tab_bar_variant" {
            div()
                .v_flex()
                .items_end()
                .gap_1p5()
                .child(controls)
                .child(tab_bar_preview(cx))
        } else {
            controls
        }
    })
    .on_reset(
        move |cx| bridge::is_changed(&SettingsService::get(cx), spec),
        move |window, cx| {
            bridge::reset_to_default(spec, Some(window), &notify_reset, &host_reset, cx);
        },
    );

    SettingItem::new(spec.label, field)
        .description(spec.hint)
        .disabled(spec.disabled)
        .keywords([SharedString::from(spec.key), SharedString::from(spec.section_label)])
}

/// 动作行（不是设置项，无 key / 默认值 / 生效方式）：跳转到宿主提供的副作用。
///
/// `label` / `hint` 是左侧说明，`button` 是右侧按钮文案，`id` 作元素 id。
pub fn action_row(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    button: &'static str,
    on_click: Rc<dyn Fn(&mut Window, &mut App)>,
) -> SettingItem {
    SettingItem::render(move |_options, _window, cx| {
        let theme = cx.theme().clone();
        div()
            .h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .text_ellipsis()
                            .text_color(theme.colors.foreground)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(theme.colors.muted_foreground)
                            .child(hint),
                    ),
            )
            .child(
                Button::new(id)
                    .ghost()
                    .small()
                    .label(button)
                    .on_click({
                        let on_click = on_click.clone();
                        move |_, window, app| on_click(window, app)
                    }),
            )
    })
    // 动作行永远不参与页面级 reset-all。
    .on_reset(|_| false, |_, _| {})
}

/// 布局分节顶部：左右侧边栏宽度占比的预览示意图。
pub fn layout_preview() -> SettingItem {
    SettingItem::render(move |_options, _window, cx| {
        let theme = cx.theme().clone();
        let settings = SettingsService::get(cx);
        let left_width = settings.layout.left_sidebar_width as f32;
        let right_width = settings.layout.right_sidebar_width as f32;
        // 中间区域宽度取固定值，仅用于计算占比（示意，不精确测量）。
        let center_width = 500.0;
        let total = left_width + right_width + center_width;
        let left_pct = (left_width / total * 100.0) as i32;
        let right_pct = (right_width / total * 100.0) as i32;
        let center_pct = (center_width / total * 100.0) as i32;

        div()
            .w_full()
            .h(rems(6.0))
            .flex_none()
            .h_flex()
            .items_stretch()
            .child(
                div()
                    .w(rems(left_pct as f32 * 0.1))
                    .flex_none()
                    .bg(theme.colors.sidebar)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("左侧\n{left_pct}%")),
            )
            .child(
                div()
                    .flex_1()
                    .bg(theme.colors.background)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("中间\n{center_pct}%")),
            )
            .child(
                div()
                    .w(rems(right_pct as f32 * 0.1))
                    .flex_none()
                    .bg(theme.colors.sidebar)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("右侧\n{right_pct}%")),
            )
    })
    // 示意图不参与 reset-all。
    .on_reset(|_| false, |_, _| {})
}

// ===== 行内控件 =====

/// 控件：按登记表的形态选（开关 / 分段预设档；数值必须是预设档）。
fn render_control(
    spec: &'static SettingSpec,
    options: &RenderOptions,
    _window: &mut Window,
    cx: &App,
    notify: &Rc<dyn Fn(&mut App)>,
    host_changed: &Rc<dyn Fn(&mut App)>,
) -> AnyElement {
    match spec.kind {
        SettingKind::Bool => {
            let on = bridge::current_value(cx, spec.key)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let notify = notify.clone();
            let host_changed = host_changed.clone();
            Switch::new(spec.key)
                .checked(on)
                // 置灰项（能力存在但暂不可用）：禁用开关，未来放开即用
                .disabled(options.is_disabled())
                .on_click(move |want: &bool, window, app| {
                    bridge::write_scalar(
                        spec.key,
                        SettingValue::Bool(*want),
                        Some(window),
                        &notify,
                        &host_changed,
                        app,
                    );
                })
                .into_any_element()
        }
        SettingKind::BoolPair { on, off } => {
            let value = bridge::current_value(cx, spec.key)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let selected = if value { 0 } else { 1 };
            let notify = notify.clone();
            let host_changed = host_changed.clone();
            segmented(
                spec.key,
                vec![on, off],
                selected,
                move |ix, window, app| {
                    bridge::write_scalar(
                        spec.key,
                        SettingValue::Bool(ix == 0),
                        Some(window),
                        &notify,
                        &host_changed,
                        app,
                    );
                },
            )
        }
        SettingKind::Enum(options) => {
            let text = bridge::current_value(cx, spec.key)
                .and_then(|v| v.as_text().map(|s| s.to_string()))
                .unwrap_or_default();
            let selected = options.iter().position(|(v, _)| *v == text).unwrap_or(0);
            let notify = notify.clone();
            let host_changed = host_changed.clone();
            segmented(
                spec.key,
                options.iter().map(|(_, label)| *label).collect(),
                selected,
                move |ix, window, app| {
                    if let Some((value, _)) = options.get(ix) {
                        bridge::write_scalar(
                            spec.key,
                            SettingValue::Text((*value).to_string()),
                            Some(window),
                            &notify,
                            &host_changed,
                            app,
                        );
                    }
                },
            )
        }
        SettingKind::Number => {
            let value = bridge::current_value(cx, spec.key)
                .and_then(|v| v.as_number())
                .unwrap_or(0.);
            let selected = spec
                .presets
                .iter()
                .position(|(v, _)| *v as f64 == value)
                .unwrap_or(0);
            let notify = notify.clone();
            let host_changed = host_changed.clone();
            segmented(
                spec.key,
                spec.presets.iter().map(|(_, label)| *label).collect(),
                selected,
                move |ix, window, app| {
                    if let Some((v, _)) = spec.presets.get(ix) {
                        bridge::write_scalar(
                            spec.key,
                            SettingValue::Number(*v as f64),
                            Some(window),
                            &notify,
                            &host_changed,
                            app,
                        );
                    }
                },
            )
        }
        // 复合值不上页（`page_rows` 保证）；真出现了就明说，不画假控件。
        SettingKind::Composite => unreachable!("page_rows 保证页面只渲染标量行"),
    }
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
        .with_size(Size::Small)
        .flex_shrink_0()
        .selected_index(selected)
        .children(labels.into_iter().map(|label| Tab::new().label(label)))
        .on_click(move |ix: &usize, window, app| on_pick(*ix, window, app))
        .into_any_element()
}

/// 恢复默认按钮（偏离默认值才出现）；默认值取自登记表，不另存一份。
fn reset_button(
    spec: &'static SettingSpec,
    notify: &Rc<dyn Fn(&mut App)>,
    host_changed: &Rc<dyn Fn(&mut App)>,
) -> Button {
    let notify = notify.clone();
    let host_changed = host_changed.clone();
    Button::new(ElementId::Name(SharedString::from(format!("{}-reset", spec.key))))
        .ghost()
        .xsmall()
        .icon(IconName::RotateCw)
        .on_click(move |_, window, app| {
            bridge::reset_to_default(spec, Some(window), &notify, &host_changed, app);
        })
}

/// RdataStation vendor patch：标签栏样式实时预览（`appearance.tab_bar_variant`）。
fn tab_bar_preview(cx: &App) -> Div {
    let theme = cx.theme().clone();
    let current = bridge::current_value(cx, "appearance.tab_bar_variant")
        .and_then(|v| v.as_text().map(|s| s.to_string()))
        .unwrap_or_else(|| "tab".to_string());
    let variant = match current.as_str() {
        "outline" => TabVariant::Outline,
        "pill" => TabVariant::Pill,
        "segmented" => TabVariant::Segmented,
        "underline" => TabVariant::Underline,
        "folder" => TabVariant::Folder,
        _ => TabVariant::Tab,
    };
    div()
        .w(rems(20.))
        .p_2()
        .rounded_md()
        .bg(theme.tokens.tab_bar)
        .border_1()
        .border_color(theme.colors.border)
        .child(
            TabBar::new("tab-preview")
                .with_variant(variant)
                .child(Tab::new().label("查询1").selected(true))
                .child(Tab::new().label("查询2"))
                .child(Tab::new().label("长标签名示例")),
        )
}

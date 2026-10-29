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
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::setting::{RenderOptions, SettingField, SettingItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar, TabVariant};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, Size, Selectable as _};
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
        let body: AnyElement = if spec.key == "appearance.tab_bar_variant" {
            div()
                .v_flex()
                .items_end()
                .gap_2()
                .child(controls)
                .child(tab_bar_preview(cx))
                .into_any_element()
        } else {
            controls.into_any_element()
        };
        // 好理解的参数不占行内说明空间：行内只留 label + 控件，说明挪到
        // 悬停控件时的 HoverCard 提示（对齐 gpui-kit tooltip 交互惯例）。
        // 禁用态（`spec.disabled` = 能力已预留）优先给"为何置灰"的原因提示；
        // 具体原因仍以行内 hint 为准（当前唯一禁用项 hint_inline=true 即走行内）。
        if spec.disabled && !spec.hint_inline {
            let key = spec.key;
            HoverCard::new(ElementId::Name(SharedString::from(format!("{key}-disabled"))))
                .trigger(body)
                .content(move |_, _window, cx| {
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child("该能力已预留，尚未启用")
                })
                .into_any_element()
        } else if spec.hint_inline {
            body.into_any_element()
        } else {
            let hint = spec.hint;
            HoverCard::new(ElementId::Name(SharedString::from(spec.key)))
                .trigger(body)
                .content(move |_, _window, cx| {
                    div()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(hint)
                })
                .into_any_element()
        }
    })
    .on_reset(
        move |cx| bridge::is_changed(&SettingsService::get(cx), spec),
        move |window, cx| {
            bridge::reset_to_default(spec, Some(window), &notify_reset, &host_reset, cx);
        },
    );

    let mut item = SettingItem::new(spec.label, field)
        .disabled(spec.disabled)
        .keywords([SharedString::from(spec.key), SharedString::from(spec.section_label)]);
    // 只有行内说明的才给组件 `.description`；悬停提示的由上面的 HoverCard 承担。
    if spec.hint_inline {
        item = item.description(spec.hint);
    }
    item
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

// ===== 类型样式矩阵（结果集面板「对齐 / 着色 / 背景色 / 字体样式」四组合并） =====

/// 把结果集面板「对齐」「着色」「背景色」「字体样式」四个登记组（各 5 项、
/// 共 20 个碎行）合并成一个表格元素：行 = 数据族（数字 / 文本 / 时间 / 布尔 /
/// BLOB），列 = 对齐、着色、背景、字体，每格一个紧凑下拉（`DropdownButton`）。
/// 解决「设置杂乱、无设计感」的根因——20 个 label + 分段条的行列排布压成
/// 1 个矩阵；也是对「类型设置分散」的收口：按数据族的样式设置只在一张表里。
///
/// 登记表 / 模型 / 读写路径**不动**（20 个 `SettingSpec` 原样保留，
/// `registry::spec` 仍是唯一权威）：这里是纯渲染层合并，写入仍走
/// `bridge::write_scalar`（唯一写入路径），页面级 reset 覆盖全部 20 项。
pub fn type_style_matrix(host: &SettingsHost, notify: &Rc<dyn Fn(&mut App)>) -> SettingItem {
    const ROWS: [(&str, &str, &str, &str, &str); 5] = [
        (
            "result_panel.align_numeric",
            "result_panel.color_numeric",
            "result_panel.bg_numeric",
            "result_panel.font_numeric",
            "数字",
        ),
        (
            "result_panel.align_text",
            "result_panel.color_text",
            "result_panel.bg_text",
            "result_panel.font_text",
            "文本",
        ),
        (
            "result_panel.align_temporal",
            "result_panel.color_temporal",
            "result_panel.bg_temporal",
            "result_panel.font_temporal",
            "时间",
        ),
        (
            "result_panel.align_bool",
            "result_panel.color_bool",
            "result_panel.bg_bool",
            "result_panel.font_bool",
            "布尔",
        ),
        (
            "result_panel.align_blob",
            "result_panel.color_blob",
            "result_panel.bg_blob",
            "result_panel.font_blob",
            "BLOB",
        ),
    ];
    const ALIGN_OPTIONS: [(&str, &str); 4] = [
        ("inherit", "继承默认"),
        ("left", "左对齐"),
        ("center", "居中"),
        ("right", "右对齐"),
    ];
    const COLOR_OPTIONS: [(&str, &str); 7] = [
        ("inherit", "不染色"),
        ("info", "信息蓝"),
        ("success", "成功绿"),
        ("warning", "警告橙"),
        ("danger", "危险红"),
        ("primary", "主品牌"),
        ("muted", "弱化灰"),
    ];
    const BG_OPTIONS: [(&str, &str); 7] = [
        ("inherit", "无背景"),
        ("info", "信息蓝浅底"),
        ("success", "成功绿浅底"),
        ("warning", "警告橙浅底"),
        ("danger", "危险红浅底"),
        ("primary", "主品牌浅底"),
        ("muted", "弱化灰浅底"),
    ];
    const FONT_OPTIONS: [(&str, &str); 5] = [
        ("inherit", "继承默认"),
        ("bold", "粗体"),
        ("italic", "斜体"),
        ("underline", "下划线"),
        ("line_through", "删除线"),
    ];
    let notify_render = notify.clone();
    let host_render = host.on_settings_changed.clone();
    let notify_reset = notify.clone();
    let host_reset = host.on_settings_changed.clone();

    SettingItem::render(move |_options, _window, cx| {
        let theme = cx.theme().clone();
        // 表头：行标签列（留空）+ 对齐 / 着色 / 背景 / 字体四列。
        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(theme.colors.border)
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.colors.muted_foreground)
            .child(div().w(rems(4.)).flex_none())
            .child(div().w(rems(9.)).flex_none().child("对齐"))
            .child(div().w(rems(10.)).flex_none().child("着色"))
            .child(div().w(rems(10.)).flex_none().child("背景"))
            .child(div().w(rems(9.)).flex_none().child("字体"));
        let mut body = div().v_flex();
        for (align_key, color_key, bg_key, font_key, name) in ROWS {
            body = body.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .child(
                        div()
                            .w(rems(4.))
                            .flex_none()
                            .text_xs()
                            .text_color(theme.colors.foreground)
                            .child(name),
                    )
                    .child(matrix_cell(
                        align_key,
                        &ALIGN_OPTIONS,
                        MatrixSwatch::None,
                        cx,
                        &notify_render,
                        &host_render,
                    ))
                    .child(matrix_cell(
                        color_key,
                        &COLOR_OPTIONS,
                        MatrixSwatch::Solid,
                        cx,
                        &notify_render,
                        &host_render,
                    ))
                    .child(matrix_cell(
                        bg_key,
                        &BG_OPTIONS,
                        MatrixSwatch::Light,
                        cx,
                        &notify_render,
                        &host_render,
                    ))
                    .child(matrix_cell(
                        font_key,
                        &FONT_OPTIONS,
                        MatrixSwatch::FontStyle,
                        cx,
                        &notify_render,
                        &host_render,
                    )),
            );
        }
        div()
            .v_flex()
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(theme.colors.border)
            .child(header)
            .child(body)
    })
    .on_reset(
        move |cx| {
            ROWS.iter().any(|(align_key, color_key, bg_key, font_key, _)| {
                matrix_changed(align_key, cx)
                    || matrix_changed(color_key, cx)
                    || matrix_changed(bg_key, cx)
                    || matrix_changed(font_key, cx)
            })
        },
        move |window, cx| {
            for (align_key, color_key, bg_key, font_key, _) in ROWS {
                matrix_reset(align_key, Some(window), &notify_reset, &host_reset, cx);
                matrix_reset(color_key, Some(window), &notify_reset, &host_reset, cx);
                matrix_reset(bg_key, Some(window), &notify_reset, &host_reset, cx);
                matrix_reset(font_key, Some(window), &notify_reset, &host_reset, cx);
            }
        },
    )
    .keywords(
        ROWS.iter().flat_map(|(align_key, color_key, bg_key, font_key, _)| {
            [
                SharedString::from(*align_key),
                SharedString::from(*color_key),
                SharedString::from(*bg_key),
                SharedString::from(*font_key),
                SharedString::from("对齐"),
                SharedString::from("着色"),
                SharedString::from("背景"),
                SharedString::from("字体"),
            ]
        }),
    )
}

/// 矩阵单元格的选项预览形态：按列语义选择（对齐列无预览；着色列 = 原色块；
/// 背景列 = 浅底色块；字体列 = 样式字例）。`inherit` 一律无色块。
#[derive(Clone, Copy, PartialEq)]
enum MatrixSwatch {
    None,
    Solid,
    Light,
    FontStyle,
}

/// 矩阵单元格：当前值 → 下拉按钮（左半区显示当前档位），选项点击 → 写设置。
///
/// 选项与写入值直接取自调用方传入的常量表（与登记表 `SettingSpec::kind` 的
/// 枚举保持一致）；当前值经 `bridge` 从 Settings 聚合读，选项未命中时显示首项。
fn matrix_cell(
    key: &'static str,
    options: &'static [(&'static str, &'static str)],
    swatch: MatrixSwatch,
    cx: &mut App,
    notify: &Rc<dyn Fn(&mut App)>,
    host_changed: &Rc<dyn Fn(&mut App)>,
) -> AnyElement {
    let text = bridge::current_value(cx, key)
        .and_then(|v| v.as_text().map(|s| s.to_string()))
        .unwrap_or_else(|| options[0].0.to_string());
    let current_label = options
        .iter()
        .find(|(v, _)| *v == text)
        .map(|(_, l)| *l)
        .unwrap_or(options[0].1);
    let notify = notify.clone();
    let host_changed = host_changed.clone();
    let theme = cx.theme().clone();
    DropdownButton::new(ElementId::Name(SharedString::from(format!("{key}-dd"))))
        .w(rems(9.))
        .flex_none()
        .button(
            Button::new(ElementId::Name(SharedString::from(format!("{key}-btn"))))
                .ghost()
                .xsmall()
                .label(current_label),
        )
        .menu_accessibility_label(current_label)
        .dropdown_menu(move |mut menu, _window, _app| {
            for (value, label) in options {
                // 每个菜单项独立持有写路径闭包（Rc 循环内逐项 clone，不能共享 move）。
                let notify = notify.clone();
                let host_changed = host_changed.clone();
                let mut item = PopupMenuItem::new(*label);
                if let Some(icon) = matrix_swatch(&theme, value, swatch) {
                    item = item.icon(icon);
                }
                menu = menu.item(
                    item.on_click(move |_, window, app| {
                        bridge::write_scalar(
                            key,
                            SettingValue::Text((*value).to_string()),
                            Some(window),
                            &notify,
                            &host_changed,
                            app,
                        );
                    }),
                );
            }
            menu
        })
        .into_any_element()
}

/// 矩阵选项预览：`swatch` 决定形态（见 `MatrixSwatch`）；色映射与登记表
/// 着色 / 背景枚举逐字一致。`inherit`（继承 / 不染色 / 无背景）无色块。
fn matrix_swatch(
    theme: &gpui_kit::component::Theme,
    value: &str,
    swatch: MatrixSwatch,
) -> Option<Icon> {
    match swatch {
        MatrixSwatch::None => None,
        MatrixSwatch::Solid => color_swatch(theme, value),
        MatrixSwatch::Light => bg_swatch(theme, value),
        MatrixSwatch::FontStyle => font_style_swatch(theme, value),
    }
}

/// 着色选项的语义色块：SVG 圆角矩形 + `currentColor`，经 `Icon::text_color`
/// 注入语义色（与 gpui-kit 图标渲染同一机制）。`inherit`（不染色）无色块。
fn color_swatch(theme: &gpui_kit::component::Theme, color_key: &str) -> Option<Icon> {
    const SWATCH_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="5" y="5" width="14" height="14" rx="4" fill="currentColor"/></svg>"#;
    let color = semantic_color(theme, color_key)?;
    Some(Icon::default().data(SWATCH_SVG).text_color(color))
}

/// 背景选项的浅底色块：`currentColor` 半透明填充 + 浅色描边，
/// 近似网格里的「语义角色浅底」观感。`inherit`（无背景）无色块。
fn bg_swatch(theme: &gpui_kit::component::Theme, color_key: &str) -> Option<Icon> {
    const SWATCH_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="4" fill="currentColor" opacity="0.3" stroke="currentColor" stroke-opacity="0.6"/></svg>"#;
    let color = semantic_color(theme, color_key)?;
    Some(Icon::default().data(SWATCH_SVG).text_color(color))
}

/// 字体选项的样式字例：SVG 文本按档位显示样式（粗体 / 斜体 / 下划线 /
/// 删除线），继承默认无色块——选项文案本身已自明。
fn font_style_swatch(theme: &gpui_kit::component::Theme, style: &str) -> Option<Icon> {
    let (svg, color): (&'static [u8], gpui_kit::Hsla) = match style {
        "bold" => (
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><text x="12" y="17" font-size="13" font-weight="bold" text-anchor="middle" fill="currentColor">A</text></svg>"#,
            theme.colors.foreground,
        ),
        "italic" => (
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><text x="12" y="17" font-size="13" font-style="italic" text-anchor="middle" fill="currentColor">A</text></svg>"#,
            theme.colors.foreground,
        ),
        "underline" => (
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><text x="12" y="17" font-size="13" text-decoration="underline" text-anchor="middle" fill="currentColor">A</text></svg>"#,
            theme.colors.foreground,
        ),
        "line_through" => (
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><text x="12" y="17" font-size="13" text-decoration="line-through" text-anchor="middle" fill="currentColor">A</text></svg>"#,
            theme.colors.foreground,
        ),
        _ => return None,
    };
    Some(Icon::default().data(svg).text_color(color))
}

/// 语义色映射（着色 / 背景共用；`inherit` 返回 None）。
fn semantic_color(theme: &gpui_kit::component::Theme, color_key: &str) -> Option<gpui_kit::Hsla> {
    match color_key {
        "info" => Some(theme.colors.info),
        "success" => Some(theme.colors.success),
        "warning" => Some(theme.colors.warning),
        "danger" => Some(theme.colors.danger),
        "primary" => Some(theme.colors.primary),
        "muted" => Some(theme.colors.muted_foreground),
        _ => None,
    }
}

/// 矩阵单格是否偏离默认值（默认值取自登记表，不另存一份）。
fn matrix_changed(key: &'static str, cx: &App) -> bool {
    let spec = crate::registry::spec(key).expect("矩阵 key 必须在登记表中");
    bridge::is_changed(&SettingsService::get(cx), spec)
}

/// 矩阵单格恢复默认（与单行字段同一恢复路径）。
fn matrix_reset(
    key: &'static str,
    window: Option<&mut Window>,
    notify: &Rc<dyn Fn(&mut App)>,
    host_changed: &Rc<dyn Fn(&mut App)>,
    cx: &mut App,
) {
    let spec = crate::registry::spec(key).expect("矩阵 key 必须在登记表中");
    bridge::reset_to_default(spec, window, notify, host_changed, cx);
}

// ===== 行内控件 =====

/// 控件：按登记表的形态选（开关 / 分段预设档；数值必须是预设档）。
fn render_control(
    spec: &'static SettingSpec,
    options: &RenderOptions,
    window: &mut Window,
    cx: &mut App,
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
        // 自由文本（如逗号分隔的关键字列表）：输入框 + 用户输入即写。
        // InputState 用 `use_keyed_state` 惰性创建并跨帧保留（每帧 `cx.new` 会丢焦点），
        // 订阅挂在 keyed state 的实体上（listener 的 `Context<T>` Deref 到 `App`，可写设置）。
        SettingKind::Text => {
            let current = bridge::current_value(cx, spec.key)
                .and_then(|v| v.as_text().map(|s| s.to_string()))
                .unwrap_or_default();
            let notify = notify.clone();
            let host_changed = host_changed.clone();
            let key = spec.key;
            let state = window.use_keyed_state(key, cx, move |window, cx| {
                let mut state = InputState::new(window, cx).placeholder(spec.hint);
                state.set_value(current.clone(), window, cx);
                // init 的 `cx` 是输入框实体的 `Context`：订阅自身 `Change`，
                // listener 的 `Context<T>` Deref 到 `App`，可直接写设置。
                let entity = cx.entity();
                let sub_entity = entity.clone();
                let _ = cx.subscribe(&sub_entity, move |_, _, _: &InputEvent, cx| {
                    let text = entity.read(cx).value().to_string();
                    bridge::write_scalar(
                        key,
                        SettingValue::Text(text),
                        None,
                        &notify,
                        &host_changed,
                        cx,
                    );
                });
                state
            });
            Input::new(&state)
                .flex_shrink_0()
                .disabled(options.is_disabled())
                .into_any_element()
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

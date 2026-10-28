//! 类型简称映射表对话框（数据源导航 → 设置 →「类型简称映射…」）。
//!
//! 维护「数据库类型 → 简称」映射（`type_id` → 简称，如 `mysql → MY`）。
//! 连接名称前的 `[简称]` 与徽标缩写共用这张表；弹窗里不展示驱动（driver）概念，
//! 只维护 类型 + 简称 两列。保存走 `SettingsService::set_type_short_map`（持久化 + 刷新）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::*;

use settings::model::default_type_short_map;
use settings::SettingsService;

use crate::components::dialog_helpers::standard_dialog_body;

/// type_id → 显示名（`mysql` → `MySQL`；未识别回退原串）。
fn type_display_name(type_id: &str) -> String {
    let mut chars = type_id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => type_id.to_string(),
    }
}

/// 行集合：默认表 + 当前表并集（保证常见类型始终出现；用户新增的也保留）。
fn collect_types(cx: &App) -> Vec<String> {
    let mut ids: Vec<String> = default_type_short_map().into_keys().collect();
    let current = SettingsService::get(cx).navigator.type_short_map;
    for k in current.keys() {
        if !ids.contains(k) {
            ids.push(k.clone());
        }
    }
    ids.sort();
    ids
}

/// 打开类型简称映射表对话框。
pub fn open_type_short_map_dialog(window: &mut Window, cx: &mut App) {
    let types = Rc::new(collect_types(cx));
    // 简称编辑行：type_id → 输入框实体（首次进入时随窗口创建并预填当前值）。
    let rows = Rc::new(RefCell::new(Vec::<(String, Entity<InputState>)>::new()));

    window.open_dialog(cx, move |dialog, window, cx| {
        // 首次进入：创建输入框并预填当前映射值。
        {
            let mut rows_borrow = rows.borrow_mut();
            if rows_borrow.is_empty() {
                let map = SettingsService::get(cx).navigator.type_short_map;
                for t in types.iter() {
                    let value = map
                        .get(t)
                        .cloned()
                        .unwrap_or_else(|| SettingsService::type_short(t, cx));
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("简称"));
                    input.update(cx, |state, ctx| state.set_value(&value, window, ctx));
                    rows_borrow.push((t.clone(), input));
                }
            }
        }

        let theme = cx.theme();

        let mut body = div().v_flex().w_full().gap_2().text_xs();
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("数据库类型 → 简称"),
                )
                .child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child("连接名称 `[简称]` 与徽标缩写共用"),
                ),
        );
        // 表头。
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .px_1()
                .text_color(theme.colors.muted_foreground)
                .child(div().flex_1().child("类型"))
                .child(div().w(rems(14.)).child("简称")),
        );
        // 行：类型名（只读）+ 简称输入。
        {
            let rows_borrow = rows.borrow();
            for (type_id, input) in rows_borrow.iter() {
                body = body.child(
                    div()
                        .h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(type_display_name(type_id)),
                        )
                        .child(
                            div()
                                .w(rems(14.))
                                .child(Input::new(input).bordered(true).w_full().text_xs()),
                        ),
                );
            }
        }

        let rows_primary = rows.clone();
        dialog
            .title("类型简称映射")
            .child(standard_dialog_body(body))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("type-short-map-save")
                            .label("保存")
                            .on_click(move |_, window, cx| {
                                let map = {
                                    let rows = rows_primary.borrow();
                                    rows.iter()
                                        .map(|(t, input)| {
                                            let v = input.read(cx).value().trim().to_string();
                                            (t.clone(), v)
                                        })
                                        .filter(|(_, v)| !v.is_empty())
                                        .collect::<std::collections::HashMap<_, _>>()
                                };
                                SettingsService::set_type_short_map(map, cx);
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("type-short-map-reset")
                            .ghost()
                            .label("恢复默认")
                            .on_click({
                                let rows = rows.clone();
                                move |_, window, cx| {
                                    let defaults = default_type_short_map();
                                    for (t, input) in rows.borrow().iter() {
                                        let v = defaults
                                            .get(t)
                                            .cloned()
                                            .unwrap_or_else(|| SettingsService::type_short(t, cx));
                                        input.update(cx, |state, ctx| {
                                            state.set_value(&v, window, ctx)
                                        });
                                    }
                                }
                            }),
                    )
                    .child(
                        Button::new("type-short-map-cancel")
                            .ghost()
                            .label("关闭")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

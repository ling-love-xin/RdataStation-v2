//! 已保存视图对话框（#10）：保存 / 管理两个覆盖层。
//!
//! 保存：名称必填（空名不提交、内联提示），**同名拦截**（`SettingsService::save_saved_view`
//! 返回 `Err` → 对话框保持打开并给一条 `danger` 提示，不覆盖、不自动改名）；
//! 保存内容 = 传入的当前 facet 快照（四项，全空视图由菜单侧语义禁止保存——调用方只在该
//! 快照非全空时提供入口，本对话框不再重复校验）。
//!
//! 管理：列出全部已保存视图（名称 + 筛选摘要），删除带确认（`on_ok` 返回 `bool`：
//! 失败保持打开 + 错误行）。数据就地重算（`Rc<RefCell<_>>`）+ `shared.notify_host(app)`，
//! 与缓存管理 / 管理分组对话框同一约定。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::{DialogButtonProps, DialogFooter};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use database::nav_host::NavFilters;
use settings::model::NavigatorFilters;

use crate::components::dialog_helpers::standard_dialog_body;
use crate::panels::Shared;

/// 筛选摘要文案（非空项拼 `source:x · type:y`；全空 = `全部`）。
///
/// 同时被导航菜单「已保存视图 ▸」与管理对话框使用，保持同一口径。
pub fn view_summary(filters: &NavigatorFilters) -> String {
    let mut parts = Vec::new();
    if let Some(s) = filters.source.as_deref() {
        parts.push(format!("source:{s}"));
    }
    if let Some(t) = filters.db_type.as_deref() {
        parts.push(format!("type:{t}"));
    }
    if let Some(d) = filters.driver.as_deref() {
        parts.push(format!("driver:{d}"));
    }
    if let Some(t) = filters.tag.as_deref() {
        parts.push(format!("tag:{t}"));
    }
    if parts.is_empty() {
        "全部".to_string()
    } else {
        parts.join(" · ")
    }
}

/// 打开「将当前筛选存为视图」对话框。
///
/// `filters` 是菜单构建瞬间的当前 facet 快照；保存成功后关闭对话框。
pub fn open_save_view_dialog(window: &mut Window, cx: &mut App, filters: NavFilters) {
    // 预览口径与持久化同源：`NavFilters`（database 侧）→ `NavigatorFilters`（settings 侧）。
    let snapshot = NavigatorFilters {
        source: filters.source,
        db_type: filters.db_type,
        driver: filters.driver,
        tag: filters.tag,
    };
    let preview = view_summary(&snapshot);

    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("视图名称"));
    let error = Rc::new(RefCell::new(None::<String>));

    let submit = {
        let name_input = name_input.clone();
        let error = error.clone();
        let snapshot = snapshot.clone();
        move |cx: &mut App| -> bool {
            let name = name_input.read(cx).value().trim().to_string();
            if name.is_empty() {
                *error.borrow_mut() = None;
                return false;
            }
            match settings::SettingsService::save_saved_view(name, snapshot.clone(), cx) {
                Ok(()) => {
                    *error.borrow_mut() = None;
                    true
                }
                Err(e) => {
                    // 同名拦截：错误行保持对话框打开，让用户改名。
                    *error.borrow_mut() = Some(e);
                    false
                }
            }
        }
    };
    // 对话框 builder 是 `Fn`（每帧重建），两份克隆在 builder 内部生成（参照 group_form_dialog）。
    let submit_for_frame = submit.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let name_empty = name_input.read(cx).value().trim().is_empty();
        let err = error.borrow().clone();
        let submit_ok = submit_for_frame.clone();
        let submit_key = submit_for_frame.clone();
        let mut body = div().v_flex().w_full().gap_2();
        body = body
            .child(field_label(theme, "名称"))
            .child(Input::new(&name_input))
            .when(name_empty, |this| this.child(hint_line(theme, "名称不能为空")));
        if let Some(msg) = err.as_ref() {
            body = body.child(hint_line(theme, msg));
        }
        // 预览：保存的就是这一组筛选（只动 facet，不含搜索框）。
        body = body
            .child(field_label(theme, "将保存的筛选"))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(preview.clone()),
            );
        dialog
            .title("将当前筛选存为视图")
            .child(standard_dialog_body(body))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("saved-view-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("saved-view-ok")
                            .primary()
                            .label("保存")
                            .on_click(move |_, window, cx| {
                                if submit_ok(cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            // Enter 确认 / Esc 取消（对话框 key context 自带绑定）。
            .on_ok(move |_, _window, cx| submit_key(cx))
            .on_cancel(|_, _, _| true)
    });
}

/// 打开「管理视图」对话框：全部视图行 + 删除（确认） + 关闭。
pub fn open_manage_views_dialog(window: &mut Window, cx: &mut App, shared: &Shared) {
    // 打开瞬间采集一次；删除回调里重新读 settings 重算。
    let rows = Rc::new(RefCell::new(collect_view_rows(cx)));
    let error = Rc::new(RefCell::new(None::<String>));
    let rows_build = rows.clone();
    let error_build = error.clone();
    let shared_build = shared.clone();

    window.open_sheet(cx, move |sheet, _window, cx| {
        let theme = cx.theme();
        let list = rows_build.borrow();
        let err = error_build.borrow();

        let mut body = div().v_flex().w_full().gap_2();
        if let Some(msg) = err.as_ref() {
            body = body.child(hint_line(theme, msg));
        }
        if list.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("暂无已保存视图。在导航面板 ⋯ 菜单「将当前筛选存为视图…」创建。"),
            );
        } else {
            for row in list.iter() {
                let rname = row.name.clone();
                let rsummary = row.summary.clone();
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(rname.clone()),
                        )
                        .child(
                            div()
                                .text_color(theme.colors.muted_foreground)
                                .child(rsummary),
                        )
                        // 删除：确认后删视图（settings 持久化）；失败保持对话框 + 错误行。
                        .child(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "saved-view-del-{rname}"
                            ))))
                            .ghost()
                            .small()
                            .label("删除")
                            .on_click({
                                let rname = rname.clone();
                                let rows = rows_build.clone();
                                let error = error_build.clone();
                                let shared = shared_build.clone();
                                move |_, window, app| {
                                    // Fn 闭包内克隆出局部变量，move 进内层（Fn）alert builder。
                                    let name_v = rname.clone();
                                    let rows_v = rows.clone();
                                    let error_v = error.clone();
                                    let shared_v = shared.clone();
                                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                                        // alert builder 也是 Fn：on_ok 需 move，先克隆成局部。
                                        let name2 = name_v.clone();
                                        let rows2 = rows_v.clone();
                                        let error2 = error_v.clone();
                                        let shared2 = shared_v.clone();
                                        alert
                                            .confirm()
                                            .title("删除视图")
                                            .description(format!(
                                                "确定删除已保存视图「{name2}」？当前筛选不受影响。"
                                            ))
                                            .button_props(
                                                DialogButtonProps::default()
                                                    .ok_text("删除")
                                                    .ok_variant(ButtonVariant::Danger)
                                                    .show_cancel(true),
                                            )
                                            .on_ok(move |_, _, app| {
                                                let ok = match settings::SettingsService::delete_saved_view(&name2, app) {
                                                    Ok(()) => {
                                                        *error2.borrow_mut() = None;
                                                        *rows2.borrow_mut() = collect_view_rows(app);
                                                        true
                                                    }
                                                    Err(e) => {
                                                        *error2.borrow_mut() =
                                                            Some(format!("删除视图失败：{e}"));
                                                        false
                                                    }
                                                };
                                                shared2.notify_host(app);
                                                ok
                                            })
                                    });
                                }
                            }),
                        ),
                );
            }
        }

        sheet
            .title("管理视图")
            .size(px(420.))
            .child(body)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("删除只移除视图定义，当前导航筛选不变。"),
            )
    });
}

/// 管理对话框的视图行（打开时从 settings 采集）。
struct ViewRow {
    name: String,
    summary: String,
}

fn collect_view_rows(cx: &App) -> Vec<ViewRow> {
    settings::SettingsService::saved_views(cx)
        .into_iter()
        .map(|v| ViewRow {
            summary: view_summary(&v.filters),
            name: v.name,
        })
        .collect()
}

/// 字段名（弱化色，与对话框内的输入框一组）。
fn field_label(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 校验提示（`danger`，不是普通说明）。
fn hint_line(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.danger)
        .child(text.to_string())
}

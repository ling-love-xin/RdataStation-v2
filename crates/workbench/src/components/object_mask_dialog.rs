//! 对象名过滤器对话框（B-1）：连接级与全局级共用一套结构。
//!
//! 连接级：include / exclude 两个多行 Textarea（每行一条 glob，空行忽略），
//! 保存 → `SettingsService::set_object_mask`；「清空」= 删除该连接规则（回到不过滤）。
//! 全局级：同结构，保存 → `set_global_mask`；「清空」= 空规则（不启用）。
//!
//! 对话框不读导航树（纯 settings 读写）；保存后由调用方刷新树 / 搜索命中区
//! （本对话框不持有导航视图句柄，刷新动作留在打开它的菜单回调里）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Theme, WindowExt as _};
use gpui_kit::*;

use settings::model::ObjectMaskRule;

/// 解析 Textarea 文本为 glob 列表（每行一条、trim、空行忽略）。
fn parse_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// 打开连接级「对象名过滤器」对话框。
///
/// `conn_id` 为空（全局入口用 `None`）时走全局规则；`conn_label` 用于标题文案。
pub fn open_object_mask_dialog(
    conn_id: Option<&str>,
    conn_label: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let rule = match conn_id {
        Some(id) => settings::SettingsService::object_mask(id, cx),
        None => settings::SettingsService::global_mask(cx),
    };
    let title = match (conn_id, conn_label) {
        (Some(_), Some(label)) => format!("对象名过滤器 · {label}"),
        _ => "全局对象名过滤器".to_string(),
    };

    let include_input = cx.new(|cx| {
        let mut state = TextareaState::new(window, cx).placeholder("每行一条 glob（如 orders*）");
        // set_value 内部 emit_events=false、清空 undo 栈：对话框打开初始化不触发 Change 副作用。
        state.set_value(rule.include.join("\n"), window, cx);
        state
    });
    let exclude_input = cx.new(|cx| {
        let mut state = TextareaState::new(window, cx).placeholder("排除（每行一条 glob）");
        state.set_value(rule.exclude.join("\n"), window, cx);
        state
    });
    // 保存 / 清空后是否关闭：保存成功即关；「清空」总是成功即关。
    let submitted = Rc::new(RefCell::new(false));

    // Entity 是引用计数句柄：克隆给保存闭包，原句柄保留给渲染层使用，避免被闭包 move。
    let include_for_save = include_input.clone();
    let exclude_for_save = exclude_input.clone();
    let save = {
        let submitted = submitted.clone();
        let conn_id = conn_id.map(str::to_string);
        move |cx: &mut App| -> bool {
            let rule = ObjectMaskRule {
                include: parse_lines(&include_for_save.read(cx).value()),
                exclude: parse_lines(&exclude_for_save.read(cx).value()),
                use_regex: false,
            };
            match &conn_id {
                Some(id) => settings::SettingsService::set_object_mask(id, Some(rule), cx),
                None => settings::SettingsService::set_global_mask(rule, cx),
            }
            *submitted.borrow_mut() = true;
            true
        }
    };
    let clear = {
        let conn_id = conn_id.map(str::to_string);
        move |cx: &mut App| -> bool {
            match &conn_id {
                Some(id) => settings::SettingsService::set_object_mask(id, None, cx),
                None => settings::SettingsService::set_global_mask(ObjectMaskRule::default(), cx),
            }
            true
        }
    };
    let save_for_frame = save.clone();
    let save_for_key = save.clone();
    let clear_for_frame = clear.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let save_ok = save_for_frame.clone();
        // on_ok 走外层 Fn 回调，捕获的保存闭包须在闭包体内再克隆，不能直接 move 外层变量。
        let save_ok_key = save_for_key.clone();
        let clear_ok = clear_for_frame.clone();
        let mut body = div().v_flex().w_full().gap_2();
        body = body
            .child(field_label(theme, "只显示（空 = 全部）"))
            .child(Textarea::new(&include_input).h(rems(5.0)))
            .child(field_label(theme, "排除（优先于只显示）"))
            .child(Textarea::new(&exclude_input).h(rems(5.0)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("规则只作用于对象名（表 / 视图 / 例程 / 序列 / 触发器），不影响库 / schema 层级。"),
            );
        dialog
            .title(title.clone())
            .child(body)
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("object-mask-clear")
                            .secondary()
                            .label("清空")
                            .on_click(move |_, window, cx| {
                                if clear_ok(cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("object-mask-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("object-mask-ok")
                            .primary()
                            .label("保存")
                            .on_click(move |_, window, cx| {
                                if save_ok(cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            .on_ok(move |_, _window, cx| save_ok_key(cx))
            .on_cancel(|_, _, _| true)
    });
    let _ = submitted; // 预留：后续如需「保存但保持打开」语义可在此读取。
}

/// 字段标签（与 saved_view_dialog 同款弱色小字）。
fn field_label(theme: &Theme, text: &str) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

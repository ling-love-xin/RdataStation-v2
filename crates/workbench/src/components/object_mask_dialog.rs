//! 对象名过滤器对话框（B-1）：连接级与全局级共用一套结构。
//!
//! 连接级：include / exclude 两个多行 Textarea（每行一条模式，空行忽略），
//! 「按正则匹配」开关决定 glob（`*` / `?`）还是正则（`(?i)` 大小写不敏感）；
//! 保存 → `SettingsService::set_object_mask`；「清空」= 删除该连接规则（回到不过滤）。
//! 全局级：同结构，保存 → `set_global_mask`；「清空」= 空规则（不启用，glob 档）。
//!
//! 对话框不读导航树（纯 settings 读写）；保存后由调用方刷新树 / 搜索命中区
//! （本对话框不持有导航视图句柄，刷新动作留在打开它的菜单回调里）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Theme, WindowExt as _};
use gpui_kit::*;

use settings::model::ObjectMaskRule;

/// 解析 Textarea 文本为模式列表（每行一条、trim、空行忽略）。
fn parse_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// include 输入框占位文案：glob / 正则两档。
fn include_placeholder(use_regex: bool) -> &'static str {
    if use_regex {
        "每行一条正则（如 ^ord.*）"
    } else {
        "每行一条 glob（如 orders*）"
    }
}

/// exclude 输入框占位文案：glob / 正则两档。
fn exclude_placeholder(use_regex: bool) -> &'static str {
    if use_regex {
        "排除（每行一条正则）"
    } else {
        "排除（每行一条 glob）"
    }
}

/// 由两个输入框文本与正则开关态构造保存规则（每行一条、trim、空行忽略）。
fn build_rule(include_raw: &str, exclude_raw: &str, use_regex: bool) -> ObjectMaskRule {
    ObjectMaskRule {
        include: parse_lines(include_raw),
        exclude: parse_lines(exclude_raw),
        use_regex,
    }
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

    // 正则开关受控态：初值取持久化规则；切换时写回并刷新占位文案与对话框层。
    let use_regex = Rc::new(RefCell::new(rule.use_regex));
    let include_input = cx.new(|cx| {
        let mut state =
            TextareaState::new(window, cx).placeholder(include_placeholder(rule.use_regex));
        // set_value 内部 emit_events=false、清空 undo 栈：对话框打开初始化不触发 Change 副作用。
        state.set_value(rule.include.join("\n"), window, cx);
        state
    });
    let exclude_input = cx.new(|cx| {
        let mut state =
            TextareaState::new(window, cx).placeholder(exclude_placeholder(rule.use_regex));
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
        let use_regex = use_regex.clone();
        move |cx: &mut App| -> bool {
            let rule = build_rule(
                &include_for_save.read(cx).value(),
                &exclude_for_save.read(cx).value(),
                *use_regex.borrow(),
            );
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
                div().h_flex().items_center().gap_2().child(
                    Switch::new("object-mask-use-regex")
                        .checked(*use_regex.borrow())
                        .label("按正则匹配")
                        .on_click({
                            let use_regex = use_regex.clone();
                            let include_input = include_input.clone();
                            let exclude_input = exclude_input.clone();
                            move |on, window, cx| {
                                let next = *on;
                                *use_regex.borrow_mut() = next;
                                // 占位文案随开关档切换；`set_placeholder` 内部 notify 输入框。
                                include_input.update(cx, |state, cx| {
                                    state.set_placeholder(include_placeholder(next), window, cx);
                                });
                                exclude_input.update(cx, |state, cx| {
                                    state.set_placeholder(exclude_placeholder(next), window, cx);
                                });
                                // 对话框闭包挂在宿主视图 render 的 dialog 层，`cx.notify()`
                                // 只标脏子树、刷不到本层；与 log_dialog 一致靠 refresh_windows
                                // 让窗口重建闭包，受控 `checked` 才会翻到新值。
                                cx.refresh_windows();
                            }
                        }),
                ),
            )
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

#[cfg(test)]
mod tests {
    // 显式列举依赖（**不通配导入**）：`super::*` 会把 `use gpui_kit::*` 带进来，
    // 于是 `#[test]` 解析成 gpui 的 `test` 宏，展开到递归上限——见 gpui-kit-dev skill。
    use super::{build_rule, exclude_placeholder, include_placeholder, parse_lines};
    use settings::model::ObjectMaskRule;

    /// 保存规则透传正则开关，逐行解析（trim、空行忽略）。
    #[test]
    fn build_rule_parses_lines_and_passes_through_regex_flag() {
        let regex = build_rule("orders*\n\n  customers*\n", "temp_*\n", true);
        assert_eq!(regex.include, vec!["orders*", "customers*"]);
        assert_eq!(regex.exclude, vec!["temp_*"]);
        assert!(regex.use_regex);

        let glob = build_rule("orders*", "temp_*", false);
        assert_eq!(glob.include, vec!["orders*"]);
        assert!(!glob.use_regex);
    }

    /// 占位文案随 glob / 正则两档切换，include 与 exclude 各自独立。
    #[test]
    fn placeholders_switch_between_glob_and_regex() {
        assert_eq!(include_placeholder(false), "每行一条 glob（如 orders*）");
        assert_eq!(include_placeholder(true), "每行一条正则（如 ^ord.*）");
        assert_eq!(exclude_placeholder(false), "排除（每行一条 glob）");
        assert_eq!(exclude_placeholder(true), "排除（每行一条正则）");
    }

    /// 「清空」走默认规则：正则开关重置回 glob 档；空文本解析为空列表。
    #[test]
    fn cleared_rule_defaults_to_glob_and_empty_lines() {
        assert!(!ObjectMaskRule::default().use_regex);
        assert!(parse_lines("").is_empty());
        assert!(parse_lines("  \n\t\n").is_empty());
    }
}

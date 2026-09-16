//! 取回（检出）对话框（P1.5）：给工作副本起个名、挑个去处，然后复制出去。
//!
//! 与归档相反，取回**不动本体**（只复制一份可写的工作副本），所以对话框不做二次确认那套
//! 仪式，但有两件事必须在动手前说清（原型 §4.2）：
//!
//! 1. **落到哪**：目标目录由宿主解析后只读展示——M6 不认识草稿箱的目录结构
//!    （`CheckoutRequest::dest_path` 由调用方给，依赖方向是 `scratchpad → analytics_resource`）；
//! 2. **改完会怎样**：提示"修改后再归档将生成 v{version+1}"，让"取回 → 改 → 再归档"这条
//!    闭环在用户点下去之前就是已知的（否则取回会被当成"下载一份"）。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

/// 打开取回对话框所需的全部信息（**宿主在事件路径备好**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutDialogSeed {
    /// 存档显示名（标题行引用它，让用户确认取的是哪一条）。
    pub resource_name: String,
    /// 默认文件名（宿主拼：`<显示名>（工作副本）<扩展名>`，见 [`suggest_work_copy_name`]）。
    pub file_name: String,
    /// 目标目录（绝对路径文案，只读展示）。
    pub target_dir_label: String,
    /// 取回时的存档版本（用于"再归档将生成 vN+1"的提示）。
    pub version: i32,
}

/// 表单的输入实体与勾选态（开窗前建好；测试可复核同一批）。
///
/// `Clone`：宿主与测试都要在开窗后继续持有它们（开窗会 `move` 一份进 builder）。
#[derive(Clone)]
pub struct CheckoutDialogInputs {
    pub file_name: Entity<InputState>,
    /// 「取回后打开」：builder 每帧重建，勾选态只能由外部持有。
    pub open_after: Rc<Cell<bool>>,
}

/// 对话框结果：文件名 + 是否顺手打开（**目标目录不回传**——那份由宿主握着，用户改不了）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutDialogResult {
    pub file_name: String,
    pub open_after: bool,
}

/// 工作副本的默认文件名：`<显示名>（工作副本）<扩展名>`（原型 §4.2）。
///
/// 扩展名单独给（不带点）：显示名里可能已经没有扩展名（"月报"），
/// 拼不出 `.sql` 的工作副本会让编辑器按未知类型打开。
pub fn suggest_work_copy_name(display_name: &str, extension: Option<&str>) -> String {
    match extension.filter(|ext| !ext.is_empty()) {
        Some(ext) => format!("{display_name}（工作副本）.{}", ext.trim_start_matches('.')),
        None => format!("{display_name}（工作副本）"),
    }
}

/// Windows 保留设备名（不含扩展名比较）：这些名字建不出文件，得在对话框里挡下。
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 文件名校验提示（`None` = 合法）。
///
/// 只做**能拦住"注定失败"的那几条**：空、路径分隔符与 Windows 非法字符、`.` / `..`、
/// 保留设备名。重名不在这里判——那是宿主的"自动改名避让"（原型 §4.2：重复取回自动改名）。
pub fn file_name_hint(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Some("文件名不能为空");
    }
    if name == "." || name == ".." {
        return Some("文件名不能是 . 或 ..");
    }
    if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Some("文件名不能包含 \\ / : * ? \" < > |");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    if RESERVED_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return Some("这是系统保留名，换一个");
    }
    None
}

/// 校验 + 取值；`None` = 不通过（对话框保持打开，内联提示已由渲染期给出）。
pub fn submit_checkout(inputs: &CheckoutDialogInputs, cx: &App) -> Option<CheckoutDialogResult> {
    let file_name = inputs.file_name.read(cx).value().trim().to_string();
    if file_name_hint(&file_name).is_some() {
        return None;
    }
    Some(CheckoutDialogResult {
        file_name,
        open_after: inputs.open_after.get(),
    })
}

/// 建表单实体并按 seed 预填。
pub fn build_inputs(
    window: &mut Window,
    cx: &mut App,
    seed: &CheckoutDialogSeed,
) -> CheckoutDialogInputs {
    let file_name = cx.new(|cx| InputState::new(window, cx).placeholder("工作副本文件名"));
    let value = seed.file_name.clone();
    file_name.update(cx, |state, cx| state.set_value(value, window, cx));
    CheckoutDialogInputs {
        file_name,
        // 默认勾上：取回的目的通常是"接着改"（改完再归档），不打开反而是多一步。
        open_after: Rc::new(Cell::new(true)),
    }
}

/// 打开取回对话框（宿主入口）。
///
/// `on_submit` 只在**校验通过**时回调一次，负责算最终落点（含重名避让）、复制并回执。
pub fn open_checkout_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: CheckoutDialogSeed,
    on_submit: impl Fn(CheckoutDialogResult, &mut App) + 'static,
) {
    let inputs = build_inputs(window, cx, &seed);
    open_checkout_dialog_with(window, cx, seed, inputs, on_submit);
}

/// 同上，但用调用方建好的输入实体（窗口测试从这条缝进来）。
pub fn open_checkout_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: CheckoutDialogSeed,
    inputs: CheckoutDialogInputs,
    on_submit: impl Fn(CheckoutDialogResult, &mut App) + 'static,
) {
    let CheckoutDialogInputs {
        file_name,
        open_after,
    } = inputs;
    let on_submit = Rc::new(on_submit);

    let submit: Rc<dyn Fn(&mut App) -> bool> = {
        let file_name = file_name.clone();
        let open_after = open_after.clone();
        let on_submit = on_submit.clone();
        Rc::new(move |cx: &mut App| -> bool {
            let inputs = CheckoutDialogInputs {
                file_name: file_name.clone(),
                open_after: open_after.clone(),
            };
            let Some(result) = submit_checkout(&inputs, cx) else {
                return false;
            };
            on_submit(result, cx);
            true
        })
    };
    let next_version = seed.version + 1;

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let hint = file_name_hint(&file_name.read(cx).value());
        let checked = open_after.get();
        let toggled = open_after.clone();
        // builder 是 `Fn`（每帧重建）：两条提交路径各拿一份**本帧的**副本（同 archive 对话框）。
        let submit_for_button = submit.clone();
        let submit_for_ok = submit.clone();

        dialog
            .title("取回（检出）")
            .w(cx.theme().font_size * ui::CHECKOUT_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .gap_2()
                            .child(readonly_row(
                                theme,
                                "存档",
                                &format!("{}（v{}）", seed.resource_name, seed.version),
                            ))
                            .child(readonly_row(theme, "目标目录", &seed.target_dir_label)),
                    )
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .gap_2()
                            .child(field_label(theme, "文件名"))
                            .child(Input::new(&file_name))
                            .when_some(hint, |this, hint| {
                                this.child(hint_line(hint, theme.colors.danger))
                            }),
                    )
                    .child(
                        Checkbox::new("checkout-dialog-open")
                            .label("取回后打开")
                            .checked(checked)
                            .on_click(move |checked, _window, _cx| toggled.set(*checked)),
                    )
                    .child(hint_line(
                        &format!("本体不动。修改后再次归档将生成 v{next_version}"),
                        theme.colors.muted_foreground,
                    )),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("checkout-dialog-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "checkout-dialog-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("checkout-dialog-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "checkout-dialog-ok".to_string())
                            .label("取回")
                            .on_click(move |_, window, cx| {
                                if !submit_for_button(cx) {
                                    return;
                                }
                                window.close_dialog(cx);
                            }),
                    ),
            )
            .on_ok(move |_, _window, cx| submit_for_ok(cx))
            .on_cancel(|_, _, _| true)
    });
}

/// 只读信息行（标签在左、值在右——与"要你填"的表单区形状刻意不同）。
fn readonly_row(theme: &Theme, label: &str, value: &str) -> Div {
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .text_xs()
        .child(
            div()
                .w(rems(ui::DETAIL_LABEL_WIDTH))
                .flex_none()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(value.to_string()),
        )
}

/// 字段名（弱化色，与它下面的输入框一组）。
fn field_label(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 一行提示（校验用 `danger`，说明用 `muted`）。
fn hint_line(text: &str, color: gpui_kit::Hsla) -> Div {
    div().w_full().text_xs().text_color(color).child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::{file_name_hint, suggest_work_copy_name};

    #[test]
    fn suggested_name_keeps_extension_outside_the_parentheses() {
        assert_eq!(
            suggest_work_copy_name("月报", Some("sql")),
            "月报（工作副本）.sql"
        );
        assert_eq!(
            suggest_work_copy_name("月报", Some(".sql")),
            "月报（工作副本）.sql",
            "扩展名带不带点都归一"
        );
        assert_eq!(
            suggest_work_copy_name("无扩展名", None),
            "无扩展名（工作副本）"
        );
        assert_eq!(suggest_work_copy_name("空扩展", Some("")), "空扩展（工作副本）");
    }

    #[test]
    fn file_name_hint_blocks_names_that_cannot_be_created() {
        assert!(file_name_hint("月报（工作副本）.sql").is_none());
        assert_eq!(file_name_hint("   "), Some("文件名不能为空"));
        assert!(file_name_hint("a/b.sql").is_some(), "路径分隔符要挡下");
        assert!(file_name_hint("a\\b.sql").is_some());
        assert!(file_name_hint("a:b.sql").is_some());
        assert!(file_name_hint("..").is_some());
        assert!(
            file_name_hint("con.sql").is_some(),
            "Windows 保留设备名建不出文件，必须在这里报"
        );
        assert!(file_name_hint("console.sql").is_none(), "别把正常名误伤");
    }
}

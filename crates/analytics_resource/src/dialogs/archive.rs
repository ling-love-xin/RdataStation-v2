//! 归档确认对话框（P1.4）：把"这次归档会变成什么"在动手前一次说清。
//!
//! 归档是**不可逆**的动作（文件从工作区搬进 `resources/` 并变只读，开发方案 R2），
//! 所以对话框要摆出：来源是什么、落到哪、来源连接是谁（归档凭证的关键一环）、
//! 目标被占用时怎么处理。字段与默认值取自原型 §4.1。
//!
//! 两条刻意的克制：
//!
//! 1. **目标位置只读**：目标相对路径由来源决定，不给输入框——手打相对路径就是绕过
//!    `PayloadStore::resolve` 的越界拒绝（模块硬约束 2 的入口守卫必须留在服务层）；
//! 2. **冲突不静默覆盖**：目标已被占用时，宿主在**开窗前**查好"改名后的落点"并作为
//!    [`ArchiveConflict`] 传进来，对话框原样摆出——用户看到的是确定的行为，不是"可能会重命名"。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

/// 「保留历史内容」输入的上限。
///
/// 100 份内容副本已是"基本不裁剪"；再加位数多半是手滑多打了一个 0，
/// 而每多一份都是整份文件副本，不值得让它悄悄生效。
pub const KEEP_VERSIONS_MAX: u32 = 100;

/// 打开归档对话框所需的全部信息（**宿主在事件路径备好**）。
///
/// 这里只放"要展示给用户看的"，不放 `ArchiveRequest` 本身：请求的构造（kind / binding /
/// rel_path 的最终落点）归宿主，对话框只回传用户能改的那几个字段
/// （见 [`ArchiveDialogResult`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDialogSeed {
    /// 来源一句话（如"草稿箱：reports/a.sql" / "本地文件：D:\x\a.sql"），只读展示。
    pub source_label: String,
    /// 目标相对路径（`resources/` 下），只读展示。
    pub rel_path: String,
    /// 显示名默认值（文件名去扩展名）。
    pub name: String,
    /// 来源连接（自动带出；`None` 显示"未记录"——**不猜**）。
    pub source_connection: Option<String>,
    /// 目标已占用时的"将改名到"提示（宿主查好；`None` = 无冲突）。
    pub conflict: Option<ArchiveConflict>,
}

/// 目标占用：不静默覆盖，也不让用户手改路径——由宿主给出确定的改名落点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveConflict {
    /// 被占用的目标相对路径（展示用，带 `resources/` 前缀）。
    pub taken_rel_path: String,
    /// 宿主定好的改名落点（展示用，同样带前缀）。
    pub resolved_rel_path: String,
}

/// 对话框结果：**只含用户能改的字段**（路径 / 来源 / kind 由宿主补）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDialogResult {
    pub name: String,
    /// 逗号 / 空格分隔的标签（已去空、去重、保序）。
    pub tags: Vec<String>,
    /// `None` = 跟随设置默认（输入框留空即此意）。
    pub keep_versions: Option<u32>,
}

/// 表单的输入实体（开窗前建好；测试可复核同一批实体）。
///
/// `Clone`：宿主与测试都要在开窗后继续持有它们（开窗会 `move` 一份进 builder）。
#[derive(Clone)]
pub struct ArchiveDialogInputs {
    pub name: Entity<InputState>,
    pub tags: Entity<InputState>,
    pub keep_versions: Entity<InputState>,
}

/// 标签输入解析：`,` / `，` / 空白分隔；去空、**去重保序**（顺序是用户敲的，别打乱）。
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in text.split([',', '，', ' ', '\t', '\n']) {
        let tag = part.trim();
        if tag.is_empty() || out.iter().any(|seen| seen == tag) {
            continue;
        }
        out.push(tag.to_string());
    }
    out
}

/// 「保留历史内容」解析：**空 = 跟随设置**（`None`），数字 = 本次覆盖。
///
/// 错误给的是**可直接当提示用**的一句话（校验与提示同源，不会出现"提示说非法、提交却过了"）。
pub fn parse_keep_versions(text: &str) -> Result<Option<u32>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let Ok(value) = text.parse::<u32>() else {
        return Err("请填数字（留空 = 跟随设置）");
    };
    if value > KEEP_VERSIONS_MAX {
        return Err("最多 100 份（留空 = 跟随设置）");
    }
    Ok(Some(value))
}

/// 显示名的校验提示（`None` = 合法）。
pub fn name_hint(name: &str) -> Option<&'static str> {
    name.trim().is_empty().then_some("显示名不能为空")
}

/// 建表单实体并按 seed 预填（宿主与测试都从这里拿到同一批输入）。
pub fn build_inputs(
    window: &mut Window,
    cx: &mut App,
    seed: &ArchiveDialogSeed,
) -> ArchiveDialogInputs {
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("显示名"));
    // 预填走 `set_value`：它**不发** `InputEvent::Change`，而本对话框的校验是每帧从值推导，
    // 因此无需额外同步条件（这正是选"推导"而非"标志位"的收益）。
    let name_value = seed.name.clone();
    name.update(cx, |state, cx| state.set_value(name_value, window, cx));

    let tags = cx.new(|cx| InputState::new(window, cx).placeholder("标签，逗号分隔（可空）"));
    let keep_versions = cx.new(|cx| InputState::new(window, cx).placeholder("跟随设置"));
    ArchiveDialogInputs {
        name,
        tags,
        keep_versions,
    }
}

/// 校验 + 取值；`None` = 不通过（对话框保持打开，内联提示已由渲染期给出）。
pub fn submit_archive(
    inputs: &ArchiveDialogInputs,
    cx: &App,
) -> Option<ArchiveDialogResult> {
    let name = inputs.name.read(cx).value().trim().to_string();
    if name_hint(&name).is_some() {
        return None;
    }
    let keep = parse_keep_versions(&inputs.keep_versions.read(cx).value()).ok()?;
    Some(ArchiveDialogResult {
        name,
        tags: parse_tags(&inputs.tags.read(cx).value()),
        keep_versions: keep,
    })
}

/// 打开归档确认对话框（宿主入口）。
///
/// `on_submit` 只在**校验通过**时回调一次，负责构造 `ArchiveRequest` 并落库（对话框不碰存储）。
pub fn open_archive_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: ArchiveDialogSeed,
    on_submit: impl Fn(ArchiveDialogResult, &mut App) + 'static,
) {
    let inputs = build_inputs(window, cx, &seed);
    open_archive_dialog_with(window, cx, seed, inputs, on_submit);
}

/// 同上，但用调用方建好的输入实体（窗口测试从这条缝进来，复核"同一批实体 + 真渲染"）。
pub fn open_archive_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: ArchiveDialogSeed,
    inputs: ArchiveDialogInputs,
    on_submit: impl Fn(ArchiveDialogResult, &mut App) + 'static,
) {
    let ArchiveDialogInputs {
        name,
        tags,
        keep_versions,
    } = inputs;
    let on_submit = Rc::new(on_submit);

    // 按钮文案把"会发生什么"写进动作里：有冲突时不是"归档"而是"改名归档"。
    let ok_text = if seed.conflict.is_some() {
        "改名归档"
    } else {
        "归档"
    };

    let submit: Rc<dyn Fn(&mut App) -> bool> = {
        let name = name.clone();
        let tags = tags.clone();
        let keep_versions = keep_versions.clone();
        let on_submit = on_submit.clone();
        Rc::new(move |cx: &mut App| -> bool {
            let inputs = ArchiveDialogInputs {
                name: name.clone(),
                tags: tags.clone(),
                keep_versions: keep_versions.clone(),
            };
            let Some(result) = submit_archive(&inputs, cx) else {
                return false;
            };
            on_submit(result, cx);
            true
        })
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        // 提示每帧从当前值推导：用户改回去，提示自然消失（无标志位可残留）。
        let name_hint = name_hint(&name.read(cx).value());
        let keep_hint = parse_keep_versions(&keep_versions.read(cx).value()).err();
        // builder 是 `Fn`（每帧重建）：两条提交路径各拿一份**本帧的**副本——
        // 内层 `move` 闭包只能拿走副本，外层不能再持有它们（否则 builder 就不是 `Fn` 了）。
        let submit_for_button = submit.clone();
        let submit_for_ok = submit.clone();

        let form = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(field_label(theme, "显示名"))
            .child(Input::new(&name))
            .when_some(name_hint, |this, hint| {
                this.child(hint_line(hint, theme.colors.danger))
            })
            .child(field_label(theme, "标签"))
            .child(Input::new(&tags))
            .child(field_label(theme, "保留历史内容"))
            .child(Input::new(&keep_versions))
            .when_some(keep_hint, |this, hint| {
                this.child(hint_line(hint, theme.colors.danger))
            });

        dialog
            .title("归档为存档")
            .w(cx.theme().font_size * ui::ARCHIVE_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(readonly_block(theme, &seed))
                    .when_some(seed.conflict.as_ref(), |this, conflict| {
                        this.child(hint_line(
                            &format!(
                                "{} 已被占用：本次将归档为 {}（不覆盖）",
                                conflict.taken_rel_path, conflict.resolved_rel_path
                            ),
                            theme.colors.warning,
                        ))
                    })
                    .child(form),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("archive-dialog-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "archive-dialog-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("archive-dialog-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "archive-dialog-ok".to_string())
                            .label(ok_text)
                            .on_click(move |_, window, cx| {
                                // 校验不过：不关窗（内联提示已出现，让用户接着改）。
                                if !submit_for_button(cx) {
                                    return;
                                }
                                window.close_dialog(cx);
                            }),
                    ),
            )
            // Enter 确认 / Esc 取消（对话框 key context 自带绑定）。
            .on_ok(move |_, _window, cx| submit_for_ok(cx))
            .on_cancel(|_, _, _| true)
    });
}

/// 只读信息区：来源 / 目标位置 / 来源连接。
///
/// 与表单区的形状刻意不同（标签在上是"要你填"，标签在左是"告诉你"），
/// 用户扫一眼就知道哪几行动不了。
fn readonly_block(theme: &Theme, seed: &ArchiveDialogSeed) -> Div {
    let mut rows = div().v_flex().w_full().gap_2();
    for (label, value) in [
        ("来源", seed.source_label.clone()),
        ("目标位置", seed.rel_path.clone()),
        (
            "来源连接",
            seed.source_connection
                .clone()
                .unwrap_or_else(|| "未记录".to_string()),
        ),
    ] {
        rows = rows.child(
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
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .child(value),
                ),
        );
    }
    rows
}

/// 字段名（弱化色，与它下面的输入框一组）。
fn field_label(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 一行提示（校验用 `danger`，结果告知用 `warning`）。
fn hint_line(text: &str, color: gpui_kit::Hsla) -> Div {
    div()
        .w_full()
        .text_xs()
        .text_color(color)
        .child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::{KEEP_VERSIONS_MAX, name_hint, parse_keep_versions, parse_tags};

    #[test]
    fn tags_split_on_commas_and_spaces_keeping_order() {
        assert_eq!(
            parse_tags(" 报表, 月度  季度，报表 "),
            vec!["报表", "月度", "季度"],
            "去空、去重保序（中文逗号也算分隔符）"
        );
        assert!(parse_tags("   ").is_empty(), "全空白 = 没有标签");
        assert!(parse_tags("").is_empty());
    }

    #[test]
    fn keep_versions_empty_means_following_settings() {
        assert_eq!(parse_keep_versions(""), Ok(None));
        assert_eq!(parse_keep_versions("  "), Ok(None));
        assert_eq!(parse_keep_versions("3"), Ok(Some(3)));
        assert_eq!(parse_keep_versions("0"), Ok(Some(0)), "0 = 只留元数据");
        assert_eq!(
            parse_keep_versions(&KEEP_VERSIONS_MAX.to_string()),
            Ok(Some(KEEP_VERSIONS_MAX))
        );
        assert!(parse_keep_versions("abc").is_err());
        assert!(parse_keep_versions("-1").is_err());
        assert!(
            parse_keep_versions(&(KEEP_VERSIONS_MAX + 1).to_string()).is_err(),
            "超出上限挡在对话框，不留给服务层裁剪"
        );
    }

    #[test]
    fn name_hint_only_fires_on_blank() {
        assert!(name_hint("月报").is_none());
        assert_eq!(name_hint("  "), Some("显示名不能为空"));
    }
}

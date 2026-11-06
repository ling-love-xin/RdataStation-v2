//! 详情头部：种类图标、可编辑名称、别名、版本与强度徽标。

use super::*;

pub(super) fn render_header(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    cx: &App,
) -> Div {
    let colors = cx.theme().colors;
    let foreground = colors.foreground;
    let muted = colors.muted_foreground;
    let tone_color = colors;
    let badge = strength_badge(detail.kind, detail.status);
    let badge_color = match badge_tone(detail.kind, detail.status) {
        BadgeTone::Success => tone_color.success,
        BadgeTone::Info => tone_color.info,
        BadgeTone::Warning => tone_color.warning,
        BadgeTone::Danger => tone_color.danger,
    };
    // 头部：kind 图标 + 名称 + 别名 + 版本 + 强度徽标
    let mut header = div().v_flex().w_full().gap_0p5().child(
        div()
            .h_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            // kind 图标（形状区分，一律 muted）：与列表行同一套，只在行内存在一个色块
            // 的前提下才允许——这里那个色块是强度徽标。
            .child(
                Icon::new(kind_icon(detail.kind))
                    .flex_none()
                    .size_3p5()
                    .text_color(muted),
            )
            .child(
                // 显示名可点改（原型 §3.1「头部可编辑」）：与 `F2` / 行右键**同一条链路**
                // （同一个对话框 + 同一个宿主端口），只是入口在详情头部。
                //
                // 为何用 `Button::ghost()` 而不是给文本挂 `on_click`：可点元素一律语义控件
                // （自带 hover / 焦点 / 键盘），这条在仓库里是硬约束（见 rds-ui-spec）。
                div()
                    .flex_1()
                    .min_w_0()
                    .child(name_link(detail, actions, foreground)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("v{}", detail.version)),
            )
            .child(div().text_xs().text_color(badge_color).child(badge)),
    );
    if let Some(alias) = detail.alias.as_deref() {
        header = header.child(alias_line(detail, actions, alias, muted));
    } else if actions.is_some_and(|a| !a.read_only) {
        // 没别名时给一个轻量入口（只读项目不给：点了也写不进去）。
        header = header.child(alias_line(detail, actions, "", muted));
    }
    if detail.status == ArchiveStatus::Missing {
        // 缺失行不给"看起来正常"的头部：名称同步转弱。
        header = header.child(
            div()
                .text_xs()
                .text_color(tone_color.danger)
                .child("本体缺失"),
        );
    }
    header
}

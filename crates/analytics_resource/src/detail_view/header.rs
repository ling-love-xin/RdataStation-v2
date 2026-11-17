//! 详情头部：种类图标、可编辑名称、别名、版本与强度徽标。

use super::*;

/// 头部标题条（原型 `.dhead`）：**不随内容滚动**。
///
/// 详情能滚很长（基本信息 + 版本 + 20 行预览 + 动作区），名字和版本号是"我在看哪一条"
/// 的唯一凭据——滚到一半被裁掉，用户就得回列表确认一次。
pub(super) fn render_head_bar(
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
    // 头部：kind 图标 + 名称 + 版本 + 强度徽标
    div()
        .h_flex()
        .w_full()
        .min_w_0()
        .flex_none()
        .gap_2()
        .px_2()
        .h(rems(ui::PANEL_HEADER_HEIGHT))
        .border_b(ui::HAIRLINE)
        .border_color(colors.border)
        // 图标 14px、版本与徽标 12px、名称是按钮：高度各不相同，靠 items_center 对齐
        .items_center()
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
            // 版本徽标（原型 `.dver`）：带一圈描边的 pill，与右侧的强度徽标一样“挂”在
            // 名字后面；裸一行 `v3` 会和文件名混成一句，看不出它是元信息。
            div()
                .flex_none()
                .h_flex()
                .h(rems(ui::ARCHIVE_BADGE_HEIGHT))
                .px_1()
                .rounded_sm()
                .border_1()
                .border_color(colors.border)
                .text_xs()
                .text_color(muted)
                .child(format!("v{}", detail.version)),
        )
        .child(div().text_xs().text_color(badge_color).child(badge))
}

/// 标题条下面那一行（原型 `.dsub`）：别名 / 本体缺失提示。
///
/// 单独一行的理由：别名是可点的（点一下开对话框），塞进 36px 的标题条里会把名称挤窄；
/// 没有别名也没有异常时**整行不摆**，不占那 22px。
pub(super) fn render_head_subline(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    cx: &App,
) -> Option<Div> {
    let muted = cx.theme().colors.muted_foreground;
    let mut line = div().h_flex().w_full().items_center().gap_1().px_2().pt_1();
    let mut empty = true;
    if let Some(alias) = detail.alias.as_deref() {
        line = line.child(alias_line(detail, actions, alias, muted));
        empty = false;
    } else if actions.is_some_and(|a| can_rename(&detail.action_context(a.read_only, 1))) {
        // 没别名时给一个轻量入口（不可改时不给：点了也写不进去）。判据与显示名同一份。
        line = line.child(alias_line(detail, actions, "", muted));
        empty = false;
    }
    if detail.status == ArchiveStatus::Missing {
        // 缺失行不给"看起来正常"的头部：名称同步转弱。
        line = line.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.danger)
                .child("本体缺失"),
        );
        empty = false;
    }
    (!empty).then_some(line)
}

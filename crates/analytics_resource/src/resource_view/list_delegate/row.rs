//! 存档行的信息层：图标、名称、版本、复现强度徽标与尾部字段。

use super::*;

pub(super) fn render_row_line(row: &ArchiveRow, is_global: bool, cx: &App) -> Div {
        let (foreground, muted, success, info, warning, danger) = {
            let colors = cx.theme().colors;
            (
                colors.foreground,
                colors.muted_foreground,
                colors.success,
                colors.info,
                colors.warning,
                colors.danger,
            )
        };
        // 全局行徽标 = 「全局」；项目行保持复现强度徽标（原型 §3.2）。
        let badge = if is_global {
            "全局"
        } else {
            strength_badge(row.kind, row.status)
        };
        let badge_color = if is_global {
            info
        } else {
            match badge_tone(row.kind, row.status) {
                BadgeTone::Success => success,
                BadgeTone::Info => info,
                BadgeTone::Warning => warning,
                BadgeTone::Danger => danger,
            }
        };
        // 缺失的行灰显（不是禁用：它仍可被选中看详情、仍可走索引修复）。
        let name_color = if row.status == ArchiveStatus::Missing {
            muted
        } else {
            foreground
        };
        let mut line = div()
            .h_flex()
            .w_full()
            .min_w_0()
            .h(rems(ui::ROW_HEIGHT))
            .gap_2()
            .child(
                // kind 图标：一律 muted（颜色信号留给复现强度徽标）；
                // `size_3p5` 是 `ui::ICON_SIZE_SM`（0.875rem = 14px）的 Tailwind 等价写法。
                Icon::new(kind_icon(row.kind))
                    .flex_none()
                    .size_3p5()
                    .text_color(muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .text_color(name_color)
                    .child(row.name.clone()),
            )
            // v1 不显示版本徐标（减少噪声）。
            .when(row.version > 1, |line| {
                line.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("v{}", row.version)),
                )
            })
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::ARCHIVE_BADGE_HEIGHT))
                    .text_xs()
                    .text_color(badge_color)
                    .child(badge),
            );
        if !row.tail.is_empty() {
            line = line.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(row.tail.clone()),
            );
        }
    line
}

//! 工作台 tab 顶部的「业务数据概览」卡片区（纯渲染）。
//!
//! 数据来自 [`crate::services::overview`]，本文件只负责把它排成四张卡：
//! 连接 / 项目元数据 / 洞察画像 / 分析库表——**主数值一律是业务数据**，
//! 模块能力数（如 mock 生成器种类）放副行，避免把「我们支持多少」冒充「用户用了多少」。
//!
//! 取数在后台线程做（见 `services/overview.rs` 的模块注释），这里拿到的只是快照；
//! 快照为 `None` 表示首帧还没回填，卡片显示「…」。

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Sizable as _, Theme};
use gpui_kit::*;

use crate::services::overview::OverviewCounts;

/// 卡片区的标题行 + 四张卡 + 失败提示条。
///
/// `on_refresh` 由宿主面板给（点「刷新」重新取数），卡片区自己不碰业务状态。
pub(crate) fn render_overview_cards(
    counts: Option<&OverviewCounts>,
    theme: &Theme,
    on_refresh: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let loading = counts.is_none();

    let header = div()
        .h_flex()
        .items_center()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.colors.foreground)
                .child("业务数据概览"),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("本机实时读数：连接、项目元数据、洞察画像、分析库"),
        )
        .child(div().flex_1())
        .child(
            Button::new("overview-refresh")
                .ghost()
                .small()
                .label("刷新")
                .on_click(move |_, window, app| on_refresh(window, app)),
        );

    // 主数值取值（拿不到时统一「—」，加载中「…」）。
    let dash = |value: Option<usize>| -> SharedString {
        match (loading, value) {
            (true, _) => "…".into(),
            (false, Some(n)) => n.to_string().into(),
            (false, None) => "—".into(),
        }
    };

    let connections_value: SharedString = if loading {
        "…".into()
    } else {
        counts.map(|c| c.connections).unwrap_or(0).to_string().into()
    };
    let connections_note: SharedString = match counts {
        Some(c) => format!("已连接 {} 个", c.connected).into(),
        None => "正在统计".into(),
    };

    let project_value: SharedString = match counts {
        Some(c) if c.project_name.is_some() => dash(c.project_tables),
        Some(_) => "未打开".into(),
        None => "…".into(),
    };
    let project_note: SharedString = match counts {
        Some(c) => match (&c.project_name, c.project_columns) {
            (Some(name), Some(columns)) => format!("{} 列 · {name}", columns).into(),
            (Some(_), None) => "列数取不到".into(),
            (None, _) => "打开项目后统计元数据".into(),
        },
        None => "正在统计".into(),
    };

    let insight_value = dash(counts.and_then(|c| c.insight_reports));
    let insight_note: SharedString = match counts {
        Some(c) if c.project_name.is_some() => "已存画像报表（表 + 结构）".into(),
        Some(_) => "打开项目后统计".into(),
        None => "正在统计".into(),
    };

    let analysis_value = dash(counts.and_then(|c| c.analysis_tables));
    let analysis_note: SharedString = match counts {
        Some(c) => format!("分析库表 · mock 生成器 {} 种", c.mock_generators).into(),
        None => "正在统计".into(),
    };

    let cards = div()
        .h_flex()
        .items_stretch()
        .gap_3()
        .w_full()
        .child(card(
            theme,
            theme.colors.primary,
            "连接",
            connections_value,
            connections_note,
        ))
        .child(card(
            theme,
            theme.colors.info,
            "项目元数据",
            project_value,
            project_note,
        ))
        .child(card(
            theme,
            theme.colors.success,
            "洞察画像",
            insight_value,
            insight_note,
        ))
        .child(card(
            theme,
            theme.colors.warning,
            "分析库",
            analysis_value,
            analysis_note,
        ));

    let mut root = div().v_flex().gap_2().w_full().child(header).child(cards);

    // 取数失败提示：只展示前两条，原因写在 `services/overview.rs` 里（不打断渲染）。
    if let Some(c) = counts {
        if !c.problems.is_empty() {
            let text = c
                .problems
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ");
            root = root.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.warning)
                    .child(text),
            );
        }
    }

    root
}

/// 单张卡：色条 + 标题 / 主数值 / 副说明。
fn card(
    theme: &Theme,
    accent: Hsla,
    title: &'static str,
    value: SharedString,
    note: SharedString,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(rems(10.))
        .v_flex()
        .gap_1()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.colors.border)
        .bg(theme.colors.popover)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(div().w_1().h_3().rounded_sm().bg(accent))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(title),
                ),
        )
        .child(
            div()
                .text_2xl()
                .font_weight(FontWeight::BOLD)
                .text_color(theme.colors.foreground)
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(note),
        )
}

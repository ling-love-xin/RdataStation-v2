//! 列卡（YData 密度对齐，2026-11-06 重构）：**一列一行可展开卡**。
//!
//! 版式对齐 YData Profiling：
//! - 折叠行 = 名称 + 类型徽标 + 缺失/唯一 + 探索价值 + **右端常驻迷你图**
//!   （类型专属：数值直方 / Top 条 / 真假占比；缺席给原因短文案）；
//! - 展开体 = 规则开关 + 五区 Tab（基础统计**左表右图** / 分布 / 检查器 / 样本 / 规则）
//!   + 检查器汇总 + SQL 入口。
//!
//! 按类型渲染拆分：[`numeric`] / [`text`] / [`datetime`] / [`boolean`] 各自拥有
//! 「统计行集 + 分布图 + 迷你图」，本模块只做分派与骨架（避免 eda.rs 膨胀；
//! 数据缺口如实登记：时间列常被 JSON 打型成文本，走 text 降级）。
//!
//! 事件接线清单（搬迁核对用，改这里先对照）：
//! - `insight-ws-col-toggle-{name}`：展开 / 收起（写 `col_open`；折叠行渲染在 folded.rs）
//! - `insight-ws-col-recalc-{name}`：中断列补算（`request_column_evaluation`）
//! - `insight-ws-col-zones-{name}`：切区（写 `col_detail_zone`；切「规则」顺带
//!   `ensure_col_rules` 惰性载入）
//! - `insight-ws-colrule-load-{name}` / `insight-ws-colrule-sql-{name}-{rule}`：
//!   规则摘要载入 / SQL 查看（`OpenSqlRequested`）
//! - `insight-ws-col-sql-{name}`：在 SQL 编辑器中查看（`OpenSqlRequested`）

use super::*;
// workspace/mod.rs 的 gpui_kit use 是私有别名（glob 不传导），显式补齐本文件所需
use gpui_kit::component::ColorName;
use gpui_kit::component::tag::Tag;

impl InsightWorkspacePanel {
    /// 列卡（折叠行 + 展开体）。
    pub(crate) fn render_col_card(
        &self,
        profile: &TableProfileView,
        col: &TableColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
        temp_table: Option<String>,
    ) -> impl IntoElement {
        let checker = column_checkers(profile, col);
        let explore = col_explore_score(col, &checker);
        let open = self.col_open.borrow().contains(&col.name);
        let zone = ColZone::of_index(*self.col_detail_zone.borrow().get(&col.name).unwrap_or(&0));
        // 探索价值徽标（原型 .cc-explore）：≥3 高亮「关注」，≥1 平淡显示，<1 不显示
        let explore_badge = if explore >= 3.0 {
            Some(
                Tag::color(ColorName::Amber)
                    .xsmall()
                    .child(format!("关注 {explore:.1}")),
            )
        } else if explore >= 1.0 {
            Some(
                Tag::secondary()
                    .outline()
                    .xsmall()
                    .child(format!("探索 {explore:.1}")),
            )
        } else {
            None
        };
        let folded =
            self.render_card_folded_row(col, &checker, entity, theme, open, explore_badge);
        let body = self.render_col_card_body(col, &checker, zone, entity, theme, temp_table);
        Collapsible::new()
            .motion_id(SharedString::from(format!(
                "insight-ws-col-motion-{}",
                col.name
            )))
            .open(open)
            .child(folded)
            .content(body)
            .w_full()
    }

    /// 列卡展开体：只看规则开关 + （五区 Tab + 区内容 + 检查器汇总）或检查器列表。
    /// 当前区的具体内容（对齐原型 `zoneBody`）。
    pub(crate) fn render_col_zone(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        zone: ColZone,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        match zone {
            ColZone::Stats => self.render_col_stats_zone(col, theme),
            ColZone::Dist => self.render_col_dist_zone(col, theme),
            ColZone::Quality => self.render_checker_list(col, checker, theme),
            ColZone::Sample => self.render_col_sample_zone(col, theme),
            ColZone::Rules => self.render_col_rules_zone(col, entity, theme),
        }
    }

}

pub(crate) mod boolean;
pub(crate) mod datetime;
pub(crate) mod folded;
pub(crate) mod numeric;
pub(crate) mod text;
pub(crate) mod body;

mod zones {
    pub(crate) mod checker;
    pub(crate) mod dist;
    pub(crate) mod rules;
    pub(crate) mod sample;
    pub(crate) mod stats;
}

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
use gpui_kit::assets::IconName as AssetsIconName;
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
        is_full: bool,
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
            self.render_card_folded_row(col, &checker, entity, theme, is_full, open, explore_badge);
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
    ///
    /// 「只看规则属性」开启后隐藏四区与汇总，只显示检查器列表（对齐原型
    /// `ruleOnly` 语义；开关始终可见，避免原型「开完关不回去」的交互缺陷）。
    pub(crate) fn render_col_card_body(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        zone: ColZone,
        entity: &Entity<Self>,
        theme: &Theme,
        temp_table: Option<String>,
    ) -> Div {
        let rule_toggle = div().h_flex().w_full().items_center().gap_2().child(
            div().flex_1().min_w_0().child(
                Switch::new(SharedString::from(format!(
                    "insight-ws-rule-only-{}",
                    col.name
                )))
                .checked(
                    self.col_rule_only
                        .borrow()
                        .get(&col.name)
                        .copied()
                        .unwrap_or(false),
                )
                .label("只看规则属性")
                .on_click({
                    let entity = entity.clone();
                    let name = col.name.clone();
                    move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            let on = panel
                                .col_rule_only
                                .borrow()
                                .get(&name)
                                .copied()
                                .unwrap_or(false);
                            panel.col_rule_only.borrow_mut().insert(name.clone(), !on);
                            cx.notify();
                        });
                    }
                }),
            ),
        );
        let rule_only = self
            .col_rule_only
            .borrow()
            .get(&col.name)
            .copied()
            .unwrap_or(false);
        if rule_only {
            return div()
                .v_flex()
                .w_full()
                .gap_1()
                .px_1()
                .pb_1()
                .child(rule_toggle)
                .child(self.render_checker_list(col, checker, theme));
        }
        // 警告块（YData Warnings 对应物）：未命中摘要直接摆在展开体顶部
        // （最多 3 条 + 等价计数），amber 底框——用户不用切到「检查器」区就知道问题
        let warnings: Vec<&ColumnChecker> = checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Miss)
            .collect();
        let warning_box = (!warnings.is_empty()).then(|| {
            let mut boxd = div()
                .v_flex()
                .w_full()
                .gap_0p5()
                .p_1()
                .rounded_sm()
                .bg(theme.colors.list)
                .border_1()
                .border_color(theme.colors.danger)
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.colors.danger)
                        .child(format!("⚠ {} 项未命中", warnings.len())),
                );
            for check in warnings.iter().take(3) {
                boxd = boxd.child(div().text_xs().text_color(theme.colors.danger).child(
                    SharedString::from(format!("{}：{}", check.name, check.detail)),
                ));
            }
            if warnings.len() > 3 {
                boxd = boxd.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("…等 {} 条（详见检查器区）", warnings.len())),
                );
            }
            boxd
        });
        // 五区 Tab（segmented；id 带列名防跨列串台）+ 当前区内容 + 检查器汇总
        let tabs = TabBar::new(SharedString::from(format!(
            "insight-ws-col-zones-{}",
            col.name
        )))
        .segmented()
        .selected_index(zone.index())
        .children(ColZone::ALL.iter().map(|z| Tab::new().label(z.label())))
        .on_click({
            let entity = entity.clone();
            let name = col.name.clone();
            let col_kind = col.kind; // Copy，闭包外捕获（col 引用不能进 'static 闭包）
            move |index, _, app| {
                entity.update(app, |panel, cx| {
                    panel
                        .col_detail_zone
                        .borrow_mut()
                        .insert(name.clone(), *index);
                    // 切到「规则」Tab 顺带惰性载入摘要（事件路径，允许取数）
                    if ColZone::of_index(*index) == ColZone::Rules {
                        panel.ensure_col_rules(&name, col_kind, cx);
                    }
                    cx.notify();
                });
            }
        });
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .px_1()
            .pb_1()
            .child(rule_toggle)
            .when_some(warning_box, |body, w| body.child(w))
            .child(tabs)
            .child(self.render_col_zone(col, checker, zone, entity, theme))
            .child(self.render_checker_summary(col, checker, theme))
            // 列卡「在 SQL 编辑器中查看」（2026-10-30 对齐原型 col-card 尾部）：
            // 生成该列的探查 SQL（带引号转义），宿主 `open_query_document` 打开
            // 不执行（`run = false`：看语句，不跑）。
            .when_some(temp_table, |body, tmp| {
                let entity = entity.clone();
                let name = col.name.clone();
                let quoted_col = col.name.replace('"', "\"\"");
                let quoted_tbl = tmp.replace('"', "\"\"");
                let sql = format!("SELECT \"{quoted_col}\" FROM \"{quoted_tbl}\" LIMIT 100");
                let title = format!("{} · {}", tmp, col.name);
                body.child(
                    div().h_flex().w_full().justify_end().child(
                        Button::new(SharedString::from(format!("insight-ws-col-sql-{}", name)))
                            .ghost()
                            .xsmall()
                            .icon(AssetsIconName::Braces)
                            .label("在 SQL 编辑器中查看")
                            .on_click(move |_, _, app| {
                                entity.update(app, |panel, cx| {
                                    panel.insight_view().update(cx, |_view, cx| {
                                        cx.emit(InsightEvent::OpenSqlRequested {
                                            sql: sql.clone(),
                                            title: title.clone(),
                                        });
                                    });
                                });
                            }),
                    ),
                )
            })
    }

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

    /// 自定义规则区（2026-11-06 YData 密度对齐「我们多出的那个 Tab」）：
    /// 适用于该列的规则摘要（id / 名称 / 描述 + 已代入参数的 SQL），
    /// SQL 入口在编辑器中查看（不执行）。
    ///
    /// 摘要**惰性载入**：缓存未命中时先给占位说明——取数发生在切 Tab 的
    /// **on_click 事件路径**（`ensure_col_rules`，事件路径允许同步取数；
    /// render 保持纯读）。数十条 TOML 规则的 HashMap 代入是毫秒级。
    pub(crate) fn render_col_rules_zone(
        &self,
        col: &TableColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let Some(briefs) = self.col_rules.borrow().get(&col.name).cloned() else {
            let entity = entity.clone();
            let name = col.name.clone();
            let kind = col.kind;
            return div()
                .v_flex()
                .w_full()
                .gap_1()
                .child(zone_note(
                    theme,
                    "规则摘要尚未载入（点击下方按钮获取适用于该列的规则）",
                ))
                .child(
                    Button::new(SharedString::from(format!(
                        "insight-ws-colrule-load-{}",
                        col.name
                    )))
                    .ghost()
                    .xsmall()
                    .icon(IconName::RefreshCw)
                    .label("载入规则摘要")
                    .on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.ensure_col_rules(&name, kind, cx);
                        });
                    }),
                );
        };
        if briefs.is_empty() {
            return zone_note(
                theme,
                "没有适用于该类型的列级规则（内置统计见「基础统计」）",
            );
        }
        let mut body = div().v_flex().w_full().gap_1();
        for brief in briefs {
            let sql_id =
                SharedString::from(format!("insight-ws-colrule-sql-{}-{}", col.name, brief.id));
            let entity = entity.clone();
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_0p5()
                    .p_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_ellipsis()
                                    .text_color(colors.foreground)
                                    .child(format!("{} · {}", brief.id, brief.name)),
                            )
                            .child(
                                Button::new(sql_id)
                                    .ghost()
                                    .xsmall()
                                    .icon(AssetsIconName::Braces)
                                    .label("SQL")
                                    .tooltip("在 SQL 编辑器中查看该规则的代入语句（不执行）")
                                    .on_click({
                                        let sql = brief.sql.clone();
                                        let title = format!("规则 {} · {}", brief.id, col.name);
                                        move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.insight_view().update(cx, |_v, cx| {
                                                    cx.emit(InsightEvent::OpenSqlRequested {
                                                        sql: sql.clone(),
                                                        title: title.clone(),
                                                    });
                                                });
                                            });
                                        }
                                    }),
                            ),
                    )
                    .when(!brief.description.is_empty(), |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(brief.description.clone()),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(SharedString::from(brief.sql.clone())),
                    ),
            );
        }
        body
    }

    /// 列规则摘要载入（事件路径：on_click 触发；同步 `with_rules` 无 I/O，
    /// 数十条规则代入毫秒级——不合规范的地方在于它在**宿主 App 上下文**，
    /// 故经后台执行器走一道，与 jobs 同款姿势）。
    fn ensure_col_rules(&mut self, column: &str, kind: ColumnKind, cx: &mut Context<Self>) {
        if self.col_rules.borrow().contains_key(column) {
            return;
        }
        let column = column.to_string();
        let column_for_task = column.clone();
        let task = cx.background_executor().spawn(async move {
            crate::service::InsightService::column_rule_briefs(None, &column_for_task, kind)
        });
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            if let Ok(briefs) = task.await {
                let _ = weak.update(cx, |panel, cx| {
                    // 取到后再查一次：并发触发下后到的写覆盖同值（幂等）
                    panel.col_rules.borrow_mut().insert(column, briefs);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 基础统计区（YData Statistics 版式）：**左侧键值统计表 + 右侧分布图**
    /// 并排；行集与图都按列类型族分派（type 文件）。
    pub(crate) fn render_col_stats_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        if col.stats_detail.is_none() {
            return zone_note(theme, "未评估全表，无列级统计（点工具栏「评估」）");
        }
        let groups = self.type_stats_groups(col);
        div()
            .h_flex()
            .w_full()
            .items_stretch()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(zone_rows_grouped(theme, &groups)),
            )
            .child(
                div()
                    .flex_none()
                    .w(rems(ui::INSIGHT_CARD_VIS_WIDTH))
                    .min_w_0()
                    .child(self.type_mini_chart(col, "zone", theme)),
            )
    }

    /// 类型分派：右栏 mini 图（zone 变体；数值直方 / Top 条 / 真假占比）。
    ///
    /// 右栏刻意用 **mini 渲染器**（无 dist id / zone 前缀 id）而不是完整
    /// `dist_visual`：Stats 右栏 / Dist 区 / 折叠行 mini 三处可能同帧共存
    /// （Collapsible 收起后内容保持挂载），Progress id 只按「列」区分会重复
    /// 注册 → Duplicate a11y node id panic（2026-11-06 真机闪退根因防御）。
    fn type_mini_chart(&self, col: &TableColumnView, variant: &str, theme: &Theme) -> Div {
        match &col.stats_detail {
            Some(crate::model::types::ColumnStatsDetail::Numeric(_)) => {
                match col.histogram.as_ref() {
                    Some(bins) if !bins.is_empty() => numeric::mini_histogram(theme, bins),
                    _ => zone_note(theme, "样本行数不足"),
                }
            }
            Some(crate::model::types::ColumnStatsDetail::Text(t)) => {
                text::mini_freq_rows(&col.name, variant, &t.top_values, theme)
                    .unwrap_or_else(|| zone_note(theme, "无频次数据"))
            }
            Some(crate::model::types::ColumnStatsDetail::DateTime(d)) => {
                datetime::mini_freq_rows(&col.name, variant, &d.monthly_distribution, theme)
                    .unwrap_or_else(|| zone_note(theme, "无频次数据"))
            }
            Some(crate::model::types::ColumnStatsDetail::Boolean(b)) => {
                boolean::mini_ratio(theme, b.true_ratio)
            }
            _ => zone_note(theme, "类型未识别"),
        }
    }

    /// 分布区：类型专属图 + 附注（数值卡的偏度 / 峰度行）。
    pub(crate) fn render_col_dist_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let mut body = div().v_flex().w_full().gap_1();
        body = body.child(self.type_dist_visual(col, theme));
        if let Some(note) = self.type_dist_notes(col, theme) {
            body = body.child(note);
        }
        body
    }

    /// 折叠态迷你图（按 scope 与类型分派；`None` = 缺席，调用方给原因文案）。
    pub(crate) fn render_col_mini_vis(
        &self,
        col: &TableColumnView,
        theme: &Theme,
        is_full: bool,
    ) -> Option<Div> {
        // 取样档：迷你统计（C7 并入取样链路，查询级过滤现算回填）
        if !is_full {
            return match col.mini_stats.as_ref()? {
                crate::model::ColumnMiniStats::Numeric(bins) => {
                    Some(numeric::mini_histogram(theme, bins))
                }
                crate::model::ColumnMiniStats::Frequencies(freqs) => {
                    text::mini_freq_rows(&col.name, "fold", freqs, theme)
                }
                crate::model::ColumnMiniStats::Boolean { true_ratio } => {
                    Some(boolean::mini_ratio(theme, *true_ratio))
                }
            };
        }
        // 全量档：评估产物（评估全表逐列回填）
        match &col.stats_detail {
            Some(crate::model::types::ColumnStatsDetail::Numeric(_)) => {
                let bins = col.histogram.as_ref()?;
                Some(numeric::mini_histogram(theme, bins))
            }
            Some(crate::model::types::ColumnStatsDetail::Text(t)) => {
                text::mini_freq_rows(&col.name, "fold", &t.top_values, theme)
            }
            Some(crate::model::types::ColumnStatsDetail::DateTime(d)) => {
                datetime::mini_freq_rows(&col.name, "fold", &d.monthly_distribution, theme)
            }
            Some(crate::model::types::ColumnStatsDetail::Boolean(b)) => {
                Some(boolean::mini_ratio(theme, b.true_ratio))
            }
            _ => None,
        }
    }

    /// 类型分派：基础统计**分组**行集（YData Statistics 分组呈现）。
    ///
    /// 最前统一插「计数」组（YData Overview 的 Distinct/Missing 对应物）：
    /// 空值数 / 唯一值——精确计数来自表级摘要（`ColumnStatBrief` 回填），
    /// 对所有类型通用。
    fn type_stats_groups(
        &self,
        col: &TableColumnView,
    ) -> Vec<(&'static str, Vec<(String, String)>)> {
        let mut groups = self.type_stats_groups_by_kind(col);
        let mut counts = Vec::new();
        if let Some(n) = col.null_count {
            counts.push(("空值数".into(), fmt_int(n.max(0) as u32)));
        }
        if let Some(u) = col.unique_count {
            counts.push(("唯一值".into(), fmt_int(u.max(0) as u32)));
        }
        if !counts.is_empty() {
            groups.insert(0, ("计数", counts));
        }
        groups
    }

    fn type_stats_groups_by_kind(
        &self,
        col: &TableColumnView,
    ) -> Vec<(&'static str, Vec<(String, String)>)> {
        match col.kind {
            ColumnKind::Numeric => numeric::stats_groups(col),
            ColumnKind::Text => text::stats_groups(col),
            ColumnKind::DateTime => datetime::stats_groups(col),
            ColumnKind::Boolean => boolean::stats_groups(col),
            ColumnKind::Unknown => vec![("统计", vec![("—".into(), "类型未识别".into())])],
        }
    }

    /// 类型分派：分布纯图。
    fn type_dist_visual(&self, col: &TableColumnView, theme: &Theme) -> Div {
        if col.stats_detail.is_none() {
            return zone_note(theme, "未评估全表，无分布数据");
        }
        match col.kind {
            ColumnKind::Numeric => numeric::dist_visual(col, theme),
            ColumnKind::Text => text::dist_visual(col, theme),
            ColumnKind::DateTime => datetime::dist_visual(col, theme),
            ColumnKind::Boolean => boolean::dist_visual(col, theme),
            ColumnKind::Unknown => zone_note(theme, "类型未识别，无分布数据"),
        }
    }

    /// 类型分派：分布附注（数值卡偏度 / 峰度；其余类型无附注）。
    fn type_dist_notes(&self, col: &TableColumnView, theme: &Theme) -> Option<Div> {
        match col.kind {
            ColumnKind::Numeric => numeric::dist_notes(col, theme),
            _ => None,
        }
    }

    /// 样本区：该列前 5 条样本值（评估回填）。
    pub(crate) fn render_col_sample_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let colors = theme.colors;
        if col.sample_values.is_empty() {
            return zone_note(theme, "无样本值（未评估或列为空）");
        }
        let mut rows = div().v_flex().w_full().gap_1();
        for (ix, value) in col.sample_values.iter().enumerate() {
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("#{}", ix + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if value == "NULL" {
                                colors.muted_foreground
                            } else {
                                colors.foreground
                            })
                            .child(value.clone()),
                    ),
            );
        }
        rows
    }

    /// 检查器命中汇总（原型 749 行）：`检查器 h/t 命中 · n 不适用` + 规则列表。
    pub(crate) fn render_checker_summary(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        if checker.is_empty() {
            return div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("该列无检查器属性（未评估全表）");
        }
        let hits = checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Hit)
            .count();
        let applicable = checker
            .iter()
            .filter(|c| c.state != ColumnCheckState::NotApplicableType)
            .count();
        let nas = checker.len() - applicable;
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .border_t_1()
            .border_color(colors.border)
            .pt_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "检查器 {hits}/{applicable} 命中{}",
                        if nas > 0 {
                            format!(" · {nas} 不适用")
                        } else {
                            String::new()
                        }
                    )),
            )
            .child(self.render_checker_list(col, checker, theme))
    }

    /// 检查器四态列表（原型 `ruleList`）：命中绿 / 未命中红 / 不适用灰。
    pub(crate) fn render_checker_list(
        &self,
        _col: &TableColumnView,
        checker: &[ColumnChecker],
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        if checker.is_empty() {
            return div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("该列无检查器属性");
        }
        let mut rows = div().v_flex().w_full().gap_1();
        // 未命中前置（YData Warnings 语义：问题一眼看到）；其余保持原序
        let mut ordered: Vec<&ColumnChecker> = checker.iter().collect();
        ordered.sort_by_key(|c| !matches!(c.state, ColumnCheckState::Miss));
        for check in ordered {
            let (state_text, color) = match check.state {
                ColumnCheckState::Hit => ("命中", colors.success),
                ColumnCheckState::Miss => ("未命中", colors.danger),
                ColumnCheckState::NotApplicableType => ("不适用", colors.muted_foreground),
                ColumnCheckState::Excluded => ("已排除", colors.muted_foreground),
            };
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_STATUS_WIDTH))
                            .text_xs()
                            .text_color(color)
                            .child(state_text),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child(check.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(colors.muted_foreground)
                            .child(check.detail.clone()),
                    ),
            );
        }
        rows
    }
}

pub(crate) mod boolean;
pub(crate) mod datetime;
pub(crate) mod folded;
pub(crate) mod numeric;
pub(crate) mod text;

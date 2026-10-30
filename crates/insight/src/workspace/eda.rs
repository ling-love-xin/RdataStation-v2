//! EDA 主域渲染：报告 Tab（列分布 / 缺失矩阵）与列卡各分区。

use super::*;
// 完整 Lucide 图标目录在 `gpui_kit::assets::IconName`（component 版只是保留子集，
// 没有 Braces）；别名避免与 `super::*` 带来的 component 版同名冲突
use gpui_kit::assets::IconName as AssetsIconName;
use gpui_kit::component::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::ColorName;

impl super::InsightWorkspacePanel {
    /// 报告主 Tab（原型 .report-tabs）：列分布 / 缺失 / 关联性 / 样本。
    pub(crate) fn render_eda_tabs(&self, _cx: &Context<Self>, entity: &Entity<Self>, theme: &Theme) -> Div {
        let current = self.eda_tab.index();
        let tabs = TabBar::new("insight-ws-eda-tabs")
            .underline()
            .selected_index(current)
            .children(EdaTab::ALL.iter().map(|t| Tab::new().label(t.label())))
            .on_click({
                let entity = entity.clone();
                move |index, _, app| {
                    entity.update(app, |panel, cx| {
                        if panel.eda_tab != EdaTab::of_index(*index) {
                            panel.eda_tab = EdaTab::of_index(*index);
                            cx.notify();
                        }
                    });
                }
            });
        div()
            .w_full()
            .border_b_1()
            .border_color(theme.colors.border)
            .child(tabs)
    }

    /// 报告主体：按主 Tab 分派四段内容。
    pub(crate) fn render_eda_body(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        _ring_px: Pixels,
    ) -> Div {
        match self.eda_tab {
            EdaTab::Cols => self.render_col_section(window, cx, profile, entity, theme),
            EdaTab::Missing => self.render_missing_matrix(profile, theme),
            EdaTab::Rel => self.render_rel_section(cx, profile, entity, theme),
            EdaTab::Sample => self.render_sample_section(profile, theme, inline_icon),
        }
    }

    /// 列分布段：字段筛选器（搜索 + 类型 + 列多选）+ 列卡列表。
    pub(crate) fn render_col_section(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let columns = &profile.columns;
        // 显式勾选为空 = 全选（避免与列集同步的簿记；200+ 列默认全看）
        let selected: Vec<&TableColumnView> = columns
            .iter()
            .filter(|c| self.sel_cols.is_empty() || self.sel_cols.contains(&c.name))
            .filter(|c| {
                self.kind_filter.map(|k| c.kind == k).unwrap_or(true)
                    && (self.col_q.is_empty() || c.name.contains(&self.col_q))
            })
            .collect();
        // 列卡「在 SQL 编辑器中查看」需要真实临时表名（源目标取样后才有；
        // 结果集 / 内存库目标自带的临时表总是可用）。无临时表时按钮不渲染。
        let temp_table: Option<String> = self.view.read(cx).target().and_then(|target| {
            self.view
                .read(cx)
                .data()
                .sample_table(target)
                .map(|t| t.to_string())
        });
        // 列卡两列网格（原型 `.col-grid`）：`flex_wrap` + 每卡 `min_w + flex_1`——
        // 容器宽于 2×min_w + gap 时双列平分，窄时自动退单列（对齐
        // property_panel 指标卡同一套写法）
        let mut cards = div().h_flex().items_stretch().w_full().gap_2().flex_wrap();
        for col in &selected {
            // 滚动锚点注册：复用同一 `ScrollAnchor` 实例（anchor_scroll 靠它持续
            // 更新 last_origin；每帧重建会让滚动位置永远停在原处）
            self.col_anchors
                .borrow_mut()
                .entry(col.name.clone())
                .or_insert_with(|| ScrollAnchor::for_handle(self.content_scroll.clone()));
            let anchor = self.col_anchors.borrow().get(&col.name).cloned();
            let col_name = col.name.clone();
            cards = cards.child(
                div()
                    .min_w(rems(ui::INSIGHT_WS_COL_CARD_MIN))
                    .flex_1()
                    .child(
                        div()
                            // anchor_scroll 在 `StatefulInteractiveElement` 上（0.8.0
                            // gpui-fast div.rs:1557），元素必须带 id 才能挂锚点
                            .id(SharedString::from(format!(
                                "insight-ws-col-anchor-{col_name}"
                            )))
                            .anchor_scroll(anchor)
                            .child(self.render_col_card(
                                profile,
                                col,
                                entity,
                                theme,
                                temp_table.clone(),
                            )),
                    ),
            );
        }
        if selected.is_empty() {
            // 无匹配列：标准 Empty 组件（对齐 database 导航「没有匹配的数据源」
            // 先例——筛选无结果用组件空态，不手搓居中文本）
            cards = cards.child(
                div().w_full().child(
                    Empty::new()
                        .header(
                            EmptyHeader::new()
                                .title(EmptyTitle::new().child(SharedString::from(
                                    "没有符合筛选条件的列",
                                )))
                                .description(EmptyDescription::new().child(SharedString::from(
                                    "调整搜索词或类型筛选后重试",
                                ))),
                        ),
                ),
            );
        }
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(self.render_field_picker(window, cx, profile, entity, theme))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("显示 {} / {} 列", selected.len(), columns.len())),
            )
            .child(cards)
    }

    /// 列卡（原型 .col-card）：折叠态 = 列名 + 类型徽标 + 探索价值徽标 + 统计行
    /// （缺失率 / 唯一率）；展开态 = 「只看规则属性」开关 + 四区
    /// （基础统计 / 分布 / 检查器 / 样本，对齐原型 `.cc-zones`）。
    pub(crate) fn render_col_card(
        &self,
        profile: &TableProfileView,
        col: &TableColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
        temp_table: Option<String>,
    ) -> impl IntoElement {
        let colors = theme.colors;
        let checker = column_checkers(profile, col);
        let explore = col_explore_score(col, &checker);
        let open = self.col_open.borrow().contains(&col.name);
        let zone = ColZone::of_index(*self.col_detail_zone.borrow().get(&col.name).unwrap_or(&0));
        // 探索价值徽标（原型 .cc-explore）：≥3 高亮「关注」，≥1 平淡显示，<1 不显示。
        // 用 `tag::Tag`（`color(ColorName)` 明暗主题自动 scale，不造裸色）
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
        let header = div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(col.name.clone()),
            )
            .child(
                // 类型徽标（原型 .kind-badge）：四族彩色胶囊，`Tag::color(ColorName)`
                // 明暗主题自动 scale；未识别用 Gray 弱化
                Tag::color(match col.kind {
                    ColumnKind::Numeric => ColorName::Green,
                    ColumnKind::Text => ColorName::Blue,
                    ColumnKind::DateTime => ColorName::Violet,
                    ColumnKind::Boolean => ColorName::Teal,
                    ColumnKind::Unknown => ColorName::Gray,
                })
                .xsmall()
                .child(col.kind.label()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "缺失 {}%",
                        col.null_rate
                            .map(|n| format!("{:.0}", n * 100.0))
                            .unwrap_or_else(|| "—".into())
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "唯一 {}%",
                        col.unique_rate
                            .map(|u| format!("{:.0}", u * 100.0))
                            .unwrap_or_else(|| "—".into())
                    )),
            )
            .when_some(explore_badge, |this, badge| this.child(badge))
            // 展开切换（语义 Button，键盘可达；原型为整行点击，等价交互）
            .child({
                let entity = entity.clone();
                let name = col.name.clone();
                let toggled = open;
                Button::new(SharedString::from(format!(
                    "insight-ws-col-toggle-{}",
                    name
                )))
                .ghost()
                .xsmall()
                .icon(if toggled {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .tooltip(if toggled {
                    "收起"
                } else {
                    "展开四区（基础统计 / 分布 / 检查器 / 样本）"
                })
                .on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| {
                        let mut open = panel.col_open.borrow_mut();
                        if toggled {
                            open.remove(&name);
                        } else {
                            open.insert(name.clone());
                        }
                        cx.notify();
                    });
                })
            });
        let body = self.render_col_card_body(col, &checker, zone, entity, theme, temp_table);
        Collapsible::new()
            .motion_id(SharedString::from(format!(
                "insight-ws-col-motion-{}",
                col.name
            )))
            .open(open)
            .child(header)
            .content(body)
            .w_full()
    }

    /// 列卡展开体：只看规则开关 + （四区 Tab + 区内容 + 检查器汇总）或检查器列表。
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
        // 四区 Tab（segmented；id 带列名防跨列串台）+ 当前区内容 + 检查器汇总
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
            move |index, _, app| {
                entity.update(app, |panel, cx| {
                    panel
                        .col_detail_zone
                        .borrow_mut()
                        .insert(name.clone(), *index);
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
            .child(tabs)
            .child(self.render_col_zone(col, checker, zone, theme))
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
                    div()
                        .h_flex()
                        .w_full()
                        .justify_end()
                        .child(
                            Button::new(SharedString::from(format!(
                                "insight-ws-col-sql-{}",
                                name
                            )))
                            .ghost()
                            .xsmall()
                            .icon(AssetsIconName::Braces)
                            .label("在 SQL 编辑器中查看")
                            .on_click(move |_, _, app| {
                                // `Self`（工作页面板）只发 `PanelEvent`，洞察意图事件
                                // 经 `insight_view()` 转发（与「＋ 新建」同一口径）；
                                // `Context::emit` 是固有方法（先例：rule_view / insight_view）
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
        theme: &Theme,
    ) -> Div {
        match zone {
            ColZone::Stats => self.render_col_stats_zone(col, theme),
            ColZone::Dist => self.render_col_dist_zone(col, theme),
            ColZone::Quality => self.render_checker_list(col, checker, theme),
            ColZone::Sample => self.render_col_sample_zone(col, theme),
        }
    }

    /// 基础统计区：按列类型族给键值行（数据来自评估回填的 `stats_detail`）。
    pub(crate) fn render_col_stats_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let Some(detail) = &col.stats_detail else {
            return zone_note(theme, "未评估全表，无列级统计（点工具栏「评估」）");
        };
        let rows: Vec<(String, String)> = match detail {
            crate::model::types::ColumnStatsDetail::Numeric(n) => vec![
                ("平均".into(), fmt_num(n.avg)),
                ("中位".into(), fmt_num(n.median)),
                ("最小".into(), fmt_num(n.min)),
                ("最大".into(), fmt_num(n.max)),
                ("P25".into(), fmt_num(n.p25)),
                ("P75".into(), fmt_num(n.p75)),
                (
                    "标准差".into(),
                    n.stddev.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
                (
                    "偏度".into(),
                    n.skewness.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
                (
                    "峰度".into(),
                    n.kurtosis.map(fmt_num).unwrap_or_else(|| "—".into()),
                ),
            ],
            crate::model::types::ColumnStatsDetail::Text(t) => vec![
                ("最短长度".into(), fmt_int(t.min_length)),
                ("最长长度".into(), fmt_int(t.max_length)),
                (
                    "Top 值".into(),
                    t.top_values
                        .iter()
                        .take(3)
                        .map(|f| format!("{}（{}）", f.value, f.count))
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
            ],
            crate::model::types::ColumnStatsDetail::DateTime(d) => vec![
                ("最早".into(), d.earliest.clone()),
                ("最晚".into(), d.latest.clone()),
                ("跨度（天）".into(), fmt_int(d.span_days.max(0) as u32)),
            ],
            crate::model::types::ColumnStatsDetail::Boolean(b) => vec![
                ("真".into(), fmt_int(b.true_count)),
                ("假".into(), fmt_int(b.false_count)),
                ("真占比".into(), fmt_pct(b.true_ratio)),
            ],
            crate::model::types::ColumnStatsDetail::Unknown => {
                vec![("统计".into(), "无可用统计".into())]
            }
        };
        zone_rows(theme, rows)
    }

    /// 分布区：数值列 = 直方图分箱条 + 偏度/峰度 + 正态性判定；文本 = Top 值条；
    /// 时间 = 月度分布；布尔 = 真占比条。数据全部来自 DuckDB 原生统计量。
    pub(crate) fn render_col_dist_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let mut body = div().v_flex().w_full().gap_1();
        match &col.stats_detail {
            Some(crate::model::types::ColumnStatsDetail::Numeric(n)) => {
                if let Some(bins) = &col.histogram {
                    body = body.child(dist_bars(theme, &bins, 8));
                } else {
                    body = body.child(zone_note(theme, "样本行数不足，未生成直方图"));
                }
                if let (Some(skew), Some(kurt)) = (n.skewness, n.kurtosis) {
                    let mut line = div().text_xs().text_color(colors.muted_foreground);
                    if let Some(judge) = normality_judge(skew, kurt) {
                        line = line.text_color(colors.warning).child(format!(
                            "{judge}：偏度 {} · 峰度 {}",
                            fmt_num(skew),
                            fmt_num(kurt)
                        ));
                    } else {
                        line = line.child(format!(
                            "偏度 {}（{}）· 峰度 {}（{}）",
                            fmt_num(skew),
                            skew_direction(skew),
                            fmt_num(kurt),
                            kurt_direction(kurt)
                        ));
                    }
                    body = body.child(line);
                }
            }
            Some(crate::model::types::ColumnStatsDetail::Text(t)) => {
                body = body.child(dist_text_bars(theme, &t.top_values, 5));
            }
            Some(crate::model::types::ColumnStatsDetail::DateTime(d)) => {
                body = body.child(dist_text_bars(theme, &d.monthly_distribution, 5));
            }
            Some(crate::model::types::ColumnStatsDetail::Boolean(b)) => {
                let bins = vec![
                    crate::model::types::DistributionBin {
                        label: "真".into(),
                        count: b.true_count,
                        ratio: b.true_ratio,
                    },
                    crate::model::types::DistributionBin {
                        label: "假".into(),
                        count: b.false_count,
                        ratio: 1.0 - b.true_ratio,
                    },
                ];
                body = body.child(dist_bars(theme, &bins, 2));
            }
            None => {
                body = body.child(zone_note(theme, "未评估全表，无分布数据"));
            }
            Some(crate::model::types::ColumnStatsDetail::Unknown) => {
                body = body.child(zone_note(theme, "类型未识别，无分布数据"));
            }
        }
        body
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

    /// 检查器命中汇总（原型 749 行）：`检查器 h/t 命中 · n 不适用` + 规则列表
    /// （dashed 分隔，弱化审计心智）。
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
        for check in checker {
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

    /// 缺失矩阵（原型 .miss-section）：按缺失率降序的列 × Progress 条 + 百分比。
    pub(crate) fn render_missing_matrix(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let mut cols: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| c.null_rate.is_some())
            .collect();
        cols.sort_by(|a, b| {
            b.null_rate
                .partial_cmp(&a.null_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut rows = div().v_flex().w_full().gap_1();
        for col in cols.iter().take(12) {
            let ratio = col.null_rate.unwrap_or(0.0) as f32;
            let highlight = ratio > 0.2;
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if highlight {
                                colors.warning
                            } else {
                                colors.muted_foreground
                            })
                            .child(col.name.clone()),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Progress::new(SharedString::from(format!(
                                "insight-ws-miss-{}",
                                col.name
                            )))
                            .value(ratio)
                            .color(if highlight {
                                colors.warning
                            } else {
                                colors.success
                            }),
                        ),
                    )
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_color(if highlight {
                                colors.warning
                            } else {
                                colors.muted_foreground
                            })
                            .child(format!("{:.0}%", ratio * 100.0)),
                    ),
            );
        }
        let total = profile.columns.len();
        let with_stats = cols.len();
        let note = if with_stats == 0 {
            "列统计摘要不可用（查询失败或表为空）".to_string()
        } else if with_stats < total {
            format!(
                "显示缺失最多的 {} 列（共 {} 列）",
                with_stats.min(12),
                total
            )
        } else {
            format!("共 {total} 列")
        };
        panel_box(theme, "缺失矩阵", note, rows)
    }
}

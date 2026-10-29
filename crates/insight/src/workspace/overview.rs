//! 表画像概览版式：报告头 / 五卡 / 类型分布 / 值得关注 / 主体分派。

use super::*;

impl super::InsightWorkspacePanel {
    /// 中间页画像版式（D66）：按载荷分派——错误 / 取样加载 / 骨架 / 表画像 / 列画像 / 空态。
    ///
    /// 注意：与右栏 `InsightView` 的六 Tab 不同，中间页是**原型画像版式**：
    /// 表目标显示五卡 + 检查器命中环 + 列类型分布 + 列画像表 + 事实提示；
    /// 列目标复用列画像四区（`render_column_profile`，宽版）。
    pub(crate) fn render_insight_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        empty_icon: Pixels,
        ring_px: Pixels,
    ) -> Div {
        let colors = theme.colors;

        if let InsightPanelState::Error { message, retryable } = self.view.read(cx).state() {
            // 错误态重试按钮挂在画像实体上（reload 走 view 的取数链）
            return error_block(message, *retryable, self.view.clone(), theme, inline_icon);
        }
        if matches!(self.view.read(cx).state(), InsightPanelState::Loading) {
            return match self.view.read(cx).loading_note() {
                // 源目标取样：两段式进度（源取样 → 画像）
                Some(note) if !note.is_empty() => self.render_sample_progress(cx, theme),
                // 临时表目标画像通常很快：纯骨架
                _ => div().p(rems(ui::PANEL_PADDING)).child(skeleton(theme)),
            };
        }
        // 画像数据 clone 到局部（`render_table_overview` 需 `&mut Context` 建
        // `use_keyed_state`，不能再持有 `self.view.read(cx)` 的借用——借用结束在 clone 时）
        if let Some(profile) = self.view.read(cx).data().table.clone() {
            return self.render_table_overview(
                window,
                cx,
                &profile,
                entity,
                &theme,
                inline_icon,
                ring_px,
            );
        }
        if let Some(profile) = self.view.read(cx).data().column.as_ref() {
            return self.render_column_overview(cx, profile, theme, inline_icon);
        }
        // 无目标 / 无数据：入口引导
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::Eye)
                    .with_size(Size::Size(empty_icon))
                    .text_color(colors.muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child("暂无洞察目标"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .text_center()
                    .child("在结果列表头或导航树上右键表，选择「查看统计」"),
            )
    }

    /// 表画像 EDA 版式（对齐 `insight-eda-prototype.html` 中间页）：
    /// 报告头（kind/scope 徽标 + 取样↔全量切换）→ 概览五卡 → 类型分布 →
    /// 值得关注 → 报告 Tab 四段（列分布 / 缺失 / 关联性 / 样本）。
    ///
    /// 与原型的字段差异（数据模型无 → 用真实字段等价替代，不编假值）：
    /// - 原型「探索价值」分卡 → 概览第五卡「值得关注」列数（缺失 / 高基数现算）；
    /// - 原型列卡「直方图 / 极值」→ 列卡折叠态统计行（类型 / 缺失 / 唯一）；
    /// - 原型关联矩阵 → 关联性子 Tab（数值 / 分类走多列规则执行，值重叠走右栏集合）。
    pub(crate) fn render_table_overview(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        ring_px: Pixels,
    ) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .p(rems(ui::PANEL_PADDING))
            .child(self.render_report_head(cx, profile, entity, theme))
            .child(self.render_overview_cards(profile, theme))
            .child(self.render_type_bar(profile, theme))
            .child(self.render_worth_section(profile, &entity, theme))
            .child(self.render_eda_tabs(cx, entity, theme))
            .child(self.render_eda_body(window, cx, profile, entity, &theme, inline_icon, ring_px))
    }

    /// 报告头（原型 .report-head）：kind 徽标 + scope 徽标 + 取样↔全量切换 + 生成报告。
    pub(crate) fn render_report_head(
        &self,
        cx: &Context<Self>,
        _profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let scope = self.view.read(cx).scope();
        let is_full = scope == crate::insight_view::InsightScope::Full;
        // 取样↔全量切换：源目标才有「取样」概念；结果集 / 临时表目标天然全量（按钮置灰）
        let can_switch = matches!(
            self.view.read(cx).target(),
            Some(InsightTarget::SourceTable { .. }) | Some(InsightTarget::SourceColumn { .. })
        );
        div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(colors.border)
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("表画像"),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .bg(colors.sidebar)
                    .border_1()
                    .border_color(colors.border)
                    .text_xs()
                    .text_color(if is_full {
                        colors.info
                    } else {
                        colors.muted_foreground
                    })
                    .child(scope.label()),
            )
            .child(div().flex_1().min_w_0())
            .when(can_switch, |this| {
                let entity = entity.clone();
                let go_full = !is_full;
                this.child(
                    Button::new("insight-ws-scope")
                        .ghost()
                        .xsmall()
                        .icon(if go_full {
                            CatalogIcon::Expand
                        } else {
                            CatalogIcon::Shrink
                        })
                        .label(if go_full {
                            "全量统计"
                        } else {
                            "取样统计"
                        })
                        .tooltip(if go_full {
                            "大表默认取样 500 行；全量需手动触发（可能较慢）"
                        } else {
                            "切回取样口径（500 行）"
                        })
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| {
                                if go_full {
                                    panel.view.update(cx, |view, cx| view.request_full(cx));
                                } else {
                                    panel.view.update(cx, |view, cx| view.request_sample(cx));
                                }
                            });
                        }),
                )
            })
    }

    /// 概览五卡（原型 .overview-cards）：总行数 / 列数 / 缺失率 / 唯一率 / 值得关注。
    ///
    /// 缺失率与唯一率是**有统计摘要的列**的均值（无摘要列不参与，避免 0 假装没缺失）；
    /// 值得关注 = 缺失 > 20% 或（唯一率 > 0.9 且非主键）或类型未识别的列数。
    pub(crate) fn render_overview_cards(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let columns = &profile.columns;
        let total = columns.len().max(1);
        let with_stats = columns.iter().filter(|c| c.null_rate.is_some()).count();
        let missing_avg = if with_stats > 0 {
            columns.iter().filter_map(|c| c.null_rate).sum::<f64>() / with_stats as f64
        } else {
            0.0
        };
        let unique_avg = if with_stats > 0 {
            columns.iter().filter_map(|c| c.unique_rate).sum::<f64>() / with_stats as f64
        } else {
            0.0
        };
        let worth = columns
            .iter()
            .filter(|c| {
                c.null_rate.map(|n| n > 0.2).unwrap_or(false)
                    || (!c.primary_key && c.unique_rate.map(|u| u > 0.9).unwrap_or(false))
                    || c.kind == ColumnKind::Unknown
            })
            .count();
        div()
            .w_full()
            .flex()
            .items_stretch()
            .gap_2()
            .child(summary_card(
                &theme,
                "总行数",
                fmt_rows(profile.row_count),
                "当前口径行数",
            ))
            .child(summary_card(
                theme,
                "列数",
                total.to_string(),
                "含主键 / 索引",
            ))
            .child(summary_card(
                &theme,
                "缺失率",
                format!("{:.1}%", missing_avg * 100.0),
                "有统计列的均值",
            ))
            .child(summary_card(
                &theme,
                "唯一率",
                format!("{:.1}%", unique_avg * 100.0),
                "有统计列的均值",
            ))
            .child(summary_card(
                &theme,
                "值得关注",
                format!("{worth} / {total}"),
                "缺失 / 高基数 / 未识别",
            ))
    }

    /// 类型分布横向条（原型 .type-bar + 图例）：按类型族计数铺色条。
    pub(crate) fn render_type_bar(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let total = profile.columns.len().max(1);
        let counts: Vec<(ColumnKind, usize)> = [
            ColumnKind::Numeric,
            ColumnKind::Text,
            ColumnKind::DateTime,
            ColumnKind::Boolean,
            ColumnKind::Unknown,
        ]
        .into_iter()
        .map(|kind| {
            (
                kind,
                profile.columns.iter().filter(|c| c.kind == kind).count(),
            )
        })
        .collect();
        let bar_colors: Vec<Hsla> = vec![
            colors.success,
            colors.info,
            colors.warning,
            colors.primary,
            colors.muted_foreground,
        ];
        let mut segs = div().w_full().h_1().flex().overflow_hidden().rounded_full();
        for (ix, (_kind, count)) in counts.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let w_ratio = (*count as f32 / total as f32).max(0.01);
            // 类型占比条：相对父宽（`w(relative(..))`），不引入绝对像素
            segs = segs.child(div().h_full().w(relative(w_ratio)).bg(bar_colors[ix]));
        }
        let mut legend = div().h_flex().w_full().items_center().gap_3();
        for (ix, (kind, count)) in counts.iter().enumerate() {
            legend = legend.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(div().w_2().h_2().rounded_sm().bg(bar_colors[ix]))
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{} {}", kind.label(), count)),
                    ),
            );
        }
        div().v_flex().w_full().gap_1().child(segs).child(legend)
    }

    /// 值得关注区（原型 .worth-section）：缺失 / 高基数 / 未识别 / 检查器未命中，
    /// 点击行**展开该列卡并滚动定位**（原型「点击跳转到该列」）。
    pub(crate) fn render_worth_section(
        &self,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let mut lines = div().v_flex().w_full().gap_1();
        let mut added = 0;
        // 检查器未命中（评估后才有）：按未命中条数降序，提示列质量问题
        if let Some(quality) = &profile.quality {
            let mut by_col: Vec<(&str, usize)> = Vec::new();
            for check in &quality.checker {
                if check.state == ColumnCheckState::Miss {
                    if let Some((_, n)) =
                        by_col.iter_mut().find(|(c, _)| *c == check.column.as_str())
                    {
                        *n += 1;
                    } else {
                        by_col.push((check.column.as_str(), 1));
                    }
                }
            }
            by_col.sort_by(|a, b| b.1.cmp(&a.1));
            for (column, n) in by_col.iter().take(3) {
                lines = lines.child(self.render_worth_row(
                    entity,
                    IconName::CircleAlert,
                    format!("{column} 检查器未命中 {n} 条"),
                    column,
                    theme,
                ));
                added += 1;
            }
        }
        // 缺失 > 20% 的列（按缺失率降序）
        let mut missing: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| c.null_rate.map(|n| n > 0.2).unwrap_or(false))
            .collect();
        missing.sort_by(|a, b| {
            b.null_rate
                .partial_cmp(&a.null_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for col in missing.iter().take(3) {
            lines = lines.child(self.render_worth_row(
                entity,
                IconName::CircleAlert,
                format!(
                    "{} 缺失率 {:.0}%",
                    col.name,
                    col.null_rate.unwrap_or(0.0) * 100.0
                ),
                &col.name,
                theme,
            ));
            added += 1;
        }
        // 高基数（唯一率 > 0.9，非主键）
        let high_card: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| !c.primary_key && c.unique_rate.map(|u| u > 0.9).unwrap_or(false))
            .collect();
        for col in high_card.iter().take(3) {
            lines = lines.child(self.render_worth_row(
                entity,
                IconName::FileText,
                format!(
                    "{} 高基数（唯一率 {:.0}%）",
                    col.name,
                    col.unique_rate.unwrap_or(0.0) * 100.0
                ),
                &col.name,
                theme,
            ));
            added += 1;
        }
        // 未识别类型
        let unknown: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Unknown)
            .collect();
        for col in unknown.iter().take(3) {
            lines = lines.child(self.render_worth_row(
                entity,
                IconName::EyeOff,
                format!("{} 类型未识别（{}）", col.name, col.data_type),
                &col.name,
                theme,
            ));
            added += 1;
        }
        if added == 0 {
            lines = lines.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("未发现明显值得关注的列"),
            );
        }
        panel_box(theme, "值得关注", format!("{added} 项"), lines)
    }

    /// 值得关注行：点击展开该列卡 + 滚动定位（`ScrollAnchor::scroll_to` 下一帧生效）。
    ///
    /// 用标准 `Button`（ghost / xsmall）承载导航动作：自带 `track_focus` / `tab_stop` /
    /// Enter·Space 在 KeyUp 激活 / 焦点环 / `role=button` 与无障碍标签，不再手搓可点 div。
    /// 滚动锚点取渲染时快照；若该列不在当前筛选结果里则只展开不滚动（列卡不在树中，滚动无目标）。
    pub(crate) fn render_worth_row(
        &self,
        entity: &Entity<Self>,
        icon: IconName,
        text: String,
        column: &str,
        theme: &Theme,
    ) -> impl IntoElement {
        let colors = theme.colors;
        let anchor = self.col_anchors.borrow().get(column).cloned();
        let entity = entity.clone();
        let column = column.to_string();
        let label = text.clone();
        Button::new(SharedString::from(format!("insight-ws-worth-{column}")))
            .ghost()
            .xsmall()
            .w_full()
            .justify_start()
            .px_2()
            .py_0p5()
            .rounded(theme.radius)
            .accessibility_label(label.clone())
            .tooltip(text.clone())
            .child(
                Icon::new(icon)
                    .with_size(Size::Size(theme.font_size * ui::INSIGHT_INLINE_ICON_SIZE))
                    .text_color(colors.info),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("›"),
            )
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| {
                    panel.col_open.borrow_mut().insert(column.clone());
                    cx.notify();
                });
                if let Some(anchor) = &anchor {
                    anchor.scroll_to(window, app);
                }
            })
    }
}

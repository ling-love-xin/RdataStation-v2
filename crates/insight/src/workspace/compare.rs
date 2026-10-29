//! 多表对比域渲染：候选勾选 / 运行中 / 结果表。

use super::*;

impl super::InsightWorkspacePanel {
    /// 对比页（D66 三态对齐原型）：
    /// 候选态（勾选表 + 开始对比）→ 运行态（进度示意）→ 结果态（事实指标着色 + 点行开画像）。
    pub(crate) fn render_compare(&self, cx: &Context<Self>, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        if self.opened.is_empty() {
            return div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child("还没有可对比的目标"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("先在导航树或结果列表上打开一张表的洞察，之后它才会出现在这里"),
                );
        }

        if self.compare_running {
            return self.render_compare_running(cx, theme);
        }
        if self.compare_done {
            return self.render_compare_result(cx, entity, theme);
        }
        self.render_compare_candidates(cx, entity, theme)
    }

    /// 对比候选态：标题 + 已选计数 + 「开始对比」+ 候选表（勾选 / 表名 / 数据源 / 行数 / 列数 / 状态）。
    pub(crate) fn render_compare_candidates(
        &self,
        cx: &Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let current_key = self.view.read(cx).target().map(target_key);
        let current_loading = matches!(self.view.read(cx).state(), InsightPanelState::Loading);

        div()
            .v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::PANEL_HEADER_HEIGHT))
                    .px(rems(ui::PANEL_PADDING))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("多表洞察对比"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("已选择 {} 张表", self.compare_selected.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("insight-ws-compare-run")
                            .primary()
                            .small()
                            .icon(IconName::Play)
                            .label("开始对比")
                            .disabled(self.compare_selected.is_empty())
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| panel.start_compare(cx));
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .p(rems(ui::PANEL_PADDING))
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("候选 = 已进入洞察的目标；聚合会话内已完成的画像摘要，不重新取数（D64 阶段②）。"),
                            )
                            .child(compare_table_header(theme))
                            .children(self.opened.iter().map(|entry| {
                                let key = entry.key.clone();
                                let selected = self.compare_selected.contains(&key);
                                // 状态徽标：当前目标且加载中 → 分析中；有摘要 → 已分析；其余 → 未分析
                                let (label, color) = if Some(key.as_str()) == current_key.as_deref()
                                    && current_loading
                                {
                                    ("分析中", colors.info)
                                } else if entry.summary.is_some() {
                                    ("已分析", colors.success)
                                } else {
                                    ("未分析", colors.muted_foreground)
                                };
                                compare_row(theme, SharedString::from(format!("insight-ws-cand-{key}")))
                                    .child(
                                        Checkbox::new(SharedString::from(format!(
                                            "insight-ws-check-{key}"
                                        )))
                                        .checked(selected)
                                        .on_click({
                                            let entity = entity.clone();
                                            let key = key.clone();
                                            move |_, _, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.toggle_candidate(&key, cx)
                                                });
                                            }
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .text_color(colors.foreground)
                                            .text_ellipsis()
                                            .child(entry.target.title()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .text_ellipsis()
                                            .child(entry.target.detail().unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                entry
                                                    .summary
                                                    .and_then(|s| s.row_count)
                                                    .map(|n| fmt_rows(n))
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                entry
                                                    .summary
                                                    .and_then(|s| s.column_count)
                                                    .map(|n| n.to_string())
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_STATUS_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(color)
                                            .child(label),
                                    )
                                    .on_click({
                                        let entity = entity.clone();
                                        let key = key.clone();
                                        move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.open_candidate(&key, cx)
                                            });
                                        }
                                    })
                            })),
                    ),
            )
    }

    /// 对比运行态：进度条 + 每行「聚合中 / 排队中」（原型运行态；聚合无真实取数工序，
    /// 进度为示意，`start_compare` 600ms 后落结果）。
    pub(crate) fn render_compare_running(&self, _cx: &Context<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let selected: Vec<_> = self
            .opened
            .iter()
            .filter(|e| self.compare_selected.contains(&e.key))
            .collect();
        div()
            .v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::PANEL_HEADER_HEIGHT))
                    .px(rems(ui::PANEL_PADDING))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("多表洞察对比"),
                    )
                    .child(div().flex_1())
                    .child(
                        Progress::new("insight-ws-compare-progress")
                            .loading(true)
                            .with_size(Size::Small)
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH * 2.0)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .p(rems(ui::PANEL_PADDING))
                            .gap_1()
                            .children(selected.into_iter().enumerate().map(|(i, entry)| {
                                let running = i == 0;
                                div()
                                    .id(SharedString::from(format!("insight-ws-run-{}", entry.key)))
                                    .h(rems(ui::ROW_HEIGHT))
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_1()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .child(entry.target.title()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .text_color(if running {
                                                colors.info
                                            } else {
                                                colors.muted_foreground
                                            })
                                            .child(if running {
                                                "聚合中…"
                                            } else {
                                                "排队中"
                                            }),
                                    )
                            })),
                    ),
            )
    }

    /// 对比结果态：表名 / 数据源 / 行数 / 列数 / 检查器命中比 / 缺失率 / 唯一率 /
    /// 值得关注列数 + 点行重开画像。阈值（缺失率 `NULL_RATE_NOTE`、唯一率
    /// `CHECKER_UNIQUE_MAX`）由 `quality_scorer` 一处提供，不在视图里再写一份；
    /// 本表不呈现质量总分或等级。
    pub(crate) fn render_compare_result(
        &self,
        _cx: &Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let selected: Vec<_> = self
            .opened
            .iter()
            .filter(|e| self.compare_selected.contains(&e.key))
            .collect();
        div()
            .v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::PANEL_HEADER_HEIGHT))
                    .px(rems(ui::PANEL_PADDING))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("多表洞察对比"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{} 张表 · 已完成", selected.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("insight-ws-compare-again")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCw)
                            .label("重新对比")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| panel.reset_compare(cx));
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .p(rems(ui::PANEL_PADDING))
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("对比为列表式汇总：行数 / 列数 / 检查器命中 / 缺失率 / 唯一率 / 值得关注列数（无评级，评级在归档后的资产库）；点某行可在中间打开该表洞察工作页。"),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .w_full()
                                    .gap_2()
                                    .px_1()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(div().flex_1().child("表"))
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .child("数据源"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .child("行数"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .child("列数"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_FACT_WIDTH))
                                            .flex_none()
                                            .child("检查器"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_FACT_WIDTH))
                                            .flex_none()
                                            .child("缺失率"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_FACT_WIDTH))
                                            .flex_none()
                                            .child("唯一率"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .child("值得关注"),
                                    ),
                            )
                            .children(selected.into_iter().map(|entry| {
                                let s = entry.summary;
                                // 检查器命中：事实计数着色（全中=success / 全不中=danger / 未评估=muted）
                                let (checker_text, checker_color) =
                                    match (s.and_then(|s| s.checker_applicable), s.and_then(|s| s.checker_hits)) {
                                        (Some(applicable), Some(hits)) if applicable > 0 => (
                                            format!("{hits}/{applicable}"),
                                            if hits == applicable {
                                                colors.success
                                            } else if hits == 0 {
                                                colors.danger
                                            } else {
                                                colors.warning
                                            },
                                        ),
                                        _ => ("—".into(), colors.muted_foreground),
                                    };
                                // 缺失率 / 唯一率：均值超经验阈值标 warning
                                let miss_color = s
                                    .and_then(|s| s.null_rate)
                                    .map(|r| if r > NULL_RATE_NOTE { colors.warning } else { colors.muted_foreground })
                                    .unwrap_or(colors.muted_foreground);
                                let uniq_color = s
                                    .and_then(|s| s.unique_rate)
                                    .map(|r| if r > crate::quality_scorer::CHECKER_UNIQUE_MAX { colors.warning } else { colors.muted_foreground })
                                    .unwrap_or(colors.muted_foreground);
                                let attention_color = s
                                    .and_then(|s| s.attention_columns)
                                    .map(|n| if n > 0 { colors.danger } else { colors.muted_foreground })
                                    .unwrap_or(colors.muted_foreground);
                                div()
                                    .id(SharedString::from(format!(
                                        "insight-ws-res-{}",
                                        entry.key
                                    )))
                                    .h(rems(ui::ROW_HEIGHT))
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_1()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .child(entry.target.title()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .text_ellipsis()
                                            .child(entry.target.detail().unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                s.and_then(|s| s.row_count)
                                                    .map(|n| fmt_rows(n))
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                s.and_then(|s| s.column_count)
                                                    .map(|n| n.to_string())
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_FACT_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(checker_color)
                                            .child(checker_text),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_FACT_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(miss_color)
                                            .child(
                                                s.and_then(|s| s.null_rate)
                                                    .map(fmt_pct)
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_FACT_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(uniq_color)
                                            .child(
                                                s.and_then(|s| s.unique_rate)
                                                    .map(fmt_pct)
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(attention_color)
                                            .child(
                                                s.and_then(|s| s.attention_columns)
                                                    .map(|n| format!("{n} 列"))
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .on_click({
                                        let entity = entity.clone();
                                        let key = entry.key.clone();
                                        move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.open_candidate(&key, cx)
                                            });
                                        }
                                    })
                            })),
                    ),
            )
    }
}

/// 对比行原语：居中行 + 行高（列内容由调用方追加）。
///
/// 带 `id` 后是 `Stateful<Div>`：hover / 点击按 id 记录状态（业务键，非下标）。
pub(crate) fn compare_row(theme: &Theme, id: SharedString) -> gpui::Stateful<Div> {
    let colors = theme.colors;
    div()
        .id(id)
        .h(rems(ui::ROW_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .rounded_sm()
        .hover(|this| this.bg(colors.sidebar_accent))
}

/// 对比候选表头（勾选 / 表名 / 数据源 / 行数 / 列数 / 状态）
pub(crate) fn compare_table_header(theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(div().w(rems(1.25)).flex_none())
        .child(div().flex_1().child("表"))
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                .flex_none()
                .child("数据源"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                .flex_none()
                .child("行数"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                .flex_none()
                .child("列数"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_STATUS_WIDTH))
                .flex_none()
                .child("状态"),
        )
}

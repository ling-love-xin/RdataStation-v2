//! 多表对比域渲染：候选勾选 / 运行中 / 结果表。

use super::*;

impl super::InsightWorkspacePanel {
    /// 对比页（D66 三态对齐原型）：
    /// 候选态（勾选表 + 开始对比）→ 运行态（进度示意）→ 结果态（事实指标着色 + 点行开画像）。
    pub(crate) fn render_compare(
        &self,
        cx: &Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
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
                            .child(compare_table_scroll(
                                ui::INSIGHT_WS_CANDIDATE_MIN_WIDTH,
                                compare_table_header(theme),
                                self.opened.iter().map(|entry| {
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
                                            // 「表」列不随容器无限压缩：给可读下限，窄窗由外层横向滚动兜住
                                            .min_w(rems(ui::INSIGHT_WS_TABLE_COL_MIN_WIDTH))
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
                            }))),
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
    ///
    /// 八列合计的刚性宽远超中央 dock 在窄窗下的可用宽，故表体走
    /// `compare_table_scroll`（内容低于最小可读宽时横向滚动，不再整列切掉）。
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
                                    .child("并排对比：**指标为行、每张勾选表为一列**（原型 .cmp-table）。行数 / 列数 / 检查器命中 / 缺失率 / 唯一率 / 值得关注列数——全为事实指标，无评级（评级归归档后的资产库）。点表头可打开该表洞察工作页。"),
                            )
                            .child(compare_table_scroll(
                                // 列数随勾选表数变化 → 最小可读宽只能在运行时按表数算
                                ui::metric_table_min_width(selected.len()),
                                div()
                                    .h_flex()
                                    .w_full()
                                    .gap_2()
                                    .px_1()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_METRIC_LABEL_WIDTH))
                                            .flex_none()
                                            .child("指标"),
                                    )
                                    .children(selected.iter().map(|entry| {
                                        div()
                                            .id(SharedString::from(format!(
                                                "insight-ws-cmp-hd-{}",
                                                entry.key
                                            )))
                                            .flex_1()
                                            .min_w(rems(ui::INSIGHT_WS_METRIC_COL_MIN_WIDTH))
                                            .text_ellipsis()
                                            .on_click({
                                                let entity = entity.clone();
                                                let key = entry.key.clone();
                                                move |_, _, app| {
                                                    entity.update(app, |panel, cx| {
                                                        panel.open_candidate(&key, cx)
                                                    });
                                                }
                                            })
                                            .child(entry.target.title())
                                    })),
                                metric_rows(&selected, theme),
                            ))
                    ),
            )
    }
}

/// 对比表（表头 + 数据行）的横向滚动容器。
///
/// 中间工作页的可用宽随窗口宽与左右 dock 收放变化，而表体是**刚性列宽**的合计
/// （各列 `w(rems(..))`，见 `ui::INSIGHT_WS_*_MIN_WIDTH` 的推导）。外层只有纵向
/// 滚动时，窄窗下右侧列会被整列切掉且**够不到**——这里给 `min_w` 定「最小可读宽」：
/// 容器够宽时 `w_full` 铺满不出滚动条，不够时才横向滚动。
pub(crate) fn compare_table_scroll(
    min_width: f32,
    header: Div,
    rows: impl IntoIterator<Item = impl IntoElement>,
) -> impl IntoElement {
    div().flex_none().w_full().overflow_x_scrollbar().child(
        div()
            .v_flex()
            .w_full()
            .min_w(rems(min_width))
            .gap_0p5()
            .child(header)
            .children(rows),
    )
}

/// 对比行原语：居中行 + 行高（列内容由调用方追加）。
///
/// 带 `id` 后是 `Stateful<Div>`：hover / 点击按 id 记录状态（业务键，非下标）。
/// 转置对比表的指标行（原型 `.cmp-table`）：**指标为行、每张勾选表为一列**。
///
/// 与旧版「表为行、指标为列」相反：指标集合固定 6 项，表数才是变量，故窄窗下横向
/// 增长的是列而不是行，读起来是「同一指标横着比几张表」而不是「一张表竖着看七个
/// 指标」。列宽随表数变化，由 `ui::metric_table_min_width` 兜底横向滚动。
fn metric_rows(entries: &[&OpenedTarget], theme: &Theme) -> Vec<Div> {
    let colors = theme.colors;
    // 六项事实指标；无摘要（未完成 / 非表目标）时统一出占位「—」而不是编 0
    let metrics: Vec<(&'static str, Box<dyn Fn(&TargetSummary) -> (String, Hsla)>)> = vec![
        (
            "总行数",
            Box::new(move |s: &TargetSummary| {
                (
                    s.row_count.map(fmt_rows).unwrap_or_else(|| "—".into()),
                    colors.muted_foreground,
                )
            }),
        ),
        (
            "列数",
            Box::new(move |s: &TargetSummary| {
                (
                    s.column_count
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "—".into()),
                    colors.muted_foreground,
                )
            }),
        ),
        (
            "检查器命中",
            Box::new(
                move |s: &TargetSummary| match (s.checker_applicable, s.checker_hits) {
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
                },
            ),
        ),
        (
            "缺失率",
            Box::new(move |s: &TargetSummary| match s.null_rate {
                Some(r) => (
                    fmt_pct(r),
                    if r > NULL_RATE_NOTE {
                        colors.warning
                    } else {
                        colors.muted_foreground
                    },
                ),
                None => ("—".into(), colors.muted_foreground),
            }),
        ),
        (
            "唯一率",
            Box::new(move |s: &TargetSummary| match s.unique_rate {
                Some(r) => (
                    fmt_pct(r),
                    if r > crate::quality_scorer::CHECKER_UNIQUE_MAX {
                        colors.warning
                    } else {
                        colors.muted_foreground
                    },
                ),
                None => ("—".into(), colors.muted_foreground),
            }),
        ),
        (
            "值得关注",
            Box::new(move |s: &TargetSummary| match s.attention_columns {
                Some(n) => (
                    format!("{n} 列"),
                    if n > 0 {
                        colors.danger
                    } else {
                        colors.muted_foreground
                    },
                ),
                None => ("—".into(), colors.muted_foreground),
            }),
        ),
    ];

    metrics
        .into_iter()
        .map(|(label, value_of)| {
            let mut row = div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .px_1()
                .h(rems(ui::ROW_HEIGHT))
                .text_sm()
                .child(
                    div()
                        .w(rems(ui::INSIGHT_WS_METRIC_LABEL_WIDTH))
                        .flex_none()
                        .text_color(colors.foreground)
                        .text_ellipsis()
                        .child(label),
                );
            for entry in entries {
                let (text, color) = match entry.summary {
                    Some(s) => value_of(&s),
                    None => ("—".into(), colors.muted_foreground),
                };
                row = row.child(
                    div()
                        .flex_1()
                        .min_w(rems(ui::INSIGHT_WS_METRIC_COL_MIN_WIDTH))
                        .text_xs()
                        .text_color(color)
                        .text_ellipsis()
                        .child(text),
                );
            }
            row
        })
        .collect()
}

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
        .child(div().w(rems(ui::INSIGHT_WS_CHECKBOX_WIDTH)).flex_none())
        .child(
            div()
                .flex_1()
                .min_w(rems(ui::INSIGHT_WS_TABLE_COL_MIN_WIDTH))
                .child("表"),
        )
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

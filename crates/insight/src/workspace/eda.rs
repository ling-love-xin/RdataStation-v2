//! EDA 主域渲染：报告 Tab（列分布 / 缺失矩阵）与列卡各分区。

use super::*;
// 完整 Lucide 图标目录在 `gpui_kit::assets::IconName`（component 版只是保留子集，
// 没有 Braces）；别名避免与 `super::*` 带来的 component 版同名冲突
use gpui_kit::component::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};

impl super::InsightWorkspacePanel {
    /// 报告主 Tab（原型 .report-tabs）：列分布 / 缺失 / 关联性 / 样本。
    pub(crate) fn render_eda_tabs(
        &self,
        _cx: &Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
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
        // 迷你图数据源按口径分流（Q2 / C7）：全量档读评估明细（stats_detail /
        // histogram），取样档读查询级过滤现算的迷你统计（mini_stats）——两档互不覆盖
        let is_full = self.view.read(cx).scope() == crate::insight_view::InsightScope::Full;
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
        // 列卡**一列一行**（2026-11-06 用户拍板，对齐 YData）：每张卡占满整行——
        // 折叠行（folded.rs）的统计块 + 右端 240px 迷你图就是按全宽设计的，
        // 半宽格子放不下；双列网格（原型 .col-grid）废弃。
        // 渐进渲染（建议 7 的低风险缓解）：超过上限默认只渲染前 CAP 张，
        // 尾部给「显示全部」——锚点跳转 / 折叠态与全量渲染完全一致
        let capped = !*self.col_show_all.borrow() && selected.len() > ui::INSIGHT_WS_COL_RENDER_CAP;
        let render_list: &[&TableColumnView] = if capped {
            &selected[..ui::INSIGHT_WS_COL_RENDER_CAP]
        } else {
            &selected
        };
        let mut cards = div().v_flex().w_full().gap_2();
        for col in render_list {
            // 滚动锚点注册：复用同一 `ScrollAnchor` 实例（anchor_scroll 靠它持续
            // 更新 last_origin；每帧重建会让滚动位置永远停在原处）
            self.col_anchors
                .borrow_mut()
                .entry(col.name.clone())
                .or_insert_with(|| ScrollAnchor::for_handle(self.content_scroll.clone()));
            let anchor = self.col_anchors.borrow().get(&col.name).cloned();
            let col_name = col.name.clone();
            cards = cards.child(
                div().w_full().child(
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
                            is_full,
                        )),
                ),
            );
        }
        if capped {
            let entity = entity.clone();
            let total = selected.len();
            cards = cards.child(
                div().w_full().flex_none().child(
                    Button::new("insight-ws-col-show-all")
                        .ghost()
                        .xsmall()
                        .w_full()
                        .label(format!(
                            "显示全部 {total} 列（当前渲染前 {} 张卡）",
                            ui::INSIGHT_WS_COL_RENDER_CAP
                        ))
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| {
                                *panel.col_show_all.borrow_mut() = true;
                                cx.notify();
                            });
                        }),
                ),
            );
        }
        if selected.is_empty() {
            // 无匹配列：标准 Empty 组件（对齐 database 导航「没有匹配的数据源」
            // 先例——筛选无结果用组件空态，不手搓居中文本）
            cards = cards.child(
                div().w_full().child(
                    Empty::new().header(
                        EmptyHeader::new()
                            .title(
                                EmptyTitle::new().child(SharedString::from("没有符合筛选条件的列")),
                            )
                            .description(
                                EmptyDescription::new()
                                    .child(SharedString::from("调整搜索词或类型筛选后重试")),
                            ),
                    ),
                ),
            );
        }
        div()
            .v_flex()
            .w_full()
            .gap_2()
            // C6 重新探查入口：走同一确认窗口（预填上次配置）；确认后重跑建表
            // （数据量 / 列 / 过滤可改）→ 新表就绪一次性替换（R5），并级联重评估
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("当前视图基于确认窗口生成的表；调整数据量 / 列 / 过滤请重新探查"),
                    )
                    .child({
                        let entity = entity.clone();
                        Button::new("insight-ws-reprobe")
                            .ghost()
                            .xsmall()
                            .icon(IconName::RotateCw)
                            .label("重新探查")
                            .tooltip("复用上次探查配置（可改数据量 / 列 / 过滤），确认后重新取数并级联重评估")
                            .on_click(move |_, _, app| {
                                entity.update(app, |panel, cx| {
                                    panel.view.update(cx, |view, cx| {
                                        if let Some(target) = view.target().cloned() {
                                            view.open_probe_dialog(target, cx);
                                        }
                                    });
                                });
                            })
                    }),
            )
            .child(self.render_field_picker(window, cx, profile, entity, theme))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("显示 {} / {} 列", selected.len(), columns.len())),
            )
            .child(cards)
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

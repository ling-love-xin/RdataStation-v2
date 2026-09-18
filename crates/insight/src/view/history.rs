//! 快照历史（Tab「历史」，Phase 5.1 / 5.2）：保存入口 + 版本列表 + 对比面板 + 清理 + 存储用量
//!
//! 列表**不另设内层滚动**：面板主体已经是滚动区（`#insight-body`），再来一层就是嵌套滚动
//! （滚轮到底后停住、外层接不上）。高度上限靠分页（`HISTORY_PAGE_SIZE`）而不是固定高度。
//!
//! 对比方向固定为「选中 → 最新」：最新一版不可点（没有更新的版本可比）。行上的方向标记
//! **不预设好坏**（方向 ≠ 好坏：空值率升就是坏，行数升通常不是）。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::description_list::{DescriptionItem, DescriptionList, DescriptionText};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::{Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::*;

use super::{muted_line, section_title};
use crate::insight_view::InsightView;
use crate::model::{
    DeltaView, DiffRowView, HistoryEntryView, HistoryView, VersionDiffView, HISTORY_PAGE_SIZE,
};
use crate::ui;

impl InsightView {
    /// 快照历史（Tab「历史」，Phase 5.1 / 5.2）：保存入口 + 版本列表 + 对比面板 + 存储用量。
    ///
    /// 列表**不另设内层滚动**：面板主体已经是滚动区（`#insight-body`），再来一层
    /// 就是嵌套滚动（滚轮到底后停住、外层接不上）。高度上限靠分页
    /// （[`HISTORY_PAGE_SIZE`]）而不是靠固定高度。
    pub(in crate::view) fn render_history(
        &self,
        history: &HistoryView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;

        // 头行：标题 + 保存（保存入口必须有：v1 的保存函数前端不可达，原型 §3.5）
        let mut body = div().v_flex().w_full().gap_2().child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(section_title("快照历史", theme)),
                )
                .child(
                    Button::new("insight-history-cleanup")
                        .ghost()
                        .small()
                        // 删快照不可撤销：没项目、没快照、或正在跑动作时不给入口
                        .label(if self.history_cleaning {
                            "清理中…"
                        } else {
                            "清理"
                        })
                        .disabled(!self.project_open || !self.history_busy_ready(history))
                        .tooltip(if !self.project_open {
                            "清理旧快照（需先打开项目）"
                        } else if history.is_empty() {
                            "还没有快照可清理"
                        } else {
                            "删掉 30 天前的快照（正文与版本链一并删，不可撤销）"
                        })
                        .on_click({
                            let entity = entity.clone();
                            move |_, window, app| {
                                entity.update(app, |view, cx| view.begin_cleanup(window, cx))
                            }
                        }),
                )
                .child(
                    Button::new("insight-history-save")
                        .small()
                        .label(if self.history_saving {
                            "保存中…"
                        } else {
                            "保存"
                        })
                        // 快照落项目目录：无项目时不给入口，而不是给一个点了没用的按钮
                        .disabled(!self.project_open || self.history_saving)
                        .tooltip(if self.project_open {
                            "把当前列的画像存一份（正文进项目 DuckDB，版本链进项目 SQLite）"
                        } else {
                            "保存快照（需先打开项目）"
                        })
                        .on_click({
                            let entity = entity.clone();
                            move |_, _, app| {
                                entity.update(app, |view, cx| view.request_snapshot_save(cx))
                            }
                        }),
                ),
        );

        if let Some(notice) = &self.history_notice {
            body = body.child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(Icon::new(IconName::TriangleAlert).size(inline_icon))
                    .child(div().flex_1().min_w_0().child(notice.clone())),
            );
        }

        // 清理回执（一次性动作的结果）：两侧条数对不上时转 danger——那是半写的信号
        if let Some(outcome) = &history.cleanup {
            body = body.child(
                div()
                    .w_full()
                    .text_xs()
                    .text_color(if outcome.is_balanced() {
                        colors.muted_foreground
                    } else {
                        colors.danger
                    })
                    .child(outcome.summary()),
            );
        }

        // 版本列表：顺序由视图模型给（存储层 `ORDER BY` 是唯一权威），渲染不再排
        if history.is_empty() {
            body = body.child(muted_line("还没有快照，点「保存」留一份", theme));
        } else {
            let mut list = div().v_flex().w_full();
            for entry in &history.entries {
                list = list.child(history_entry_row(
                    entry,
                    entity,
                    self.compare_target.as_deref() == Some(entry.version_id.as_str()),
                    theme,
                ));
            }
            body = body.child(list);

            // 上一行是唯一不能点的那一行（没有更新的版本可比）；多版时教一次怎么用
            if history.entries.len() < 2 {
                body = body.child(muted_line("再存一版就能对比", theme));
            } else if history.diff.is_none() && self.compare_target.is_none() {
                body = body.child(muted_line("点更早的一版，和当前对比", theme));
            }

            if history.truncated {
                body = body.child(muted_line(
                    &format!("只列出最近 {HISTORY_PAGE_SIZE} 条"),
                    theme,
                ));
            }
        }

        // 对比面板：出现与否**只认载荷**（点过但结果未到时给一行取数提示）
        if let Some(diff) = &history.diff {
            body = body.child(self.render_version_diff(diff, entity, theme));
        } else if self.compare_target.is_some() {
            body = body.child(muted_line("读取版本正文…", theme));
        }

        // 存储用量：拿不到就整行不显示（编一个 0 会让人以为历史被清了）
        if let Some(line) = history.stats_line() {
            body = body.child(muted_line(&line, theme));
        }

        body
    }

    /// 清理入口能不能点：项目开着、列表里有东西、且当前没有别的历史动作在跑
    /// （保存与清理都在动同一批快照，不允许并行）
    pub(crate) fn history_busy_ready(&self, history: &HistoryView) -> bool {
        !history.is_empty() && !self.history_saving && !self.history_cleaning
    }

    /// 版本对比面板（Phase 5.2）：头行（与谁比 + 关掉）+ 逐字段 `旧 → 新` + 差值。
    ///
    /// 行集合来自视图模型（与「列」Tab 的基础统计同源），这里只负责把
    /// **方向**映射到主题角色：方向 ≠ 好坏，所以不用 `success` / `danger` 预设评价；
    /// 不变的那一档要安静（它是大多数行）。
    fn render_version_diff(
        &self,
        diff: &VersionDiffView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;

        let head = div()
            .h_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(colors.foreground)
                    .text_ellipsis()
                    .child(format!("与 {} 对比", diff.baseline_label)),
            )
            .child(
                Button::new("insight-diff-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("关掉对比")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| entity.update(app, |view, cx| view.dismiss_diff(cx))
                    }),
            );

        // 逐行：`旧 → 新` + 差值（两侧的展示串与「列」Tab 同源，不另立一套口径）
        let mut rows = DescriptionList::new()
            .columns(1)
            .bordered(false)
            .with_size(Size::Small)
            .label_width(rems(ui::INSIGHT_DIFF_LABEL_WIDTH));
        for row in &diff.rows {
            let value = diff_row_value(row, theme).into_any_element();
            rows = rows.child(
                DescriptionItem::new(row.label.clone()).value(DescriptionText::AnyElement(value)),
            );
        }

        div()
            // 测试锚点：`debug_bounds` 只认 `debug_selector`（非测试构建 no-op）
            .debug_selector(|| "insight-version-diff".to_string())
            .v_flex()
            .w_full()
            .gap_1()
            .p_2()
            .rounded_sm()
            .bg(colors.list_hover)
            .child(head)
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(diff.summary()),
            )
            .child(rows)
    }
}

/// 历史列表的一行：时间 + 短版本号 + 类型 + 首版/当前标记 + 选中态。
///
/// 短版本号必须露出来：`created_at` 只有秒级精度（D18），同一秒存两次快照时
/// 时间戳会重复，到那时告状 / 比对的靠的只能是这个 id。
///
/// 用 `ListItem` 而不是手搭 div：hover / 选中 / 禁用三态与其它列表保持一套视觉，
/// 键盘与 a11y 也不用手接。**最新一版不可点**（没有更新的版本可比）。
fn history_entry_row(
    entry: &HistoryEntryView,
    entity: &Entity<InsightView>,
    selected: bool,
    theme: &Theme,
) -> ListItem {
    let colors = theme.colors;
    let mut row = ListItem::new(ElementId::Name(
        format!("insight-history-{}", entry.version_id).into(),
    ))
        .selected(selected)
        .disabled(entry.is_latest)
        .px_1()
        .text_xs()
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(colors.foreground)
                        .child(entry.created_at.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.muted_foreground)
                        .child(entry.short_version.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.muted_foreground)
                        .child(entry.data_type.clone()),
                ),
        );

    // 标记只给「值得一眼看出」的几行：最新一版、首版、链被剪过的头部。
    // 其余行都挂在链上（有父版本），逐行打标只是噪声。
    if entry.is_latest {
        row = row.child(history_chip("当前", colors.primary, theme));
    } else if !entry.has_parent {
        row = row.child(history_chip("首版", colors.muted_foreground, theme));
    } else if entry.chain_truncated {
        // K14：清理剪掉了链的起段——**不谎称首版**，也不假装链是完整的。
        // 数据层面不改（父版号是真的），只在界面上如实说明。
        row = row.child(history_chip("更早的版本已清理", colors.muted_foreground, theme));
    }

    // 最新那行不给点击：方向固定为「选中 → 最新」，拿最新当基准无从比
    if entry.is_latest {
        return row;
    }
    // 闭包要 `'static`：实体克隆一份进闭包（引用会逃出函数体）
    let entity = entity.clone();
    let version_id = entry.version_id.clone();
    row.on_click(move |_, _window, app| {
        entity.update(app, |view, cx| view.toggle_compare_version(&version_id, cx));
    })
}

/// 版本行上的小标记（文案色 + 淡底；不写裸 hex）
fn history_chip(text: &str, color: Hsla, theme: &Theme) -> Div {
    div()
        .flex_none()
        .px_1()
        .rounded_sm()
        .bg(theme.colors.list_hover)
        .text_color(color)
        .child(text.to_string())
}

/// 对比的一行值：`旧 → 新` + 差值（方向用箭头与色档区分，颜色不预设好坏）
fn diff_row_value(row: &DiffRowView, theme: &Theme) -> Div {
    let colors = theme.colors;
    let color = match row.delta {
        // 方向 ≠ 好坏：上升不一定是好事（空值率升就是坏），所以只用中性色
        DeltaView::Up(_) => colors.info,
        DeltaView::Down(_) => colors.warning,
        DeltaView::Same => colors.muted_foreground,
        DeltaView::Changed => colors.primary,
    };
    let marker = match row.delta {
        DeltaView::Up(_) => "▲",
        DeltaView::Down(_) => "▼",
        DeltaView::Same => "●",
        DeltaView::Changed => "◆",
    };

    div()
        .h_flex()
        .w_full()
        .gap_2()
        .text_xs()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .text_color(colors.muted_foreground)
                .child(format!("{} → {}", row.old, row.new)),
        )
        .child(
            div()
                .flex_none()
                .text_color(color)
                .child(format!("{} {marker}", row.delta.text())),
        )
}

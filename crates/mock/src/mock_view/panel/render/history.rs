//! 生成历史段：最近若干次运行（重放 / 删除都在事件路径上交给后台）。

use super::*;

impl MockPanel {
    /// 生成历史段：最近若干次运行（重放 / 删除都在事件路径上交给后台）。
    ///
    /// 列表与错误分开呈现：读失败时**保留旧列表**（能看到的旧数据比一片空白有用）。
    /// 行按 `created_at` 倒序（存储层已排），每行的 id 用任务 id——用得下标会在
    /// 「最新在前」的插入下把 hover 等按 id 记的状态串行。
    pub(crate) fn render_history(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // 主题一次 clone：下面循环里还要可变借用 `cx`（`cx.entity()` / `use_keyed_state`）
        let theme = cx.theme().clone();
        let fg = theme.colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let danger = cx.theme().colors.danger;
        let success = cx.theme().colors.success;

        let header = {
            let entity = cx.entity();
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(div().flex_1().min_w_0().child(section(
                    &theme,
                    format!("生成历史（最近 {} 次）", crate::history::HISTORY_LIMIT),
                )))
                .child(
                    button("mock-history-refresh", "刷新", ActionKind::Quiet)
                        .xsmall()
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.refresh_history(cx));
                        }),
                )
        };

        let mut section = div().v_flex().gap_1().w_full().child(header);
        if let Some(error) = self.history_error.clone() {
            section = section.child(error_line(&theme, error));
        }
        if self.history_loading && !self.history_loaded {
            // 不确定加载给一个 `Spinner`（小号转圈），文字保留便于弱网可读与测试断言
            return section.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(Spinner::new().small())
                    .child(div().text_xs().text_color(muted).child("读取中…")),
            );
        }
        if self.history_loaded && self.history.is_empty() {
            if self.history_error.is_none() {
                section = section.child(
                    Empty::new().header(
                        EmptyHeader::new()
                            // 历史空态给语义图标（收件箱 = 记录的去处）
                            .media(
                                EmptyMedia::new()
                                    .with_variant(EmptyMediaVariant::Icon)
                                    .child(
                                        Icon::new(IconName::Inbox)
                                            .size(rems(ui::PANEL_ICON_SIZE)),
                                    ),
                            )
                            .title(EmptyTitle::new().child("还没有生成记录"))
                            .description(
                                EmptyDescription::new().child("跑一次生成后，记录会出现在这里"),
                            ),
                    ),
                );
            }
            return section;
        }

        for task in self.history.clone() {
            let rows = task.generated_rows.unwrap_or(task.row_count).max(0);
            let rows = with_thousands(rows as u64);
            let stamp = task
                .created_at
                .as_deref()
                .map(crate::history::time_label)
                .unwrap_or_default();
            let stamp = (!stamp.is_empty())
                .then(|| div().flex_none().text_xs().text_color(muted).child(stamp));
            // 状态文案按语义取色：失败的原因原文展出来（截断由布局管）
            let (status_text, status_color) = match task.status.as_str() {
                // S5：场景历史行标注「场景 X · seed Y」（模板名已作为行标题展示）
                "success" if task.scene_id.is_some() => (
                    format!(
                        "成功 · seed {}",
                        task.seed
                            .map(|seed| seed.to_string())
                            .unwrap_or_else(|| "随机".to_string())
                    ),
                    success,
                ),
                "success" if task.save_format.as_deref() == Some(crate::history::SAVE_FORMAT_TABLE) => {
                    (format!("成功 · {rows} 行 · 已追加"), success)
                }
                "success" => (format!("成功 · {rows} 行"), success),
                _ => (
                    task.error_message
                        .clone()
                        .unwrap_or_else(|| "失败".to_string()),
                    danger,
                ),
            };

            let replay = {
                let entity = cx.entity();
                let id = task.id.clone();
                button(
                    SharedString::from(format!("mock-history-replay-{id}")),
                    "重放",
                    ActionKind::Quiet,
                )
                .xsmall()
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.replay_history(id, cx));
                })
            };
            let delete = {
                let entity = cx.entity();
                let id = task.id.clone();
                danger_text_button(
                    SharedString::from(format!("mock-history-delete-{id}")),
                    "删除",
                    cx.theme(),
                )
                .on_click(move |_, _, app| {
                    let id = id.clone();
                    entity.update(app, |panel, cx| panel.delete_history(id, cx));
                })
            };

            // 历史卡片：与模板卡片、列卡片同走 `card` 原语。
            // 操作（重放 / 删除）悬停 / 聚焦才显形——同一模块里三种清单必须同一口径。
            let card_handle = window
                .use_keyed_state(
                    ElementId::Name(SharedString::from(format!("mock-history-card-{}", task.id))),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx);
            let card_focused = card_handle.is_focused(window);
            let active = self.hovered_history.as_deref() == Some(task.id.as_str()) || card_focused;
            section = section.child(
                card_interactive(&theme)
                    .id(ElementId::Name(SharedString::from(format!(
                        "mock-history-{}",
                        task.id
                    ))))
                    .track_focus(&card_handle)
                    .tab_stop(true)
                    .when(card_focused, |c| c.border_color(theme.colors.ring))
                    .on_hover({
                        let entity = cx.entity();
                        let hover_id = task.id.clone();
                        move |&hovering, _, app| {
                            entity.update(app, |panel, cx| {
                                let next = hovering.then(|| hover_id.clone());
                                if panel.hovered_history != next {
                                    panel.hovered_history = next;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .text_ellipsis()
                                    .child(task.table_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(text_hint(&theme))
                                    .child(format!("{rows} 行")),
                            ),
                    )
                    .child(card_divider(&theme))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .children(stamp)
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(status_color)
                                            .text_ellipsis()
                                            .child(status_text),
                                    ),
                            )
                            .child(if active {
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_none()
                                    .child(replay)
                                    .child(delete)
                            } else {
                                div().flex_none().w(rems(ui::ROW_OPS_WIDTH))
                            }),
                    ),
            );
        }
        section
    }
}

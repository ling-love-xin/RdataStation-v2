//! 提示条：通知、索引问题、撤销归档与状态栏。

use super::*;

impl ResourcesPanel {
    pub(super) fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.clone()?;
        let (border, warning, info) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.info)
        };
        // 只读是约束提示（warning），其余是信息（info）——同一字段不一律同色。
        let color = if self.snapshot.read_only {
            warning
        } else {
            info
        };
        Some(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .border_b(ui::HAIRLINE)
                .border_color(border)
                .text_color(color)
                .child(notice)
                .into_any_element(),
        )
    }
    /// 归档撤销栏（原型 §4.1）：给"把文件从工作区搬走"一个立即反悔的窗口。
    ///
    /// 固定在状态行**上方**（与 M5 撤销栏同位）：它是刚发生那次动作的出口，不能随列表滚走；
    /// 文案直接引用显示名，用户不用回想刚搬的是哪一条。
    pub(super) fn render_undo_bar(&self, undo: &ArchiveUndo, cx: &mut Context<Self>) -> Div {
        let (foreground, primary, hover_bg) = {
            let colors = cx.theme().colors;
            (colors.foreground, colors.primary, colors.list_hover)
        };
        let host = self.host.clone();
        let token = undo.clone();
        div()
            .flex_none()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            // 栏高保持 `ROW_HEIGHT`（24px）：`py_1` + 20px 的按钮会多出 4px，“贴着状态行”的
            // 这条栏一高一矮会跟着上下跳。
            .py_0p5()
            .bg(hover_bg)
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .text_color(foreground)
                    .child(format!("已归档「{}」", undo.name)),
            )
            .child(
                // 撤销是**唯一**的入口（`Ctrl+Z` 未绑），所以它得是语义按钮：自绘 div 没有
                // hover / 焦点 / 键盘；`Button` 自带 `track_focus` + `tab_stop`，Enter/Space 可激活。
                Button::new("archive-undo")
                    .ghost()
                    .xsmall()
                    .debug_selector(|| "archive-undo".to_string())
                    .label("撤销")
                    .text_color(primary)
                    .tooltip("把这条存档退回归档前的位置")
                    .on_click(move |_, window, cx| host.request_undo_archive(&token, window, cx)),
            )
    }
    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> Div {
        let (border, warning, muted) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.muted_foreground)
        };
        let counts = self.snapshot.counts;
        let has_issues = counts.has_issues();
        // 「全局」实例下计数行换成全局口径（`ArchiveCounts` 是项目专用：kind 桶 /
        // 异常桶只对项目行有意义）；异常入口同理只在项目口径下摆。
        let counts_line = match self.instance {
            InstanceFilter::Global => {
                let n = self.snapshot.global_rows.len();
                if self.loading {
                    format!("加载中… · 全局 {n} 项")
                } else {
                    format!("全局 {n} 项")
                }
            }
            _ => counts.status_line(self.loading),
        };
        let has_issues = has_issues && self.instance != InstanceFilter::Global;
        let host = self.host.clone();

        div()
            .flex_none()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .justify_between()
            .px_2()
            .border_t(ui::HAIRLINE)
            .border_color(border)
            .child(
                div()
                    .text_xs()
                    .text_color(if has_issues { warning } else { muted })
                    .child(counts_line),
            )
            .when(has_issues, |bar| {
                bar.child(
                    Button::new("index-repair")
                        .ghost()
                        .label("修复…")
                        .on_click(move |_, window, cx| host.request_index_repair(window, cx)),
                )
            })
    }
    /// D2：索引异常首次进入的一次性提示条（仅项目实例；全局实例没有本地索引口径）。
    ///
    /// 与状态行「修复…」同源数据：这里只在首次进入时把异常"叫出来"一次，关闭后会话内
    /// 不再出现；纯内存标记，不进快照、不进索引统计口径。
    pub(super) fn render_index_issue_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.instance == InstanceFilter::Global {
            return None;
        }
        let counts = &self.snapshot.counts;
        if !counts.has_issues() || self.announced_index_issue.get() {
            return None;
        }
        let (warning, muted, border) = {
            let colors = cx.theme().colors;
            (colors.warning, colors.muted_foreground, colors.border)
        };
        let mut parts = Vec::new();
        if counts.missing > 0 {
            parts.push(format!("本体缺失 {} 项", counts.missing));
        }
        if counts.drifted > 0 {
            parts.push(format!("内容已变 {} 项", counts.drifted));
        }
        let summary = parts.join("、");
        Some(
            div()
                .flex_none()
                .w_full()
                .h_flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .border_b(ui::HAIRLINE)
                .border_color(border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(warning)
                        .child(format!("发现{summary}，可在状态行「修复…」处理。")),
                )
                .child(
                    Button::new("index-issue-banner-close")
                        .ghost()
                        .xsmall()
                        .label("×")
                        .text_color(muted)
                        .on_click(cx.listener(|panel, _event, _window, cx| {
                            panel.announced_index_issue.set(true);
                            cx.notify();
                        })),
                ),
        )
    }
}

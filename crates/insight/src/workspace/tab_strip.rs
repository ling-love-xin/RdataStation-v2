//! 顶部 tab 条（A 方案：每打开一个洞察目标占一个 tab）。

use super::*;

impl super::InsightWorkspacePanel {
    /// 顶部 tab 条：每打开过的洞察目标占一个 tab（A 方案，2026-10-30）。
    ///
    /// - tab 状态点颜色：当前 tab 按面板状态（Loading 黄 / Error 红 / 成功绿），
    ///   非当前 tab 灰色——一眼看出哪个在跑、哪个挂了。
    /// - 关闭按钮（✕）：调 `close_opened`；最后一个 tab 关掉后面板回空态。
    /// - 「+」按钮：emit `InsightEvent::NewInsightRequested`（宿主弹多来源选择）。
    pub(crate) fn render_tab_strip(
        &mut self,
        entity: &Entity<Self>,
        theme: &Theme,
        tab_max_width: Pixels,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let colors = theme.colors;
        let current_key = self.view.read(cx).target().map(target_key);
        let current_ix = self
            .opened
            .iter()
            .position(|o| Some(o.key.clone()) == current_key);
        let weak = entity.downgrade();
        TabBar::new("insight-ws-tabs")
            // `Tab::max_width` 是 `pub(super)`（只能 tab 模块内部用）——tab 条级
            // `max_width` 才是公开 API（0.8.1 vendor tab_bar.rs:133）：每 tab 超宽
            // 省略号截断。它只收 `Pixels`，故用 render 入口处的主题基准字号换算
            // （此前写死 `× 16.0`，用户调 UI 字号时这一处不跟着动）。
            .max_width(tab_max_width)
            .children(self.opened.iter().enumerate().map(|(ix, entry)| {
                let key = entry.key.clone();
                let is_current = current_ix == Some(ix);
                let dot = if is_current {
                    match self.view.read(cx).state() {
                        InsightPanelState::Loading => Badge::new().dot().color(colors.warning),
                        InsightPanelState::Error { .. } => Badge::new().dot().color(colors.danger),
                        _ => Badge::new().dot().color(colors.success),
                    }
                } else {
                    Badge::new().dot().color(colors.muted_foreground)
                };
                let w_close = weak.clone();
                let w_activate = weak.clone();
                Tab::new()
                    .label(entry.target.title())
                    .prefix(dot)
                    .suffix(
                        Button::new(format!("insight-ws-tab-close-{key}"))
                            .ghost()
                            .icon(IconName::Close)
                            .on_click(move |_, _, app| {
                                let _ = w_close.update(app, |panel, cx| panel.close_opened(ix, cx));
                            }),
                    )
                    .on_click(move |_, _, app| {
                        let _ = w_activate.update(app, |panel, cx| panel.activate_opened(ix, cx));
                    })
            }))
            .when_some(current_ix, |bar, ix| bar.selected_index(ix))
            .suffix(
                Button::new("insight-ws-tab-new")
                    .ghost()
                    .icon(IconName::Plus)
                    .on_click(move |_, _, app| {
                        let _ = weak.update(app, |panel, cx| {
                            panel
                                .insight_view()
                                .update(cx, |_view, cx| cx.emit(InsightEvent::NewInsightRequested))
                        });
                    }),
            )
    }
}

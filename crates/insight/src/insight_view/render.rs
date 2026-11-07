use super::*;

impl Render for InsightView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        // 图标尺寸从主题字号换算：常量保持 rem 倍率，视图不出现裸 px
        let rem = window.rem_size();
        let empty_icon = rem * ui::EMPTY_ICON_SIZE;
        let inline_icon = rem * ui::INSIGHT_INLINE_ICON_SIZE;
        // 下钻区摘要的检查器命中环（68px 外框，对齐原型右栏 grade-ring）
        let overview_ring = rem * ui::INSIGHT_OVERVIEW_RING_SIZE;

        div()
            .key_context("insight")
            // ⟳ 的键位（`Ctrl+Shift+R`，绑在 `insight` context 上；键位在 app 层注册）
            .on_action(cx.listener(Self::on_refresh))
            .v_flex()
            .size_full()
            .bg(theme.colors.background)
            .child(self.render_header(&entity, theme))
            .children(self.render_project_hint(theme))
            // 右栏对齐 EDA 原型右侧面板（2026-10-30 重构）：不再有 Tab 条与目标头——
            // body = 「已洞察表集合（it-list）」+「选中目标下钻（drillzone）」，
            // 目标名与类型徽标由下钻区 dz-title 承担。
            .child(
                div()
                    .id("insight-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .gap_2()
                            .p(rems(ui::PANEL_PADDING))
                            // 洞察表集合：会话级管理，先于选中表的下钻可见
                            .child(self.render_insight_set(&entity, theme))
                            .children(self.render_drillzone(
                                &entity,
                                theme,
                                inline_icon,
                                overview_ring,
                                empty_icon,
                            )),
                    ),
            )
    }
}

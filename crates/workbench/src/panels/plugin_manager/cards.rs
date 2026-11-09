//! 插件管理页的卡片网格（两列）。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

use plugin::PluginInfo;
use plugin::wasm::PluginState;

use super::PluginManagerPanel;
use super::widgets::{
    contribution_summary, plugin_action_buttons, plugin_kind_label, plugin_state_label,
    select_button,
};
use crate::ui;

impl PluginManagerPanel {
    pub(crate) fn render_card_grid(
        &self,
        plugins: &[PluginInfo],
        snapshot: &super::PluginSnapshot,
        selected_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let busy = snapshot.progress.is_some();
        let theme = cx.theme().clone();
        let manager = cx.entity();
        let panel = self.plugin_entity();

        let mut rows: Vec<Div> = Vec::new();
        for chunk in plugins.chunks(2) {
            let mut row = div().h_flex().items_stretch().gap_3().w_full();
            for plugin in chunk {
                let selected = selected_id == Some(plugin.id.as_str());
                let state_color = match plugin.state {
                    PluginState::Active | PluginState::Loaded => theme.colors.success,
                    PluginState::Inactive => theme.colors.muted_foreground,
                    PluginState::Error => theme.colors.danger,
                };

                let select_manager = manager.clone();
                let select_id = plugin.id.clone();
                let select = select_button("card", &plugin.id, move |_, app| {
                    let id = select_id.clone();
                    let _ = select_manager.update(app, |this, cx| this.select(id, cx));
                });

                let mut card = div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h(rems(ui::PLUGIN_CARD_MIN_HEIGHT))
                    .p_3()
                    .gap_2()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(if selected {
                        theme.colors.primary
                    } else {
                        theme.colors.border
                    })
                    .bg(theme.colors.list)
                    .child(
                        div()
                            .h_flex()
                            .items_start()
                            .gap_2()
                            .child(
                                div()
                                    .v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_ellipsis()
                                            .child(plugin.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.colors.muted_foreground)
                                            .text_ellipsis()
                                            .child(format!(
                                                "{}  ·  v{}",
                                                plugin.id, plugin.version
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(state_color)
                                    .child(plugin_state_label(plugin.state)),
                            )
                            .child(select),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .text_ellipsis()
                            .child(if plugin.metadata.description.is_empty() {
                                "（无描述）".to_string()
                            } else {
                                plugin.metadata.description.clone()
                            }),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(plugin_kind_label(plugin.kind))
                            .child("·")
                            .child(contribution_summary(snapshot.contributions.get(&plugin.id))),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .children(plugin_action_buttons("card", plugin, busy, panel.clone())),
                    );

                if selected {
                    card = card.border_color(theme.colors.primary);
                }
                row = row.child(card);
            }
            if chunk.len() == 1 {
                row = row.child(div().flex_1());
            }
            rows.push(row);
        }

        div().v_flex().gap_3().p_4().w_full().children(rows)
    }
}

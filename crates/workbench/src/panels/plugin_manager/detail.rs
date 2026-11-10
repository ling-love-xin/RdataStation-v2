//! 插件管理页右侧详情栏（基本信息 / 贡献点 / 运行状态 / 危险区）。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{ActiveTheme, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use plugin::wasm::PluginState;

use super::widgets::{
    contribution_summary, plugin_action_buttons, plugin_kind_label, plugin_state_label,
};
use super::{PluginManagerPanel, PluginSnapshot};
use crate::ui;

impl PluginManagerPanel {
    pub(crate) fn render_detail_pane(
        &self,
        snapshot: &PluginSnapshot,
        selected_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let busy = snapshot.progress.is_some();
        let selected = selected_id.and_then(|id| {
            snapshot
                .plugins
                .iter()
                .find(|plugin| plugin.id == id)
                .cloned()
        });

        let mut pane = div()
            .v_flex()
            .flex_none()
            .w(rems(ui::PLUGIN_MANAGER_DETAIL_WIDTH))
            .h_full()
            .min_h_0()
            .border_l(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .overflow_y_scrollbar();

        let Some(plugin) = selected else {
            return pane
                .items_center()
                .justify_center()
                .p_4()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.colors.muted_foreground)
                        .child("选择左侧卡片查看详情"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("卡片的「详情」按钮会把插件放到这里。"),
                );
        };

        pane = pane
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::MEDIUM)
                            .text_ellipsis()
                            .child(plugin.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .text_ellipsis()
                            .child(format!("{}  ·  v{}", plugin.id, plugin.version)),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(section_title("基本信息", &theme))
                    .child(
                        DescriptionList::vertical()
                            .columns(1)
                            .item("插件 ID", plugin.id.clone(), 1)
                            .item("名称", plugin.name.clone(), 1)
                            .item("版本", plugin.version.clone(), 1)
                            .item("类型", plugin_kind_label(plugin.kind), 1)
                            .item("作者", plugin.metadata.author.clone(), 1)
                            .item(
                                "入口",
                                if plugin.metadata.entry_point.is_empty() {
                                    "—".to_string()
                                } else {
                                    plugin.metadata.entry_point.clone()
                                },
                                1,
                            ),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(section_title("贡献点", &theme))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(contribution_summary(
                                snapshot.contributions.get(&plugin.id),
                            )),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(section_title("运行状态", &theme))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(match plugin.state {
                                        PluginState::Active | PluginState::Loaded => {
                                            theme.colors.success
                                        }
                                        PluginState::Inactive => theme.colors.muted_foreground,
                                        PluginState::Error => theme.colors.danger,
                                    })
                                    .child(plugin_state_label(plugin.state)),
                            )
                            .when_some(plugin.error.as_deref(), |row, error| {
                                row.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(theme.colors.danger)
                                        .child(error.to_string()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("项目引用：只读查询（DependencyManager::check_dependents）已存在，详情栏尚未接线，暂不展示。"),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .child(section_title("管理动作", &theme))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .children(plugin_action_buttons(
                                "detail",
                                &plugin,
                                busy,
                                self.plugin_entity(),
                            )),
                    )
                    .child(Separator::horizontal().color(theme.colors.border))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("卸载会从磁盘删除插件目录，但保留 plugin-data 下的用户数据。"),
                    ),
            );

        pane
    }
}

fn section_title(title: &str, theme: &Theme) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.colors.foreground)
        .child(title.to_string())
}

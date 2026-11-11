//! 插件管理页右侧详情栏（基本信息 / 贡献点 / 运行状态 / 危险区）。

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::{ActiveTheme, Sizable, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use plugin::wasm::PluginState;

use super::widgets::{
    contribution_summary, plugin_action_buttons, plugin_backend_label,
    plugin_driver_registration_label, plugin_file_buttons, plugin_kind_label,
    plugin_receives_database_credentials, plugin_state_label,
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
        // 依赖项只有装着的插件才能跳详情；没装时按钮置灰并说明原因，
        // 比让用户点一下发现没反应（或去搜索框里猜）更直接。
        let installed_ids: Vec<String> = snapshot
            .plugins
            .iter()
            .map(|plugin| plugin.id.clone())
            .collect();
        let detail_manager = cx.entity();

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
                            .item("运行形态", plugin_backend_label(&plugin), 1)
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
                    )
                    .when(
                        !plugin.contribution_items.is_empty(),
                        |section| {
                            section.child(div().v_flex().gap_1().children(
                                plugin.contribution_items.iter().map(|item| {
                                    div()
                                        .v_flex()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .px_2()
                                                        .py_0p5()
                                                        .rounded(theme.radius)
                                                        .border_1()
                                                        .border_color(theme.colors.border)
                                                        .text_xs()
                                                        .text_color(theme.colors.foreground)
                                                        .child(item.kind.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_sm()
                                                        .text_color(theme.colors.foreground)
                                                        .text_ellipsis()
                                                        .child(item.label.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.colors.muted_foreground)
                                                .child(match item.detail.as_deref() {
                                                    Some(detail) => {
                                                        format!("{} · {}", item.id, detail)
                                                    }
                                                    None => item.id.clone(),
                                                }),
                                        )
                                }),
                            ))
                        },
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(section_title("驱动", &theme))
                    .child({
                        let mut block = div().v_flex().gap_2();
                        if plugin.drivers.is_empty() {
                            block = block.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("此插件没有声明数据库驱动。"),
                            );
                        } else {
                            block = block
                                .child(div().v_flex().gap_2().children(
                                    plugin.drivers.iter().map(|driver| {
                                        let status_label =
                                            plugin_driver_registration_label(&plugin, &driver.id);
                                        let status_color = match status_label {
                                            "已登记" => theme.colors.success,
                                            "已登记 · 已停用" => theme.colors.warning,
                                            "未登记" => theme.colors.danger,
                                            _ => theme.colors.muted_foreground,
                                        };
                                        div()
                                            .v_flex()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .h_flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(theme.colors.foreground)
                                                            .child(driver.display_name.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                theme.colors.muted_foreground,
                                                            )
                                                            .child(driver.id.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .px_2()
                                                            .py_0p5()
                                                            .rounded(theme.radius)
                                                            .border_1()
                                                            .border_color(status_color)
                                                            .text_xs()
                                                            .text_color(status_color)
                                                            .child(status_label),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.colors.muted_foreground)
                                                    .child(format!(
                                                        "默认端口 {} · 分类 {}",
                                                        driver
                                                            .default_port
                                                            .map(|port| port.to_string())
                                                            .unwrap_or_else(|| "—".to_string()),
                                                        driver
                                                            .category
                                                            .clone()
                                                            .unwrap_or_else(|| {
                                                                "relational".to_string()
                                                            }),
                                                    )),
                                            )
                                    }),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("「已登记」表示 global.db 的 drivers 表已有此行（连接对话框下拉可见）；「未登记」时先看上方运行状态与失败原因。"),
                                );
                        }
                        block
                    }),
            )
            .child({
                let mut section = div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(section_title("依赖关系", &theme));

                // 上游：本插件声明需要谁（清单 dependencies）
                section = section.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("本插件依赖"),
                        )
                        .when(plugin.dependencies.is_empty(), |group| {
                            group.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("未声明依赖。"),
                            )
                        })
                        .when(!plugin.dependencies.is_empty(), |group| {
                            group.child(div().h_flex().flex_wrap().gap_1().children(
                                plugin.dependencies.iter().map(|dependency| {
                                    let installed = installed_ids.contains(dependency);
                                    let id = dependency.clone();
                                    let manager = detail_manager.clone();
                                    Button::new(SharedString::from(format!(
                                        "detail-dependency-{}-{dependency}",
                                        plugin.id
                                    )))
                                    .ghost()
                                    .label(dependency.clone())
                                    .with_size(Size::XSmall)
                                    .disabled(!installed)
                                    .tooltip(if installed {
                                        "在右侧打开该插件详情"
                                    } else {
                                        "插件未安装，无法打开"
                                    })
                                    .on_click(move |_, _, app| {
                                        let id = id.clone();
                                        let _ = manager.update(app, |this, cx| {
                                            this.select(id.clone(), cx);
                                        });
                                    })
                                }),
                            ))
                        }),
                );

                // 下游：谁依赖本插件（卸载检查同一份反向索引）
                section = section.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("被其他插件依赖"),
                        )
                        .child(match plugin.dependents.as_deref() {
                            None => div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("未查询（全局库未初始化，停用 / 卸载前无法前置检查）。")
                                .into_any_element(),
                            Some([]) => div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("没有其他插件依赖它。")
                                .into_any_element(),
                            Some(dependents) => div()
                                .h_flex()
                                .flex_wrap()
                                .gap_1()
                                .children(dependents.iter().map(|dependent| {
                                    let installed = installed_ids.contains(dependent);
                                    let id = dependent.clone();
                                    let manager = detail_manager.clone();
                                    Button::new(SharedString::from(format!(
                                        "detail-dependent-{}-{dependent}",
                                        plugin.id
                                    )))
                                    .ghost()
                                    .label(dependent.clone())
                                    .with_size(Size::XSmall)
                                    .disabled(!installed)
                                    .tooltip(if installed {
                                        "在右侧打开该插件详情"
                                    } else {
                                        "插件未安装，无法打开"
                                    })
                                    .on_click(move |_, _, app| {
                                        let id = id.clone();
                                        let _ = manager.update(app, |this, cx| {
                                            this.select(id.clone(), cx);
                                        });
                                    })
                                }))
                                .into_any_element(),
                        })
                        .when(
                            plugin.dependents.as_deref().is_some_and(|d| !d.is_empty()),
                            |group| {
                                group.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("停用会让它们立即失效；卸载前需先卸载依赖它的插件。"),
                                )
                            },
                        ),
                );

                section
            })
            .child({
                let permissions = &plugin.permissions;
                let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
                if !permissions.sidecar.is_empty() {
                    groups.push(("进程", permissions.sidecar.clone()));
                }
                if !permissions.driver.is_empty() {
                    groups.push(("数据面", permissions.driver.clone()));
                }
                if !permissions.frontend.is_empty() {
                    groups.push(("前端", permissions.frontend.clone()));
                }
                if !permissions.wasm.is_empty() {
                    groups.push(("沙箱", permissions.wasm.clone()));
                }
                if !permissions.host.is_empty() {
                    groups.push(("Host API", permissions.host.clone()));
                }
                let has_any = !groups.is_empty() || permissions.credentials.is_some();

                div()
                    .v_flex()
                    .gap_2()
                    .p_4()
                    .border_b(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(section_title("权限", &theme))
                    .when(!has_any, |section| {
                        section.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("清单未声明权限。"),
                        )
                    })
                    .children(groups.into_iter().map(|(label, values)| {
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .flex_wrap()
                                    .children(values.into_iter().map(|value| {
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded(theme.radius)
                                            .border_1()
                                            .border_color(theme.colors.border)
                                            .text_xs()
                                            .text_color(theme.colors.foreground)
                                            .child(value)
                                    })),
                            )
                    }))
                    .when(plugin_receives_database_credentials(&plugin), |section| {
                        section.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child("会接收数据库密码（仅用于建立连接）。"),
                        )
                    })
                    .when(!permissions.host.is_empty(), |section| {
                        section.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("Host API 默认拒绝：声明只表示申请，不等于已放行。"),
                        )
                    })
            })
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
                    .child(
                        div().h_flex().flex_wrap().items_center().gap_1().children(
                            plugin_file_buttons("detail", &plugin, self.plugin_entity()),
                        ),
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

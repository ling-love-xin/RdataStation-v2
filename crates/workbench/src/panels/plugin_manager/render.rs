//! 插件管理页整页骨架：标题行 / 统计 / 反馈 / 筛选 / 滚动卡片区 / 右侧详情。

use gpui_kit::base::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{ActiveTheme, IconName, Sizable, Size, WindowExt as _};
use gpui_kit::{Context, Div, FontWeight, IntoElement, SharedString, Window, div, rems};

use super::widgets::{
    driver_registry_rows, filtered_plugins, plugin_driver_chain, plugin_driver_stats,
    plugin_driver_stats_label, plugin_state_is_active, plugin_stats, DriverChainLevel,
    DriverRegistryStatus,
};
use super::*;
use crate::ui;

impl PluginManagerPanel {
    pub(crate) fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let snapshot = self.snapshot(cx);
        let stats = plugin_stats(&snapshot.plugins);
        let driver_stats = plugin_driver_stats(&snapshot.plugins);
        let driver_stats_label = plugin_driver_stats_label(driver_stats);
        let filter = self.filter();
        let query = self.query();
        let filtered = filtered_plugins(&snapshot.plugins, filter, &query);
        let selected_id = self
            .selected_id()
            .or_else(|| filtered.first().map(|p| p.id.clone()));

        let header = Toolbar::new("plugin-manager-head")
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .content(
                div().v_flex().gap_1().child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("插件管理"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("本地已安装 · 所有操作立即生效"),
                        ),
                ),
            )
            .content(div().flex_1())
            .child(
                Button::new("plugin-manager-install-file")
                    .label("安装插件")
                    .with_size(Size::Small)
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.install_file(cx));
                        }
                    }),
            )
            .child(
                Button::new("plugin-manager-install-dir")
                    .ghost()
                    .label("从目录导入")
                    .with_size(Size::Small)
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.install_dir(cx));
                        }
                    }),
            )
            .content(Separator::vertical())
            .child(
                Button::new("plugin-manager-refresh")
                    .ghost()
                    .icon(IconName::RefreshCw)
                    .with_size(Size::Small)
                    .tooltip("重新同步驱动登记并读取插件清单")
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.refresh(cx));
                        }
                    }),
            );

        let stats_row = div()
            .h_flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child(format!("全部 {}", stats.total))
            .child(format!("已启用 {}", stats.enabled))
            .child(format!("已停用 {}", stats.disabled))
            .child(format!("出错 {}", stats.error));
        // 驱动登记为 0 时用 warning 色：插件在、下拉却空，就是这一格。
        let drivers_missing = driver_stats.declared > 0 && driver_stats.registered == 0;
        let stats_row = stats_row.child(
            div()
                .text_color(if drivers_missing || driver_stats.unknown {
                    theme.colors.warning
                } else {
                    theme.colors.muted_foreground
                })
                .child(driver_stats_label),
        );
        // 批量入口放在统计行右侧：用户看到「已停用 N」时，下一步通常就是把这批拉起来。
        // 停用全部会一次性改变所有驱动可见性，必须先确认；启用全部只碰 Inactive，无需确认。
        let busy = snapshot.progress.is_some();
        let mut stats_row = stats_row
            .child(div().flex_1())
            .child(
                Button::new("plugin-manager-enable-all")
                    .ghost()
                    .label(SharedString::from(format!(
                        "启用全部已停用（{}）",
                        stats.disabled
                    )))
                    .with_size(Size::XSmall)
                    .tooltip("启用并激活当前所有已停用的插件")
                    .disabled(busy || stats.disabled == 0)
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| {
                                this.set_all_plugins_enabled(true, cx)
                            });
                        }
                    }),
            );
        // 出错行单独给批量重试：冲突方停掉后，另一个插件不会因清单未变而自动恢复。
        if stats.error > 0 {
            stats_row = stats_row.child(
                Button::new("plugin-manager-retry-all")
                    .ghost()
                    .label(SharedString::from(format!(
                        "重试全部出错（{}）",
                        stats.error
                    )))
                    .with_size(Size::XSmall)
                    .tooltip("重新解析清单并加载所有出错插件；结果仍失败时保留原因")
                    .disabled(busy)
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| {
                                this.retry_all_failed_plugins(cx)
                            });
                        }
                    }),
            );
        }
        let stats_row = stats_row
            .child(
                Button::new("plugin-manager-disable-all")
                    .ghost()
                    .label(SharedString::from(format!(
                        "停用全部已启用（{}）",
                        stats.enabled
                    )))
                    .with_size(Size::XSmall)
                    .tooltip("停用当前所有已启用插件（会先确认）")
                    .disabled(busy || stats.enabled == 0)
                    .on_click({
                        let manager = cx.entity();
                        let count = stats.enabled;
                        move |_, window, app| {
                            let manager = manager.clone();
                            window.open_alert_dialog(app, move |alert, _window, _cx| {
                                let manager = manager.clone();
                                alert
                                    .confirm()
                                    .title("停用全部已启用插件")
                                    .description(SharedString::from(format!(
                                        "将停用 {count} 个已启用插件，驱动会立即从连接下拉中移除。\n\
                                         插件目录与 plugin-data 都会保留，可随时重新启用。"
                                    )))
                                    .ok_text("停用全部")
                                    .cancel_text("取消")
                                    .on_ok(move |_window, _cx, cx| {
                                        let _ = manager.update(cx, |this, cx| {
                                            this.set_all_plugins_enabled(false, cx)
                                        });
                                        true
                                    })
                            });
                        }
                    }),
            );

        // 驱动链路：把「插件在、下拉空」的四段成败摊开，卡点写在同一行。
        let chain = plugin_driver_chain(&snapshot.plugins);
        let registry_rows = driver_registry_rows(&snapshot.plugins);
        let mut chain_row = div()
            .h_flex()
            .items_center()
            .flex_wrap()
            .gap_2()
            .px_4()
            .py_2()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .text_xs()
            .child(
                div()
                    .text_color(theme.colors.muted_foreground)
                    .child("驱动链路"),
            );
        for (index, step) in chain.steps.iter().enumerate() {
            if index > 0 {
                chain_row =
                    chain_row.child(div().text_color(theme.colors.muted_foreground).child("→"));
            }
            let tone = match step.level {
                DriverChainLevel::Ok => theme.colors.success,
                DriverChainLevel::Warn => theme.colors.warning,
                DriverChainLevel::Bad => theme.colors.danger,
            };
            chain_row = chain_row.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.colors.border)
                    .child(div().size_1p5().rounded_full().bg(tone))
                    .child(step.label)
                    .child(
                        div()
                            .text_color(theme.colors.muted_foreground)
                            .child(step.detail.clone()),
                    ),
            );
        }
        if let Some(bottleneck) = chain.bottleneck.clone() {
            chain_row = chain_row.child(div().text_color(theme.colors.warning).child(bottleneck));
        }
        // 冲突时直接给出「停用」入口：加载期冲突会让两个插件都不可用，用户必须
        // 停掉其中一个才能恢复；只写「请先停用」而不给按钮，等于让用户自己去找。
        for conflict in &chain.conflicts {
            for plugin_id in &conflict.plugins {
                let id = plugin_id.clone();
                let manager = cx.entity();
                chain_row = chain_row.child(
                    Button::new(SharedString::from(format!("plugin-chain-disable-{plugin_id}")))
                        .ghost()
                        .label(SharedString::from(format!("停用 {plugin_id}")))
                        .with_size(Size::Small)
                        .on_click(move |_, _, app| {
                            let _ = manager
                                .update(app, |this, cx| this.disable_plugin(id.clone(), cx));
                        }),
                );
            }
        }
        // 登记总表默认收起：链路行已经给出总体成败，明细留给「哪个 id、哪个插件、登记成什么样」。
        if !registry_rows.is_empty() {
            chain_row = chain_row.child(
                Button::new("plugin-manager-driver-registry")
                    .ghost()
                    .label(SharedString::from(if self.show_driver_registry {
                        "收起登记明细".to_string()
                    } else {
                        format!("登记明细（{}）", registry_rows.len())
                    }))
                    .with_size(Size::XSmall)
                    .tooltip("按驱动 id 查看每个插件在 drivers 表的真实登记状态")
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.toggle_driver_registry(cx));
                        }
                    }),
            );
        }

        let registry_table = (self.show_driver_registry && !registry_rows.is_empty()).then(|| {
            div()
                .v_flex()
                .gap_1()
                .px_4()
                .py_2()
                .border_b(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .text_xs()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .py_1()
                        .text_color(theme.colors.muted_foreground)
                        .child(div().flex_1().child("驱动"))
                        .child(div().flex_1().child("显示名"))
                        .child(div().flex_1().child("插件"))
                        .child(div().flex_none().child("登记状态")),
                )
                .children(registry_rows.iter().map(|row| {
                    let tone = match row.status {
                        DriverRegistryStatus::Registered => theme.colors.success,
                        DriverRegistryStatus::Disabled | DriverRegistryStatus::Unknown => {
                            theme.colors.warning
                        }
                        DriverRegistryStatus::Unregistered => theme.colors.danger,
                    };
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .py_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(row.driver_id.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .child(row.display_name.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .text_color(if plugin_state_is_active(row.plugin_state) {
                                    theme.colors.foreground
                                } else {
                                    theme.colors.muted_foreground
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .child(row.plugin_id.clone()),
                                )
                                .child({
                                    let manager = cx.entity();
                                    let plugin_id = row.plugin_id.clone();
                                    Button::new(SharedString::from(format!(
                                        "registry-select-{}",
                                        row.plugin_id
                                    )))
                                    .ghost()
                                    .label("查看")
                                    .with_size(Size::XSmall)
                                    .tooltip("在右侧打开该插件详情")
                                    .on_click(
                                        move |_, _, app| {
                                            let id = plugin_id.clone();
                                            let _ = manager.update(app, |this, cx| {
                                                this.select(id, cx);
                                            });
                                        },
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex_none()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child(div().size_1p5().rounded_full().bg(tone))
                                .child(row.status.label())
                                .children(row.conflict.then(|| {
                                    div().text_color(theme.colors.danger).child("· id 冲突")
                                })),
                        )
                }))
        });

        // 操作记录（D8d）：默认收起。这里只做「看得见」——最近 N 条 + 结果过滤 + 刷新。
        // 失败行必须带原因，所以 `reason` 独占一列，不做省略后再拼提示。
        let ops_log = self.operations();
        let ops_filter = self.ops_filter();
        let ops_snapshot = {
            let log = ops_log.borrow();
            // 借用不能跨出这个块（`log` 在块尾释放）：过滤结果先克隆成自有行
            let rows: Vec<engine::persistence::driver_operation_store::DriverOperationRow> =
                widgets::filtered_operations(&log.rows, ops_filter)
                    .into_iter()
                    .cloned()
                    .collect();
            (rows, log.rows.len(), log.error.clone(), log.loading)
        };
        // 导出结果（D8e）：按钮点完之后的一句反馈，路径原样给出（用户要能直接去拿文件）
        let ops_export = {
            let log = ops_log.borrow();
            (
                log.export_note.clone(),
                log.export_error.clone(),
                log.exporting,
            )
        };
        let ops_count = ops_snapshot.1;
        let ops_row = div()
            .h_flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .text_xs()
            .child(
                Button::new("plugin-manager-operations")
                    .ghost()
                    .label(SharedString::from(if self.show_operations() {
                        "收起操作记录".to_string()
                    } else if ops_count == 0 {
                        "操作记录".to_string()
                    } else {
                        format!("操作记录（{ops_count}）")
                    }))
                    .with_size(Size::XSmall)
                    .tooltip("安装 / 启停 / 卸载 / 重载 / 缺驱动被拒等动作的留痕")
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.toggle_operations(cx));
                        }
                    }),
            )
            .child(
                div()
                    .text_color(theme.colors.muted_foreground)
                    .child("留痕只为可查，失败原因原样保留")
            )
            .child(div().flex_1());

        let ops_filter_ix = widgets::OperationResultFilter::ALL
            .iter()
            .position(|item| *item == ops_filter)
            .unwrap_or(0);
        let ops_row = ops_row
            .child(
                TabBar::new("plugin-manager-ops-filter")
                    .segmented()
                    .with_size(Size::XSmall)
                    .selected_index(ops_filter_ix)
                    .children(
                        widgets::OperationResultFilter::ALL
                            .iter()
                            .map(|item| Tab::new().label(item.label())),
                    )
                    .on_click({
                        let manager = cx.entity();
                        move |ix, _window, app| {
                            if let Some(filter) =
                                widgets::OperationResultFilter::ALL.get(*ix)
                            {
                                let _ = manager.update(app, |this, cx| {
                                    this.set_ops_filter(*filter, cx)
                                });
                            }
                        }
                    }),
            )
            .child(
                Button::new("plugin-manager-ops-refresh")
                    .ghost()
                    .icon(IconName::RefreshCw)
                    .with_size(Size::XSmall)
                    .tooltip("重新读取最近的操作留痕")
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.load_operations(cx));
                        }
                    }),
            )
            .child(
                // 导出 JSON（D8e）：排障时把留痕拿走比翻页实用；导出的是**全量**（1000 条上限），
                // 不是屏幕上那 50 条。按钮不做禁用，靠 `exporting` 状态挡住重复点。
                Button::new("plugin-manager-ops-export")
                    .ghost()
                    .label(SharedString::from(if ops_export.2 {
                        "导出中…".to_string()
                    } else {
                        "导出 JSON".to_string()
                    }))
                    .with_size(Size::XSmall)
                    .tooltip("把最近 1000 条留痕写成 JSON（排障用），落到全局目录的 exports/")
                    .on_click({
                        let manager = cx.entity();
                        move |_, _, app| {
                            let _ = manager.update(app, |this, cx| this.export_operations(cx));
                        }
                    }),
            )
            .children(ops_export.0.clone().map(|note| {
                div()
                    .text_color(theme.colors.success)
                    .child(note)
            }))
            .children(ops_export.1.clone().map(|error| {
                div()
                    .text_color(theme.colors.danger)
                    .child(error)
            }));

        let ops_table = (self.show_operations()).then(|| {
            let mut table = div()
                .v_flex()
                .gap_1()
                .px_4()
                .py_2()
                .border_b(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .text_xs();
            if let Some(error) = ops_snapshot.2.clone() {
                // 表不在 / 读不到：直接说清，不呈现成「0 条留痕」
                table = table.child(
                    div()
                        .text_color(theme.colors.warning)
                        .child(format!("操作记录读取失败：{error}")),
                );
            } else if ops_snapshot.3 {
                table = table.child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child("正在读取操作记录…"),
                );
            } else if ops_snapshot.0.is_empty() {
                table = table.child(
                    div().text_color(theme.colors.muted_foreground).child(
                        if ops_count == 0 {
                            "还没有操作记录（装插件、启停、改驱动之后会出现在这里）"
                        } else {
                            "当前筛选下没有记录"
                        },
                    ),
                );
            } else {
                table = table.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_3()
                        .py_1()
                        .text_color(theme.colors.muted_foreground)
                        .child(div().w(rems(14.)).flex_none().child("时间"))
                        .child(div().w(rems(7.)).flex_none().child("动作"))
                        .child(div().w(rems(5.)).flex_none().child("结果"))
                        .child(div().w(rems(18.)).flex_none().child("对象"))
                        .child(div().flex_1().min_w_0().child("原因 / 说明")),
                );
                for row in &ops_snapshot.0 {
                    let ok = row.operation.result == "ok";
                    let tone = if ok {
                        theme.colors.success
                    } else {
                        theme.colors.danger
                    };
                    table = table.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_3()
                            .py_1()
                            .child(
                                div()
                                    .w(rems(14.))
                                    .flex_none()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(row.occurred_at.clone()),
                            )
                            .child(
                                div()
                                    .w(rems(7.))
                                    .flex_none()
                                    .child(widgets::operation_action_label(&row.operation.action)),
                            )
                            .child(
                                div()
                                    .w(rems(5.))
                                    .flex_none()
                                    .text_color(tone)
                                    .child(widgets::operation_result_label(&row.operation.result)),
                            )
                            .child(
                                div()
                                    .w(rems(18.))
                                    .flex_none()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .child(widgets::operation_target(row)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_color(if ok {
                                        theme.colors.muted_foreground
                                    } else {
                                        theme.colors.foreground
                                    })
                                    .child(row.operation.reason.clone().unwrap_or_default()),
                            ),
                    );
                }
            }
            table
        });

        // 操作反馈：错误 > 进行中 > 成功回执（同一时刻只显示一条，避免三行挤在一起）。
        let feedback = if let Some(error) = snapshot.error.as_deref() {
            Some(
                div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(error.to_string())
                    .into_any_element(),
            )
        } else if let Some(progress) = snapshot.progress.as_deref() {
            Some(
                div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(progress.to_string())
                    .into_any_element(),
            )
        } else if let Some(notice) = snapshot.notice.as_deref() {
            Some(
                div()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.success)
                    .child(notice.to_string())
                    .into_any_element(),
            )
        } else {
            None
        };

        // 热插拔只登记 `pending_reloads`，不会自动热更正在运行的插件；管理页给出显式入口。
        let pending = plugin::get_plugin_manager()
            .map(|manager| manager.pending_reloads().len())
            .unwrap_or(0);
        let reload_banner = (pending > 0).then(|| {
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .border_b(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .bg(theme.colors.warning.opacity(0.10))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(format!(
                            "{pending} 个插件的文件已变化；重载后新版本才会在当前会话生效。"
                        )),
                )
                .child(
                    Button::new("plugin-manager-reload")
                        .label("立即重载")
                        .with_size(Size::XSmall)
                        .on_click({
                            let manager = cx.entity();
                            move |_, _, app| {
                                let _ = manager.update(app, |this, cx| this.reload(cx));
                            }
                        }),
                )
        });

        let input = self.ensure_search_input(window, cx);
        let filter_ix = PluginFilter::ALL
            .iter()
            .position(|item| *item == filter)
            .unwrap_or(0);
        let toolbar = div()
            .h_flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_b(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .child(
                Input::new(&input)
                    .w(rems(ui::PLUGIN_MANAGER_SEARCH_WIDTH))
                    .with_size(Size::Small),
            )
            .child(
                TabBar::new("plugin-manager-filter")
                    .segmented()
                    .with_size(Size::Small)
                    .selected_index(filter_ix)
                    .children(
                        PluginFilter::ALL
                            .iter()
                            .map(|item| Tab::new().label(item.label())),
                    )
                    .on_click({
                        let manager = cx.entity();
                        move |ix, _window, app| {
                            if let Some(filter) = PluginFilter::ALL.get(*ix) {
                                let _ =
                                    manager.update(app, |this, cx| this.set_filter(*filter, cx));
                            }
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("共 {} 个", filtered.len())),
            );

        let grid = if filtered.is_empty() {
            let title = if snapshot.plugins.is_empty() {
                "尚未安装插件"
            } else {
                "没有匹配的插件"
            };
            let description = if snapshot.plugins.is_empty() {
                "点击右上角「安装插件」选择 .zip / .tar.gz，或「从目录导入」开发目录。"
            } else {
                "调整搜索词或状态筛选后再试。"
            };
            Empty::new()
                .header(
                    EmptyHeader::new()
                        .title(EmptyTitle::new().child(title))
                        .description(EmptyDescription::new().child(description)),
                )
                .into_any_element()
        } else {
            self.render_card_grid(&filtered, &snapshot, selected_id.as_deref(), cx)
                .into_any_element()
        };

        let detail = self.render_detail_pane(&snapshot, selected_id.as_deref(), cx);

        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(theme.colors.background)
            .child(header)
            .child(stats_row)
            .child(chain_row)
            .children(registry_table)
            .child(ops_row)
            .children(ops_table)
            .children(feedback)
            .children(reload_banner)
            .child(toolbar)
            .child(
                div()
                    .h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_y_scrollbar()
                            .child(grid),
                    )
                    .child(detail),
            )
    }
}

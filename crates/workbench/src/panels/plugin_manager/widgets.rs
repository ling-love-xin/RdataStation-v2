//! 插件管理页的纯函数与动作按钮。
//!
//! 这里只做「状态 → 文案 / 筛选 / 统计 / 按钮」：不读窗口、不读主题，
//! 渲染层负责把结果套上 `cx.theme().colors.*`。

use gpui_kit::base::Disableable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::{App, SharedString, Window};

use plugin::ContributionCounts;
use plugin::PluginInfo;
use plugin::PluginKind;
use plugin::wasm::PluginState;

use super::super::left_tabs::{PluginPanel, uninstall_confirm_description};

/// 状态筛选档（对齐原型 toolbar 的五档里去掉「可更新」——RDS 没有市场 / 远程索引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginFilter {
    All,
    Enabled,
    Disabled,
    Error,
}

impl PluginFilter {
    pub(crate) const ALL: [PluginFilter; 4] = [
        PluginFilter::All,
        PluginFilter::Enabled,
        PluginFilter::Disabled,
        PluginFilter::Error,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            PluginFilter::All => "全部",
            PluginFilter::Enabled => "已启用",
            PluginFilter::Disabled => "已停用",
            PluginFilter::Error => "出错",
        }
    }

    fn matches(self, plugin: &PluginInfo) -> bool {
        match self {
            PluginFilter::All => true,
            PluginFilter::Enabled => plugin_state_is_active(plugin.state),
            PluginFilter::Disabled => plugin.state == PluginState::Inactive,
            PluginFilter::Error => plugin.state == PluginState::Error,
        }
    }
}

/// 页面顶部统计（全部 / 已启用 / 已停用 / 出错）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PluginStats {
    pub total: usize,
    pub enabled: usize,
    pub disabled: usize,
    pub error: usize,
}

pub(crate) fn plugin_stats(plugins: &[PluginInfo]) -> PluginStats {
    let mut stats = PluginStats {
        total: plugins.len(),
        ..PluginStats::default()
    };
    for plugin in plugins {
        match plugin.state {
            PluginState::Active | PluginState::Loaded => stats.enabled += 1,
            PluginState::Inactive => stats.disabled += 1,
            PluginState::Error => stats.error += 1,
        }
    }
    stats
}

/// 按筛选档 + 搜索词过滤，并按名称排序（纯函数，单测覆盖）。
pub(crate) fn filtered_plugins(
    plugins: &[PluginInfo],
    filter: PluginFilter,
    query: &str,
) -> Vec<PluginInfo> {
    let query = query.trim().to_lowercase();
    let mut out: Vec<PluginInfo> = plugins
        .iter()
        .filter(|plugin| filter.matches(plugin))
        .filter(|plugin| {
            if query.is_empty() {
                return true;
            }
            let haystack = format!(
                "{} {} {} {}",
                plugin.name, plugin.id, plugin.metadata.description, plugin.metadata.author
            )
            .to_lowercase();
            haystack.contains(&query)
        })
        .cloned()
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub(crate) fn plugin_state_is_active(state: PluginState) -> bool {
    matches!(state, PluginState::Active | PluginState::Loaded)
}

pub(crate) fn plugin_state_label(state: PluginState) -> &'static str {
    match state {
        PluginState::Active => "已启用",
        PluginState::Loaded => "已加载",
        PluginState::Inactive => "已停用",
        PluginState::Error => "出错",
    }
}

pub(crate) fn plugin_kind_label(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Unknown => "清单损坏",
        PluginKind::Sidecar => "侧车进程",
        PluginKind::Frontend => "前端扩展",
    }
}

/// 贡献点一行摘要（右栏详情 / 卡片元信息共用）。
pub(crate) fn contribution_summary(counts: Option<&ContributionCounts>) -> String {
    let Some(counts) = counts else {
        return "贡献点：无".to_string();
    };
    if counts.is_empty() {
        return "贡献点：无".to_string();
    }
    format!(
        "命令 {} · 面板 {} · 设置 {} · 驱动 {}",
        counts.commands, counts.panels, counts.settings, counts.drivers
    )
}

/// 卡片 / 详情栏共用的管理动作按钮。
///
/// `scope` 用来区分卡片与详情栏的同名 `ElementId`（同屏两处都渲染时不能撞 id）。
pub(crate) fn plugin_action_buttons(
    scope: &str,
    plugin: &PluginInfo,
    busy: bool,
    entity: gpui_kit::Entity<PluginPanel>,
) -> Vec<Button> {
    let id = plugin.id.clone();
    let active = plugin_state_is_active(plugin.state);
    let action_id = id.clone();
    let action = if plugin.kind == PluginKind::Unknown {
        Button::new(SharedString::from(format!("plugin-{scope}-{id}-retry")))
            .ghost()
            .with_size(Size::XSmall)
            .label("重试")
            .tooltip("重新解析清单并加载（修好清单或卸载冲突插件后使用）")
            .disabled(busy)
            .on_click({
                let entity = entity.clone();
                move |_, _window, app| {
                    let _ = entity.update(app, |panel, cx| {
                        panel.retry_plugin(action_id.clone(), cx);
                    });
                    app.stop_propagation();
                }
            })
    } else if active {
        Button::new(SharedString::from(format!("plugin-{scope}-{id}-disable")))
            .ghost()
            .with_size(Size::XSmall)
            .label("停用")
            .tooltip("停用后本会话立即停止运行（进程停用），下次启动不再激活")
            .disabled(busy)
            .on_click({
                let entity = entity.clone();
                move |_, _window, app| {
                    let _ = entity.update(app, |panel, cx| {
                        panel.set_plugin_enabled(action_id.clone(), false, cx);
                    });
                    app.stop_propagation();
                }
            })
    } else {
        Button::new(SharedString::from(format!("plugin-{scope}-{id}-enable")))
            .ghost()
            .with_size(Size::XSmall)
            .label("启用")
            .tooltip("启用并立即激活")
            .disabled(busy)
            .on_click({
                let entity = entity.clone();
                move |_, _window, app| {
                    let _ = entity.update(app, |panel, cx| {
                        panel.set_plugin_enabled(action_id.clone(), true, cx);
                    });
                    app.stop_propagation();
                }
            })
    };

    let uninstall_id = id.clone();
    let description = uninstall_confirm_description(&plugin.name, &id);
    let uninstall = Button::new(SharedString::from(format!("plugin-{scope}-{id}-uninstall")))
        .ghost()
        .with_size(Size::XSmall)
        .label("卸载")
        .tooltip("从磁盘删除该插件（保留 plugin-data 用户数据）")
        .disabled(busy)
        .on_click({
            let entity = entity.clone();
            move |_, window, app| {
                let entity = entity.clone();
                let id = uninstall_id.clone();
                let description = description.clone();
                window.open_alert_dialog(app, move |alert, _window, _cx| {
                    let entity = entity.clone();
                    let id = id.clone();
                    alert
                        .confirm()
                        .title("卸载插件")
                        .description(SharedString::from(description.clone()))
                        .ok_text("卸载")
                        .cancel_text("取消")
                        .ok_variant(ButtonVariant::Danger)
                        .on_ok(move |_window, _cx, cx| {
                            let _ = entity.update(cx, |panel, cx| {
                                panel.uninstall_plugin(id.clone(), cx);
                            });
                            true
                        })
                });
                app.stop_propagation();
            }
        });

    vec![action, uninstall]
}

/// 详情按钮（卡片右上角 / 列表行均可复用；`on_select` 由渲染层注入）。
pub(crate) fn select_button(
    scope: &str,
    id: &str,
    on_select: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(SharedString::from(format!("plugin-{scope}-{id}-select")))
        .ghost()
        .with_size(Size::XSmall)
        .label("详情")
        .tooltip("在右侧详情栏查看该插件")
        .on_click(move |_, window, app| {
            on_select(window, app);
            app.stop_propagation();
        })
}

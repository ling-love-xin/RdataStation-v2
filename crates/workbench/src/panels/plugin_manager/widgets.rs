//! 插件管理页的纯函数与动作按钮。
//!
//! 这里只做「状态 → 文案 / 筛选 / 统计 / 按钮」：不读窗口、不读主题，
//! 渲染层负责把结果套上 `cx.theme().colors.*`。

use gpui_kit::base::Disableable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::{Sizable, Size};
use gpui_kit::{App, ClipboardItem, SharedString, Window};
use std::collections::BTreeMap;

use plugin::ContributionCounts;
use plugin::PluginInfo;
use plugin::PluginKind;
use plugin::manifest::BackendRuntime;
use plugin::wasm::PluginState;

use super::super::left_tabs::{PluginPanel, uninstall_confirm_description};

/// 状态筛选档（对齐原型 toolbar 的五档里去掉「可更新」——RDS 没有市场 / 远程索引）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginFilter {
    All,
    Enabled,
    Disabled,
    Error,
    Drivers,
}

impl PluginFilter {
    pub(crate) const ALL: [PluginFilter; 5] = [
        PluginFilter::All,
        PluginFilter::Enabled,
        PluginFilter::Disabled,
        PluginFilter::Error,
        PluginFilter::Drivers,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            PluginFilter::All => "全部",
            PluginFilter::Enabled => "已启用",
            PluginFilter::Disabled => "已停用",
            PluginFilter::Error => "出错",
            PluginFilter::Drivers => "含驱动",
        }
    }

    fn matches(self, plugin: &PluginInfo) -> bool {
        match self {
            PluginFilter::All => true,
            PluginFilter::Enabled => plugin_state_is_active(plugin.state),
            PluginFilter::Disabled => plugin.state == PluginState::Inactive,
            PluginFilter::Error => plugin.state == PluginState::Error,
            // 「含驱动」不按状态筛：排障时经常要先找到驱动，再看它为什么没进下拉。
            PluginFilter::Drivers => !plugin.drivers.is_empty(),
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

/// 驱动的真实登记统计（声明数 / 已登记数 / 是否有插件读不到登记表）。
///
/// 驱动下拉的数据源是 `global.db` 的 `drivers` 表；插件已加载但驱动没登记时，
/// 下拉里就是空的。页头把这个数字摊开，用户不用逐个插件点详情去猜。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PluginDriverStats {
    pub declared: usize,
    pub registered: usize,
    /// 有插件声明了驱动、但这次没读到登记表（`registered_drivers == None`）。
    pub unknown: bool,
    /// 同一驱动 id 被多个**启用中**插件声明（加载期会拒绝，两个插件都不可用）。
    pub conflicts: usize,
}

/// 一个驱动 id 被多个启用插件声明的冲突。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DriverIdConflict {
    pub driver_id: String,
    pub plugins: Vec<String>,
}

/// 找出被多个启用中插件同时贡献的驱动 id。
///
/// 只看 `Active` / `Loaded`：停用的插件不参与注册，不会触发加载期冲突；把它算进来
/// 会让用户看到「冲突」却找不到该停用谁。
pub(crate) fn driver_id_conflicts(plugins: &[PluginInfo]) -> Vec<DriverIdConflict> {
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for plugin in plugins {
        if !plugin_state_is_active(plugin.state) {
            continue;
        }
        for driver in &plugin.drivers {
            owners
                .entry(driver.id.as_str())
                .or_default()
                .push(plugin.id.as_str());
        }
    }
    owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(driver_id, mut owners)| {
            owners.sort_unstable();
            owners.dedup();
            DriverIdConflict {
                driver_id: driver_id.to_string(),
                plugins: owners.into_iter().map(str::to_string).collect(),
            }
        })
        .collect()
}

pub(crate) fn plugin_driver_stats(plugins: &[PluginInfo]) -> PluginDriverStats {
    let mut stats = PluginDriverStats::default();
    for plugin in plugins {
        stats.declared += plugin.drivers.len();
        match plugin.registered_drivers.as_deref() {
            Some(registrations) => {
                stats.registered += plugin
                    .drivers
                    .iter()
                    .filter(|driver| {
                        registrations.iter().any(|registration| {
                            registration.id == driver.id && registration.enabled
                        })
                    })
                    .count();
            }
            None if !plugin.drivers.is_empty() => stats.unknown = true,
            None => {}
        }
    }
    stats.conflicts = driver_id_conflicts(plugins).len();
    stats
}

/// 页头那一条统计文案。`unknown` 只在真读不到登记表时出现，不冒充「未登记」。
pub(crate) fn plugin_driver_stats_label(stats: PluginDriverStats) -> String {
    if stats.declared == 0 {
        return "驱动 0".to_string();
    }
    let base = if stats.unknown {
        format!(
            "驱动 {}/{} 已登记 · 状态未知",
            stats.registered, stats.declared
        )
    } else {
        format!("驱动 {}/{} 已登记", stats.registered, stats.declared)
    };
    if stats.conflicts > 0 {
        format!("{base} · {} 个驱动 id 冲突", stats.conflicts)
    } else {
        base
    }
}

/// 驱动链路某一步的健康度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DriverChainLevel {
    Ok,
    Warn,
    Bad,
}

/// 驱动链路的一步（清单声明 → 插件启用 → 写入 drivers 表 → 连接下拉）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DriverChainStep {
    pub label: &'static str,
    pub detail: String,
    pub level: DriverChainLevel,
}

/// 驱动链路快照：四步状态 + 当前卡点（全通时没有卡点）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginDriverChain {
    pub steps: Vec<DriverChainStep>,
    pub bottleneck: Option<String>,
    /// 同时被多个启用插件贡献的驱动 id（加载期会拒绝，需先移除其中一个）。
    pub conflicts: Vec<DriverIdConflict>,
}

/// 把「清单声明 → 激活 → 写 drivers 表 → 下拉可见」这条链摊成可判定的四步。
///
/// 四个事实全部来自 `PluginInfo` 的缓存快照，不额外查库；`unknown` 表示本次没读到
/// `global.db`，此时第三步是「警告」而不是「未写入」，不能把读不到当成失败。
pub(crate) fn plugin_driver_chain(plugins: &[PluginInfo]) -> PluginDriverChain {
    let conflicts = driver_id_conflicts(plugins);
    let mut declared = 0usize;
    let mut active = 0usize;
    let mut registered = 0usize;
    // 行是否存在与行是否可用要分开数：激活失败的插件会把行写进 drivers 表但置为停用，
    // 这时「没写入」的提示是错的，用户会去查登记代码而不是查插件错误。
    let mut present = 0usize;
    let mut disabled = 0usize;
    let mut unknown = false;
    for plugin in plugins {
        declared += plugin.drivers.len();
        if plugin_state_is_active(plugin.state) {
            active += plugin.drivers.len();
        }
        match plugin.registered_drivers.as_deref() {
            Some(registrations) => {
                for driver in &plugin.drivers {
                    let Some(registration) = registrations
                        .iter()
                        .find(|registration| registration.id == driver.id)
                    else {
                        continue;
                    };
                    present += 1;
                    if registration.enabled {
                        registered += 1;
                    } else {
                        disabled += 1;
                    }
                }
            }
            None if !plugin.drivers.is_empty() => unknown = true,
            None => {}
        }
    }

    let steps = vec![
        DriverChainStep {
            label: "清单声明",
            detail: format!("{declared} 个驱动"),
            level: if declared > 0 {
                DriverChainLevel::Ok
            } else {
                DriverChainLevel::Bad
            },
        },
        DriverChainStep {
            label: "插件启用",
            detail: if conflicts.is_empty() {
                format!("{active} 个驱动在启用插件内")
            } else {
                format!("{active} 个驱动在启用插件内 · {} 个 id 冲突", conflicts.len())
            },
            level: if active > 0 && conflicts.is_empty() {
                DriverChainLevel::Ok
            } else {
                DriverChainLevel::Bad
            },
        },
        DriverChainStep {
            label: "写入 drivers 表",
            detail: if unknown {
                "登记表未读取".to_string()
            } else if present == 0 {
                "0 行已写入".to_string()
            } else if registered == 0 {
                format!("{present} 行已写入 · {disabled} 行停用")
            } else if disabled > 0 {
                format!("{registered} 行可用 · {disabled} 行停用")
            } else {
                format!("{registered} 行可用")
            },
            level: if unknown {
                DriverChainLevel::Warn
            } else if registered > 0 {
                DriverChainLevel::Ok
            } else if present > 0 {
                // 行在、但全部停用：这不是没登记，而是插件没激活成功。
                DriverChainLevel::Warn
            } else {
                DriverChainLevel::Bad
            },
        },
        DriverChainStep {
            label: "连接下拉",
            detail: if registered > 0 {
                format!("{registered} 个驱动可选")
            } else {
                "下拉为空".to_string()
            },
            level: if registered > 0 {
                DriverChainLevel::Ok
            } else {
                DriverChainLevel::Bad
            },
        },
    ];

    let bottleneck = if declared == 0 {
        Some("卡在：没有任何插件声明驱动。".to_string())
    } else if active == 0 {
        Some("卡在：声明了驱动的插件都未启用。".to_string())
    } else if !conflicts.is_empty() {
        let detail = conflicts
            .iter()
            .map(|conflict| {
                format!(
                    "{} 由 {} 同时声明",
                    conflict.driver_id,
                    conflict.plugins.join("、")
                )
            })
            .collect::<Vec<_>>()
            .join("；");
        Some(format!(
            "卡在：驱动 id 冲突（{detail}），先停用或卸载其中一个插件。"
        ))
    } else if unknown {
        Some("卡在：本次没读到 global.db 的 drivers 表。".to_string())
    } else if present == 0 {
        Some("卡在：驱动行没有写入 global.db（看插件错误与 sidecar.log）。".to_string())
    } else if registered == 0 {
        Some(
            "卡在：驱动行已写入但处于停用状态（插件激活没成功，看插件错误与 sidecar.log）。"
                .to_string(),
        )
    } else if registered < declared {
        Some(format!(
            "注意：{} 个驱动不可用（{} 行停用、{} 个未登记）。",
            declared - registered,
            disabled,
            declared - present
        ))
    } else {
        None
    };

    PluginDriverChain {
        steps,
        bottleneck,
        conflicts,
    }
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
            let mut haystack = format!(
                "{} {} {} {}",
                plugin.name, plugin.id, plugin.metadata.description, plugin.metadata.author
            )
            .to_lowercase();
            // 驱动 id / 显示名 / 分类也进搜索：用户找的是「Oracle」「SQL Server」，
            // 不是插件包名（`rds.driver.oracle-rust` 这类 id 记不住）。
            for driver in &plugin.drivers {
                haystack.push(' ');
                haystack.push_str(&driver.id.to_lowercase());
                haystack.push(' ');
                haystack.push_str(&driver.display_name.to_lowercase());
                if let Some(category) = driver.category.as_deref() {
                    haystack.push(' ');
                    haystack.push_str(&category.to_lowercase());
                }
            }
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

/// 卡片上的形态一行：`侧车进程 · 原生 rust` / `侧车进程 · python 解释器`。
///
/// 原生 sidecar 与脚本 sidecar 都显示「侧车进程」，用户只能靠运行时语言区分；
/// 不显示 `runtime` 时会把「不需要解释器」的 Rust 插件误读成缺环境。
pub(crate) fn plugin_backend_label(plugin: &PluginInfo) -> String {
    let kind = plugin_kind_label(plugin.kind);
    match plugin.runtime {
        Some(BackendRuntime::Rust) => format!("{kind} · 原生 rust"),
        Some(BackendRuntime::Java) => format!("{kind} · java 解释器"),
        Some(BackendRuntime::Python) => format!("{kind} · python 解释器"),
        None => kind.to_string(),
    }
}

/// 详情栏驱动条目的登记状态文案。
///
/// 清单声明只说明「插件想贡献什么」，真正决定连接对话框看不看得到的是 `drivers` 表里
/// 有没有这一行。`None`（本次没读到全局库）不能当成「未登记」——两者给的下一步动作不同。
pub(crate) fn plugin_driver_registration_label(
    plugin: &PluginInfo,
    driver_id: &str,
) -> &'static str {
    let Some(rows) = plugin.registered_drivers.as_deref() else {
        return "登记状态未知";
    };
    match rows.iter().find(|row| row.id == driver_id) {
        Some(row) if row.enabled => "已登记",
        Some(_) => "已登记 · 已停用",
        None => "未登记",
    }
}

/// 是否声明了「会接收数据库密码」的凭据权限（详情栏要显式告警）。
pub(crate) fn plugin_receives_database_credentials(plugin: &PluginInfo) -> bool {
    plugin.permissions.credentials.as_deref() == Some("database")
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

/// 生成可粘贴到问题反馈 / 排查记录里的纯文本诊断。
///
/// 覆盖「驱动为什么没出现」需要的全部事实：状态、错误、运行形态、安装目录、
/// sidecar 日志路径，以及每个驱动在 `global.db` 的登记结果。
pub(crate) fn plugin_diagnostics(plugin: &PluginInfo) -> String {
    let mut lines = vec![
        format!("插件：{}（{}）", plugin.name, plugin.id),
        format!("版本：{}", plugin.version),
        format!("类型：{}", plugin_kind_label(plugin.kind)),
        format!("运行形态：{}", plugin_backend_label(plugin)),
        format!("状态：{}", plugin_state_label(plugin.state)),
        format!(
            "安装目录：{}",
            plugin
                .install_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "未上报".to_string())
        ),
        format!(
            "sidecar 日志：{}",
            plugin::sidecar_log_path(&plugin.id).display()
        ),
    ];
    if let Some(error) = plugin.error.as_deref() {
        lines.push(format!("错误：{error}"));
    }
    if plugin.drivers.is_empty() {
        lines.push("驱动：未声明".to_string());
    } else {
        lines.push("驱动：".to_string());
        for driver in &plugin.drivers {
            lines.push(format!(
                "  - {}（{}）· {} · 默认端口 {}",
                driver.display_name,
                driver.id,
                plugin_driver_registration_label(plugin, &driver.id),
                driver
                    .default_port
                    .map(|port| port.to_string())
                    .unwrap_or_else(|| "—".to_string()),
            ));
        }
    }
    lines.join("\n")
}

/// 卸载被拦截的原因（有插件依赖它时返回说明；未读到依赖表时**不拦**）。
///
/// 缓存未知（`None`）不能当成「没人依赖」而拦住卸载：真正的拦截由
/// `PluginManager::uninstall_plugin` 实时查库兜底，这里只做已知情况下的前置提示。
pub(crate) fn plugin_uninstall_block_reason(plugin: &PluginInfo) -> Option<String> {
    let dependents = plugin.dependents.as_deref()?;
    if dependents.is_empty() {
        return None;
    }
    Some(format!("被 {} 依赖，需先卸载它们", dependents.join("、")))
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
    let block_reason = plugin_uninstall_block_reason(plugin);
    let uninstall_hint = match &block_reason {
        Some(reason) => reason.clone(),
        None => "从磁盘删除该插件（保留 plugin-data 用户数据）".to_string(),
    };
    let uninstall = Button::new(SharedString::from(format!("plugin-{scope}-{id}-uninstall")))
        .ghost()
        .with_size(Size::XSmall)
        .label("卸载")
        .tooltip(uninstall_hint)
        .disabled(busy || block_reason.is_some())
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

/// 详情栏的文件动作：打开插件目录 / 打开 sidecar 日志。
///
/// 这两个入口解决的是「插件显示已启用、但驱动没出现」时只能靠日志文件排查的问题：
/// - 目录来自 `PluginInfo::install_path`（wasm 运行时不上报磁盘路径 → 按钮禁用）；
/// - 日志路径与 sidecar 启动时写 stderr 的落点同一口径（`plugin::sidecar_log_path`），
///   还没启动过就没有文件 → 按钮禁用，而不是点了没反应。
pub(crate) fn plugin_file_buttons(
    scope: &str,
    plugin: &PluginInfo,
    entity: gpui_kit::Entity<PluginPanel>,
) -> Vec<Button> {
    let dir_path = plugin.install_path.clone();
    let dir_hint = match dir_path.as_ref() {
        Some(path) if path.exists() => format!("在文件管理器打开：{}", path.display()),
        Some(path) => format!("安装目录已不存在：{}", path.display()),
        None => "该插件不上报安装目录（wasm 运行时）".to_string(),
    };
    let dir_disabled = !dir_path.as_ref().is_some_and(|path| path.exists());

    let log_path = plugin::sidecar_log_path(&plugin.id);
    let log_exists = log_path.exists();
    let log_hint = if log_exists {
        format!("用默认应用打开：{}", log_path.display())
    } else {
        format!(
            "还没有 sidecar 日志（驱动未启动过）：{}",
            log_path.display()
        )
    };

    let dir_button = Button::new(SharedString::from(format!(
        "plugin-{scope}-{}-open-dir",
        plugin.id
    )))
    .ghost()
    .with_size(Size::XSmall)
    .label("目录")
    .tooltip(SharedString::from(dir_hint))
    .disabled(dir_disabled)
    .on_click({
        let entity = entity.clone();
        let path = dir_path;
        move |_, _window, app| {
            let Some(path) = path.as_ref() else {
                return;
            };
            let result = opener::open(path);
            let _ = entity.update(app, |panel, cx| {
                panel.show_notice(
                    match &result {
                        Ok(()) => format!("已打开插件目录：{}", path.display()),
                        Err(error) => format!("打开插件目录失败：{error}"),
                    },
                    cx,
                );
            });
        }
    });

    let log_button = Button::new(SharedString::from(format!(
        "plugin-{scope}-{}-open-log",
        plugin.id
    )))
    .ghost()
    .with_size(Size::XSmall)
    .label("日志")
    .tooltip(SharedString::from(log_hint))
    .disabled(!log_exists)
    .on_click({
        let entity = entity.clone();
        let path = log_path;
        move |_, _window, app| {
            let result = opener::open(&path);
            let _ = entity.update(app, |panel, cx| {
                panel.show_notice(
                    match &result {
                        Ok(()) => format!("已打开 sidecar 日志：{}", path.display()),
                        Err(error) => format!("打开 sidecar 日志失败：{error}"),
                    },
                    cx,
                );
            });
        }
    });

    // 「复制诊断」：把状态 / 错误 / 路径 / 驱动登记一次性带走，比让用户逐段截图可靠。
    let diagnostics = plugin_diagnostics(plugin);
    let plugin_id = plugin.id.clone();
    let copy_diagnostics_button = Button::new(SharedString::from(format!(
        "plugin-{scope}-{}-copy-diagnostics",
        plugin.id
    )))
    .ghost()
    .with_size(Size::XSmall)
    .label("复制诊断")
    .tooltip("复制插件状态、驱动登记与日志路径，便于排查")
    .on_click({
        let entity = entity.clone();
        move |_, _window, app| {
            app.write_to_clipboard(ClipboardItem::new_string(diagnostics.clone()));
            let _ = entity.update(app, |panel, cx| {
                panel.show_notice(format!("已复制 {plugin_id} 的诊断信息"), cx);
            });
        }
    });

    vec![dir_button, log_button, copy_diagnostics_button]
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

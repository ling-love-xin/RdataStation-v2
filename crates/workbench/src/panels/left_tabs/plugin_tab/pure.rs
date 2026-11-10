use super::super::*;

/// 面板是否有进行中的操作（纯函数，便于单测）。
///
/// `progress` 既是状态栏文案也是忙碌标记：安装 / 重载 / 启停 / 卸载 / 重试任一在跑，
/// 其它入口都必须拒绝或置灰——它们会同时改 `plugins/<id>` 目录或进程状态。
pub(crate) fn panel_busy(progress: &Rc<RefCell<Option<String>>>) -> bool {
    progress.borrow().is_some()
}

/// 贡献点明细（纯函数，便于单测）：只列非零类别，驱动也算贡献。
pub(crate) fn contribution_breakdown(counts: ContributionCounts) -> String {
    let mut parts = Vec::new();
    if counts.commands > 0 {
        parts.push(format!("命令 {}", counts.commands));
    }
    if counts.panels > 0 {
        parts.push(format!("面板 {}", counts.panels));
    }
    if counts.settings > 0 {
        parts.push(format!("设置 {}", counts.settings));
    }
    if counts.drivers > 0 {
        parts.push(format!("驱动 {}", counts.drivers));
    }
    parts.join(" · ")
}

/// 插件行第二行文案（纯函数，便于单测）：无错误时只显示 id；
/// 出错时把原因接在后面（`id · 原因：…`），面板行高固定 3rem，放不下第三行。
pub(crate) fn plugin_row_subtitle(id: &str, error: Option<&str>) -> String {
    match error {
        Some(reason) => format!("{id} · 原因：{reason}"),
        None => id.to_string(),
    }
}

/// 安装回执（纯函数，便于单测）：把「库里的启用状态 + 运行时状态」翻成一句人话。
///
/// 覆盖安装 / 重装会沿用库里的 `is_enabled`：停用中的插件装完仍停用，回执不能只说
/// 「已安装」；激活失败（`PluginState::Error`，含 `ActivationFailed` / `LoadFailed`）
/// 也要在这里说，否则用户以为装好了却连不上。
pub(crate) fn install_receipt(
    name: &str,
    version: &str,
    code: &str,
    is_enabled: bool,
    state: Option<PluginState>,
) -> String {
    if !is_enabled {
        format!("已安装 {name} v{version}（{code}，保持停用）")
    } else {
        match state {
            Some(PluginState::Error) => {
                format!("已安装 {name} v{version}（{code}），但加载或激活失败，详见日志")
            }
            Some(PluginState::Active) => format!("已安装并启用 {name} v{version}（{code}）"),
            _ => format!("已安装 {name} v{version}（{code}）"),
        }
    }
}

/// 该事件是否可能改变插件列表（纯函数，便于单测）。
///
/// 生命周期事件都会改列表或状态；`Custom` 是插件自定义事件，与面板清单无关，
/// 订阅侧不能因为它每来一条就拉一次 `list_plugins`。
pub(crate) fn event_touches_plugin_list(event: &plugin::events::PluginEvent) -> bool {
    !matches!(event, plugin::events::PluginEvent::Custom { .. })
}

/// 插件状态徽标（文案 + 语义色）。
pub(crate) fn plugin_state_label(state: PluginState) -> (&'static str, &'static str) {
    match state {
        PluginState::Active => ("已激活", "success"),
        PluginState::Loaded => ("已加载", "info"),
        PluginState::Inactive => ("已停用", "muted_foreground"),
        PluginState::Error => ("出错", "danger"),
    }
}

/// 卸载确认框正文（纯函数，便于单测）：说清删什么、留什么、什么情况会被拒。
pub(crate) fn uninstall_confirm_description(name: &str, id: &str) -> String {
    format!(
        "将删除插件「{name}」（{id}）的安装目录，并停掉它正在运行的进程。\n\
         插件的用户数据（plugin-data/{id}）会保留。\n\
         若其他插件声明依赖它，卸载会被拒绝并列出引用者。确定卸载？"
    )
}

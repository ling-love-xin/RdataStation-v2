//! 结果集「自动刷新」下拉（刷新钮右箭头）：对齐 DBeaver `AutoRefreshControl` 的箭头菜单。
//!
//! 真值（逐行取证，`plugins/org.jkiss.dbeaver.ui/src/org/jkiss/dbeaver/ui/controls/autorefresh/AutoRefreshControl.java`）：
//! - **控件形态**：`ToolItem(toolbar, SWT.DROP_DOWN | SWT.NO_FOCUS)`（`:148`）。左半（主钮）点 =
//!   立即刷新（`ResultSetViewer.java:2067-2071` 传的 `refreshData(null)`）；右半（箭头）点 =
//!   弹菜单（`widgetSelected` 里 `e.detail == SWT.ARROW`，`:207`）。
//! - **菜单项**（`:216-256`）：
//!   1. `sql_editor_resultset_filter_panel_menu_customize`（中文「自定义...」）→ `runCustomized`
//!      → `AutoRefreshConfigDialog`（间隔 + 错误时停止），确定后 `enableAutoRefresh(true)`；
//!   2. `..._menu_stop`（「停止」），启用条件 = `isAutoRefreshEnabled()`；
//!   3. 分隔线；
//!   4. 预设间隔 `AUTO_REFRESH_DEFAULTS = {1, 5, 10, 15, 30, 60}`（`:194`，单位秒）；当前间隔
//!      不在预设且 `> 0` 时插到列表**首位**（`:230-233`）；**首项**用单数文案
//!      「每 {0} 秒刷新一次」，其余用「... {0} 秒」（`:238-239`）；当前已开启且命中某项 →
//!      `setDefaultItem`（`:241-243`，语义上是"当前默认项"）；
//!   5. 若有 hint，再分隔 + 一条**禁用**项（`:247-256`；注释说明它兼作"防误触下坠"的下沿填充）。
//! - **开启态视觉**：图标换 `UIIcon.CLOCK_STOP`、tooltip 换「停止自动刷新」（`:174-184`）；
//!   关闭态回默认图标与 tooltip（`defRefreshIcon` / `defRefreshText`）。
//!
//! 本仓落点：主钮保持既有「⟳ 刷新」文字形态（真值为纯图标），右箭头由 `DropdownButton` 提供；
//! 菜单项由本文件的**纯函数**给出——`PopupMenu` 不暴露菜单项（见 `gpui-kit-dev` skill），
//! 抽出来才能断言长相。

use shared::i18n::{tr, tr_dyn};

/// DBeaver `AUTO_REFRESH_DEFAULTS`（`AutoRefreshControl.java:194`，单位秒）。
pub const AUTO_REFRESH_DEFAULTS: [u32; 6] = [1, 5, 10, 15, 30, 60];

/// 菜单末尾那条提示（真值 `controls_resultset_viewer_frequent_refresh_hint`，
/// `ResultSetMessages_zh.properties:146`）。
///
/// 真值的 hint 来自 `setHintSupplier`，按事务状态二选一（`AutoRefreshControl.java:292-302`）：
/// autocommit → 本条；非 autocommit → `..._switch_autocommit_hint`（`:184`，「切换到自动提交
/// 以查看外部更改」）。本仓没有事务状态端口可问，固定取 autocommit 口径，登记为有意差异。
pub const AUTO_REFRESH_HINT: &str = "提示：频繁刷新可能会导致服务器负载过高";

/// 菜单项点下去要做什么（真值 `:218-245` 三个入口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoRefreshAction {
    /// 「自定义...」：打开刷新配置对话框（本仓无该对话框，入队给宿主）
    Configure,
    /// 「停止」：关掉自动刷新
    Stop,
    /// 预设间隔（秒）：开启自动刷新并记下间隔
    SetInterval(u32),
}

/// 一条菜单项（`grep` 面 + 断言面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoRefreshEntry {
    pub label: String,
    /// `None` = 纯文案项（真值里只有 hint 项属于这类，且是禁用的）
    pub action: Option<AutoRefreshAction>,
    pub separator_before: bool,
    /// "当前默认项"（真值 `setDefaultItem`，`:241-243`）
    pub checked: bool,
}

/// 生成刷新箭头菜单的全部项。
///
/// `enabled` = 自动刷新当前是否打开；`interval_secs` = 当前间隔（0 = 从未配置，
/// 与 DBeaver `RefreshSettings.refreshInterval` 的默认值一致，`RefreshSettings.java:26`）。
/// `hint` = 菜单末尾那条**禁用提示**（真值 `:247-256`：`setHintSupplier` 给的文案非空才加，
/// 且前面再补一条分隔线；注释说明它兼作"防误触下坠"的下沿填充）。
pub fn menu_entries(enabled: bool, interval_secs: u32, hint: Option<&str>) -> Vec<AutoRefreshEntry> {
    let mut entries = vec![
        AutoRefreshEntry {
            label: tr("自定义...").to_string(),
            action: Some(AutoRefreshAction::Configure),
            separator_before: false,
            checked: false,
        },
        AutoRefreshEntry {
            label: tr("停止").to_string(),
            action: Some(AutoRefreshAction::Stop),
            separator_before: false,
            checked: false,
        },
    ];
    // 预设列表：当前间隔不在预设里且 > 0 → 插到首位（真值 `:230-233`）
    let mut presets: Vec<u32> = AUTO_REFRESH_DEFAULTS.to_vec();
    if interval_secs > 0 && !presets.contains(&interval_secs) {
        presets.insert(0, interval_secs);
    }
    for (index, secs) in presets.into_iter().enumerate() {
        // 首项用单数 pattern，其余用省略号 pattern（真值 `:238-239`）
        let template = if index == 0 {
            "每 {0} 秒刷新一次"
        } else {
            "... {0} 秒"
        };
        let label = tr_dyn(template).replace("{0}", &secs.to_string());
        entries.push(AutoRefreshEntry {
            label,
            action: Some(AutoRefreshAction::SetInterval(secs)),
            separator_before: index == 0,
            checked: enabled && secs == interval_secs,
        });
    }
    // 真值 `:247-256`：hint 非空才摆——分隔 + 一条**禁用**项（`action: None` 就是这条的
    // 判据，渲染侧见 `menu.item(..disabled(true))`）。空串按"没 hint"处理（真值
    // `CommonUtils.isEmpty` 同口径）。
    if let Some(text) = hint.map(str::trim).filter(|text| !text.is_empty()) {
        entries.push(AutoRefreshEntry {
            label: text.to_string(),
            action: None,
            separator_before: true,
            checked: false,
        });
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_starts_with_customize_then_stop_then_presets() {
        let entries = menu_entries(false, 0, None);
        assert_eq!(entries.len(), 2 + AUTO_REFRESH_DEFAULTS.len());
        assert_eq!(entries[0].label, "自定义...");
        assert_eq!(entries[0].action, Some(AutoRefreshAction::Configure));
        assert_eq!(entries[1].label, "停止");
        assert_eq!(entries[1].action, Some(AutoRefreshAction::Stop));
        // 首个预设项前有分隔线（真值 `:225`）
        assert!(entries[2].separator_before);
        assert_eq!(entries[2].label, "每 1 秒刷新一次");
        assert_eq!(entries[3].label, "... 5 秒");
        assert_eq!(
            entries.last().and_then(|e| e.action),
            Some(AutoRefreshAction::SetInterval(60))
        );
    }

    #[test]
    fn interval_outside_presets_is_inserted_first() {
        let entries = menu_entries(true, 7, None);
        // 「自定义...」「停止」之后的第一条预设 = 当前的非预设间隔
        assert_eq!(entries[2].action, Some(AutoRefreshAction::SetInterval(7)));
        assert_eq!(entries[2].label, "每 7 秒刷新一次");
        assert_eq!(entries[3].label, "... 1 秒");
    }

    #[test]
    fn enabled_marks_only_the_active_interval() {
        let entries = menu_entries(true, 10, None);
        let checked: Vec<&AutoRefreshEntry> = entries.iter().filter(|e| e.checked).collect();
        assert_eq!(checked.len(), 1, "只有当前间隔算默认项");
        assert_eq!(checked[0].action, Some(AutoRefreshAction::SetInterval(10)));
    }

    #[test]
    fn disabled_marks_nothing() {
        let entries = menu_entries(false, 10, None);
        assert!(entries.iter().all(|e| !e.checked), "未开启时没有默认项");
        // 「停止」项的启用态由渲染侧按 enabled 决定（真值 `:223`）
        assert_eq!(entries[1].action, Some(AutoRefreshAction::Stop));
    }

    #[test]
    fn hint_is_a_disabled_trailing_item_after_a_separator() {
        // 真值 `:247-256`：hint 非空 → 分隔 + 禁用项，且排在所有预设之后
        let entries = menu_entries(true, 10, Some("提示：频繁刷新可能会导致服务器负载过高"));
        let last = entries.last().expect("至少有一条 hint");
        assert_eq!(last.label, "提示：频繁刷新可能会导致服务器负载过高");
        assert_eq!(last.action, None, "hint 项没有动作（渲染侧画成禁用项）");
        assert!(last.separator_before, "hint 前面要补一条分隔线");
        assert!(!last.checked);
        // 空串 / 纯空白按「没 hint」处理（真值 `CommonUtils.isEmpty` 同口径）
        assert_eq!(menu_entries(true, 10, Some("   ")).len(), 2 + AUTO_REFRESH_DEFAULTS.len());
        assert_eq!(menu_entries(true, 10, None).len(), 2 + AUTO_REFRESH_DEFAULTS.len());
    }
}
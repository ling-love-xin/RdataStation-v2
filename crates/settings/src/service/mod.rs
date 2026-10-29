//! 设置服务骨架：加载、读取、修改（含主题即时切换）的**唯一读写入口**。
//!
//! - `value_by_key`（读臂）与 `apply_by_key`（写臂）是横切护栏：分发到各域
//!   `service/<域>.rs` 的 `apply_slot`（写），各域读写访问器（set_*/getter）在
//!   对应域文件的 `impl SettingsService` 块里。
//! - 新增一个域 = `service/<域>.rs` 提供 `apply_slot` + 访问器，再在骨架
//!   `apply_by_key` 的分发 match 里补一组 arm（登记点）。

use gpui_kit::App;
use gpui_kit::component::ThemeMode;

use crate::model::{AppearanceMode, Settings};
use crate::persistence::{load_settings, publish_connection_defaults};
use crate::registry::{SettingValue, Slot};

mod appearance;
mod connection;
mod editor;
mod insight;
mod layout;
mod logging;
mod navigator;
mod notifications;
mod projects;
mod resources;
mod result_panel;
mod scratchpad;
mod startup;

// 副作用出口 sink 的 re-export：lib.rs 据此保持 `settings::install_*` 公共路径不变。
pub use insight::install_insight_memory_limit_sink;
pub use logging::install_log_level_sink;

/// 主题模式的文本形态（落盘值，与 `registry` 的枚举选项同源）。
pub(crate) fn theme_mode_text(mode: AppearanceMode) -> String {
    serde_json::to_value(mode)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// 文本 → 主题模式；未知文本返回 `None`（**不猜**，由调用方拒绝并提示）。
pub(crate) fn theme_mode_from_text(text: &str) -> Option<AppearanceMode> {
    serde_json::from_value(serde_json::Value::String(text.to_string())).ok()
}

/// 显式模式 → `ThemeMode`；`System` 返回 `None`（由调用方走系统外观解析，不静默落到一侧）。
pub(crate) fn explicit_theme_mode(mode: AppearanceMode) -> Option<ThemeMode> {
    match mode {
        AppearanceMode::Light => Some(ThemeMode::Light),
        AppearanceMode::Dark => Some(ThemeMode::Dark),
        AppearanceMode::System => None,
    }
}

/// 按 key 读当前值（页面渲染、「非默认值」判定与契约测试共用）。
///
/// 返回 `None`：key 未登记，或该项是复合值（标量类型表达不了）。
pub fn value_by_key(settings: &Settings, key: &str) -> Option<SettingValue> {
    let slot = crate::registry::slot_for(key)?;
    Some(match slot {
        Slot::AppearanceThemeMode => {
            SettingValue::Text(theme_mode_text(settings.appearance.theme_mode))
        }
        Slot::AppearanceFontSize => {
            SettingValue::Number(settings.appearance.font_size as f64)
        }
        Slot::AppearanceLocale => SettingValue::Text(settings.appearance.locale.clone()),
        Slot::AppearanceTabBarMaxWidth => {
            SettingValue::Number(settings.appearance.tab_bar_max_width as f64)
        }
        Slot::AppearanceTabBarVariant => {
            SettingValue::Text(settings.appearance.tab_bar_variant.clone())
        }
        Slot::EditorShowLineNumbers => SettingValue::Bool(settings.editor.show_line_numbers),
        Slot::EditorShowFolds => SettingValue::Bool(settings.editor.show_folds),
        Slot::EditorIndentFolds => SettingValue::Bool(settings.editor.indent_folding),
        Slot::EditorFoldOpenKeywords => SettingValue::Text(settings.editor.fold_open_keywords.clone()),
        Slot::EditorFoldCloseKeywords => SettingValue::Text(settings.editor.fold_close_keywords.clone()),
        Slot::EditorSoftWrap => SettingValue::Bool(settings.editor.soft_wrap),
        Slot::EditorAutoClose => SettingValue::Bool(settings.editor.auto_close),
        Slot::EditorSmartIndent => SettingValue::Bool(settings.editor.smart_indent),
        Slot::EditorInlineCompletion => SettingValue::Bool(settings.editor.inline_completion),
        Slot::EditorDefaultTabSize => {
            SettingValue::Number(settings.editor.default_tab_size as f64)
        }
        Slot::EditorDefaultHardTabs => SettingValue::Bool(settings.editor.default_hard_tabs),
        Slot::EditorDefaultLineEnding => {
            SettingValue::Text(settings.editor.default_line_ending.clone())
        }
        Slot::EditorDefaultEncoding => {
            SettingValue::Text(settings.editor.default_encoding.clone())
        }
        Slot::EditorFormatKeywordCase => {
            SettingValue::Text(settings.editor.format_keyword_case.clone())
        }
        Slot::EditorFormatIndentWidth => {
            SettingValue::Number(settings.editor.format_indent_width as f64)
        }
        Slot::EditorFormatOnSave => SettingValue::Bool(settings.editor.format_on_save),
        Slot::NavigatorSourceShortCode => SettingValue::Bool(settings.navigator.source_short_code),
        Slot::NavigatorShowTags => SettingValue::Bool(settings.navigator.show_tags),
        Slot::NavigatorShowScope => SettingValue::Bool(settings.navigator.show_scope),
        Slot::NavigatorShowNameTypeShort => {
            SettingValue::Bool(settings.navigator.show_name_type_short)
        }
        Slot::NavigatorRowEstimate => SettingValue::Bool(settings.navigator.row_estimate),
        Slot::NavigatorShowInlineStats => {
            SettingValue::Bool(settings.navigator.show_inline_stats)
        }
        Slot::NavigatorShowInlineComment => {
            SettingValue::Bool(settings.navigator.show_inline_comment)
        }
        Slot::NavigatorShowInlineSize => SettingValue::Bool(settings.navigator.show_inline_size),
        // 复合值（facet 筛选）没有标量形态：它的读写走导航面板自己的入口。
        Slot::NavigatorFilters => return None,
        // 复合值（已保存视图）：同上。
        Slot::NavigatorSavedViews => return None,
        // 复合值（可见库白名单，S3-①）：读写走 SettingsService。
        Slot::NavigatorVisibleDatabases => return None,
        // 复合值（全局对象名 mask，B-1）：读写走对象名过滤器对话框。
        Slot::NavigatorGlobalMask => return None,
        Slot::NavigatorShowChips => SettingValue::Bool(settings.navigator.show_chips),
        Slot::NavigatorShowSearch => SettingValue::Bool(settings.navigator.show_search),
        Slot::NavigatorAutoConnectStartup => SettingValue::Bool(settings.navigator.auto_connect_on_startup),
        Slot::ResourcesKeepVersions => {
            SettingValue::Number(settings.resources.keep_versions as f64)
        }
        Slot::ResourcesDefaultSort => SettingValue::Text(settings.resources.default_sort.clone()),
        // 复合值（折叠状态）：读写走资产库面板自己的入口。
        Slot::ResourcesCollapsedGroups => return None,
        // 复合值（默认分组）：同上（入口在分组头右键）。
        Slot::ResourcesDefaultGroup => return None,
        Slot::ConnectTimeoutMs => {
            SettingValue::Number(settings.connection_defaults.connect_timeout_ms as f64)
        }
        Slot::LanDisableTls => SettingValue::Bool(settings.connection_defaults.lan_disable_tls),
        Slot::ProjectSortMode => SettingValue::Text(settings.projects.sort_mode.clone()),
        Slot::LogMinLevel => SettingValue::Text(settings.logging.min_level.as_str().to_string()),
        Slot::StartupDefaultLeftPanel => {
            SettingValue::Text(settings.startup.default_left_panel.clone())
        }
        // 复合值（工具栏按钮布局）：读写走草稿箱 ⋯ 菜单自己的入口。
        Slot::ScratchpadToolbar => return None,
        // 分区高度上限（数值）：读写走分区拖拽 handle，无标量入口。
        Slot::ScratchpadGroupMaxH => SettingValue::Number(settings.scratchpad.group_max_h as f64),
        Slot::InsightMemoryLimitMb => {
            SettingValue::Number(settings.insight.memory_limit_mb as f64)
        }
        Slot::InsightFullScanConfirm => {
            SettingValue::Bool(settings.insight.full_scan_confirm)
        }
        Slot::InsightWorkSetLimit => SettingValue::Number(settings.insight.work_set_limit as f64),
        Slot::InsightStatusUsage => SettingValue::Bool(settings.insight.status_usage),
        Slot::ResultPanelFactStorage => SettingValue::Text(
            settings.result_panel.fact_storage.as_str().to_string(),
        ),
        Slot::ResultPanelShowColumnComments => {
            SettingValue::Bool(settings.result_panel.show_column_comments)
        }
        Slot::ResultPanelShowTypeBadge => {
            SettingValue::Bool(settings.result_panel.show_type_badge)
        }
        Slot::ResultPanelLoopSelection => {
            SettingValue::Bool(settings.result_panel.loop_selection)
        }
        Slot::ResultPanelSelectOnModifierClick => {
            SettingValue::Bool(settings.result_panel.select_on_modifier_click)
        }
        Slot::ResultPanelPersistColumnWidths => {
            SettingValue::Bool(settings.result_panel.persist_column_widths)
        }
        // 【类型样式】13 项枚举：落盘值 → 设置页显示
        Slot::ResultPanelNumberFormat => {
            SettingValue::Text(settings.result_panel.number_format.as_str().to_string())
        }
        Slot::ResultPanelNullPlaceholder => {
            SettingValue::Text(settings.result_panel.null_placeholder.as_str().to_string())
        }
        Slot::ResultPanelBoolDisplay => {
            SettingValue::Text(settings.result_panel.bool_display.as_str().to_string())
        }
        Slot::ResultPanelAlignNumeric => {
            SettingValue::Text(settings.result_panel.align_numeric.as_str().to_string())
        }
        Slot::ResultPanelAlignText => {
            SettingValue::Text(settings.result_panel.align_text.as_str().to_string())
        }
        Slot::ResultPanelAlignTemporal => {
            SettingValue::Text(settings.result_panel.align_temporal.as_str().to_string())
        }
        Slot::ResultPanelAlignBool => {
            SettingValue::Text(settings.result_panel.align_bool.as_str().to_string())
        }
        Slot::ResultPanelAlignBlob => {
            SettingValue::Text(settings.result_panel.align_blob.as_str().to_string())
        }
        Slot::ResultPanelColorNumeric => {
            SettingValue::Text(settings.result_panel.color_numeric.as_str().to_string())
        }
        Slot::ResultPanelColorText => {
            SettingValue::Text(settings.result_panel.color_text.as_str().to_string())
        }
        Slot::ResultPanelColorTemporal => {
            SettingValue::Text(settings.result_panel.color_temporal.as_str().to_string())
        }
        Slot::ResultPanelColorBool => {
            SettingValue::Text(settings.result_panel.color_bool.as_str().to_string())
        }
        Slot::ResultPanelColorBlob => {
            SettingValue::Text(settings.result_panel.color_blob.as_str().to_string())
        }
        Slot::ResultPanelBgNumeric => {
            SettingValue::Text(settings.result_panel.bg_numeric.as_str().to_string())
        }
        Slot::ResultPanelBgText => {
            SettingValue::Text(settings.result_panel.bg_text.as_str().to_string())
        }
        Slot::ResultPanelBgTemporal => {
            SettingValue::Text(settings.result_panel.bg_temporal.as_str().to_string())
        }
        Slot::ResultPanelBgBool => {
            SettingValue::Text(settings.result_panel.bg_bool.as_str().to_string())
        }
        Slot::ResultPanelBgBlob => {
            SettingValue::Text(settings.result_panel.bg_blob.as_str().to_string())
        }
        Slot::ResultPanelFontNumeric => {
            SettingValue::Text(settings.result_panel.font_numeric.as_str().to_string())
        }
        Slot::ResultPanelFontText => {
            SettingValue::Text(settings.result_panel.font_text.as_str().to_string())
        }
        Slot::ResultPanelFontTemporal => {
            SettingValue::Text(settings.result_panel.font_temporal.as_str().to_string())
        }
        Slot::ResultPanelFontBool => {
            SettingValue::Text(settings.result_panel.font_bool.as_str().to_string())
        }
        Slot::ResultPanelFontBlob => {
            SettingValue::Text(settings.result_panel.font_blob.as_str().to_string())
        }
        Slot::LayoutLocked => SettingValue::Bool(settings.layout.locked),
        Slot::LayoutLeftSidebarWidth => {
            SettingValue::Number(settings.layout.left_sidebar_width as f64)
        }
        Slot::LayoutRightSidebarWidth => {
            SettingValue::Number(settings.layout.right_sidebar_width as f64)
        }
        Slot::NotificationSuccessAutohideSecs => {
            SettingValue::Number(settings.notifications.success_autohide_secs as f64)
        }
        Slot::NotificationWarningAutohideSecs => {
            SettingValue::Number(settings.notifications.warning_autohide_secs as f64)
        }
        Slot::NotificationErrorAutohide => {
            SettingValue::Bool(settings.notifications.error_autohide)
        }
        Slot::NotificationMergeWindowSecs => {
            SettingValue::Number(settings.notifications.merge_window_secs as f64)
        }
    })
}

/// 设置服务：加载、读取、修改（含主题即时切换）。
pub struct SettingsService;

impl SettingsService {
    /// 启动时调用：加载磁盘配置并注册为 global。
    pub fn init(cx: &mut App) {
        if !cx.has_global::<Settings>() {
            let settings = load_settings();
            publish_connection_defaults(&settings.connection_defaults);
            cx.set_global(settings);
        }
    }

    /// 读取当前设置。
    pub fn get(cx: &App) -> Settings {
        cx.global::<Settings>().clone()
    }

    /// 当前设置快照（与 [`Self::get`] 等价；供测试 / 异步路径取一份当前值落盘）。
    pub fn snapshot(cx: &App) -> Settings {
        cx.global::<Settings>().clone()
    }

    /// 按 key 写值（**唯一写入路径**）：改 global → 落盘 → 按生效方式通知。
    ///
    /// 分发靠 `registry::{slot_for, slot_kind}`（形态不符由契约测试提前拦住）。
    /// 各域写入逻辑在 `service/<域>.rs` 的 `apply_slot`；返回 `false` 表示拒绝
    /// （key 未登记 / 值形态不符 / 复合值 / 文本无法解析）——调用方（页面）据此提示，
    /// **不静默吞掉**。
    pub fn apply_by_key(
        key: &str,
        value: SettingValue,
        window: Option<&mut gpui_kit::Window>,
        cx: &mut App,
    ) -> bool {
        let Some(slot) = crate::registry::slot_for(key) else {
            return false;
        };
        match slot {
            Slot::AppearanceThemeMode | Slot::AppearanceFontSize | Slot::AppearanceLocale | Slot::AppearanceTabBarMaxWidth | Slot::AppearanceTabBarVariant => appearance::apply_slot(slot, value, window, cx),
            Slot::EditorShowLineNumbers | Slot::EditorShowFolds | Slot::EditorIndentFolds | Slot::EditorFoldOpenKeywords | Slot::EditorFoldCloseKeywords | Slot::EditorSoftWrap | Slot::EditorAutoClose | Slot::EditorSmartIndent | Slot::EditorInlineCompletion | Slot::EditorDefaultTabSize | Slot::EditorDefaultHardTabs | Slot::EditorDefaultLineEnding | Slot::EditorDefaultEncoding | Slot::EditorFormatKeywordCase | Slot::EditorFormatIndentWidth | Slot::EditorFormatOnSave => editor::apply_slot(slot, value, window, cx),
            Slot::NavigatorSourceShortCode | Slot::NavigatorShowTags | Slot::NavigatorShowScope | Slot::NavigatorShowNameTypeShort | Slot::NavigatorRowEstimate | Slot::NavigatorShowInlineStats | Slot::NavigatorShowInlineComment | Slot::NavigatorShowInlineSize | Slot::NavigatorShowChips | Slot::NavigatorShowSearch | Slot::NavigatorAutoConnectStartup | Slot::NavigatorFilters | Slot::NavigatorSavedViews | Slot::NavigatorVisibleDatabases | Slot::NavigatorGlobalMask => navigator::apply_slot(slot, value, window, cx),
            Slot::ResourcesCollapsedGroups | Slot::ResourcesDefaultGroup | Slot::ResourcesKeepVersions | Slot::ResourcesDefaultSort => resources::apply_slot(slot, value, window, cx),
            Slot::ConnectTimeoutMs | Slot::LanDisableTls => connection::apply_slot(slot, value, window, cx),
            Slot::ProjectSortMode => projects::apply_slot(slot, value, window, cx),
            Slot::LogMinLevel => logging::apply_slot(slot, value, window, cx),
            Slot::StartupDefaultLeftPanel => startup::apply_slot(slot, value, window, cx),
            Slot::ScratchpadGroupMaxH | Slot::ScratchpadToolbar => scratchpad::apply_slot(slot, value, window, cx),
            Slot::InsightMemoryLimitMb | Slot::InsightFullScanConfirm | Slot::InsightWorkSetLimit | Slot::InsightStatusUsage => insight::apply_slot(slot, value, window, cx),
            Slot::ResultPanelFactStorage | Slot::ResultPanelShowColumnComments | Slot::ResultPanelShowTypeBadge | Slot::ResultPanelLoopSelection | Slot::ResultPanelSelectOnModifierClick | Slot::ResultPanelPersistColumnWidths | Slot::ResultPanelNumberFormat | Slot::ResultPanelNullPlaceholder | Slot::ResultPanelBoolDisplay | Slot::ResultPanelAlignNumeric | Slot::ResultPanelAlignText | Slot::ResultPanelAlignTemporal | Slot::ResultPanelAlignBool | Slot::ResultPanelAlignBlob | Slot::ResultPanelColorNumeric | Slot::ResultPanelColorText | Slot::ResultPanelColorTemporal | Slot::ResultPanelColorBool | Slot::ResultPanelColorBlob | Slot::ResultPanelBgNumeric | Slot::ResultPanelBgText | Slot::ResultPanelBgTemporal | Slot::ResultPanelBgBool | Slot::ResultPanelBgBlob | Slot::ResultPanelFontNumeric | Slot::ResultPanelFontText | Slot::ResultPanelFontTemporal | Slot::ResultPanelFontBool | Slot::ResultPanelFontBlob => result_panel::apply_slot(slot, value, window, cx),
            Slot::LayoutLocked | Slot::LayoutLeftSidebarWidth | Slot::LayoutRightSidebarWidth => layout::apply_slot(slot, value, window, cx),
            Slot::NotificationSuccessAutohideSecs | Slot::NotificationWarningAutohideSecs | Slot::NotificationErrorAutohide | Slot::NotificationMergeWindowSecs => notifications::apply_slot(slot, value, window, cx),
        }
    }
}

//! rds-settings — 设置项登记表（**准入的代码侧权威**）。
//!
//! 为什么要有这张表：设置项最容易长成"有字段、有界面行，但没有消费方"的装饰
//! （2026-09-16 审计发现 7 个这样的字段，见架构文档 §7.2）。把登记信息放进代码、
//! 用契约测试锁住三条不变式，装饰项就进不来：
//!
//! 1. **登记项必须在模型里**（每个 key 都能在 `Settings::default()` 序列化结果里找到）；
//! 2. **模型里的叶子必须登记**（新增字段不登记 → 测试失败，这就是"准入"）；
//! 3. **默认值一致**（登记表写的默认值 == 模型默认值，保证"恢复默认"有唯一目标）。
//!
//! 文档侧：`docs/architecture/settings/settings-architecture.md` §6 是同一张表的可读版，
//! §7.1 的准入五条解释每条字段为什么必须存在。**改动顺序**：先改本表，再改 `model.rs`
//! 与消费方，最后同步文档表。
//!
//! 范围：本表只收录**已落地**的项；待接线项（如 M6 的 `resources.keep_versions`）在
//! 落地前不进本表——否则第 1 条不变式不成立。

// 仅契约测试需要（生产代码不按路径取值，直接读登记信息与强类型访问器）。
#[cfg(test)]
use serde_json::Value;
mod entries;
use std::sync::OnceLock;

/// 设置项的取值形状（决定页面用什么控件，也是"值语义"的唯一声明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// 布尔开关（页面用 `Switch`）。
    Bool,
    /// 布尔但两端有明确名字（页面用分段控件；`on` 对应 `true`）。
    BoolPair { on: &'static str, off: &'static str },
    /// 枚举（页面用分段控件；元素为 `(落盘值, 显示名)`）。
    ///
    /// 当前页面**一律用分段控件**（≤ 4 项最舒展，5 项也能放下）；条目更多时
    /// 再引入下拉，别在这里写"超过 N 项换控件"的愿望——页面不会读这句话。
    Enum(&'static [(&'static str, &'static str)]),
    /// 自由文本（页面用输入框；落盘为字符串）。如逗号分隔的关键字列表。
    Text,
    /// 数值（页面用输入框 + 校验）。
    Number,
    /// 复合值：多个子键打成一个登记项（子键不单独登记）。
    Composite,
}

/// 形态标签：`SettingKind` 带数据（枚举选项、两态文案），**跨表比较只用标签**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindTag {
    Bool,
    BoolPair,
    Enum,
    Text,
    Number,
    Composite,
}

impl SettingKind {
    /// 取形态标签。
    pub fn tag(self) -> KindTag {
        match self {
            Self::Bool => KindTag::Bool,
            Self::BoolPair { .. } => KindTag::BoolPair,
            Self::Enum(_) => KindTag::Enum,
            Self::Text => KindTag::Text,
            Self::Number => KindTag::Number,
            Self::Composite => KindTag::Composite,
        }
    }
}

/// 设置项的取值（读、写共用的最小标量类型）。
///
/// 枚举以**落盘值**的文本表达（显示名在 `SettingKind::Enum` / `BoolPair` 里）；
/// 复合值（`SettingKind::Composite`）无法用本类型表达——它也没有页面写入路径。
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl SettingValue {
    /// 取布尔（形态不符返回 `None`）。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// 取数值。
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(v) => Some(*v),
            _ => None,
        }
    }

    /// 取文本（枚举的落盘值）。
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(v) => Some(v.as_str()),
            _ => None,
        }
    }
}

/// 写入槽位：`slot_for` / `slot_kind` / `slot_is_scalar` 三张表一起构成
/// "键 → 类型化读写"的**唯一分发表**（`SettingsService::apply_by_key` / `value_by_key` 用它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    AppearanceThemeMode,
    EditorShowLineNumbers,
    /// 【2026-10-27】代码折叠开关（标量布尔，默认开；设在设置页，编辑器 render 同步）。
    EditorShowFolds,
    /// 【2026-10-28】基于缩进的折叠开关（标量布尔，默认开；设在设置页，编辑器 render 同步）。
    EditorIndentFolds,
    /// 【2026-10-28】自定义开块关键字列表（折叠用；逗号分隔）。
    EditorFoldOpenKeywords,
    /// 【2026-10-28】自定义闭块关键字列表（折叠用；逗号分隔）。
    EditorFoldCloseKeywords,
    /// 【2026-10-27】软换行开关（标量布尔，默认开，与内核一致；设在设置页，编辑器 render 同步）。
    EditorSoftWrap,
    /// 【2026-10-27】自动闭合括号 / 引号（标量布尔，默认开；设在设置页，编辑器 render 同步）。
    EditorAutoClose,
    /// 【2026-10-27】智能缩进（标量布尔，默认开；设在设置页，编辑器 render 同步）。
    EditorSmartIndent,
    /// 【2026-10-27】AI 补全开关（标量布尔，默认关；**当前置灰**——能力预留，放开前恒不产出）。
    EditorInlineCompletion,
    EditorDefaultTabSize,
    EditorDefaultHardTabs,
    EditorDefaultLineEnding,
    EditorDefaultEncoding,
    /// 【2026-10-28】SQL 格式化关键字大小写（`"preserve"` / `"upper"` / `"lower"`）。
    EditorFormatKeywordCase,
    /// 【2026-10-28】SQL 格式化缩进宽度（空格数；2 = sqlglot 原生）。
    EditorFormatIndentWidth,
    /// 【2026-10-28】保存时自动格式化 SQL（默认关）。
    EditorFormatOnSave,
    AppearanceFontSize,
    AppearanceLocale,
    AppearanceTabBarMaxWidth,
    AppearanceTabBarVariant,
    NavigatorSourceShortCode,
    NavigatorShowTags,
    NavigatorShowScope,
    NavigatorShowNameTypeShort,
    NavigatorFilters,
    /// 已保存视图（复合值：命名 facet 快照列表；读写走导航面板自己的入口）。
    NavigatorSavedViews,
    /// 每连接可见库白名单（S3-① 复合值：`conn_id → 名字集合`；读写走 SettingsService）。
    NavigatorVisibleDatabases,
    /// 表/视图行估计徽标开关（S3-④ 标量布尔，默认开；只上设置页）。
    NavigatorRowEstimate,
    /// 表/视图行右侧统计徽标整体开关（需求④，默认开；设置页「表行统计」）。
    NavigatorShowInlineStats,
    /// 表行「注释副标」子开关（需求②，默认开；总开关开时仍受此开关）。
    NavigatorShowInlineComment,
    /// 表行「≈ KB 大小」徽标开关（需求②，默认开；总开关开时仍受此开关）。
    NavigatorShowInlineSize,
    /// 面板全局对象名 mask（B-1 复合值；读写走对象名过滤器对话框，不上设置页）。
    NavigatorGlobalMask,
    /// 连接行标签 chip 常显开关（合并分支新增字段，2026-10-28 补登记）。
    NavigatorShowChips,
    /// 导航树顶部文件搜索框显隐开关（合并分支新增字段，2026-10-28 补登记）。
    NavigatorShowSearch,
    /// 启动时自动连接上次打开的连接（默认关，2026-10-28 补登记）。
    NavigatorAutoConnectStartup,
    ResourcesKeepVersions,
    ResourcesDefaultSort,
    ResourcesCollapsedGroups,
    ResourcesDefaultGroup,
    ConnectTimeoutMs,
    LanDisableTls,
    ProjectSortMode,
    LogMinLevel,
    StartupDefaultLeftPanel,
    /// 草稿箱工具栏布局（复合值：按钮稳定标识 → { visible, side }；读写走 ⋯ 菜单）。
    ScratchpadToolbar,
    /// 草稿箱拖拽分区高度上限（复合值：None = 默认；拖拽 handle 写回，2026-10-28 补登记）。
    ScratchpadGroupMaxH,
    InsightMemoryLimitMb,
    InsightFullScanConfirm,
    InsightWorkSetLimit,
    InsightStatusUsage,
    /// 结果集事实档载体偏好（auto / memory / table；分流纯函数在 `shared::fact`，
    /// 触发物化路径 B 批接通消费）。
    ResultPanelFactStorage,
    /// 列头是否直接显示字段注释（默认 false；面板工具栏另有 B1 临时开关）。
    ResultPanelShowColumnComments,
    /// 列头是否显示类型徽标（123/Aa/◷/✓/01 浅底符号）。
    ResultPanelShowTypeBadge,
    /// 键盘导航是否循环选择（到末尾后循环回开头）。
    ResultPanelLoopSelection,
    /// 是否启用 Ctrl+点击多选（修饰键多选）。
    ResultPanelSelectOnModifierClick,
    /// 列宽是否跨会话持久化（关闭则只在当前会话内记忆）。
    ResultPanelPersistColumnWidths,
    /// 【类型样式】数字显示格式（raw / grouped / fixed2 / scientific）。
    ResultPanelNumberFormat,
    /// 【类型样式】NULL 显示占位（null / emptyset / dash）。
    ResultPanelNullPlaceholder,
    /// 【类型样式】布尔显示映射（true_false / cn / digits）。
    ResultPanelBoolDisplay,
    /// 【类型样式】按族对齐覆盖（inherit / left / center / right）。
    ResultPanelAlignNumeric,
    ResultPanelAlignText,
    ResultPanelAlignTemporal,
    ResultPanelAlignBool,
    ResultPanelAlignBlob,
    /// 【类型样式】按族前景色角色（inherit / info / success / warning / danger / primary / muted）。
    ResultPanelColorNumeric,
    ResultPanelColorText,
    ResultPanelColorTemporal,
    ResultPanelColorBool,
    ResultPanelColorBlob,
    ResultPanelBgNumeric,
    ResultPanelBgText,
    ResultPanelBgTemporal,
    ResultPanelBgBool,
    ResultPanelBgBlob,
    ResultPanelFontNumeric,
    ResultPanelFontText,
    ResultPanelFontTemporal,
    ResultPanelFontBool,
    ResultPanelFontBlob,
    /// 布局锁定（默认 false；render 时同步到 DockArea::set_locked）。
    LayoutLocked,
    /// 左侧侧边栏宽度（默认 250px；render 时同步到 DockArea::set_dock_size）。
    LayoutLeftSidebarWidth,
    /// 右侧侧边栏宽度（默认 300px；render 时同步到 DockArea::set_dock_size）。
    LayoutRightSidebarWidth,
    NotificationSuccessAutohideSecs,
    NotificationWarningAutohideSecs,
    NotificationErrorAutohide,
    NotificationMergeWindowSecs,
    NotificationErrorToSystem,
    NotificationWarningToSystem,
    NotificationSuccessToSystem,
    NotificationInfoToSystem,
    NotificationDoNotDisturb,
}

/// key → 槽位（登记项都必须有槽位；由契约测试保证）。
pub fn slot_for(key: &str) -> Option<Slot> {
    Some(match key {
        "appearance.theme_mode" => Slot::AppearanceThemeMode,
        "editor.show_line_numbers" => Slot::EditorShowLineNumbers,
        "editor.show_folds" => Slot::EditorShowFolds,
        "editor.indent_folding" => Slot::EditorIndentFolds,
        "editor.fold_open_keywords" => Slot::EditorFoldOpenKeywords,
        "editor.fold_close_keywords" => Slot::EditorFoldCloseKeywords,
        "editor.soft_wrap" => Slot::EditorSoftWrap,
        "editor.auto_close" => Slot::EditorAutoClose,
        "editor.smart_indent" => Slot::EditorSmartIndent,
        "editor.inline_completion" => Slot::EditorInlineCompletion,
        "editor.default_tab_size" => Slot::EditorDefaultTabSize,
        "editor.default_hard_tabs" => Slot::EditorDefaultHardTabs,
        "editor.default_line_ending" => Slot::EditorDefaultLineEnding,
        "editor.default_encoding" => Slot::EditorDefaultEncoding,
        "editor.format_keyword_case" => Slot::EditorFormatKeywordCase,
        "editor.format_indent_width" => Slot::EditorFormatIndentWidth,
        "editor.format_on_save" => Slot::EditorFormatOnSave,
        "appearance.font_size" => Slot::AppearanceFontSize,
        "appearance.locale" => Slot::AppearanceLocale,
        "appearance.tab_bar_max_width" => Slot::AppearanceTabBarMaxWidth,
        "appearance.tab_bar_variant" => Slot::AppearanceTabBarVariant,
        "navigator.source_short_code" => Slot::NavigatorSourceShortCode,
        "navigator.show_tags" => Slot::NavigatorShowTags,
        "navigator.show_scope" => Slot::NavigatorShowScope,
        "navigator.show_name_type_short" => Slot::NavigatorShowNameTypeShort,
        "navigator.filters" => Slot::NavigatorFilters,
        "navigator.saved_views" => Slot::NavigatorSavedViews,
        "navigator.visible_databases" => Slot::NavigatorVisibleDatabases,
        "navigator.row_estimate" => Slot::NavigatorRowEstimate,
        "navigator.show_inline_stats" => Slot::NavigatorShowInlineStats,
        "navigator.show_inline_comment" => Slot::NavigatorShowInlineComment,
        "navigator.show_inline_size" => Slot::NavigatorShowInlineSize,
        "navigator.global_mask" => Slot::NavigatorGlobalMask,
        "navigator.show_chips" => Slot::NavigatorShowChips,
        "navigator.show_search" => Slot::NavigatorShowSearch,
        "navigator.auto_connect_on_startup" => Slot::NavigatorAutoConnectStartup,
        "resources.keep_versions" => Slot::ResourcesKeepVersions,
        "resources.default_sort" => Slot::ResourcesDefaultSort,
        "resources.collapsed_groups" => Slot::ResourcesCollapsedGroups,
        "resources.default_group" => Slot::ResourcesDefaultGroup,
        "connection_defaults.connect_timeout_ms" => Slot::ConnectTimeoutMs,
        "connection_defaults.lan_disable_tls" => Slot::LanDisableTls,
        "projects.sort_mode" => Slot::ProjectSortMode,
        "logging.min_level" => Slot::LogMinLevel,
        "startup.default_left_panel" => Slot::StartupDefaultLeftPanel,
        "scratchpad.toolbar" => Slot::ScratchpadToolbar,
        "scratchpad.group_max_h" => Slot::ScratchpadGroupMaxH,
        "insight.memory_limit_mb" => Slot::InsightMemoryLimitMb,
        "insight.full_scan_confirm" => Slot::InsightFullScanConfirm,
        "insight.work_set_limit" => Slot::InsightWorkSetLimit,
        "insight.status_usage" => Slot::InsightStatusUsage,
        "result_panel.fact_storage" => Slot::ResultPanelFactStorage,
        "result_panel.show_column_comments" => Slot::ResultPanelShowColumnComments,
        "result_panel.show_type_badge" => Slot::ResultPanelShowTypeBadge,
        "result_panel.loop_selection" => Slot::ResultPanelLoopSelection,
        "result_panel.select_on_modifier_click" => Slot::ResultPanelSelectOnModifierClick,
        "result_panel.persist_column_widths" => Slot::ResultPanelPersistColumnWidths,
        "result_panel.number_format" => Slot::ResultPanelNumberFormat,
        "result_panel.null_placeholder" => Slot::ResultPanelNullPlaceholder,
        "result_panel.bool_display" => Slot::ResultPanelBoolDisplay,
        "result_panel.align_numeric" => Slot::ResultPanelAlignNumeric,
        "result_panel.align_text" => Slot::ResultPanelAlignText,
        "result_panel.align_temporal" => Slot::ResultPanelAlignTemporal,
        "result_panel.align_bool" => Slot::ResultPanelAlignBool,
        "result_panel.align_blob" => Slot::ResultPanelAlignBlob,
        "result_panel.color_numeric" => Slot::ResultPanelColorNumeric,
        "result_panel.color_text" => Slot::ResultPanelColorText,
        "result_panel.color_temporal" => Slot::ResultPanelColorTemporal,
        "result_panel.color_bool" => Slot::ResultPanelColorBool,
        "result_panel.color_blob" => Slot::ResultPanelColorBlob,
        "result_panel.bg_numeric" => Slot::ResultPanelBgNumeric,
        "result_panel.bg_text" => Slot::ResultPanelBgText,
        "result_panel.bg_temporal" => Slot::ResultPanelBgTemporal,
        "result_panel.bg_bool" => Slot::ResultPanelBgBool,
        "result_panel.bg_blob" => Slot::ResultPanelBgBlob,
        "result_panel.font_numeric" => Slot::ResultPanelFontNumeric,
        "result_panel.font_text" => Slot::ResultPanelFontText,
        "result_panel.font_temporal" => Slot::ResultPanelFontTemporal,
        "result_panel.font_bool" => Slot::ResultPanelFontBool,
        "result_panel.font_blob" => Slot::ResultPanelFontBlob,
        "layout.locked" => Slot::LayoutLocked,
        "layout.left_sidebar_width" => Slot::LayoutLeftSidebarWidth,
        "layout.right_sidebar_width" => Slot::LayoutRightSidebarWidth,
        "notifications.success_autohide_secs" => Slot::NotificationSuccessAutohideSecs,
        "notifications.warning_autohide_secs" => Slot::NotificationWarningAutohideSecs,
        "notifications.error_autohide" => Slot::NotificationErrorAutohide,
        "notifications.merge_window_secs" => Slot::NotificationMergeWindowSecs,
        "notifications.error_to_system" => Slot::NotificationErrorToSystem,
        "notifications.warning_to_system" => Slot::NotificationWarningToSystem,
        "notifications.success_to_system" => Slot::NotificationSuccessToSystem,
        "notifications.info_to_system" => Slot::NotificationInfoToSystem,
        "notifications.do_not_disturb" => Slot::NotificationDoNotDisturb,
        _ => return None,
    })
}

/// 槽位的值形态（必须与登记表的 `kind.tag()` 一致；由契约测试保证）。
pub fn slot_kind(slot: Slot) -> KindTag {
    match slot {
        Slot::AppearanceThemeMode => KindTag::Enum,
        Slot::EditorShowLineNumbers
        | Slot::EditorDefaultHardTabs
        | Slot::EditorShowFolds
        | Slot::EditorIndentFolds
        | Slot::EditorSoftWrap
        | Slot::EditorAutoClose
        | Slot::EditorSmartIndent
        | Slot::EditorInlineCompletion => KindTag::Bool,
        Slot::EditorDefaultTabSize => KindTag::Number,
        Slot::EditorDefaultLineEnding | Slot::EditorDefaultEncoding => KindTag::Enum,
        Slot::EditorFoldOpenKeywords | Slot::EditorFoldCloseKeywords => KindTag::Text,
        Slot::AppearanceFontSize => KindTag::Number,
        Slot::AppearanceLocale => KindTag::Enum,
        Slot::AppearanceTabBarMaxWidth => KindTag::Number,
        Slot::AppearanceTabBarVariant => KindTag::Enum,
        Slot::NavigatorSourceShortCode => KindTag::BoolPair,
        Slot::NavigatorShowTags
        | Slot::NavigatorShowScope
        | Slot::NavigatorShowNameTypeShort
        | Slot::NavigatorRowEstimate
        | Slot::NavigatorShowInlineStats
        | Slot::NavigatorShowInlineComment
        | Slot::NavigatorShowInlineSize => KindTag::Bool,
        Slot::ConnectTimeoutMs => KindTag::Number,
        // 保留份数是有符号数：`-1` = 全留（哨兵值，不是"少一份"）。
        Slot::ResourcesKeepVersions => KindTag::Number,
        Slot::ResourcesDefaultSort => KindTag::Enum,
        // 复合值（项目根 → 折叠的分组 key）：没有页面写入路径，与 `navigator.filters` 同类。
        Slot::ResourcesCollapsedGroups => KindTag::Composite,
        // 复合值（项目根 → 默认分组 id）：同上（分组 id 是每项目自己生成的，不能平铺一份）。
        Slot::ResourcesDefaultGroup => KindTag::Composite,
        Slot::NavigatorFilters => KindTag::Composite,
        Slot::NavigatorSavedViews => KindTag::Composite,
        Slot::NavigatorVisibleDatabases => KindTag::Composite,
        Slot::NavigatorGlobalMask => KindTag::Composite,
        Slot::NavigatorShowChips | Slot::NavigatorShowSearch | Slot::NavigatorAutoConnectStartup => KindTag::Bool,
        Slot::LanDisableTls => KindTag::BoolPair,
        Slot::ProjectSortMode => KindTag::Enum,
        Slot::LogMinLevel => KindTag::Enum,
        Slot::StartupDefaultLeftPanel => KindTag::Enum,
        // 复合值（工具栏按钮布局）：没有页面写入路径，读写走草稿箱 ⋯ 菜单。
        Slot::ScratchpadToolbar => KindTag::Composite,
        // 复合值（拖拽分区高度上限）：读写走草稿箱分区拖拽 handle，不上设置页。
        Slot::ScratchpadGroupMaxH => KindTag::Number,
        Slot::InsightMemoryLimitMb | Slot::InsightWorkSetLimit => KindTag::Number,
        Slot::InsightFullScanConfirm | Slot::InsightStatusUsage => KindTag::Bool,
        Slot::ResultPanelFactStorage => KindTag::Enum,
        Slot::ResultPanelShowColumnComments => KindTag::Bool,
        Slot::ResultPanelShowTypeBadge | Slot::ResultPanelLoopSelection
        | Slot::ResultPanelSelectOnModifierClick | Slot::ResultPanelPersistColumnWidths => KindTag::Bool,
        // 【类型样式】23 项全是枚举（设置页下拉）。
        Slot::ResultPanelNumberFormat
        | Slot::ResultPanelNullPlaceholder
        | Slot::ResultPanelBoolDisplay
        | Slot::ResultPanelAlignNumeric
        | Slot::ResultPanelAlignText
        | Slot::ResultPanelAlignTemporal
        | Slot::ResultPanelAlignBool
        | Slot::ResultPanelAlignBlob
        | Slot::ResultPanelColorNumeric
        | Slot::ResultPanelColorText
        | Slot::ResultPanelColorTemporal
        | Slot::ResultPanelColorBool
        | Slot::ResultPanelColorBlob
        | Slot::ResultPanelBgNumeric
        | Slot::ResultPanelBgText
        | Slot::ResultPanelBgTemporal
        | Slot::ResultPanelBgBool
        | Slot::ResultPanelBgBlob
        | Slot::ResultPanelFontNumeric
        | Slot::ResultPanelFontText
        | Slot::ResultPanelFontTemporal
        | Slot::ResultPanelFontBool
        | Slot::ResultPanelFontBlob => KindTag::Enum,
        Slot::LayoutLocked => KindTag::Bool,
        Slot::LayoutLeftSidebarWidth | Slot::LayoutRightSidebarWidth => KindTag::Number,
        // 【通知】三个秒数（0 = 不自动消失 / 不合并）是数值，其余（自动消失、
        // 推送到系统、勿扰）都是布尔。
        Slot::NotificationSuccessAutohideSecs
        | Slot::NotificationWarningAutohideSecs
        | Slot::NotificationMergeWindowSecs => KindTag::Number,
        Slot::NotificationErrorAutohide
        | Slot::NotificationErrorToSystem
        | Slot::NotificationWarningToSystem
        | Slot::NotificationSuccessToSystem
        | Slot::NotificationInfoToSystem
        | Slot::NotificationDoNotDisturb => KindTag::Bool,
        // 【2026-10-28】SQL 格式化：关键字大小写是枚举，缩进宽度是数值，保存时格式化是布尔。
        Slot::EditorFormatKeywordCase => KindTag::Enum,
        Slot::EditorFormatIndentWidth => KindTag::Number,
        Slot::EditorFormatOnSave => KindTag::Bool,
    }
}

/// 槽位是否可表达为标量值（复合值不可：它没有页面写入路径）。
pub fn slot_is_scalar(slot: Slot) -> bool {
    !matches!(
        slot,
        Slot::NavigatorFilters
            | Slot::NavigatorSavedViews
            | Slot::NavigatorVisibleDatabases
            | Slot::NavigatorGlobalMask
            | Slot::ResourcesCollapsedGroups
            | Slot::ScratchpadToolbar
            | Slot::ScratchpadGroupMaxH
    )
}

/// 生效方式（页面的说明行必须讲清楚，禁止"改了不知道生效没"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingEffect {
    /// 改完立刻反映到界面（需要主动通知重绘）。
    Immediate,
    /// 下一次操作才用到（建连 / 排序 / 归档），无需通知。
    NextUse,
    /// 需要重载进程（页面必须标注 + 提供重启入口）。
    Restart,
}

/// 入口位置：登记是义务，**上不上设置页是判断**（登记表 ≠ 页面清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingEntry {
    /// 只上设置页。
    Page,
    /// 只在操作现场（拖拽、局部菜单），上页反而添乱。
    Module,
    /// 两处都有：必须是同一 `SettingsService` 写入路径，禁止各存一份。
    Both,
}

/// 一条设置项的登记信息。
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    /// 稳定点分命名，**必须与 `Settings` 的 JSON 字段路径一致**。
    pub key: &'static str,
    /// 所属节（页面左侧分节导航的 id）。
    pub section: &'static str,
    /// 节的显示名。
    pub section_label: &'static str,
    /// 节内子分组 id（页面把同节行按分组归入带标题的卡片；`None` = 单分组节）。
    pub group: &'static str,
    /// 子分组显示名（侧栏两级导航与分组卡片标题；`None` 表示该节无子分组，
    /// 页面渲染为无名分组，对齐 gpui-kit demo 的 General 页）。
    pub group_label: Option<&'static str>,
    /// 行标签（页面上那一行叫什么）。
    pub label: &'static str,
    /// 行说明（弱化小字；也承载"生效方式"这类边界说明）。
    pub hint: &'static str,
    /// 说明的呈现方式：`true` = 行内常显小字；`false` = 不占行内空间，
    /// 改为**悬停控件时以 HoverCard 提示**（适合 label 本身自明的好理解参数，
    /// 让设置页更清爽）。
    pub hint_inline: bool,
    pub kind: SettingKind,
    /// 数值项的预设档（`(落盘值, 显示名)`；空 = 无预设档）。
    ///
    /// 页面上的数值行**必须**有预设档：自由输入要成套的校验与单位处理，
    /// 在真需要之前不做（契约测试强制该约定）；预设档必须含默认值。
    pub presets: &'static [(i64, &'static str)],
    /// 默认值（JSON 字面量）；与 `model.rs` 的 `Default` 逐项比对。
    pub default_json: &'static str,
    pub effect: SettingEffect,
    pub entry: SettingEntry,
    /// 消费方（`文件::符号`）——准入第 1 条要求能点到具体符号。
    pub consumer: &'static str,
    /// 复合值：契约测试不展开它的子键。
    pub composite: bool,
    /// 置灰（能力存在但暂不可用）：设置页渲染为禁用态（开关不可操作），
    /// 默认值保留在 model（未来放开即用）。用于「已预留但未实现」的能力位。
    pub disabled: bool,
}

impl SettingSpec {
    /// 默认值（页面「恢复默认」写入的目标）。
    ///
    /// 从 `default_json` 解析——单一来源，不另存一份常量（另存一份就会漂）。
    /// 复合值返回 `None`（没有标量默认值）。
    pub fn default_value(&self) -> Option<SettingValue> {
        match self.kind.tag() {
            KindTag::Bool | KindTag::BoolPair => {
                Some(SettingValue::Bool(self.default_json == "true"))
            }
            KindTag::Number => serde_json::from_str::<f64>(self.default_json)
                .ok()
                .map(SettingValue::Number),
            KindTag::Enum | KindTag::Text => serde_json::from_str::<String>(self.default_json)
                .ok()
                .map(SettingValue::Text),
            KindTag::Composite => None,
        }
    }
}

/// 设置项登记表（已落地项；顺序 = 页面上的顺序）。
///
/// 节顺序：外观 → 数据源导航 → 资产库 → 连接默认值 → 项目 → 日志。
/// 全部登记项：惰性合并各域（`entries::all()`）并缓存为 `&'static` 切片。
///
/// 拆域后无法用 `const` 字面量拼数组，故用 `OnceLock` 惰性构建并 `Box::leak`；
/// `spec()` / `page_rows()` 仍返回 `&'static`，对外契约不变。
static REGISTRY: OnceLock<&'static [SettingSpec]> = OnceLock::new();

/// 全部登记项（固定域序 = 登记顺序；`page_groups` / `sections` 契约依赖）。
pub fn registry() -> &'static [SettingSpec] {
    REGISTRY.get_or_init(|| Box::leak(entries::all().into_boxed_slice()))
}

/// 按 key 查登记项。
pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    registry().iter().find(|s| s.key == key)
}

/// 节清单（按登记顺序去重）——页面的分节导航直接用它，顺序不由视图另定。
pub fn sections() -> Vec<(&'static str, &'static str)> {
    // 只返回**上页**的节：导航只列出有实际内容的分节页面。
    // 纯 Module 节（如草稿箱，其设置全在面板本地菜单/拖拽 handle 上）不进导航，
    // 否则会渲染一个空页面。顺序按登记顺序去重。
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    for s in page_rows() {
        if !out.iter().any(|(id, _)| *id == s.section) {
            out.push((s.section, s.section_label));
        }
    }
    out
}

/// 节的显示名（页面渲染节标题与搜索结果面包屑用）。
pub fn section_label(id: &str) -> Option<&'static str> {
    registry()
        .iter()
        .find(|s| s.section == id)
        .map(|s| s.section_label)
}

/// 一个节内的子分组（按登记顺序去重）：页面按它生成分组卡片，侧栏据它生成两级导航。
///
/// `label` 为 `None` 的单分组节（如布局 / 资产库）渲染为**无名**分组卡片，
/// 对齐 gpui-kit demo 的 General 页；多分组节（外观 / 编辑器 / 数据源导航 / 结果集面板）
/// 每个分组带标题。
pub fn page_groups(section: &str) -> Vec<(&'static str, Option<&'static str>)> {
    let mut out: Vec<(&'static str, Option<&'static str>)> = Vec::new();
    for s in page_rows() {
        if s.section == section && !out.iter().any(|(id, _)| *id == s.group) {
            out.push((s.group, s.group_label));
        }
    }
    out
}

/// 上设置页的行（`entry` 非 `Module`）；页面只渲染这些。
pub fn page_rows() -> impl Iterator<Item = &'static SettingSpec> {
    registry().iter().filter(|s| s.entry != SettingEntry::Module)
}

// ===== 契约测试：登记表 ↔ 模型 的三条不变式 =====
//
// 下面三个辅助函数只服务于契约测试（生产代码不需要按路径取值 ——
// 页面将来直接读 `registry` 的登记信息 + `SettingsService` 的强类型访问器）。

/// 取默认值的 JSON 表示（测试与将来文档生成的共同基准）。
#[cfg(test)]
fn default_value() -> Value {
    serde_json::to_value(crate::model::Settings::default()).expect("Settings 可序列化")
}

/// 按点分路径取值（复合键取到对象本身）。
#[cfg(test)]
fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in key.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// 收集默认值的全部叶子路径；遇到登记的复合键就整体收起，不展开子键。
#[cfg(test)]
fn leaf_paths(root: &Value) -> Vec<String> {
    fn walk(prefix: &str, value: &Value, out: &mut Vec<String>) {
        // 复合键（如 `navigator.filters`）整体算一个叶子：它的子键不单独登记。
        if !prefix.is_empty() && spec(prefix).is_some_and(|s| s.composite) {
            out.push(prefix.to_string());
            return;
        }
        match value {
            Value::Object(map) => {
                // 字符串键值数据表（当前仅 `navigator.type_short_map`：类型 → 简称）由
                // 弹窗维护，不是声明式设置项：整体豁免（不逐键下沉、不要求登记）。
                if prefix == "navigator.type_short_map" {
                    return;
                }
                for (k, v) in map {
                    let child = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&child, v, out);
                }
            }
            _ => {
                if !prefix.is_empty() {
                    out.push(prefix.to_string());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk("", root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    // 安全模式：逐项列举需要的东西，不通配导入（见 gpui-kit-dev skill「窗口测试」）
    use serde_json::from_str;

    /// 不变式 1：登记项必须能落到模型里（防止"登记了不存在的字段"）。
    #[test]
    fn every_registered_key_exists_in_the_model() {
        let default = default_value();
        for s in registry() {
            assert!(
                lookup(&default, s.key).is_some(),
                "登记项 `{}` 在 Settings 里找不到对应字段",
                s.key
            );
        }
    }

    /// 不变式 2：模型里的每个叶子都必须登记（**这条是准入的护栏**：新字段不登记就红）。
    #[test]
    fn every_model_leaf_is_registered() {
        let default = default_value();
        let mut leaves = leaf_paths(&default);
        leaves.sort();
        let mut keys: Vec<String> = registry().iter().map(|s| s.key.to_string()).collect();
        keys.sort();
        assert_eq!(
            leaves,
            keys,
            "模型叶子与登记表不一致。多出的叶子（未登记的新字段）= {:?}；多出的登记项 = {:?}",
            leaves
                .iter()
                .filter(|p| !keys.contains(p))
                .collect::<Vec<_>>(),
            keys.iter()
                .filter(|p| !leaves.contains(p))
                .collect::<Vec<_>>()
        );
    }

    /// 不变式 3：登记表写的默认值 == 模型默认值（"恢复默认"才有唯一目标）。
    #[test]
    fn registered_defaults_match_the_model() {
        let default = default_value();
        for s in registry() {
            let actual = lookup(&default, s.key).expect("不变式 1 已保证存在");
            let expected: Value = from_str(s.default_json).expect("默认值是合法 JSON");
            assert_eq!(
                actual, &expected,
                "`{}` 的登记默认值 {} 与模型默认值 {} 不一致",
                s.key, s.default_json, actual
            );
        }
    }

    /// 枚举项：默认值必须在候选值里（否则页面渲染不出选中的那一项）。
    #[test]
    fn enum_defaults_are_among_the_options() {
        let default = default_value();
        for s in registry() {
            if let SettingKind::Enum(options) = s.kind {
                let actual = lookup(&default, s.key).expect("存在");
                let value = actual
                    .as_str()
                    .unwrap_or_else(|| panic!("`{}` 登记为枚举，模型默认值却不是字符串", s.key));
                assert!(
                    options.iter().any(|(v, _)| *v == value),
                    "`{}` 的默认值 `{}` 不在候选 {:?} 里",
                    s.key,
                    value,
                    options.iter().map(|(v, _)| *v).collect::<Vec<_>>()
                );
                // 显示名不得为空（页面直接用这两列渲染）
                for (v, label) in options.iter() {
                    assert!(!label.is_empty(), "`{}` 的选项 `{}` 缺显示名", s.key, v);
                }
            }
        }
    }

    /// 两态项：默认值必须是布尔，且两端文案非空。
    #[test]
    fn bool_pair_specs_are_complete() {
        let default = default_value();
        for s in registry() {
            if let SettingKind::BoolPair { on, off } = s.kind {
                let actual = lookup(&default, s.key).expect("存在");
                assert!(
                    actual.is_boolean(),
                    "`{}` 登记为两态布尔，模型默认值不是布尔",
                    s.key
                );
                assert!(!on.is_empty() && !off.is_empty(), "`{}` 缺两态文案", s.key);
                assert_ne!(on, off, "`{}` 的两态文案相同，用户看不出区别", s.key);
            }
        }
    }

    /// 登记表自身的卫生：key 唯一、命名规范、必填字段非空、composite 与形态自洽。
    #[test]
    fn registry_is_wellformed() {
        let mut seen = Vec::new();
        for s in registry() {
            assert!(!seen.contains(&s.key), "key `{}` 重复登记", s.key);
            seen.push(s.key);

            for seg in s.key.split('.') {
                assert!(!seg.is_empty(), "key `{}` 有空段", s.key);
                assert!(
                    seg.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                    "key `{}` 的段 `{}` 应为小写 snake_case",
                    s.key,
                    seg
                );
            }
            assert!(!s.section.is_empty(), "`{}` 缺节 id", s.key);
            assert!(!s.section_label.is_empty(), "`{}` 缺节显示名", s.key);
            assert!(!s.label.is_empty(), "`{}` 缺行标签", s.key);
            assert!(
                !s.hint.is_empty(),
                "`{}` 缺行说明（生效方式要能读出来）",
                s.key
            );
            assert!(
                !s.consumer.is_empty(),
                "`{}` 缺消费方（准入第 1 条）",
                s.key
            );

            let expected: Value = from_str(s.default_json).expect("默认值是合法 JSON");
            assert_eq!(
                s.composite,
                expected.is_object(),
                "`{}` 的 composite 标记与默认值形态不符",
                s.key
            );
            assert_eq!(
                s.kind == SettingKind::Composite,
                s.composite,
                "`{}` 的 kind 与 composite 标记不一致",
                s.key
            );
        }
    }

    /// 同一节的行必须在登记表里相邻（去重后节序 = 页面导航顺序，避免视图另排一遍）。
    #[test]
    fn sections_are_contiguous() {
        let mut seen: Vec<&str> = Vec::new();
        let mut current = None;
        for s in registry() {
            if current != Some(s.section) {
                assert!(
                    !seen.contains(&s.section),
                    "节 `{}` 在登记表里被打断（页面导航顺序会前后跳）",
                    s.section
                );
                seen.push(s.section);
                current = Some(s.section);
            }
        }
        // 导航只列出有上页内容的节：纯 Module 节（如草稿箱）不渲染空页面。
        let page_section_count = registry()
            .iter()
            .filter(|s| s.entry != SettingEntry::Module)
            .map(|s| s.section)
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(
            sections().len(),
            page_section_count,
            "导航只应列出有上页内容的节（纯 Module 节应被排除）"
        );
        for (id, label) in sections() {
            assert_eq!(
                section_label(id),
                Some(label),
                "节 `{}` 的显示名查不回来（页面标题与面包屑会空）",
                id
            );
        }
        let module_only = registry()
            .iter()
            .filter(|s| s.entry == SettingEntry::Module)
            .count();
        assert_eq!(
            page_rows().count(),
            registry().len() - module_only,
            "page_rows 与 entry 分类不一致（页面会多渲染或少渲染行）"
        );
        assert!(page_rows().next().is_some(), "页面至少要有一行");
    }

    /// 分组（gpui-kit demo 两级导航）：每行必须归入某分组；同节分组不能被打断
    /// （A → B → A）；同一 `(节, 分组)` 的显示名必须一致；`page_groups` 与登记顺序一致；
    /// 多分组节的分组都必须带标题（单分组节为无名分组）。
    #[test]
    fn groups_are_contiguous_and_consistently_named() {
        for s in page_rows() {
            assert!(!s.group.is_empty(), "`{}` 缺分组 id", s.key);
        }
        // 同节内分组必须连续：允许同组多行连续出现（如「主题与语言」3 行），
        // 但分组不能在离开后再次出现（A → B → A 会打乱侧栏两级导航顺序）。
        let mut current_section: Option<&str> = None;
        let mut seen_groups: Vec<(&str, &str)> = Vec::new();
        for s in registry() {
            if current_section != Some(s.section) {
                current_section = Some(s.section);
                seen_groups.clear();
            }
            let key = (s.section, s.group);
            if seen_groups.last() != Some(&key) {
                if seen_groups.contains(&key) {
                    panic!(
                        "节 `{}` 的分组 `{}` 被打断（页面导航顺序会前后跳）",
                        s.section, s.group
                    );
                }
                seen_groups.push(key);
            }
        }
        // 同一 (section, group) 的显示名必须一致。
        let mut labels: std::collections::HashMap<(&str, &str), Option<&'static str>> =
            Default::default();
        for s in page_rows() {
            let k = (s.section, s.group);
            if let Some(existing) = labels.get(&k) {
                assert_eq!(*existing, s.group_label, "分组 `{}` 的显示名不一致", s.group);
            } else {
                labels.insert(k, s.group_label);
            }
        }
        // page_groups：顺序与登记一致；单分组节恰一个分组；多分组节分组都带标题。
        for (section, _) in sections() {
            let groups = page_groups(section);
            assert!(!groups.is_empty(), "节 `{}` 无分组", section);
            let mut expected: Vec<&str> = Vec::new();
            for s in page_rows() {
                if s.section == section && !expected.contains(&s.group) {
                    expected.push(s.group);
                }
            }
            let actual: Vec<&str> = groups.iter().map(|(g, _)| *g).collect();
            assert_eq!(actual, expected, "节 `{}` 的 page_groups 顺序与登记不一致", section);
            if groups.len() > 1 {
                for (_, label) in groups {
                    assert!(label.is_some(), "多分组节 `{}` 的分组必须有标题", section);
                }
            }
        }
    }

    /// 每个登记项都要有槽位，且**槽位形态与登记形态一致**——不一致就是
    /// "页面按一种控件渲染、写入路径按另一种解释"，要到运行期才炸。
    #[test]
    fn every_registered_key_has_a_matching_slot() {
        for s in registry() {
            let slot = slot_for(s.key).unwrap_or_else(|| panic!("登记项 `{}` 没有写入槽位", s.key));
            assert_eq!(
                slot_kind(slot),
                s.kind.tag(),
                "`{}` 的槽位形态与登记形态不一致",
                s.key
            );
        }
    }

    /// 上页的行必须是标量 + 可写；数值行必须有预设档。
    #[test]
    fn page_rows_are_scalar_and_writable() {
        for s in registry().iter().filter(|s| s.entry != SettingEntry::Module) {
            let slot = slot_for(s.key).expect("上页的项必须有槽位");
            assert!(slot_is_scalar(slot), "`{}` 是复合值，不能上页", s.key);
            assert_ne!(
                s.kind.tag(),
                KindTag::Composite,
                "`{}` 是复合值，不能上页",
                s.key
            );
            if s.kind.tag() == KindTag::Number {
                assert!(
                    !s.presets.is_empty(),
                    "数值行 `{}` 必须有预设档（自由输入要成套校验，真需要再做）",
                    s.key
                );
            }
        }
    }

    /// 预设档只能给数值行、显示名非空、**必须包含默认值**
    /// （否则「恢复默认」会落到档位之外）。
    #[test]
    fn number_presets_are_consistent() {
        let default = default_value();
        for s in registry() {
            if s.presets.is_empty() {
                continue;
            }
            assert_eq!(
                s.kind.tag(),
                KindTag::Number,
                "`{}` 不是数值行却给了预设档",
                s.key
            );
            for (_, label) in s.presets {
                assert!(!label.is_empty(), "`{}` 的预设档缺显示名", s.key);
            }
            let actual = lookup(&default, s.key)
                .and_then(|v| v.as_f64())
                .expect("数值默认值");
            assert!(
                s.presets.iter().any(|(v, _)| *v as f64 == actual),
                "`{}` 的默认值 {} 不在预设档 {:?} 里",
                s.key,
                actual,
                s.presets.iter().map(|(v, _)| *v).collect::<Vec<_>>()
            );
        }
    }

    /// `default_value()` 与 `default_json` 同源：三种标量形状各自可解析，复合值给 `None`。
    #[test]
    fn default_value_follows_the_declared_kind() {
        for s in registry() {
            let ok = match s.kind.tag() {
                KindTag::Bool | KindTag::BoolPair => {
                    matches!(s.default_value(), Some(SettingValue::Bool(_)))
                }
                KindTag::Number => matches!(s.default_value(), Some(SettingValue::Number(_))),
                KindTag::Enum | KindTag::Text => matches!(s.default_value(), Some(SettingValue::Text(_))),
                KindTag::Composite => s.default_value().is_none(),
            };
            assert!(ok, "`{}` 的默认值形态与登记形态不符", s.key);
        }
    }
}

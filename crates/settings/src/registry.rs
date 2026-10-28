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
        "layout.locked" => Slot::LayoutLocked,
        "layout.left_sidebar_width" => Slot::LayoutLeftSidebarWidth,
        "layout.right_sidebar_width" => Slot::LayoutRightSidebarWidth,
        "notifications.success_autohide_secs" => Slot::NotificationSuccessAutohideSecs,
        "notifications.warning_autohide_secs" => Slot::NotificationWarningAutohideSecs,
        "notifications.error_autohide" => Slot::NotificationErrorAutohide,
        "notifications.merge_window_secs" => Slot::NotificationMergeWindowSecs,
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
        // 【类型样式】13 项全是枚举（设置页下拉）。
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
        | Slot::ResultPanelColorBlob => KindTag::Enum,
        Slot::LayoutLocked => KindTag::Bool,
        Slot::LayoutLeftSidebarWidth | Slot::LayoutRightSidebarWidth => KindTag::Number,
        // 【通知】三个秒数（0 = 不自动消失 / 不合并）是数值，错误自动消失是布尔。
        Slot::NotificationSuccessAutohideSecs
        | Slot::NotificationWarningAutohideSecs
        | Slot::NotificationMergeWindowSecs => KindTag::Number,
        Slot::NotificationErrorAutohide => KindTag::Bool,
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
pub const REGISTRY: &[SettingSpec] = &[
    SettingSpec {
        key: "appearance.theme_mode",
        section: "appearance",
        section_label: "外观",
        group: "theme",
        group_label: Some("主题与语言"),
        label: "主题模式",
        hint: "立即切换明暗配色并持久化；「跟随系统」随系统外观实时变化",
        hint_inline: true,
        kind: SettingKind::Enum(&[("light", "浅色"), ("dark", "深色"), ("system", "跟随系统")]),
        presets: &[],
        default_json: "\"light\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "app/src/main.rs（启动应用 + 系统外观监听）+ SettingsService::set_theme_mode",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "appearance.font_size",
        section: "appearance",
        section_label: "外观",
        group: "theme",
        group_label: Some("主题与语言"),
        label: "界面字号",
        hint: "整界面缩放（主题字号与窗口 rem 基准同步）；档位越小控件越紧凑，改完立即生效",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (13, "13px"),
            (14, "14px"),
            (15, "15px"),
            (16, "16px（默认）"),
            (17, "17px"),
        ],
        default_json: "16.0",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/app/src/main.rs（启动应用 + open_window 同步 rem）+ SettingsService::set_font_size（运行时：Theme::update + Window::set_rem_size）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "appearance.locale",
        section: "appearance",
        section_label: "外观",
        group: "theme",
        group_label: Some("主题与语言"),
        label: "界面语言",
        hint: "中英双语界面文案即时切换并持久化（未登记词回退中文）",
        hint_inline: false,
        kind: SettingKind::Enum(&[("zh", "中文"), ("en", "English")]),
        presets: &[],
        default_json: "\"zh\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "SettingsService::set_locale（同步 shared::i18n + refresh_windows）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "appearance.tab_bar_max_width",
        section: "appearance",
        section_label: "外观",
        group: "tabbar",
        group_label: Some("标签栏"),
        label: "标签栏最大宽度",
        hint: "窗口多 / 连接名长时的标签宽度上限（px）；标签文字省略，关闭按钮 ✕ 保持完整",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (120, "120px"),
            (160, "160px"),
            (200, "200px（默认）"),
            (240, "240px"),
            (320, "320px"),
        ],
        default_json: "200.0",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/view.rs（装配 DockSkin 应用）+ vendor/gpui-component/src/dock/tab_panel.rs（渲染读取）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "appearance.tab_bar_variant",
        section: "appearance",
        section_label: "外观",
        group: "tabbar",
        group_label: Some("标签栏"),
        label: "标签栏样式",
        hint: "标签条形态：标签页 / 描边 / 胶囊 / 分段 / 下划线（对应 TabVariant 五态）",
        hint_inline: false,
        kind: SettingKind::Enum(&[
            ("tab", "标签页"),
            ("outline", "描边"),
            ("pill", "胶囊"),
            ("segmented", "分段"),
            ("underline", "下划线"),
        ]),
        presets: &[],
        default_json: "\"tab\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/view.rs（装配 DockSkin 应用）+ vendor/gpui-component/src/dock/tab_panel.rs（渲染读取）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.show_line_numbers",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "显示行号",
        hint: "编辑区行号槽（已开文档立即生效；行号槽收窄的折叠入口不随此开关恢复）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步内核 set_line_number）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.show_folds",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "代码折叠",
        hint: "显示/隐藏代码折叠（默认开；关闭时折叠槽与 chevron 一并消失，大文件档位仍自动降级）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步 shared.show_folds → sync_fold）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.indent_folding",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "基于缩进的折叠",
        hint: "VS Code 风格：只要下一行缩进比当前行大就出现折叠按钮（默认开；关闭后只保留括号和关键字折叠）",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步 shared.indent_folding → sync_fold）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.fold_open_keywords",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "折叠开块关键字",
        hint: "逗号分隔的关键字列表，大写匹配（默认：CASE,BEGIN,IF,LOOP,WHILE,FOR,REPEAT）",
        hint_inline: true,
        kind: SettingKind::Text,
        presets: &[],
        default_json: "\"CASE,BEGIN,IF,LOOP,WHILE,FOR,REPEAT\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步 shared.fold_open_keywords → sync_fold）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.fold_close_keywords",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "折叠闭块关键字",
        hint: "逗号分隔的关键字列表，大写匹配（默认：END，含 END IF / END CASE 等复合词）",
        hint_inline: true,
        kind: SettingKind::Text,
        presets: &[],
        default_json: "\"END\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步 shared.fold_close_keywords → sync_fold）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.soft_wrap",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "自动换行",
        hint: "长行是否软换行（默认开；关闭后长行横向滚动，状态栏不再挤占换行微标）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步内核 set_soft_wrap）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.auto_close",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "自动闭合括号",
        hint: "输入 ( [ { 与引号时自动补全另一半（默认开；关闭后需手动补全）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步内核 set_auto_close）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.smart_indent",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "智能缩进",
        hint: "回车后按括号结构与语言规则自动选择缩进（默认开；关闭后回车固定行首）",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（render 同步内核 set_smart_indent）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.inline_completion",
        section: "editor",
        section_label: "编辑器",
        group: "editing",
        group_label: Some("编辑行为"),
        label: "AI 补全（Inline Completion）",
        hint: "输入停顿后由 AI 建议后续代码（灰字预览，Tab 接受）——能力已预留，当前版本暂不可用，放开后此开关立即生效",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/lsp/completions.rs::CompletionProvider::inline_completion（内核槽位，未挂 AI 源）",
        composite: false,
        disabled: true,
    },
    SettingSpec {
        key: "editor.default_tab_size",
        section: "editor",
        section_label: "编辑器",
        group: "defaults",
        group_label: Some("新文档默认值"),
        label: "默认缩进",
        hint: "新文档的缩进宽度（空格数）；已开文档用「重设 ▾」单独改",
        hint_inline: false,
        kind: SettingKind::Number,
        presets: &[
            (2, "2 空格"),
            (4, "4 空格（默认）"),
            (8, "8 空格"),
        ],
        default_json: "4",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel::apply_settings_defaults（新建文档）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.default_hard_tabs",
        section: "editor",
        section_label: "编辑器",
        group: "defaults",
        group_label: Some("新文档默认值"),
        label: "默认缩进用制表符",
        hint: "新文档缩进用 Tab（与「默认缩进」宽度并存：Tab = 该宽度）；已开文档用「重设 ▾」单独改",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel::apply_settings_defaults（新建文档）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.default_line_ending",
        section: "editor",
        section_label: "编辑器",
        group: "defaults",
        group_label: Some("新文档默认值"),
        label: "默认换行",
        hint: "新文档的换行符（LF / CRLF）；已开文档用「重设 ▾」单独改",
        hint_inline: false,
        kind: SettingKind::Enum(&[("LF", "LF"), ("CRLF", "CRLF")]),
        presets: &[],
        default_json: "\"LF\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel::apply_settings_defaults（新建文档）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.default_encoding",
        section: "editor",
        section_label: "编辑器",
        group: "defaults",
        group_label: Some("新文档默认值"),
        label: "默认编码",
        hint: "新文档的保存编码（与「重设 ▾」同一套编码档）；已开文档用「重设 ▾」单独改",
        hint_inline: false,
        kind: SettingKind::Enum(&[
            ("UTF-8", "UTF-8"),
            ("UTF-8-BOM", "UTF-8 (BOM)"),
            ("GBK", "GBK"),
            ("UTF-16LE", "UTF-16 LE"),
            ("UTF-16BE", "UTF-16 BE"),
        ]),
        presets: &[],
        default_json: "\"UTF-8\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel::apply_settings_defaults（新建文档）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.format_keyword_case",
        section: "editor",
        section_label: "编辑器",
        group: "format",
        group_label: Some("SQL 格式化"),
        label: "关键字大小写",
        hint: "格式化后关键字统一大小写（保留原样 / 大写 / 小写）；只改关键字，字符串与注释内容不受影响",
        hint_inline: false,
        kind: SettingKind::Enum(&[
            ("preserve", "保持原样"),
            ("upper", "大写"),
            ("lower", "小写"),
        ]),
        presets: &[],
        default_json: "\"preserve\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel::format_document（读设置注入 FormatOptions）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.format_indent_width",
        section: "editor",
        section_label: "编辑器",
        group: "format",
        group_label: Some("SQL 格式化"),
        label: "缩进宽度",
        hint: "格式化后行首缩进空格数（2 = sqlglot 原生；仅作用于格式化输出，多行字符串内的缩进不受影响）",
        hint_inline: false,
        kind: SettingKind::Number,
        presets: &[(2, "2 空格（默认）"), (4, "4 空格"), (8, "8 空格")],
        default_json: "2",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel::format_document（读设置注入 FormatOptions）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "editor.format_on_save",
        section: "editor",
        section_label: "编辑器",
        group: "format",
        group_label: Some("SQL 格式化"),
        label: "保存时自动格式化",
        hint: "Ctrl+S 落盘前先格式化（解析不了的部分原样保留）；仅 SQL / 通用模式生效",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/editor/src/view/host.rs::EditorHostPanel（保存流程先 format_document 再落盘）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.source_short_code",
        section: "navigator",
        section_label: "数据源导航",
        group: "names",
        group_label: Some("名称与标签"),
        label: "来源标识",
        hint: "连接行右端显示短码或文字（同一列，宽度随模式切换）",
        hint_inline: false,
        kind: SettingKind::BoolPair {
            on: "短码 P/G/GP",
            off: "文字（项目 / 全局 / 共享）",
        },
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "workbench/src/database/src/nav_view.rs::render_connection_row",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_tags",
        section: "navigator",
        section_label: "数据源导航",
        group: "names",
        group_label: Some("名称与标签"),
        label: "显示标签",
        hint: "连接行名称下方最多 2 个标签 chip，其余折叠为 +N",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "workbench/src/database/src/nav_view.rs::render_connection_row",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_scope",
        section: "navigator",
        section_label: "数据源导航",
        group: "names",
        group_label: Some("名称与标签"),
        label: "显示归属域",
        hint: "「来源标识」的一部分：行尾右对齐的归属域列（可独立于短码开关关闭；关闭后整列隐藏）",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "workbench/src/database/src/nav_view.rs::render_connection_row",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_name_type_short",
        section: "navigator",
        section_label: "数据源导航",
        group: "names",
        group_label: Some("名称与标签"),
        label: "连接名称显示类型简称",
        hint: "名称前以方括号显示类型简称（如 `[MY] 192.168.3.138`；简称表可在「类型简称映射…」弹窗维护，默认同徽标缩写）",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Both,
        consumer: "workbench/src/database/src/nav_view.rs::render_connection_row",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.filters",
        section: "navigator",
        section_label: "数据源导航",
        group: "filter",
        group_label: Some("过滤与视图"),
        label: "facet 筛选",
        hint: "归属域 / 类型 / 驱动 / 标签筛选（导航面板内 chips 与「筛选 ▾」）",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{\"source\":null,\"db_type\":null,\"driver\":null,\"tag\":null}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "workbench/src/panels/mod.rs::SidebarPanel::new + nav.rs::write_nav_filters",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.saved_views",
        section: "navigator",
        section_label: "数据源导航",
        group: "filter",
        group_label: Some("过滤与视图"),
        label: "已保存视图",
        hint: "命名 facet 快照列表（导航面板 ⋯ 菜单「将当前筛选存为视图…」保存、子菜单应用）",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "workbench/src/components/saved_view_dialog.rs + nav_view/chrome.rs（⋯ 菜单视图组）",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.visible_databases",
        section: "navigator",
        section_label: "数据源导航",
        group: "filter",
        group_label: Some("过滤与视图"),
        label: "可见库白名单",
        hint: "每连接的可见库白名单（空 / 未设置 = 默认隐藏系统库与系统 schema；设置后原样展示白名单，含系统对象）。连接行右键「数据库可见性…」维护",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "database/src/navigator_service.rs（树加载过滤，经 NavHost 快照传入）+ workbench/src/components/visible_databases_dialog.rs",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.global_mask",
        section: "navigator",
        section_label: "数据源导航",
        group: "filter",
        group_label: Some("过滤与视图"),
        label: "全局对象名过滤器",
        hint: "面板内全部连接对象名的兜底 glob 过滤（include/exclude 每行一条，exclude 优先于 include）；空规则 = 不启用，与每连接 mask 取 AND。经导航面板 ⋯ 菜单「全局对象名过滤器…」维护",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{\"include\":[],\"exclude\":[],\"use_regex\":false}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "workbench/src/components/object_mask_dialog.rs + database/src/nav_view/actions.rs / rows.rs（每连接 mask 与全局 mask 叠加过滤）",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_search",
        section: "navigator",
        section_label: "数据源导航",
        group: "filter",
        group_label: Some("过滤与视图"),
        label: "导航文件搜索框显隐",
        hint: "导航树顶部文件搜索输入框是否显示（合并分支新增字段，2026-10-28 补登记）。读写走导航面板工具栏开关",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "database/src/nav_view/（导航树顶部搜索框显隐）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.auto_connect_on_startup",
        section: "navigator",
        section_label: "数据源导航",
        group: "filter",
        group_label: Some("过滤与视图"),
        label: "启动时自动连接上次打开的连接",
        hint: "应用启动时是否自动连接上次展开的连接（默认关）。开启后启动后台建连，用户无需手动点击",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "database/src/nav_view/actions.rs（prewarm_expanded_connections 开头判断）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.row_estimate",
        section: "navigator",
        section_label: "数据源导航",
        group: "inline",
        group_label: Some("行内信息"),
        label: "行估计徽标",
        hint: "表/视图行尾显示后台回填的行数估计（≈ N）；关闭后不排估计任务、不显示徽标。PG 取 reltuples、MySQL 取 TABLE_ROWS，其余驱动不支持",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "database/src/nav_view/actions.rs（row_estimate 门控估计任务）+ nav_view/rows.rs（≈ N 徽标渲染）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_inline_stats",
        section: "navigator",
        section_label: "数据源导航",
        group: "inline",
        group_label: Some("行内信息"),
        label: "表行统计徽标",
        hint: "表/视图行右侧统计徽标总开关（注释副标、≈ KB 大小、分区·继承等）；关闭则全部不显，子开关语义不冲突",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "database/src/nav_view/rows.rs::render_nav_node（表行右侧徽标总闸）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_inline_comment",
        section: "navigator",
        section_label: "数据源导航",
        group: "inline",
        group_label: Some("行内信息"),
        label: "表行注释副标",
        hint: "表/视图行显示对象注释副标（总开关开时仍受此开关）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "database/src/nav_view/rows.rs::render_nav_node（comment_subline）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_inline_size",
        section: "navigator",
        section_label: "数据源导航",
        group: "inline",
        group_label: Some("行内信息"),
        label: "表行大小徽标",
        hint: "表/视图行显示 ≈ KB 大小徽标（总开关开时仍受此开关）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "database/src/nav_view/rows.rs::render_nav_node（≈ KB 徽标）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "navigator.show_chips",
        section: "navigator",
        section_label: "数据源导航",
        group: "inline",
        group_label: Some("行内信息"),
        label: "连接行标签 chip 常显",
        hint: "连接行尾部标签 chip 是否常显；关闭时悬停/选中才出现（合并分支新增字段，2026-10-28 补登记）。读写走导航面板 ⋯ 菜单「显示标签」开关",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "database/src/nav_view/rows.rs（连接行标签 chip 显隐）+ database/src/nav_view/actions.rs（⋯ 菜单开关）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "resources.keep_versions",
        section: "resources",
        section_label: "资产库",
        group: "resources",
        group_label: None,
        label: "历史内容保留",
        hint: "每份存档保留几份历史内容副本（版本行永久保留，界面上以「副本缺失」标注）；对之后的归档 / 版本还原生效",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (0, "只留元数据"),
            (5, "5 份"),
            (10, "10 份"),
            (20, "20 份"),
            (-1, "全部保留"),
        ],
        default_json: "5",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/services/resource_jobs.rs::open_service（归档 / 版本还原作业）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "resources.default_sort",
        section: "resources",
        section_label: "资产库",
        group: "resources",
        group_label: None,
        label: "默认排序",
        hint: "资产的列表排序字段（在面板里点排序会记住上次的选择；方向按字段惯例：名称升序，时间 / 大小 / 版本降序）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("name", "名称"),
            ("archived_at", "归档时间"),
            ("updated_at", "更新时间"),
            ("size", "大小"),
            ("version", "版本号"),
        ]),
        presets: &[],
        default_json: "\"name\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Both,
        consumer: "workbench/src/components/resource_host.rs::remember_sort（写）+ panels/resources.rs::build_resources_panel（读）→ analytics_resource::ResourcesPanel::set_sort",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "resources.collapsed_groups",
        section: "resources",
        section_label: "资产库",
        group: "resources",
        group_label: None,
        label: "分组折叠状态",
        hint: "每个项目里折叠了哪些分组（面板内点击即记；按项目分开存，换项目各归各的）",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{}",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Module,
        consumer: "workbench/src/components/resource_host.rs::remember_collapsed（写）+ panels/resources.rs::build_resources_panel（读）→ analytics_resource::ResourcesPanel::set_collapsed",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "resources.default_group",
        section: "resources",
        section_label: "资产库",
        group: "resources",
        group_label: None,
        label: "默认分组",
        hint: "归档时预选哪个分组（分组头右键里设；按项目分开存，未设 = 未分组）",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{}",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Module,
        consumer: "workbench/src/components/resource_host.rs::remember_default_group（写）+ archive_seed / build_resources_panel（读）→ analytics_resource::dialogs::archive::ArchiveDialogSeed::group_id",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "connection_defaults.connect_timeout_ms",
        section: "connection",
        section_label: "连接默认值",
        group: "connection",
        group_label: None,
        label: "建连超时",
        hint: "超时判定本次尝试失败并自动重试一次；对之后新建的连接生效",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (5_000, "5s"),
            (15_000, "15s"),
            (30_000, "30s"),
            (60_000, "60s"),
        ],
        default_json: "15000",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/services/connection_service.rs::connect_with_type",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "connection_defaults.lan_disable_tls",
        section: "connection",
        section_label: "连接默认值",
        group: "connection",
        group_label: None,
        label: "LAN 直连 TLS",
        hint: "LAN / 本机直连时显式关闭 TLS（规避 sqlx 默认 prefer 的握手卡顿）；公网不受影响",
        hint_inline: true,
        kind: SettingKind::BoolPair {
            on: "关 TLS",
            off: "保留 TLS",
        },
        presets: &[],
        default_json: "true",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/services/connection_service.rs::apply_lan_tls_default",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "projects.sort_mode",
        section: "projects",
        section_label: "项目",
        group: "projects",
        group_label: None,
        label: "列表排序",
        hint: "项目选择器的排序方式（选择器内也有循环按钮）",
        hint_inline: false,
        kind: SettingKind::Enum(&[
            ("last_opened", "最近打开"),
            ("name", "名称"),
            ("created", "创建时间"),
        ]),
        presets: &[],
        default_json: "\"last_opened\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Both,
        consumer: "workbench/src/view.rs::WorkbenchView::new（读）+ components/project_host.rs（写）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "logging.min_level",
        section: "logging",
        section_label: "日志",
        group: "logging",
        group_label: None,
        label: "日志级别",
        hint: "低于该级别的记录不写文件也不落库（改完立即生效；排障时可临时调成 DEBUG）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("TRACE", "TRACE"),
            ("DEBUG", "DEBUG"),
            ("INFO", "INFO"),
            ("WARN", "WARN"),
            ("ERROR", "ERROR"),
        ]),
        presets: &[],
        default_json: "\"INFO\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/app/src/main.rs（启动取值）+ engine::logging::reload_log_level（运行时，经装配层注册的 sink）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "startup.default_left_panel",
        section: "startup",
        section_label: "启动",
        group: "startup",
        group_label: None,
        label: "启动时默认显示的模块",
        hint: "应用启动时左侧栏默认停在哪一个模块（活动栏也可以随时切换）",
        hint_inline: false,
        kind: SettingKind::Enum(&[
            ("database", "数据库导航"),
            ("draft", "草稿箱"),
            ("resources", "资产库"),
            ("plugin", "插件"),
        ]),
        presets: &[],
        default_json: "\"database\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/view.rs::WorkbenchView::init_workspace（读设置 → Shared.active_left）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "scratchpad.toolbar",
        section: "scratchpad",
        section_label: "草稿箱",
        group: "scratchpad",
        group_label: None,
        label: "工具栏按钮（显示/隐藏与位置）",
        hint: "⋯ 更多设置：每个按钮可切换显示/隐藏与左/右位置；空配置 = 全部显示 + 默认位置（左 5 右 2）；改完立即生效并持久化",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "crates/scratchpad/src/scratchpad_view/chrome.rs::（⋯ 菜单读写）+ scratchpad_view.rs::ScratchpadView::new（启动读入）",
        composite: true,
        disabled: false,
    },
    SettingSpec {
        key: "scratchpad.group_max_h",
        section: "scratchpad",
        section_label: "草稿箱",
        group: "scratchpad",
        group_label: None,
        label: "分区高度上限",
        hint: "草稿/引用/回收站可拖拽分区的高度上限（rems，默认 7.5 = 面板默认）；拖拽 handle 写回（B3 新增，2026-10-28 补登记）。不上设置页，读写走分区拖拽 handle",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[],
        default_json: "7.5",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "crates/scratchpad/src/scratchpad_view.rs::ScratchpadView::new（启动读入）+ chrome.rs::persist_group_max_h（拖拽写回）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "insight.memory_limit_mb",
        section: "insight",
        section_label: "洞察",
        group: "insight",
        group_label: None,
        label: "分析引擎内存上限",
        hint: "内存 DuckDB（洞察 / 结果集 / Mock 临时表共享）的内存上限（MB）；超限溢写到磁盘临时目录；改完立即生效，已在内存的数据不因此释放",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (512, "512 MB"),
            (1024, "1 GB"),
            (2048, "2 GB（默认）"),
            (4096, "4 GB"),
            (8192, "8 GB"),
        ],
        default_json: "2048",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/services/duckdb_host.rs（装配层经 install_insight_memory_limit_sink 应用到 DuckDBManager::set_memory_limit_mb）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "insight.full_scan_confirm",
        section: "insight",
        section_label: "洞察",
        group: "insight",
        group_label: None,
        label: "全量统计前确认",
        hint: "源目标切「全量」口径前先弹确认（大表全量会慢且占内存）；关闭后点「全量统计」直接执行",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/components/insight_actions.rs::insight_full_confirm_requested（宿主弹窗判断）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "insight.work_set_limit",
        section: "insight",
        section_label: "洞察",
        group: "insight",
        group_label: None,
        label: "工作集提示阈值",
        hint: "洞察集合内表数超过该值，集合卡以警告色提示先清理（集合表跳过 TTL / 上限清理，一直占内存）；0 = 不提示",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (0, "不提示"),
            (10, "10 张"),
            (20, "20 张"),
            (30, "30 张（默认）"),
            (50, "50 张"),
        ],
        default_json: "30",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/components/insight_actions.rs::apply_insight_prefs（宿主推送）+ crates/insight/src/view/set.rs（渲染）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "insight.status_usage",
        section: "insight",
        section_label: "洞察",
        group: "insight",
        group_label: None,
        label: "显示内存占用",
        hint: "中间页工具栏显示分析引擎内存占用（「内存 X MB / 上限 · 临时表 N」）；关闭后停止轮询采样",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/components/insight_actions.rs::apply_insight_prefs（宿主推送）+ crates/insight/src/workspace.rs（轮询与渲染）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.fact_storage",
        section: "result_panel",
        section_label: "结果集面板",
        group: "data",
        group_label: Some("数据与显示"),
        label: "事实档载体",
        hint: "进分析 / 带类型导出 / 表格快照时事实数据的存放方式：自动按估算水位分流（8 MiB 上界 / 3 MiB 滞回线）；强制内存档常驻 Rust 堆；强制表档进 DuckDB 常驻表（表档 B 批接通，未接通时按降级档处理）；网格渲染与文本导出不受此偏好影响",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("auto", "自动"),
            ("memory", "内存档"),
            ("table", "表档"),
        ]),
        presets: &[],
        default_json: "\"auto\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/shared/src/fact.rs::storage_choice / dual_estimate（分流纯函数）；B 批由 workbench 装配层在触发物化路径读取本偏好并映射到 shared::StoragePref",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.show_column_comments",
        section: "result_panel",
        section_label: "结果集面板",
        group: "data",
        group_label: Some("数据与显示"),
        label: "列头直接显示注释",
        hint: "开启后列名下方第二行小字显示字段注释（截短 + 悬停看全）；关闭时注释只在列头悬停 tooltip 里。面板工具栏另有临时开关可覆盖此偏好",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_th（列头第二行注释小字）；面板工具栏 B1 临时开关在 sync 时覆盖",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.show_type_badge",
        section: "result_panel",
        section_label: "结果集面板",
        group: "data",
        group_label: Some("数据与显示"),
        label: "列头显示类型徽标",
        hint: "开启后列名前显示类型符号徽标（123=数字、Aa=字符、◷=日期、✓=布尔、01=二进制）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_th（类型徽标渲染）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.number_format",
        section: "result_panel",
        section_label: "结果集面板",
        group: "data",
        group_label: Some("数据与显示"),
        label: "数字显示格式",
        hint: "数字族（整数 / 浮点 / 定点小数）的显示格式：原始值 / 千分位 / 两位小数 / 科学计数。只影响网格显示，复制与导出仍用原始值",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("raw", "原始值"),
            ("grouped", "千分位"),
            ("fixed2", "两位小数"),
            ("scientific", "科学计数"),
        ]),
        presets: &[],
        default_json: "\"raw\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::format_cell_text（数字族显示格式化；经 host apply_result_style 即时生效）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.null_placeholder",
        section: "result_panel",
        section_label: "结果集面板",
        group: "data",
        group_label: Some("数据与显示"),
        label: "NULL 显示占位",
        hint: "NULL 值的显示占位（判定仍按原始值 `NULL`；灰斜体渲染不变）：NULL 原文 / ∅ / 破折号",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("null", "NULL"),
            ("emptyset", "∅"),
            ("dash", "—"),
        ]),
        presets: &[],
        default_json: "\"null\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::format_cell_text（NULL 占位替换）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.bool_display",
        section: "result_panel",
        section_label: "结果集面板",
        group: "data",
        group_label: Some("数据与显示"),
        label: "布尔显示映射",
        hint: "布尔列的显示形态：true/false / 是/否 / 1/0（等宽字体渲染不变）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("true_false", "true / false"),
            ("cn", "是 / 否"),
            ("digits", "1 / 0"),
        ]),
        presets: &[],
        default_json: "\"true_false\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::format_cell_text（布尔族映射）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.loop_selection",
        section: "result_panel",
        section_label: "结果集面板",
        group: "interaction",
        group_label: Some("选择与操作"),
        label: "键盘循环选择",
        hint: "键盘导航到末尾后循环回开头（关闭则到末尾停止）",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::new_table_state（TableState::loop_selection）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.select_on_modifier_click",
        section: "result_panel",
        section_label: "结果集面板",
        group: "interaction",
        group_label: Some("选择与操作"),
        label: "Ctrl+点击多选",
        hint: "开启后 Ctrl+点击可多选行/列/单元格",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::new_table_state（TableState::select_on_modifier_click）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.persist_column_widths",
        section: "result_panel",
        section_label: "结果集面板",
        group: "interaction",
        group_label: Some("选择与操作"),
        label: "列宽跨会话持久化",
        hint: "开启后列宽记忆跨会话保留；关闭则只在当前会话内记忆",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "true",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::column_widths（会话内记忆；跨会话持久化 backlog）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.align_numeric",
        section: "result_panel",
        section_label: "结果集面板",
        group: "align",
        group_label: Some("对齐"),
        label: "对齐：数字",
        hint: "数字族的对齐覆盖（继承 = 数字默认右对齐）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "继承默认"),
            ("left", "左对齐"),
            ("center", "居中"),
            ("right", "右对齐"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::cell_style（按族对齐覆盖）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.align_text",
        section: "result_panel",
        section_label: "结果集面板",
        group: "align",
        group_label: Some("对齐"),
        label: "对齐：文本",
        hint: "文本族（文本 / JSON / UUID / 未知）的对齐覆盖（继承 = 左对齐）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "继承默认"),
            ("left", "左对齐"),
            ("center", "居中"),
            ("right", "右对齐"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::cell_style（按族对齐覆盖）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.align_temporal",
        section: "result_panel",
        section_label: "结果集面板",
        group: "align",
        group_label: Some("对齐"),
        label: "对齐：时间",
        hint: "时间族（时间戳 / 日期 / 时间）的对齐覆盖（继承 = 左对齐 + 等宽）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "继承默认"),
            ("left", "左对齐"),
            ("center", "居中"),
            ("right", "右对齐"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::cell_style（按族对齐覆盖）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.align_bool",
        section: "result_panel",
        section_label: "结果集面板",
        group: "align",
        group_label: Some("对齐"),
        label: "对齐：布尔",
        hint: "布尔族的对齐覆盖（继承 = 左对齐 + 等宽）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "继承默认"),
            ("left", "左对齐"),
            ("center", "居中"),
            ("right", "右对齐"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::cell_style（按族对齐覆盖）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.align_blob",
        section: "result_panel",
        section_label: "结果集面板",
        group: "align",
        group_label: Some("对齐"),
        label: "对齐：BLOB",
        hint: "BLOB 族的对齐覆盖（继承 = 左对齐；BLOB 摘要格）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "继承默认"),
            ("left", "左对齐"),
            ("center", "居中"),
            ("right", "右对齐"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::cell_style（按族对齐覆盖）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.color_numeric",
        section: "result_panel",
        section_label: "结果集面板",
        group: "color",
        group_label: Some("着色"),
        label: "前景色：数字",
        hint: "数字族的前景色角色（预设色板从主题语义 token 派生，明暗主题自动适配；继承 = 不染色）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "不染色"),
            ("info", "信息蓝"),
            ("success", "成功绿"),
            ("warning", "警告橙"),
            ("danger", "危险红"),
            ("primary", "主品牌"),
            ("muted", "弱化灰"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_td（ColorRolePref → theme.colors.*）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.color_text",
        section: "result_panel",
        section_label: "结果集面板",
        group: "color",
        group_label: Some("着色"),
        label: "前景色：文本",
        hint: "文本族的前景色角色（预设色板从主题语义 token 派生；继承 = 不染色）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "不染色"),
            ("info", "信息蓝"),
            ("success", "成功绿"),
            ("warning", "警告橙"),
            ("danger", "危险红"),
            ("primary", "主品牌"),
            ("muted", "弱化灰"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_td（ColorRolePref → theme.colors.*）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.color_temporal",
        section: "result_panel",
        section_label: "结果集面板",
        group: "color",
        group_label: Some("着色"),
        label: "前景色：时间",
        hint: "时间族的前景色角色（预设色板从主题语义 token 派生；继承 = 不染色）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "不染色"),
            ("info", "信息蓝"),
            ("success", "成功绿"),
            ("warning", "警告橙"),
            ("danger", "危险红"),
            ("primary", "主品牌"),
            ("muted", "弱化灰"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_td（ColorRolePref → theme.colors.*）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.color_bool",
        section: "result_panel",
        section_label: "结果集面板",
        group: "color",
        group_label: Some("着色"),
        label: "前景色：布尔",
        hint: "布尔族的前景色角色（预设色板从主题语义 token 派生；继承 = 不染色）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "不染色"),
            ("info", "信息蓝"),
            ("success", "成功绿"),
            ("warning", "警告橙"),
            ("danger", "危险红"),
            ("primary", "主品牌"),
            ("muted", "弱化灰"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_td（ColorRolePref → theme.colors.*）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "result_panel.color_blob",
        section: "result_panel",
        section_label: "结果集面板",
        group: "color",
        group_label: Some("着色"),
        label: "前景色：BLOB",
        hint: "BLOB 族的前景色角色（预设色板从主题语义 token 派生；继承 = 不染色）",
        hint_inline: true,
        kind: SettingKind::Enum(&[
            ("inherit", "不染色"),
            ("info", "信息蓝"),
            ("success", "成功绿"),
            ("warning", "警告橙"),
            ("danger", "危险红"),
            ("primary", "主品牌"),
            ("muted", "弱化灰"),
        ]),
        presets: &[],
        default_json: "\"inherit\"",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/result_panel/src/grid.rs::render_td（ColorRolePref → theme.colors.*）",
        composite: false,
        disabled: false,
    },
    // ===== 布局设置 =====
    SettingSpec {
        key: "layout.locked",
        section: "layout",
        section_label: "布局",
        group: "layout",
        group_label: None,
        label: "锁定布局",
        hint: "开启后用户不能通过拖动调整面板大小或位置",
        hint_inline: false,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/view.rs（render 时同步到 DockArea::set_locked）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "layout.left_sidebar_width",
        section: "layout",
        section_label: "布局",
        group: "layout",
        group_label: None,
        label: "左侧侧边栏宽度",
        hint: "左侧面板的默认宽度（px）；上方示意图显示占比",
        hint_inline: false,
        kind: SettingKind::Number,
        presets: &[
            (200, "200px"),
            (250, "250px（默认）"),
            (300, "300px"),
            (350, "350px"),
        ],
        default_json: "250",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/view.rs（render 时同步到 DockArea::set_dock_size）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "layout.right_sidebar_width",
        section: "layout",
        section_label: "布局",
        group: "layout",
        group_label: None,
        label: "右侧侧边栏宽度",
        hint: "右侧面板的默认宽度（px）；上方示意图显示占比",
        hint_inline: false,
        kind: SettingKind::Number,
        presets: &[
            (250, "250px"),
            (300, "300px（默认）"),
            (350, "350px"),
            (400, "400px"),
        ],
        default_json: "300",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/view.rs（render 时同步到 DockArea::set_dock_size）",
        composite: false,
        disabled: false,
    },
    // ===== 通知设置 =====
    SettingSpec {
        key: "notifications.success_autohide_secs",
        section: "notifications",
        section_label: "通知",
        group: "notifications",
        group_label: None,
        label: "成功/信息自动消失",
        hint: "操作成功、普通信息类通知自动消失的秒数；0 = 不自动消失",
        hint_inline: false,
        kind: SettingKind::Number,
        presets: &[
            (0, "不自动消失"),
            (2, "2 秒"),
            (3, "3 秒（默认）"),
            (5, "5 秒"),
        ],
        default_json: "3",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/notification.rs（notify 时读取设置决定 autohide）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "notifications.warning_autohide_secs",
        section: "notifications",
        section_label: "通知",
        group: "notifications",
        group_label: None,
        label: "警告自动消失",
        hint: "警告类通知自动消失的秒数；0 = 不自动消失",
        hint_inline: false,
        kind: SettingKind::Number,
        presets: &[
            (0, "不自动消失"),
            (3, "3 秒"),
            (5, "5 秒（默认）"),
            (10, "10 秒"),
        ],
        default_json: "5",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/notification.rs（notify 时读取设置决定 autohide）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "notifications.error_autohide",
        section: "notifications",
        section_label: "通知",
        group: "notifications",
        group_label: None,
        label: "错误自动消失",
        hint: "错误类通知是否自动消失；默认关闭（必须手动关闭），避免漏掉错误",
        hint_inline: true,
        kind: SettingKind::Bool,
        presets: &[],
        default_json: "false",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/notification.rs（notify 时读取设置决定 autohide）",
        composite: false,
        disabled: false,
    },
    SettingSpec {
        key: "notifications.merge_window_secs",
        section: "notifications",
        section_label: "通知",
        group: "notifications",
        group_label: None,
        label: "同类通知合并窗口",
        hint: "短时间内连续多个同类通知合并成一个计数（如批量连接时）；0 = 不合并",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[
            (0, "不合并"),
            (2, "2 秒（默认）"),
            (3, "3 秒"),
            (5, "5 秒"),
        ],
        default_json: "2",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Page,
        consumer: "crates/workbench/src/notification.rs（notify 时检查是否在合并窗口内）",
        composite: false,
        disabled: false,
    },
];

/// 按 key 查登记项。
pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    REGISTRY.iter().find(|s| s.key == key)
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
    REGISTRY
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
    REGISTRY.iter().filter(|s| s.entry != SettingEntry::Module)
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
        for s in REGISTRY {
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
        let mut keys: Vec<String> = REGISTRY.iter().map(|s| s.key.to_string()).collect();
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
        for s in REGISTRY {
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
        for s in REGISTRY {
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
        for s in REGISTRY {
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
        for s in REGISTRY {
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
        for s in REGISTRY {
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
        let page_section_count = REGISTRY
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
        let module_only = REGISTRY
            .iter()
            .filter(|s| s.entry == SettingEntry::Module)
            .count();
        assert_eq!(
            page_rows().count(),
            REGISTRY.len() - module_only,
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
        for s in REGISTRY {
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
        for s in REGISTRY {
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
        for s in REGISTRY.iter().filter(|s| s.entry != SettingEntry::Module) {
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
        for s in REGISTRY {
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
        for s in REGISTRY {
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

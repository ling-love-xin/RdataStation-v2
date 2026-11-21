//! 结果集面板设置 model（struct + 默认值 + 默认函数）。
use super::default_true;
use serde::{Deserialize, Serialize};

/// 带类型导出 / IronCalc 快照）在 B 批接通消费，渲染与文本导出不读本偏好。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultPanel {
    /// 事实档载体：`auto`（默认，按双份估算水位分流）/ `memory`（强制内存档）/
    /// `table`（强制表档；B 批接通后直灌常驻表，A 批未接通时按降级档处理）。
    #[serde(default = "default_result_fact_storage")]
    pub fact_storage: FactStoragePref,
    /// 列头是否**直接显示**字段注释（默认 `false`：注释只在 hover tooltip 里）。
    /// 开启后列名下方第二行小字显示注释（截短 + ellipsis；注释为空不占位）。
    /// 面板工具栏另有 B1 临时开关，覆盖此全局偏好。
    #[serde(default)]
    pub show_column_comments: bool,
    /// 【DBeaver 对齐】列头是否显示类型徽标（123/Aa/◷/✓/01 浅底符号）。
    #[serde(default = "default_true")]
    pub show_type_badge: bool,
    /// 【DBeaver 对齐】键盘导航是否循环选择（到末尾后循环回开头）。
    #[serde(default = "default_true")]
    pub loop_selection: bool,
    /// 【DBeaver 对齐】是否启用 Ctrl+点击多选（修饰键多选）。
    #[serde(default = "default_true")]
    pub select_on_modifier_click: bool,
    /// 【DBeaver 对齐】列宽是否跨会话持久化（关闭则只在当前会话内记忆）。
    #[serde(default = "default_true")]
    pub persist_column_widths: bool,
    /// 【DBeaver 对齐】状态栏是否显示「选定的行数」（`resultset.show.selected.rows`，
    /// 出厂 `false`：DBeaver `DataEditorsPreferencesInitializer` 三颗全关）。
    #[serde(default)]
    pub selection_stat_rows: bool,
    /// 【DBeaver 对齐】状态栏是否显示「选定的列数」（`resultset.show.selected.columns`）。
    #[serde(default)]
    pub selection_stat_columns: bool,
    /// 【DBeaver 对齐】状态栏是否显示「选定的单元格数」（`resultset.show.selected.cells`）。
    #[serde(default)]
    pub selection_stat_cells: bool,
    /// 【DBeaver 对齐 `resultset.show.filterPanel`】是否显示筛选条（默认 `true`，
    /// `DataEditorsPreferencesInitializer.java:67` 出厂即为 true）。
    ///
    /// 作用对象是工具栏左段**整条筛选区**（SQL 预览 / 展开 / 输入 / 运行 / 清除 /
    /// 筛选设置 / 历史 / 后退前进 / 结果内查找），等价于 DBeaver 的 `ResultSetFilterPanel`
    /// 整块 setVisible（`ResultSetViewer.updateFilterPanelVisibility:2002-2012`）。
    /// 关掉后回开的入口只剩右键 layout 段的「显示过滤器面板」（DBeaver 同：
    /// `fillLayoutMenu:3589-3592` 的 `ToggleFilterPanelAction`）。
    #[serde(default = "default_true")]
    pub show_filter_panel: bool,
    /// 【R5 对齐 DBeaver `viewerSash.setWeights`】结果集右栏（页签侧栏）宽度，单位 px
    /// （基准字号 16px 下的像素；实际按界面字号同比例缩放）。
    ///
    /// 语义边界（实证见 `docs/architecture/editor/result-panel.md` §8.45）：
    /// - DBeaver 存的是**千分比** `panelRatio`（面板占整行的千分之几，默认 300），
    ///   按展示类型（presentation）分段、跨结果集共用；
    /// - 本仓右栏是行内固定宽元素（R5 第一步口径），没有「按行宽测比例」的设施，
    ///   故存**像素**而不是比例 —— 窗口变宽时右栏宽度不跟着变，这是已知差异；
    /// - 作用域与 DBeaver 一致：**全局一份**（跨结果集 / 跨文档共用），不随会话快照落库。
    #[serde(default = "default_result_sidebar_width")]
    pub sidebar_width: i64,
    /// 【R6 对齐 DBeaver `resultSetSize` / `RESULT_SET_MAX_ROWS`】「取下一段」一次多少行。
    ///
    /// 语义边界（`docs/architecture/editor/result-panel.md` §8.85）：只影响**之后**的分段取数
    /// （`ExecTarget::Segment { limit }`），不改已加载行、不触发重查；下限 1（对齐
    /// `ResultSetPreferences.MIN_SEGMENT_SIZE`）。DBeaver 把这份值存在**数据源容器**上
    /// （连接级），本仓存**全局一份**（与右栏宽度同一作用域口径）。
    #[serde(default = "default_result_segment_rows")]
    pub segment_rows: i64,
    /// 【DBeaver 对齐】状态栏被隐藏的命令 id（逗号分隔，如 `undo,redo`）。
    ///
    /// 对应 DBeaver 的「工具条配置」：`plugin.xml` 里每条结果集底部命令都挂了
    /// `org.jkiss.dbeaver.ui.toolbar.configuration.visible` 的 `visibleWhen`
    /// （`:1381-1397` 的 `defaultVisibility` 只是默认值），隐藏的命令随时能勾回来。
    /// 面板状态行末端的 ▾ 菜单写这里（点一次就落盘一次）；设置页也允许手填，
    /// 未知 id 在面板侧被忽略（id 与命令的对应见 `result_panel::ports::StatusCommand::id`）。
    /// 作用域是**全局一份**（跨结果集 / 跨文档共用），不随会话快照落库。
    #[serde(default = "default_hidden_status_commands")]
    pub hidden_status_commands: String,
    /// 【计算页持久化，对齐 DBeaver `AggregateColumnsPanel.loadSettings/saveSettings`】
    /// 计算页启用的统计项，逗号分隔的稳定 id（`count,countdistinct,nulls,min,max,median,`
    /// `mode,avg,sum`，顺序即行序）。
    ///
    /// 空串是合法状态（用户把九项全 × 掉）；「设置里没写过」由 `serde(default)` 落到
    /// 出厂默认集（非空 + 去重计数）。未知 id 在面板侧忽略（id ↔ 项对应见
    /// `result_panel::ports::StatsItem::id`）。
    /// 作用域 = 全局一份（跨结果集 / 跨文档共用），不随会话快照落库。
    #[serde(default = "default_result_stats_items")]
    pub stats_items: String,
    /// 【分组页持久化，对齐 DBeaver `ResultSetPreferences.RS_GROUPING_DEFAULT_SORTING`】
    /// 分组结果排序口径的稳定 id：`none`（不排序，保持首现顺序）/ `value`（按分组键值升序）
    /// / `countdesc`（按组内行数降序）。
    ///
    /// DBeaver 那边存的是 `""` / `ASC` / `DESC`（空 = 不排序，且排序只作用于 count 列），
    /// 本仓的 `none` 与 `""` 等价、「按计数降序」与 `DESC` 等价，「按值升序」是超集档
    /// （差异登记在文档）。作用域 = 全局一份（DBeaver 是按连接存，本仓沿用结果集偏好的全局口径）。
    #[serde(default = "default_result_group_sort")]
    pub group_sort: String,
    /// 【分组页持久化，对齐 DBeaver `ResultSetPreferences.RS_GROUPING_SHOW_DUPLICATES_ONLY`】
    /// 是否只显示重复分组（组内行数 > 1）。默认 false（DBeaver 取 `getBoolean` 缺省也是 false，
    /// 该键在 plugin.xml 里没有 initializer）。
    #[serde(default)]
    pub group_duplicates_only: bool,
    /// 【右栏页签持久化，对齐 DBeaver `PresentationSettings.enabledPanelIds`】右栏启用了哪些页，
    /// 逗号分隔的稳定 id（`browser,value,stats,group,reference,execsql`，顺序无关——
    /// 读的时候按规范序归一）。空串 = 「没写过」→ 出厂六页全开（DBeaver
    /// `activateDefaultPanels:1396-1400` 同样把空集合补成默认面板）。
    /// 未知 id 在面板侧丢弃（id ↔ 页对应见 `result_panel::ports::RightTab::id`）。
    /// 作用域 = 全局一份（DBeaver 是随工作区/演示设置存，本仓沿用结果集偏好的全局口径）。
    #[serde(default = "default_result_open_tabs")]
    pub open_tabs: String,
    /// 【类型样式】数字显示格式（默认原始值；只影响渲染显示，复制／导出仍用原始值）。
    #[serde(default)]
    pub number_format: workbench_shell::result_style::NumberFormatPref,
    /// 【类型样式】NULL 显示占位（默认 `NULL` 原文；判定仍按原文）。
    #[serde(default)]
    pub null_placeholder: workbench_shell::result_style::NullPlaceholderPref,
    /// 【类型样式】布尔显示映射（默认 `true` / `false`）。
    #[serde(default)]
    pub bool_display: workbench_shell::result_style::BoolDisplayPref,
    /// 【类型样式】按族对齐覆盖（默认全部跟随类型默认：数字右对齐 / 其余左对齐）。
    #[serde(default)]
    pub align_numeric: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_text: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_temporal: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_bool: workbench_shell::result_style::AlignPref,
    #[serde(default)]
    pub align_blob: workbench_shell::result_style::AlignPref,
    /// 【类型样式】按族前景色角色（默认全部不染色；预设色板从主题语义 token 派生）。
    #[serde(default)]
    pub color_numeric: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_text: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_temporal: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_bool: workbench_shell::result_style::ColorRolePref,
    #[serde(default)]
    pub color_blob: workbench_shell::result_style::ColorRolePref,
    /// 【类型样式】按族背景色（默认全部无背景；语义角色浅底 + 自定义 hex）。
    #[serde(default)]
    pub bg_numeric: workbench_shell::result_style::BgColorPref,
    #[serde(default)]
    pub bg_text: workbench_shell::result_style::BgColorPref,
    #[serde(default)]
    pub bg_temporal: workbench_shell::result_style::BgColorPref,
    #[serde(default)]
    pub bg_bool: workbench_shell::result_style::BgColorPref,
    #[serde(default)]
    pub bg_blob: workbench_shell::result_style::BgColorPref,
    /// 【类型样式】按族字体样式（默认全部跟随类型默认）。
    #[serde(default)]
    pub font_numeric: workbench_shell::result_style::FontStylePref,
    #[serde(default)]
    pub font_text: workbench_shell::result_style::FontStylePref,
    #[serde(default)]
    pub font_temporal: workbench_shell::result_style::FontStylePref,
    #[serde(default)]
    pub font_bool: workbench_shell::result_style::FontStylePref,
    #[serde(default)]
    pub font_blob: workbench_shell::result_style::FontStylePref,
}

impl Default for ResultPanel {
    fn default() -> Self {
        Self {
            fact_storage: default_result_fact_storage(),
            show_column_comments: false,
            show_type_badge: true,
            loop_selection: true,
            select_on_modifier_click: true,
            persist_column_widths: true,
            // 选中统计三项出厂全关（对齐 DBeaver `DataEditorsPreferencesInitializer:86-88`）
            selection_stat_rows: false,
            selection_stat_columns: false,
            selection_stat_cells: false,
            // 筛选条出厂显示（对齐 `DataEditorsPreferencesInitializer.java:67`）
            show_filter_panel: true,
            // 右栏宽度出厂值 = `result_panel::ui::RESULT_SIDEBAR_RIGHT_WIDTH`（20rem @16px）
            sidebar_width: default_result_sidebar_width(),
            segment_rows: default_result_segment_rows(),
            hidden_status_commands: default_hidden_status_commands(),
            stats_items: default_result_stats_items(),
            group_sort: default_result_group_sort(),
            group_duplicates_only: false,
            open_tabs: default_result_open_tabs(),
            // 类型样式默认全继承 / 原始值（与渲染现状逐项一致）
            number_format: workbench_shell::result_style::NumberFormatPref::Raw,
            null_placeholder: workbench_shell::result_style::NullPlaceholderPref::Null,
            bool_display: workbench_shell::result_style::BoolDisplayPref::TrueFalse,
            align_numeric: workbench_shell::result_style::AlignPref::Inherit,
            align_text: workbench_shell::result_style::AlignPref::Inherit,
            align_temporal: workbench_shell::result_style::AlignPref::Inherit,
            align_bool: workbench_shell::result_style::AlignPref::Inherit,
            align_blob: workbench_shell::result_style::AlignPref::Inherit,
            color_numeric: workbench_shell::result_style::ColorRolePref::Inherit,
            color_text: workbench_shell::result_style::ColorRolePref::Inherit,
            color_temporal: workbench_shell::result_style::ColorRolePref::Inherit,
            color_bool: workbench_shell::result_style::ColorRolePref::Inherit,
            color_blob: workbench_shell::result_style::ColorRolePref::Inherit,
            bg_numeric: workbench_shell::result_style::BgColorPref::Inherit,
            bg_text: workbench_shell::result_style::BgColorPref::Inherit,
            bg_temporal: workbench_shell::result_style::BgColorPref::Inherit,
            bg_bool: workbench_shell::result_style::BgColorPref::Inherit,
            bg_blob: workbench_shell::result_style::BgColorPref::Inherit,
            font_numeric: workbench_shell::result_style::FontStylePref::Inherit,
            font_text: workbench_shell::result_style::FontStylePref::Inherit,
            font_temporal: workbench_shell::result_style::FontStylePref::Inherit,
            font_bool: workbench_shell::result_style::FontStylePref::Inherit,
            font_blob: workbench_shell::result_style::FontStylePref::Inherit,
        }
    }
}

impl ResultPanel {
    /// 【类型样式】导出为渲染消费用的聚合偏好。
    ///
    /// 消费方（editor host 构造同步 / workbench 设置变更推送）都从这里取，
    /// 不必各自把 23 项字段再拼一遍；偏好与模型字段一一对应。
    pub fn type_style(&self) -> workbench_shell::result_style::TypeStylePrefs {
        workbench_shell::result_style::TypeStylePrefs {
            number_format: self.number_format,
            null_placeholder: self.null_placeholder,
            bool_display: self.bool_display,
            align_numeric: self.align_numeric,
            align_text: self.align_text,
            align_temporal: self.align_temporal,
            align_bool: self.align_bool,
            align_blob: self.align_blob,
            color_numeric: self.color_numeric,
            color_text: self.color_text,
            color_temporal: self.color_temporal,
            color_bool: self.color_bool,
            color_blob: self.color_blob,
            bg_numeric: self.bg_numeric.clone(),
            bg_text: self.bg_text.clone(),
            bg_temporal: self.bg_temporal.clone(),
            bg_bool: self.bg_bool.clone(),
            bg_blob: self.bg_blob.clone(),
            font_numeric: self.font_numeric,
            font_text: self.font_text,
            font_temporal: self.font_temporal,
            font_bool: self.font_bool,
            font_blob: self.font_blob,
        }
    }
}

/// 事实档载体偏好（落盘 `auto` / `memory` / `table`，serde snake_case）。
///
/// 决策层枚举是 `shared::StoragePref`（纯函数入参，不引 serde）；这里独立定义持久化形态，
/// 经 [`FactStoragePref::to_shared`] 映射——设置层不把 serde 序列化约束拖进事实层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FactStoragePref {
    /// 自动：按双份估算水位分流（8 MiB 上界 / 3 MiB 滞回线）。
    #[default]
    Auto,
    /// 强制内存档：即使超水位也驻 Rust 堆。
    Memory,
    /// 强制表档：即使在水位内也走常驻表（B 批接通）。
    Table,
}

impl FactStoragePref {
    /// 映射到决策层 [`shared::StoragePref`]。
    pub fn to_shared(self) -> ::shared::StoragePref {
        match self {
            Self::Auto => ::shared::StoragePref::Auto,
            Self::Memory => ::shared::StoragePref::Memory,
            Self::Table => ::shared::StoragePref::Table,
        }
    }

    /// 全部取值（登记表枚举项、测试穷尽遍历用）。
    pub const ALL: [FactStoragePref; 3] = [
        FactStoragePref::Auto,
        FactStoragePref::Memory,
        FactStoragePref::Table,
    ];

    /// 落盘值文本（登记表枚举落盘值 / `value_by_key` 回读用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Memory => "memory",
            Self::Table => "table",
        }
    }

    /// 解析落盘值；未知值返回 `None`，由写入路径拒绝而不是猜。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|value| value.as_str() == s.trim())
    }
}

fn default_result_fact_storage() -> FactStoragePref {
    FactStoragePref::Auto
}

/// 结果集右栏宽度出厂值：320px（= `result_panel::ui::RESULT_SIDEBAR_RIGHT_WIDTH` 的
/// 20rem 在基准字号 16px 下的像素值；常量在那一侧单处定义，此处只写数值并保持一致）。
fn default_result_sidebar_width() -> i64 {
    320
}

/// 「一段」默认行数：1000（= `result_panel::ui::RESULT_SEGMENT_ROWS_DEFAULT` 与编辑器
/// `execution::SEGMENT_ROWS` 同值；常量在那两处定义，此处只写数值并保持一致）。
fn default_result_segment_rows() -> i64 {
    1000
}

/// 状态栏隐藏命令的出厂值：撤销 / 重做（DBeaver `plugin.xml:1381-1397` 里仅这两项
/// `defaultVisibility="false"` 且本仓不构成「唯一入口」；`fetch.page` 有意保持显示）。
fn default_hidden_status_commands() -> String {
    "undo,redo".to_string()
}

/// 计算页出厂启用集：`count`（非空）+ `countdistinct`（去重计数）。
///
/// 对齐 DBeaver：九个聚合函数里仅 `count` / `countDistinct` 标了 `default="true"`
/// （`plugin.xml:89-99`），`loadDefaultFunctions()` 只收 `isDefault()` 的项。
/// 本仓九项与 DBeaver 九函数一一对应（见 `result_panel::ports::StatsItem`），
/// 默认集口径与那边一致（2026-11-19 补上 `countdistinct` 后由「只有非空」改为两项）。
fn default_result_stats_items() -> String {
    "count,countdistinct".to_string()
}

/// 分组结果排序口径的出厂值：`value`（按分组键值升序）。
///
/// 与 `GroupingPanel` 面板内初值同一档（`lifecycle.rs` 里 `group_sort: GroupSort::Value`），
/// 避免「设置里的默认」与「面板里的默认」两处给出不同答案。
fn default_result_group_sort() -> String {
    "value".to_string()
}

/// 右栏启用页的出厂值：六页全开（规范序）。
///
/// 对齐 DBeaver：内置面板都注册在 `availablePanels` 里，`activateDefaultPanels:1396-1400`
/// 会把 `enabledPanelIds` 补成默认集合——本仓的默认集合就是六页全开。
fn default_result_open_tabs() -> String {
    "browser,value,stats,group,reference,execsql".to_string()
}

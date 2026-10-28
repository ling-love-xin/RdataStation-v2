//! rds-settings — 设置 model。
//!
//! 分节设置项（serde 序列化，缺失字段回退默认值，向前兼容）：
//! - `appearance`：外观（主题模式）
//! - `connection_defaults`：连接默认值（建连超时、LAN 直连 TLS）
//! - `projects`：项目列表偏好（排序方式）
//! - `navigator`：数据源导航（来源标识、显示开关、属性面板宽度、facet 筛选）
//! - `resources`：资产库（历史内容保留、默认排序、分组折叠状态）
//! - `logging`：日志最低级别
//!
//! 主题模式用 `AppearanceMode`（显式 `light` / `dark` / 跟随系统 `system`，serde
//! snake_case，与 gpui-kit 0.7.1 主题系统对齐）：显式模式直接转 `ThemeMode`
//! 应用；`system` 由装配层在启动与系统外观变化时解析为实际明暗。
//!
//! ## 准入（2026-09-16 起）
//!
//! **字段不是"想加就加"**：每个字段都必须在 `registry.rs` 的登记表里有对应条目
//! （消费方 / 默认值 / 生效方式 / 入口 / 是否上页），否则
//! `registry::tests::every_model_leaf_is_registered` 直接失败。被裁掉的 7 个
//! "有字段、有界面行，但没有消费方"的字段见
//! `docs/architecture/settings/settings-architecture.md` §7.2；新增字段的判定标准见 §7.1。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
/// 顶层设置。新增字段必须带 `#[serde(default)]`，保证旧配置向前兼容。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub appearance: Appearance,
    /// 编辑器常规设置（行号 / 新文档缩进 / 换行 / 编码默认值；文档级「重设 ▾」优先）
    #[serde(default)]
    pub editor: Editor,
    #[serde(default)]
    pub connection_defaults: ConnectionDefaults,
    #[serde(default)]
    pub projects: Projects,
    #[serde(default)]
    pub navigator: Navigator,
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub logging: Logging,
    #[serde(default)]
    pub startup: Startup,
    #[serde(default)]
    pub scratchpad: Scratchpad,
    #[serde(default)]
    pub insight: Insight,
    /// 结果集面板偏好（事实档载体分流；A 批仅注册偏好，分流纯函数在 `shared::fact`，
    /// 触发物化路径 B 批接通消费）。
    #[serde(default)]
    pub result_panel: ResultPanel,
    /// 布局设置（侧边栏宽度、布局锁定等）。
    #[serde(default)]
    pub layout: Layout,
    /// 通知设置（自动消失时间、批量合并）。
    #[serde(default)]
    pub notifications: Notifications,
}

/// 通知设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notifications {
    /// 成功/信息类通知自动消失时间（秒）。0 = 不自动消失。
    #[serde(default = "default_notify_success_autohide_secs")]
    pub success_autohide_secs: u32,
    /// 警告类通知自动消失时间（秒）。0 = 不自动消失。
    #[serde(default = "default_notify_warning_autohide_secs")]
    pub warning_autohide_secs: u32,
    /// 错误类通知是否自动消失（默认 false，必须手动关闭）。
    #[serde(default)]
    pub error_autohide: bool,
    /// 同类通知合并窗口（秒）。短时间内连续多个同类通知合并成一个计数。0 = 不合并。
    #[serde(default = "default_notify_merge_window_secs")]
    pub merge_window_secs: u32,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            success_autohide_secs: default_notify_success_autohide_secs(),
            warning_autohide_secs: default_notify_warning_autohide_secs(),
            error_autohide: false,
            merge_window_secs: default_notify_merge_window_secs(),
        }
    }
}

fn default_notify_success_autohide_secs() -> u32 {
    3
}

fn default_notify_warning_autohide_secs() -> u32 {
    5
}

fn default_notify_merge_window_secs() -> u32 {
    2
}

/// 布局设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    /// 是否锁定布局（用户不能通过拖动调整面板大小或位置）。
    #[serde(default = "default_layout_locked")]
    pub locked: bool,
    /// 左侧侧边栏宽度（px）。
    #[serde(default = "default_layout_left_sidebar_width")]
    pub left_sidebar_width: i64,
    /// 右侧侧边栏宽度（px）。
    #[serde(default = "default_layout_right_sidebar_width")]
    pub right_sidebar_width: i64,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            locked: default_layout_locked(),
            left_sidebar_width: default_layout_left_sidebar_width(),
            right_sidebar_width: default_layout_right_sidebar_width(),
        }
    }
}

/// 布局锁定：默认关闭。
fn default_layout_locked() -> bool {
    false
}

/// 左侧侧边栏宽度：默认 250px。
fn default_layout_left_sidebar_width() -> i64 {
    250
}

/// 右侧侧边栏宽度：默认 300px。
fn default_layout_right_sidebar_width() -> i64 {
    300
}

/// 洞察模块资源与交互偏好。
///
/// 四个项对应洞察模块的资源占用治理（2026-10 决策）：
/// - `memory_limit_mb`：分析引擎（进程级内存 DuckDB 单例）的内存上限。洞察 /
///   编辑器结果集 / Mock 的临时表都在这条连接上，共吃同一个上限；超限溢写
///   到磁盘临时目录。运行时即时应用（`SET memory_limit`，见 `DuckDBManager::set_memory_limit_mb`）。
/// - `full_scan_confirm`：源目标切「全量」口径前是否弹确认。全量建表在大表上
///   慢且占内存，默认确认；关掉则点「全量统计」直接执行（与「取样」同速）。
/// - `work_set_limit`：洞察集合（会话工作集）的提示阈值。集合内的表跳过 TTL /
///   上限清理（T21），数量超阈值时集合卡以警告色提示先清理；`0` = 不提示。
/// - `status_usage`：中间页工具栏是否显示分析引擎内存占用（「内存 X MB / 上限」）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    #[serde(default = "default_insight_memory_limit_mb")]
    pub memory_limit_mb: i64,
    #[serde(default = "default_insight_full_scan_confirm")]
    pub full_scan_confirm: bool,
    #[serde(default = "default_insight_work_set_limit")]
    pub work_set_limit: i64,
    #[serde(default = "default_insight_status_usage")]
    pub status_usage: bool,
}

impl Default for Insight {
    fn default() -> Self {
        Self {
            memory_limit_mb: default_insight_memory_limit_mb(),
            full_scan_confirm: default_insight_full_scan_confirm(),
            work_set_limit: default_insight_work_set_limit(),
            status_usage: default_insight_status_usage(),
        }
    }
}

/// 分析引擎内存上限（MB）：默认 2GB，与 `DuckDBManager::DEFAULT_MEMORY_LIMIT` 一致。
fn default_insight_memory_limit_mb() -> i64 {
    2048
}

/// 全量统计前确认：默认开启（大表全量是慢操作，值得一道确认）。
fn default_insight_full_scan_confirm() -> bool {
    true
}

/// 洞察集合提示阈值：默认 30 张（工作集超过 30 张提示先清理）。
fn default_insight_work_set_limit() -> i64 {
    30
}

/// 工具栏内存占用显示：默认开启。
fn default_insight_status_usage() -> bool {
    true
}

/// 结果集面板偏好（事实档载体分流；方案 §4.1 / §8.3）。
///
/// A 批只注册偏好与 `shared::fact::storage_choice` 纯函数；触发物化路径（进分析 /
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
    /// 【类型样式】数字显示格式（默认原始值；只影响渲染显示，复制 / 导出仍用原始值）。
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
        }
    }
}

impl ResultPanel {
    /// 【类型样式】导出为渲染消费用的聚合偏好。
    ///
    /// 消费方（editor host 构造同步 / workbench 设置变更推送）都从这里取，
    /// 不必各自把 13 项字段再拼一遍；偏好与模型字段一一对应。
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
        Self::ALL.into_iter().find(|value| value.as_str() == s.trim())
    }
}

fn default_result_fact_storage() -> FactStoragePref {
    FactStoragePref::Auto
}

/// 草稿箱侧栏专属设置（2026-10-11 接线：⋯ 更多设置持久化到用户配置）。
///
/// 目前只有一个复合项 `toolbar`；消费方是草稿箱面板的 ⋯ 菜单（操作现场修改，
/// 见登记表 `scratchpad.toolbar`）。**空表 = 全部按钮显示 + 默认位置**（左 5 右 2），
/// 与面板 `ToolbarButtonKey::side()` 的默认口径一致——只记录用户显式改过的键。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scratchpad {
    /// 工具栏按钮配置：键 = 按钮稳定标识（`search`/`new_file`/`new_folder`/
    /// `reference`/`import`/`sort`/`refresh`）；值 = 显示/隐藏 + 左/右位置。
    #[serde(default)]
    pub toolbar: BTreeMap<String, ToolbarButtonCfg>,
    /// 草稿/引用/回收站可拖拽分区的最大高度（rem，默认 7.5 = 面板默认，
    /// 区间 [2.0, 18.0]，2026-10-28 B3 接线持久化；数值形态方便设置契约）。
    #[serde(default)]
    pub group_max_h: f32,
}

impl Default for Scratchpad {
    fn default() -> Self {
        Self {
            toolbar: BTreeMap::new(),
            group_max_h: 7.5,
        }
    }
}

/// 单个工具栏按钮的配置（⋯ 菜单写入；落盘 `{ "visible": bool, "side": "left"|"right" }`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolbarButtonCfg {
    /// 是否在工具栏显示（默认显示）。
    #[serde(default = "default_toolbar_visible")]
    pub visible: bool,
    /// 所在侧（默认 `left`；`right` 显式写才用——面板按按钮默认侧兜底，
    /// 因此空配置与「未写 side」都回到原型默认左 5 右 2）。
    #[serde(default = "default_toolbar_side")]
    pub side: String,
    /// 工具栏显示顺序（⋯ 菜单 ↑↓ 调整；未写/负值 = 原型默认顺序（ALL 枚举序））。
    #[serde(default = "default_toolbar_order")]
    pub order: i32,
}

impl Default for ToolbarButtonCfg {
    fn default() -> Self {
        Self {
            visible: default_toolbar_visible(),
            side: default_toolbar_side(),
            order: default_toolbar_order(),
        }
    }
}

fn default_toolbar_order() -> i32 {
    -1
}

fn default_toolbar_visible() -> bool {
    true
}

fn default_toolbar_side() -> String {
    "left".to_string()
}

/// 启动行为（用户点名：启动时默认显示哪个模块——左侧栏的 `LeftPanel`）。
///
/// 落盘值见登记表 `startup.default_left_panel`；默认 `database`（数据库导航），
/// 与 `Shared::new` 的构造默认保持一致（设置未写入时行为不变）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Startup {
    /// 启动时左侧栏默认显示的模块（`database` / `draft` / `resources` / `plugin`）。
    #[serde(default = "default_startup_left_panel")]
    pub default_left_panel: String,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            default_left_panel: default_startup_left_panel(),
        }
    }
}

fn default_startup_left_panel() -> String {
    "database".to_string()
}

/// 外观：主题模式 + 界面字号。
///
/// `font_size` 是**界面字号缩放**（px，默认 16）：启动与变更时同步应用到
/// 主题字号（`theme.font_size`，影响 `font_size * N` 类宽度）与窗口 rem 基准
/// （`Window::set_rem_size`，rems / Tailwind 档位 / 布局全缩放）——二者同步，
/// 全 UI 一致缩放。v7 曾裁撤该字段（当时口径"字号权威在主题资产"）；2026-10-11
/// 按「可配置项进设置」决策复活为字号档位，旧配置里的 `font_size: 13.0` 直接兼容生效。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appearance {
    #[serde(default)]
    pub theme_mode: AppearanceMode,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// 界面语言（`"zh"` / `"en"`，默认中文）。变更时同步到 `shared::i18n`
    /// （全 UI 文案即时切换），见 `SettingsService::set_locale`。
    #[serde(default = "default_locale")]
    pub locale: String,
    /// 标签栏单标签最大宽度（px，默认 200）。窗口多 / 长连接名时给上限 + 省略，
    /// 关闭按钮（✕）保持完整。装配期应用到 DockSkin（vendor `tab_panel.rs` 读取）。
    #[serde(default = "default_tab_bar_max_width")]
    pub tab_bar_max_width: f32,
    /// 标签栏样式（`"tab"` / `"outline"` / `"pill"` / `"segmented"` / `"underline"`，
    /// 默认 `"tab"`），对应 `TabVariant` 五态；装配期映射后应用到 DockSkin。
    #[serde(default = "default_tab_bar_variant")]
    pub tab_bar_variant: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme_mode: AppearanceMode::Light,
            font_size: default_font_size(),
            locale: default_locale(),
            tab_bar_max_width: default_tab_bar_max_width(),
            tab_bar_variant: default_tab_bar_variant(),
        }
    }
}

fn default_font_size() -> f32 {
    16.0
}

fn default_locale() -> String {
    "zh".to_string()
}

fn default_tab_bar_max_width() -> f32 {
    200.0
}

fn default_tab_bar_variant() -> String {
    "tab".to_string()
}

/// 编辑器常规设置。
///
/// 消费方在 editor crate（`EditorHostPanel`）：行号开关即时生效（渲染同步到内核
/// `set_line_number`），缩进 / 换行 / 编码是**新文档与无会话文档**的默认值——
/// 已开文档的编码 / 换行 / 缩进仍走「重设 ▾」（文档级优先，见
/// `crates/editor/src/view/host.rs` 的重设菜单）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Editor {
    /// 编辑区是否显示行号（即时生效；内核 `LayoutMode::CodeEditor.line_number`）。
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
    /// 是否启用代码折叠（2026-10-27 起开关移到设置页；默认开）。
    /// 即时生效：编辑器 render 权威同步点把设置值同步到 `shared.show_folds` →
    /// `sync_fold` 重跑（install / set_folding(false)）。
    #[serde(default = "default_show_folds")]
    pub show_folds: bool,
    /// 是否软换行（默认开，与内核 `EditorState` 默认一致；关闭后长行横向滚动）。
    /// 即时生效：编辑器 render 权威同步点把设置值同步到内核 `set_soft_wrap`。
    #[serde(default = "default_soft_wrap")]
    pub soft_wrap: bool,
    /// 是否自动闭合括号 / 引号（默认开，与内核一致；关闭后输入 `(` 不再补 `)`）。
    /// 即时生效：编辑器 render 权威同步点同步内核 `set_auto_close`。
    #[serde(default = "default_auto_close")]
    pub auto_close: bool,
    /// 是否智能缩进（默认开；Enter 后按括号结构 / 语言规则选缩进）。
    /// 即时生效：编辑器 render 权威同步点同步内核 `set_smart_indent`。
    #[serde(default = "default_smart_indent")]
    pub smart_indent: bool,
    /// AI 补全（Inline Completion）开关——**当前版本置灰**（registry 登记 disabled），
    /// 内核槽位 `CompletionProvider::inline_completion` 未挂任何 AI 源，恒不产出建议。
    /// 默认关；未来接入 AI 源后放开 disabled，此值立即生效。
    #[serde(default = "default_inline_completion")]
    pub inline_completion: bool,
    /// 新文档默认缩进宽度（空格数；文档级「重设 ▾」可改单个文档）。
    #[serde(default = "default_tab_size")]
    pub default_tab_size: i64,
    /// 新文档默认是否用制表符缩进（false = 空格）。
    #[serde(default = "default_hard_tabs")]
    pub default_hard_tabs: bool,
    /// 新文档默认换行（`"LF"` / `"CRLF"`；落盘值，消费方按文本解析）。
    #[serde(default = "default_line_ending")]
    pub default_line_ending: String,
    /// 新文档默认编码（`"UTF-8"` / `"UTF-8-BOM"` / `"GBK"` / `"UTF-16LE"` / `"UTF-16BE"`；
    /// 落盘值，消费方按文本解析）。
    #[serde(default = "default_encoding")]
    pub default_encoding: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            show_line_numbers: default_show_line_numbers(),
            show_folds: default_show_folds(),
            soft_wrap: default_soft_wrap(),
            auto_close: default_auto_close(),
            smart_indent: default_smart_indent(),
            inline_completion: default_inline_completion(),
            default_tab_size: default_tab_size(),
            default_hard_tabs: default_hard_tabs(),
            default_line_ending: default_line_ending(),
            default_encoding: default_encoding(),
        }
    }
}

fn default_show_line_numbers() -> bool {
    true
}

/// 折叠默认开（2026-10-27：能力表置回 true 后，开关只控制是否显示，不关掉能力）
fn default_show_folds() -> bool {
    true
}

/// 软换行默认开（与内核 `EditorState::new` 的 `soft_wrap: true` 一致）
fn default_soft_wrap() -> bool {
    true
}

/// 自动闭合默认开（与内核 `LayoutMode::code_editor` 的 `auto_close: true` 一致）
fn default_auto_close() -> bool {
    true
}

/// 智能缩进默认开（与内核 `LayoutMode::code_editor` 的 `smart_indent: true` 一致）
fn default_smart_indent() -> bool {
    true
}

/// AI 补全默认关（当前版本置灰不可用；放开后接入 AI 源再改默认）
fn default_inline_completion() -> bool {
    false
}

fn default_tab_size() -> i64 {
    4
}

fn default_hard_tabs() -> bool {
    false
}

fn default_line_ending() -> String {
    "LF".to_string()
}

fn default_encoding() -> String {
    "UTF-8".to_string()
}

/// 主题模式：显式明暗，或跟随系统外观。
///
/// 落盘为 `"light"` / `"dark"` / `"system"`（serde snake_case）；`"system"` 在
/// 启动与系统外观变化时由装配层解析为实际明暗（`Theme::sync_system_appearance`），
/// 这里只记"选哪套策略"，不落实际模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    #[default]
    Light,
    Dark,
    System,
}

/// 连接默认值：新建连接时的超时与直连 TLS 策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDefaults {
    /// 建连超时（毫秒）：超过即判定本次尝试失败（会自动重试一次）。默认 15000。
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// 未配置 SSL 档案且目标为 LAN / 本机时，显式关闭 TLS（规避 sqlx 默认 `prefer`
    /// 握手卡顿）。默认开；公网 / 需要 TLS 的场景可关。
    #[serde(default = "default_true")]
    pub lan_disable_tls: bool,
}

impl Default for ConnectionDefaults {
    fn default() -> Self {
        Self {
            connect_timeout_ms: default_connect_timeout_ms(),
            lan_disable_tls: true,
        }
    }
}

impl gpui_kit::Global for Settings {}

/// 项目：项目名册的展示偏好（排序方式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projects {
    /// 项目列表排序方式（`last_opened` / `name` / `created`）。
    #[serde(default = "default_project_sort")]
    pub sort_mode: String,
}

impl Default for Projects {
    fn default() -> Self {
        Self {
            sort_mode: default_project_sort(),
        }
    }
}

/// 数据源导航的 facet 筛选（UI 偏好；`None` = 未启用）。
///
/// 这是**复合值**：在登记表里作为一项（`navigator.filters`）登记，子键不单独登记。
/// 归属域为唯一常驻 chips，其余为「筛选 ▾」弹层；`db_type` 存 `drivers.type_id`，
/// `driver` 存驱动 id，`tag` 存标签文本，`source` 存 `project` / `global` / `shared`。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NavigatorFilters {
    /// 归属域筛选（`project` / `global` / `shared`；`None` = 全部）。
    #[serde(default)]
    pub source: Option<String>,
    /// 数据库类型筛选（`drivers.type_id`）。
    #[serde(default)]
    pub db_type: Option<String>,
    /// 驱动 id 筛选。
    #[serde(default)]
    pub driver: Option<String>,
    /// 标签筛选。
    #[serde(default)]
    pub tag: Option<String>,
}

/// 数据源导航：来源标识展示形式、显示开关与属性面板宽度（UI 偏好，跨项目）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Navigator {
    /// 来源标识用短码（`P` / `G` / `GP`）；false 时用文字（项目 / 全局 / 共享）。
    #[serde(default = "default_true")]
    pub source_short_code: bool,
    /// 连接行是否显示标签 chip（默认关；开启后「≤2 chip + `+N`」）。
    #[serde(default)]
    pub show_tags: bool,
    /// 连接行是否显示归属域列（默认开）。
    #[serde(default = "default_true")]
    pub show_scope: bool,
    /// 面板是否显示来源筛选 chips 行（全部/项目/全局/共享，默认开）。
    #[serde(default = "default_true")]
    pub show_chips: bool,
    /// 面板是否显示搜索行（输入框 + 模式切换，默认开）。
    #[serde(default = "default_true")]
    pub show_search: bool,
    /// 连接名称前是否以 `[简称]` 显示类型简称（如 `[MY] 192.168.3.138`）。默认开。
    /// 简称来自 `type_short_map`（「类型简称映射…」弹窗维护），缺省回退徽标缩写。
    #[serde(default = "default_true")]
    pub show_name_type_short: bool,
    /// 数据库类型 → 简称 映射表（`type_id` → 简称）。默认同徽标缩写
    /// （PG/MY/MA/SQ/DK/MS/OR/CH/MG/RD/GH）；未登记类型回退类型名前 2 字母。
    #[serde(default)]
    pub type_short_map: std::collections::HashMap<String, String>,
    /// facet 筛选（类型 / 驱动 / 标签 / 归属域）。
    #[serde(default)]
    pub filters: NavigatorFilters,
    /// 已保存视图（命名 facet 快照，跨项目 UI 偏好）。
    ///
    /// 视图名为键（同名唯一、天然拦截）；`BTreeMap` 按名有序，子菜单与管理对话框
    /// 按此顺序展示。读写走导航面板自己的入口。
    #[serde(default)]
    pub saved_views: BTreeMap<String, NavigatorFilters>,
    /// 每连接的可见库白名单（S3-①）：`conn_id → 可见集合`。
    ///
    /// 键不存在 = 默认模式（树隐藏系统库 / 系统 schema）；键存在 = 白名单模式
    /// （原样展示集合里的名字，含系统对象）。集合里可同时含 catalog 名与 schema 名。
    /// 空集合 = 显式「全部隐藏」，等价于默认模式但对用户可见（弹框里全部取消勾选）。
    #[serde(default)]
    pub visible_databases: BTreeMap<String, Vec<String>>,
    /// 表/视图行是否显示行估计徽标（S3-④，默认开）：PG `reltuples` / MySQL
    /// `TABLE_ROWS` 后台回填，行尾「≈ N」弱色徽标；关闭后不排估计任务、不显示徽标。
    #[serde(default = "default_true")]
    pub row_estimate: bool,
    /// 表/视图行右侧统计徽标**整体**开关（需求④，默认开）：分区 / 继承 / ≈行估计 /
    /// 大小(B/KB) / 注释副标 一并开或关。关时即使驱动档案默认开也不显示；
    /// 开时仍受各驱动档案（`family_inline_profile`）的 show_size / show_description 门控。
    /// 「为何只有 PG 有」：分区/继承/大小/行估计由驱动档案 + 方言查询支持度共同决定
    /// （PG 档案默认开 size、方言支持行估计；SQLite/DuckDB 档案关 size 且不支持行估计）。
    #[serde(default = "default_true")]
    pub show_inline_stats: bool,
    /// 表行「注释副标」子开关（需求②，默认开）：总开关 `show_inline_stats` 开时仍受
    /// 此项门控；关时表/视图行的注释副标不显示（其余统计项不受影响）。
    #[serde(default = "default_true")]
    pub show_inline_comment: bool,
    /// 表行「大小徽标」子开关（需求②，默认开）：总开关开时仍受此项门控；关时
    /// 表行的 `≈ KB` 大小徽标不显示。「行数」子项由既有 `row_estimate` 独立承担。
    #[serde(default = "default_true")]
    pub show_inline_size: bool,
    /// 每连接的对象名 mask（B-1）：`conn_id → 规则`。
    ///
    /// 键不存在 = 该连接不过滤；键存在 = 按规则过滤**已加载**的对象行（表 / 视图 /
    /// 例程 / 序列 / 触发器，只作用于对象层，catalog / schema 层不进 mask）。
    /// 过滤是纯内存操作：只滤已加载进 `children` 的子集，`child_total`（分页判断）
    /// 保持原始候选计数不变。断连连接的缓存对象照常过滤（B-1-E）。
    #[serde(default)]
    pub object_masks: BTreeMap<String, ObjectMaskRule>,
    /// 面板全局对象名 mask（B-1）：对面板内全部可见连接的对象名兜底过滤。
    /// 空规则 = 不启用；与 per-conn mask、facet 均为 AND。
    #[serde(default)]
    pub global_mask: ObjectMaskRule,
}

/// 对象名过滤规则（B-1，glob / 正则双档，由 `use_regex` 切换）。
///
/// 匹配语义：include 为空 = 全部候选进入候选集；exclude 为空 = 不排除；**exclude 优先于
/// include**（命中 include 且未命中 exclude 的对象保留）。`use_regex=false` 走 glob
///（`*` 任意多字符 / `?` 单字符，大小写不敏感）；`use_regex=true` 走正则（`(?i)`
/// 大小写不敏感，编译失败条目视为永不命中）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ObjectMaskRule {
    /// include 模式列表（每行一条，空行忽略；空 = 全量候选；glob / 正则由 `use_regex` 决定）。
    #[serde(default)]
    pub include: Vec<String>,
    /// exclude 模式列表（每行一条，空行忽略；空 = 不排除；glob / 正则由 `use_regex` 决定）。
    #[serde(default)]
    pub exclude: Vec<String>,
    /// `true` = 正则匹配；`false` = glob 匹配。
    #[serde(default)]
    pub use_regex: bool,
}

impl ObjectMaskRule {
    /// 规则是否「启用」：include 或 exclude 任一非空（空规则 = 该层不过滤）。
    pub fn is_active(&self) -> bool {
        !self.include.is_empty() || !self.exclude.is_empty()
    }
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            source_short_code: true,
            show_tags: false,
            show_scope: true,
            show_chips: false,
            show_search: true,
            show_name_type_short: true,
            type_short_map: default_type_short_map(),
            filters: NavigatorFilters::default(),
            saved_views: BTreeMap::new(),
            visible_databases: BTreeMap::new(),
            row_estimate: true,
            show_inline_stats: true,
            show_inline_comment: true,
            show_inline_size: true,
            object_masks: BTreeMap::new(),
            global_mask: ObjectMaskRule::default(),
        }
    }
}

/// 一个已保存视图（访问器返回载体：`name` 来自 map 键，`filters` 是快照）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    /// 视图名（map 键，同名唯一）。
    pub name: String,
    /// 保存时的筛选快照（四项均可为 `None`；全空视图禁止保存）。
    pub filters: NavigatorFilters,
}

/// 类型简称映射默认值：与导航徽标缩写（`nav_type_badge`）同源，
/// 设置层独立持有种子（不依赖 database crate，依赖只向下）。
/// 需要新增类型时，同步维护 `primitives.rs::nav_type_badge` 与此表。
pub fn default_type_short_map() -> std::collections::HashMap<String, String> {
    [
        ("postgresql", "PG"),
        ("mysql", "MY"),
        ("mariadb", "MA"),
        ("sqlite", "SQ"),
        ("duckdb", "DK"),
        ("mssql", "MS"),
        ("oracle", "OR"),
        ("clickhouse", "CH"),
        ("mongodb", "MG"),
        ("redis", "RD"),
        ("github", "GH"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// 资产库（M6）：分析存档策略。
///
/// 与导航那种"显示偏好"不同，这里的值**会改变服务行为**（配额裁剪），所以它同时是
/// `ArchiveService` 的入参——设置 → 作业 → 服务一条线，转成领域类型的那一步在宿主侧
/// （`KeepVersions::from_setting`，见 `analytics_resource::model`）。
///
/// 保留的是**历史内容副本**：版本行（元数据）永远不删，界面上表现为"副本缺失"（架构 §5.2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    /// 每份存档保留的历史内容副本份数：`-1` = 全部保留（不裁剪），`0` = 只留版本元数据，
    /// `n ≥ 1` = 保留最近 n 份。默认 5。
    ///
    /// 用有符号数（而不是 `u32`）就是为了 `-1` 这个哨兵：它是 v1 与架构文档里已有的口径
    /// （`docs/architecture/analytics_resource/analytics-resource-architecture.md` §5.2）。
    #[serde(default = "default_keep_versions")]
    pub keep_versions: i64,
    /// 资产库列表的默认排序字段（`name` / `archived_at` / `updated_at` / `size` / `version`）。
    ///
    /// 面板里点一次排序就会写回这里（“默认排序” = 上次用的那个）——方向不入库，
    /// 由字段惯例给（名称升序，时间 / 大小 / 版本降序）。
    #[serde(default = "default_resource_sort")]
    pub default_sort: String,
    /// 分组折叠状态：**项目根路径 → 该项目里折叠的分组 key 列表**。
    ///
    /// 为什么要按项目分桶：分组 id 是每个项目自己生成的，平铺一份列表会在“打开另一个项目”
    /// 时被“清掉不认识的 id”那一步（面板会丢掉已删分组的折叠标记）抹掉别的项目的记录。
    /// 路径换名 / 移动后这一项会重新从空开始（视图状态，不值得为它做路径重写）。
    #[serde(default)]
    pub collapsed_groups: BTreeMap<String, Vec<String>>,
    /// 默认分组：**项目根路径 → 该项目的默认分组 id**（归档对话框预选它）。
    ///
    /// 为何按项目分桶：与 [`collapsed_groups`](Self::collapsed_groups) 同——分组 id 是
    /// 每项目自己生成的。为何**不跟随上次归档**（案 B）：默认分组回答的是“这个项目里我
    /// 习惯把产物放哪”（一份稳定设置），而不是“上次做了什么”；分组归属是内容属性，
    /// 猜错的代价与视图状态不对称（判据见 `analytics-resource-dev-plan.md` §4 的两案对比）。
    #[serde(default)]
    pub default_group: BTreeMap<String, String>,
}

impl Default for Resources {
    fn default() -> Self {
        Self {
            keep_versions: default_keep_versions(),
            default_sort: default_resource_sort(),
            collapsed_groups: BTreeMap::new(),
            default_group: BTreeMap::new(),
        }
    }
}

/// 日志：写文件 / 落库的级别门槛。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    /// 最低级别：低于它的记录既不写文件也不落库（stderr 同样按此过滤）。默认 `Info`。
    /// 排障时临时调成 `DEBUG`，比改环境变量重启省事。
    #[serde(default = "default_log_min_level")]
    pub min_level: LogMinLevel,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            min_level: default_log_min_level(),
        }
    }
}

/// 日志最低级别（与 `engine::logging::LogLevel` **同词表**）。
///
/// 为什么在这里独立定义而不是复用它：设置层不依赖 engine——引一个 `LogLevel`
/// 会把双引擎（duckdb / sqlx / russh）整个拖进设置页的依赖图。两侧的对应关系
/// 由装配层做一次文本转换（`LogLevel::parse_level`），词表对不上会在那里暴露。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogMinLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogMinLevel {
    /// 全部取值（登记表的枚举项、测试的穷尽遍历用）。
    pub const ALL: [LogMinLevel; 5] = [
        LogMinLevel::Trace,
        LogMinLevel::Debug,
        LogMinLevel::Info,
        LogMinLevel::Warn,
        LogMinLevel::Error,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LogMinLevel::Trace => "TRACE",
            LogMinLevel::Debug => "DEBUG",
            LogMinLevel::Info => "INFO",
            LogMinLevel::Warn => "WARN",
            LogMinLevel::Error => "ERROR",
        }
    }

    /// 解析落盘值（大小写不敏感；未知值返回 `None`，由调用方拒绝而不是猜）。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

fn default_log_min_level() -> LogMinLevel {
    LogMinLevel::Info
}

fn default_true() -> bool {
    true
}
fn default_connect_timeout_ms() -> u64 {
    15_000
}
fn default_project_sort() -> String {
    "last_opened".to_string()
}
fn default_keep_versions() -> i64 {
    5
}
fn default_resource_sort() -> String {
    "name".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧配置必须能解析：既缺新节（`navigator`），又带着已裁撤的节与字段
    /// （`general` / `engine` / `connection_defaults.default_driver`）。
    ///
    /// `appearance.font_size` 是**复活字段**（v7 曾裁撤，2026-10-11 按「字号缩放
    /// 进设置」决策复活）：旧配置里的 `13.0` 现在会直接生效（不再被忽略）。
    /// 裁撤是向后兼容安全的：`Settings` 未开 `deny_unknown_fields`，未知内容被忽略。
    #[test]
    fn legacy_config_with_removed_sections_still_loads() {
        let legacy = r#"{
            "general":{"language":"zh-CN","restore_last_workspace":true},
            "appearance":{"theme_mode":"dark","font_size":13.0},
            "engine":{"workspace_dir":"D:/ws","cache_dir":"D:/cache"},
            "connection_defaults":{"default_driver":"duckdb","query_timeout_ms":15000}
        }"#;
        let s: Settings = serde_json::from_str(legacy).expect("旧配置应可解析");

        // 仍在用的字段按文件里的值生效
        assert_eq!(s.appearance.theme_mode, AppearanceMode::Dark);
        // 复活字段：旧配置的数字字号直接生效（不再是"被忽略"）。
        assert_eq!(s.appearance.font_size, 13.0);
        // 裁撤的节 / 字段回退默认，不留残影
        assert_eq!(s.connection_defaults.connect_timeout_ms, 15_000);
        assert!(s.connection_defaults.lan_disable_tls);
        assert!(s.navigator.source_short_code);
        // v7 新增：标签默认不显、归属域列默认显。
        assert!(!s.navigator.show_tags, "标签默认不显示");
        assert!(s.navigator.show_scope, "归属域列默认显示");
        // v7 facet 筛选默认全空。
        assert!(s.navigator.filters.source.is_none());
        assert!(s.navigator.filters.db_type.is_none());
        assert!(s.navigator.filters.driver.is_none());
        assert!(s.navigator.filters.tag.is_none());
    }

    /// facet 筛选可序列化往返（`None` 不被写成显式 null 之外的异常形态）。
    #[test]
    fn navigator_filters_roundtrip() {
        let mut s = Settings::default();
        s.navigator.filters = NavigatorFilters {
            source: Some("global".into()),
            db_type: Some("postgres".into()),
            driver: None,
            tag: Some("prod".into()),
        };
        let text = serde_json::to_string(&s).expect("序列化");
        let back: Settings = serde_json::from_str(&text).expect("反序列化");
        assert_eq!(back.navigator.filters.source.as_deref(), Some("global"));
        assert_eq!(back.navigator.filters.db_type.as_deref(), Some("postgres"));
        assert!(back.navigator.filters.driver.is_none());
        assert_eq!(back.navigator.filters.tag.as_deref(), Some("prod"));
    }

    /// 已保存视图：序列化往返 + 空对象默认回退（旧配置无此节时不报错）。
    #[test]
    fn saved_views_roundtrip() {
        let mut s = Settings::default();
        s.navigator.saved_views.insert(
            "生产库".into(),
            NavigatorFilters {
                source: Some("project".into()),
                db_type: Some("postgres".into()),
                driver: None,
                tag: Some("prod".into()),
            },
        );
        let text = serde_json::to_string(&s).expect("序列化");
        let back: Settings = serde_json::from_str(&text).expect("反序列化");
        let view = back
            .navigator
            .saved_views
            .get("生产库")
            .expect("视图名是键");
        assert_eq!(view.source.as_deref(), Some("project"));
        assert_eq!(view.tag.as_deref(), Some("prod"));

        // 旧配置（无 saved_views 节）回退空 map，不 panic。
        let legacy: Settings = serde_json::from_str("{}").expect("旧配置可读");
        assert!(legacy.navigator.saved_views.is_empty());
    }

    /// 事实档载体默认 `auto`；旧配置（无 `result_panel` 节）回退默认，不报错。
    #[test]
    fn fact_storage_defaults_to_auto() {
        assert_eq!(Settings::default().result_panel.fact_storage, FactStoragePref::Auto);

        let legacy: Settings = serde_json::from_str("{}").expect("旧配置可读");
        assert_eq!(legacy.result_panel.fact_storage, FactStoragePref::Auto);
    }

    /// 三档序列化往返；落盘文本为 snake_case（auto / memory / table）。
    #[test]
    fn fact_storage_roundtrip_three_tiers() {
        for pref in FactStoragePref::ALL {
            let mut settings = Settings::default();
            settings.result_panel.fact_storage = pref;
            let text = serde_json::to_string(&settings).expect("序列化");
            let back: Settings = serde_json::from_str(&text).expect("反序列化");
            assert_eq!(back.result_panel.fact_storage, pref);
            assert!(text.contains(&format!(r#""fact_storage":"{}""#, pref.as_str())));
        }
    }

    /// 三档映射到决策层 `shared::StoragePref`；未知落盘值拒绝解析，不猜。
    #[test]
    fn fact_storage_maps_to_shared_and_rejects_unknown() {
        assert_eq!(FactStoragePref::Auto.to_shared(), ::shared::StoragePref::Auto);
        assert_eq!(FactStoragePref::Memory.to_shared(), ::shared::StoragePref::Memory);
        assert_eq!(FactStoragePref::Table.to_shared(), ::shared::StoragePref::Table);

        assert!(FactStoragePref::parse("disk").is_none());
        assert!(FactStoragePref::parse("").is_none());
        for pref in FactStoragePref::ALL {
            assert_eq!(FactStoragePref::parse(pref.as_str()), Some(pref));
        }
    }
}

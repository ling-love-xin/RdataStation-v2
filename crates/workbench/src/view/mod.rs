//! RdataStation v2 工作台视图（WorkbenchView）— v5 布局
//!
//! 按 GPUI-kit 编码规范搭建 DockArea 可拖拽布局工作台：
//! - 依赖只向下：视图只使用 gpui-kit（gpui / base / component），不承载业务逻辑；
//! - 窗口第一层由 App Shell 的 `Root` 装配；本视图负责标题栏 + 左右活动栏 + DockArea + 状态栏；
//! - 左 Dock：草稿箱 / 数据库导航 / 资源分析 / 插件；右 Dock：洞察 / Mock 生成 / 历史；
//! - 边栏三模式：展开 / 收起（`toggle_dock`）/ 完全隐藏（`remove_dock`，gpui-kit 0.7.1）；
//!   模式状态存于 `Shared`，`render` 统一同步到 Dock（权威同步点）；
//! - Quick Open：搜索 + 命令融合（标题栏居中入口 / Ctrl+P），受控自绘弹层；
//! - 颜色一律取自 `cx.theme().colors`（禁止 raw hex/rgb）。

use std::rc::Rc;

use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::dialog::{Dialog, DialogButtonProps, DialogContent};
use gpui_kit::component::dock::{
    DockArea, DockLayout, DockPlacement, DockSkin, PanelId, PanelStyle, panel_handle,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::sidebar::{Sidebar, SidebarCollapsible, SidebarMenu, SidebarMenuItem};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::TabVariant;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Side, TitleBar};
use gpui_kit::*;

use scratchpad::scratchpad_view::render_scratchpad_file_info_pane;

use insight::InsightWorkspacePanel;

use crate::commands::{
    CloseProject, FocusNavSearch, HideSidebars, RestoreSidebars, SwitchProject, ToggleQuickGoto,
    ToggleQuickOpen,
};
use crate::components::editor_host::WorkbenchEditorHost;
use crate::panels::{
    ArchivePanel, DatabaseNavPanel, EditorPanel, HistoryPanelTab, InsightPanelTab, MockPanelTab,
    PluginManagerPanel, PluginPanel, ProjectActionRequest, PropertyRequest, QueryRequest,
    ResourcePanelTab, RightSidebarPanel, ScratchPanel, Shared, SidebarPanel,
};
use crate::quick_open::model::Mode;
use crate::quick_open::palette::QuickOpenPalette;
use crate::services::editor_completion::WorkbenchCompletion;
use crate::services::editor_lsp::{LspBlock, WorkbenchLsp};
use crate::ui;
use mock::mock_view::{
    JobBarSize, MockDetailView, MockPanel, eta_text_compact, focus_detail_tab, job_progress_bar,
};
use settings::SettingsService;
use settings::commands::{OpenSettings, ToggleThemeMode};
use settings::settings_page::{SettingsHost, SettingsPage};

/// 左侧活动栏面板。
///
/// `LeftPanel` / `RightPanel` / `SidebarMode` / `ConnectionItem` 已下沉外壳 crate
/// `workbench_shell::model`（视图下沉的前置：特性 crate 也要能命名它们）：
/// 这里重导保持 `crate::view::X` / `rds_workbench::X` 路径不变。
/// 三模式状态机 {@link toggle_hidden_mode} 仍在本文件（属逻辑，不属数据）。
pub use workbench_shell::model::{ConnectionItem, LeftPanel, RightPanel, SidebarMode};

/// 工作台视图：标题栏 + 左右活动栏 + DockArea（左/右/中央）+ 状态栏。
pub struct WorkbenchView {
    /// 工作台共享状态（`pub(crate)`：Quick Open 宿主装配在 `quick_open/host.rs`）。
    pub(crate) shared: Shared,
    /// DockArea 实体（render 首次调用时懒初始化，需要 &mut Window）。
    area: Option<Entity<DockArea>>,
    sidebar: Option<Entity<SidebarPanel>>,
    editor: Option<Entity<EditorPanel>>,
    right_sidebar: Option<Entity<RightSidebarPanel>>,
    // 真面板化（左右 dock 模块 tab 薄壳）：每个模块 = 独立 dock tab。
    // 壳持有内层 crate 视图实体（经 sidebar / right_sidebar 的访问器取），
    // dock 自动生成 tab 条 → 同面切换 / 拖拽分栏；活动栏点击走 `select_*_panel`。
    left_nav_tab: Option<Entity<DatabaseNavPanel>>,
    left_scratch_tab: Option<Entity<ScratchPanel>>,
    left_resource_tab: Option<Entity<ResourcePanelTab>>,
    left_plugin_tab: Option<Entity<PluginPanel>>,
    /// 插件管理页（中央常驻 tab，单实例；左 Dock「管理」按钮打开 / 聚焦）。
    plugin_manager: Option<Entity<PluginManagerPanel>>,
    right_insight_tab: Option<Entity<InsightPanelTab>>,
    right_mock_tab: Option<Entity<MockPanelTab>>,
    right_history_tab: Option<Entity<HistoryPanelTab>>,
    right_archive_tab: Option<Entity<ArchivePanel>>,
    /// Quick Open 浮层实体（首帧渲染时懒创建）；浮层自持输入 / 结果 / 键盘通道。
    /// ：宿主装配在 （兄弟模块，非本模块子级）。
    pub(crate) quick_open_palette: Option<Entity<QuickOpenPalette>>,
    /// 打开后待办：通知浮层去清输入 / 落选中 / 聚焦（浮层可能上一帧才创建）。
    pub(crate) quick_open_open_pending: bool,
    /// 本次打开待生效的浮层档位（`Default` = Ctrl+P，`Goto` = Ctrl+Shift+O）。
    pub(crate) quick_open_mode_pending: Mode,
    /// 设置页实体（首次打开时懒创建）。
    /// 设置页独立窗口句柄（`None` = 未打开；窗口关闭后 `update` 返回 Err，据此判活）。
    settings_window: Option<AnyWindowHandle>,
    /// M1 项目管理输入实体（懒创建）。
    project_inputs: Option<project::ui::ProjectInputs>,
    /// M1 项目选择器面板（中央 tab 组常驻 tab；强持有防 GC，见 `project_picker.rs`）。
    project_picker: Option<Entity<crate::panels::ProjectPickerPanel>>,
    /// M1 项目视图宿主（构造期组装；项目视图位于 `project` crate）。
    project_host: Option<project::ui::ProjectUiHost>,
    /// 订阅句柄（保持连接选中事件的订阅存活）。
    /// 编辑面板观察句柄（其 notify 级联到宿主，保证对话框层内容同步）。
    _editor_subscription: Option<Subscription>,
    /// 资产库面板观察句柄（M6：左栏选中 / 数据变化 → 唤醒右栏「存档详情」）。
    ///
    /// 观察的是**面板实体**而不是 `SidebarPanel`：子实体的 `notify` 不级联到父面板，
    /// 订在侧栏上一条都收不到。句柄必须被持有——drop 即解除观察。
    _archive_subscription: Option<Subscription>,
    /// M8 洞察**中间工作页**的取数 / 动作订阅句柄（`jobs::attach` +
    /// `insight_actions::attach` + 右栏联动 observe；面板懒创建时建立一次）。
    ///
    /// 面板实体本身只经 `Shared::insight_workspace_panel` 弱句柄持有：tab ✕ 关闭后
    /// 实体随 Dock 移除而 drop，下次入口自动重建（与 search_results 同例）。
    _insight_ws_subs: Vec<Subscription>,
    /// M8 洞察规则目录监听句柄（改动 `.rule.toml` 后自动重载规则集）。
    ///
    /// 句柄必须**被持有**：它是 RAII 语义——drop 即停后台线程，
    /// 因此不能建成临时值（否则会在行尾被回收，监听静默失效）。
    /// 前缀下划线表示「仅为持有，不读取」，与 `_subscription` 同一约定。
    _insight_rules_watcher: Option<insight::RulesWatcher>,
    /// SQL 编辑器的文档集合（跨面板共享；一个面板 = 一个标签 = 一份文档）。
    ///
    /// 放在 workbench 是因为它是**装配宿主**：`crates/editor` 不依赖本 crate，
    /// 由宿主把服务句柄交给各面板，并把它接进中央 Dock 的 tab 组。
    editor_service: editor::shared::EditorShared,
    /// 已开的编辑面板（按 `DocumentId` 复用：同文档不重复建面板）。
    editor_hosts: Vec<Entity<editor::view::host::EditorHostPanel>>,
    /// 编辑器面板状态变化订阅（#47 状态栏上抛：host 发 `EditorHostEvent::StatusChanged`
    /// 时宿主重绘，全局状态栏活动文档上下文段随之刷新）。句柄必须被持有，drop 即失效；
    /// 面板关闭后订阅随实体 drop 失效，Vec 里留着无害（文档数有限）。
    _editor_status_subs: Vec<Subscription>,
    /// 草稿执行回执 → 元数据回写泵（1 s 一拍；句柄必须被持有，drop 即停）。
    scratchpad_meta_pump: Option<Task<()>>,
    /// 【#54 B】语义诊断目录快照注入泵（1 s 一拍；句柄必须被持有，drop 即停）。
    /// 复用 [`editor_completion`] 的 L1 目录：补全端口实例由本字段持有，
    /// 泵每拍读「活动文档绑定连接 → 推快照」（见 `services/editor_catalog.rs`）。
    catalog_pump: Option<Task<()>>,
    /// 【#54 B】补全端口实例（`editor_completion::attach` 的返回值；目录快照唯一来源）。
    editor_completion: Option<Rc<WorkbenchCompletion>>,
    /// 【LSP】语言服务端口句柄（状态栏 LSP 管理器持有它做启停 / 回收）；
    /// 测试装配 / 未启用 = `None`（状态栏不摆这一项）。
    lsp: Option<Rc<WorkbenchLsp>>,
    /// 【LSP】空闲回收 + 状态栏指示刷新泵（1 s 一拍；句柄必须被持有，drop 即停）。
    lsp_reaper: Option<Task<()>>,
    /// 【LSP】上一次看到的端口状态版本（不同则 `notify` 重绘状态栏指示）。
    last_lsp_epoch: u64,
    /// Dock 皮肤句柄（`init_workspace` 装配时持有；设置页「标签栏最大宽度 / 样式」
    /// 变更经 `on_settings_changed` 应用到这里，渲染读取点在 vendor `dock/tab_panel.rs`）。
    dock_skin: Option<Rc<DockSkin>>,
}

mod actions;
mod dock;
mod lifecycle;
mod render;
mod status;
mod workspace;

pub(crate) use status::restore_snapshot;
pub use status::{lsp_status_chip, toggle_hidden_mode};

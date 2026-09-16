//! 面板/宿主共用的纯数据模型。
//!
//! 为什么住在 shell 而不是 workbench：这些类型是**视图与宿主的公共词汇**——
//! 活动栏面板枚举（布局状态机用）、边栏三模式、连接条目（导航树 / 连接详情 / 缓存对话框都用）。
//! 一旦视图下沉到特性 crate（`database` 的导航视图、`scratchpad` 的草稿箱视图），
//! 它们必须能被**两侧同时命名**；放在 workbench 里会让特性 crate 反向依赖 workbench（成环）。
//!
//! 约束：本模块只放**纯数据 + 纯展示助手**（`label` / `icon`），不放业务逻辑与 I/O。
//! workbench 侧通过 `crate::view::{...}` 重导，路径保持不变。

use gpui_kit::component::Icon;

/// 左侧活动栏面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftPanel {
    /// 草稿箱（M5 草稿能力占位）
    Draft,
    /// 数据库导航（M4，数据源连接 + 对象树）
    Database,
    /// 资源分析（M6）
    Resources,
    /// 插件（M9）
    Plugin,
}

/// 右侧活动栏面板。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    /// 洞察（M8）
    Insight,
    /// Mock 数据生成（M7）
    Mock,
    /// 历史（查询历史）
    History,
}

/// 边栏三模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    /// 展开：活动栏 + 边栏可见
    Expanded,
    /// 收起：边栏隐藏、活动栏保留（dock 保留、离屏）
    Collapsed,
    /// 完全隐藏：活动栏 + 边栏均隐藏（dock 移除）
    Hidden,
}

impl LeftPanel {
    pub const ALL: [LeftPanel; 4] = [
        LeftPanel::Draft,
        LeftPanel::Database,
        LeftPanel::Resources,
        LeftPanel::Plugin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftPanel::Draft => "草稿箱",
            LeftPanel::Database => "数据库导航",
            LeftPanel::Resources => "资产库",
            LeftPanel::Plugin => "插件",
        }
    }

    /// 活动栏图标（Lucide）：默认图标集（`IconName`）不含这些语义图标，
    /// 应用已注册 `AllAssets`（全量目录），故按资产路径直接引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            LeftPanel::Draft => "icons/notebook-text.svg",
            LeftPanel::Database => "icons/database.svg",
            LeftPanel::Resources => "icons/chart-column.svg",
            LeftPanel::Plugin => "icons/puzzle.svg",
        };
        Icon::default().path(path)
    }
}

impl RightPanel {
    pub const ALL: [RightPanel; 3] = [RightPanel::Insight, RightPanel::Mock, RightPanel::History];

    pub fn label(self) -> &'static str {
        match self {
            RightPanel::Insight => "洞察",
            RightPanel::Mock => "Mock 生成",
            RightPanel::History => "历史",
        }
    }

    /// 活动栏图标（Lucide），与左侧同样按资产路径引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            RightPanel::Insight => "icons/lightbulb.svg",
            RightPanel::Mock => "icons/dice-5.svg",
            RightPanel::History => "icons/clock.svg",
        };
        Icon::default().path(path)
    }
}

/// 连接条目（Round 21 起由全局系统库真实数据填充；Round 22 扩展完整元数据）。
#[derive(Debug, Clone)]
pub struct ConnectionItem {
    pub id: String,
    pub name: String,
    pub driver: String,
    /// 运行时是否已连接（连接管理器维护，由加载器填充；记录有效性由持久化层过滤）。
    pub connected: bool,
    /// 真实元数据（Round 22）：主机 / 端口 / 数据库 / Schema。
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub description: Option<String>,
    /// DuckDB 联邦（本地加速）开关。
    pub use_duckdb_fed: bool,
    pub created_at: String,
    pub updated_at: String,
}

//! 面板/宿主共用的纯数据模型。
//!
//! 为什么住在 shell 而不是 workbench：这些类型是**视图与宿主的公共词汇**——
//! 活动栏面板枚举（布局状态机用）、边栏三模式、连接条目（导航树 / 连接详情 / 缓存对话框都用）。
//! 一旦视图下沉到特性 crate（`database` 的导航视图、`scratchpad` 的草稿箱视图），
//! 它们必须能被**两侧同时命名**；放在 workbench 里会让特性 crate 反向依赖 workbench（成环）。
//!
//! 约束：本模块只放**纯数据 + 纯展示助手**（`label` / `icon` / 数据换算），不放业务逻辑与 I/O。
//! workbench 侧通过 `crate::view::{...}` 重导，路径保持不变。
//!
//! 命名空间分工：左侧活动栏面板枚举（布局状态机用）、边栏三模式、连接条目、
//! 以及**宿主与特性视图都要命名的纯数据**（`QueryRequest` / `GroupFormSeed`）。
//!
//! 关于 `GroupFormSeed`：分组表单对话框归 workbench，但**初值类型**同时被导航视图
//! （下沉后的 `database`）与项目对话框（workbench）构造，故定义在此。
//! 为保持本 crate 不依赖 `engine`，`for_existing` 按字段逐个传入，不收分组行类型。

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
    /// 存档详情（M6：选中存档的归档凭证）
    Archive,
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
    pub const ALL: [RightPanel; 4] = [
        RightPanel::Insight,
        RightPanel::Mock,
        RightPanel::History,
        RightPanel::Archive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RightPanel::Insight => "洞察",
            RightPanel::Mock => "Mock 生成",
            RightPanel::History => "历史",
            RightPanel::Archive => "存档详情",
        }
    }

    /// 活动栏图标（Lucide），与左侧同样按资产路径引用。
    pub fn icon(self) -> Icon {
        let path = match self {
            RightPanel::Insight => "icons/lightbulb.svg",
            RightPanel::Mock => "icons/dice-5.svg",
            RightPanel::History => "icons/clock.svg",
            RightPanel::Archive => "icons/file-text.svg",
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

/// 导航 → 中央编辑器的「打开查询」请求（B11）。
///
/// 与 `open_file_request` 同一口径：生产端（导航菜单 / 拖拽 / 后台回填）**拿不到 `Window`**
/// （`SidebarEvent` 订阅回调、`apply_sql_results` 都只有 `Context`），而开文档与写内核都要窗口，
/// 因此这里只入队，由宿主 `render` 消费。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    /// 要绑定的连接（`None` = 不绑定，跟随当前活动连接）
    pub conn_id: Option<String>,
    /// 要打开的 SQL（空 = 只开一份空白查询）
    pub sql: String,
    /// 打开后是否立即执行：「查看数据」为真（M4 遗留的“查看数据不自动执行”在此关闭），
    /// 「生成 SQL」模板为假（只给草稿，不该替用户跑写语句）
    pub run: bool,
}

/// 分组表单初值（新建 / 编辑分组对话框的入参）。
#[derive(Clone, Debug)]
pub struct GroupFormSeed {
    /// 编辑的分组 ID（`None` = 新建）。
    pub id: Option<String>,
    /// 名称初值。
    pub name: String,
    /// 描述初值（`None` / 空白都视为空）。
    pub description: Option<String>,
}

impl GroupFormSeed {
    /// 新建：`default_name` 建议给自动去重后的默认名（避免空表单与空名校验）。
    pub fn for_new(default_name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: default_name.into(),
            description: None,
        }
    }

    /// 编辑现有分组（字段逐个传入：本 crate 不依赖 `engine` 的分组行类型）。
    pub fn for_existing(
        id: impl Into<String>,
        name: impl Into<String>,
        description: Option<String>,
    ) -> Self {
        Self {
            id: Some(id.into()),
            name: name.into(),
            description,
        }
    }
}

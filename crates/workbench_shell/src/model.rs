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

    /// 落盘值 → 面板（设置项 `startup.default_left_panel` 的取值）。
    ///
    /// 未知值返回 `None`，由调用方回退默认——设置文件被手改坏时不会崩。
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "draft" => LeftPanel::Draft,
            "database" => LeftPanel::Database,
            "resources" => LeftPanel::Resources,
            "plugin" => LeftPanel::Plugin,
            _ => return None,
        })
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
            RightPanel::Insight => "icons/chart-line.svg",
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

// ===== 编辑器 / 结果面板共享模型（2026-10-23 自 rds-editor 下移） =====
//
// 这些类型同时被编辑器宿主与（下沉后的）结果面板 crate 命名，住在 shell 才能让两侧
// 依赖方向都只向下；editor 侧经重导出保持 `crate::model::DocumentId` 等旧路径不变。

/// 文档 / 笔记的稳定标识
///
/// **一经生成永不变**，也不得用路径或下标替代：v1 用 `filePath` 派生面板 id，另存为后
/// 新旧 key 失配（tab 找不到、结果错位），是"用路径当身份"的经典代价。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 执行通道（三者互斥，默认源库直连）
///
/// 决定能力边界（原型 §5.7）：源库直连可写源库对象 / 有事务；本地加速 / 联邦是
/// 只读挂载（写语句被拒、无事务语义）。UI 载体 = 工具栏「执行位置 ▾」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecChannel {
    /// 源库直连（实时数据；写语句与事务都在这档）
    #[default]
    Source,
    /// DuckDB 本地加速（`ATTACH` 源库；**只读挂载**，读的是源库的实时数据）
    Accelerated,
    /// 联邦查询（DuckDB + 多个外部源）
    Federated,
}

impl ExecChannel {
    pub const ALL: [ExecChannel; 3] = [Self::Source, Self::Accelerated, Self::Federated];

    /// 界面文案（菜单项 / 状态栏 / 徽标共用同一套词）
    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "源库",
            Self::Accelerated => "本地加速",
            Self::Federated => "联邦",
        }
    }

    /// 结果集标签上的徽标短码（原型 §5.4：`源库` / `加速` / `联邦`）
    ///
    /// 与 [`Self::label`] 不同：徽标挤在标签里，用两字短码。
    pub fn badge(self) -> &'static str {
        match self {
            Self::Source => "源库",
            Self::Accelerated => "加速",
            Self::Federated => "联邦",
        }
    }

    /// 作用**源库对象**的写语句 / DDL 允许吗（本地副本只读）
    pub fn allows_source_writes(self) -> bool {
        matches!(self, Self::Source)
    }

    /// 事务允许吗（加速 / 联邦没有事务语义）
    pub fn allows_transactions(self) -> bool {
        matches!(self, Self::Source)
    }

    /// 语句是不是在本机引擎（DuckDB）上跑的
    ///
    /// 它与“数据新鲜度”无关——加速档读的也是源库的实时数据（切片二实测），
    /// 真正被它决定的是**能力边界**：写语句落在源库对象上时会被拒（只读挂载），
    /// 事务没有意义（DuckDB 侧不参与源库事务）。
    pub fn runs_locally(self) -> bool {
        !matches!(self, Self::Source)
    }

    /// 拒绝写语句时给用户的话（原型 §5.7 的口径）
    pub fn write_refusal(self) -> &'static str {
        self.label()
    }

    /// 写拒绝的完整理由（两档共用一句话：**外部源只读**）
    ///
    /// 特意不写“源库是只读挂载的”：那对 L2（Oracle 这类扫描器不支持 `READ_ONLY`）不成立——
    /// 对用户的结论一样（写不进去），但机制不同（会话层拒），文案不能说得比实现更具体。
    pub fn write_refusal_reason(self) -> String {
        format!(
            "{}通道不能写源库（外部源只读），请切回源库再试",
            self.write_refusal()
        )
    }

    /// 从持久化的短码还原（认不出回源库——不假装记住了别的）
    pub fn from_code(code: &str) -> Self {
        match code {
            "accelerated" => Self::Accelerated,
            "federated" => Self::Federated,
            _ => Self::Source,
        }
    }

    /// 持久化用的短码
    pub fn code(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Accelerated => "accelerated",
            Self::Federated => "federated",
        }
    }
}

/// 【P1】一列的元数据注释（结果集字段注释；`None` = 该列没有来源列 / 表列元数据缺失）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnAnnotation {
    /// 来源表（限定名 `schema.表` 或裸名；表达式列无来源 = `None`）
    pub source_table: Option<String>,
    /// 来源列名（表达式列无来源 = `None`）
    pub source_column: Option<String>,
    /// 字段注释（NavCache 列元数据；空 = 无注释）
    pub comment: Option<String>,
    /// 列类型（NavCache 的 `data_type`，驱动原始名；区别于渲染层的类型标签）
    pub data_type: Option<String>,
    /// 是否可空
    pub nullable: Option<bool>,
    /// 默认值
    pub default_value: Option<String>,
}

#[cfg(test)]
mod activity_icon_tests {
    use super::*;

    /// 右活动栏每个面板的图标必须真的在资产里能取到——
    /// 漏了就是渲染成空白，用户看不到入口（曾因 `icons/lightbulb.svg` 不存在
    /// 导致洞察图标空白，用户误以为模块未接入）。
    #[test]
    fn right_activity_bar_icons_resolve_in_the_asset_source() {
        for panel in RightPanel::ALL {
            // Icon 结构不暴露 path，这里用 label 反查路径表（与 icon() 的 match 同构）
            let path = match panel {
                RightPanel::Insight => "icons/chart-line.svg",
                RightPanel::Mock => "icons/dice-5.svg",
                RightPanel::History => "icons/clock.svg",
                RightPanel::Archive => "icons/file-text.svg",
            };
            assert!(
                gpui_kit::assets::AllAssets::get(path).is_some(),
                "右活动栏图标资产里没有 {path}（面板 {panel:?}）"
            );
        }
    }
}

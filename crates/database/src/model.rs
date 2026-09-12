//! rds-database — 导航领域模型（M4）
//!
//! 面向 GPUI 视图的导航树模型：节点携带稳定 key（用于展开态持久化）、
//! 来源标识与懒加载状态；与 engine `SchemaObject` / `ColumnDetail` 解耦，
//! 由 `navigator_service` 负责映射。
//!
//! 设计对应 `docs/architecture/database/database-navigator-prototype-design.md`：
//! - 来源短码 `P` / `G` / `GP`（§2.3）
//! - 分组多对多 + 标签多值（§2.2）
//! - 导航状态持久化（§6.4）

use serde::{Deserialize, Serialize};

/// 连接来源标识（决定可见性与生命周期）。
///
/// 与 engine `id_prefix` 一一对应：`P_`/`G_`/`GP_`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavSource {
    /// 项目连接（`P_`，仅当前项目可见）
    Project,
    /// 全局连接（`G_`，所有项目可见）
    Global,
    /// 项目引用全局快照（`GP_`，共享至当前项目）
    Shared,
}

impl Default for NavSource {
    fn default() -> Self {
        Self::Project
    }
}

impl NavSource {
    /// 来源短码（UI 默认展示，§2.3）
    pub fn code(self) -> &'static str {
        match self {
            Self::Project => "P",
            Self::Global => "G",
            Self::Shared => "GP",
        }
    }

    /// 来源中文标签（短码⇄文字开关时使用）
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "项目",
            Self::Global => "全局",
            Self::Shared => "共享",
        }
    }

    /// 从连接 ID 前缀解析来源。
    pub fn from_conn_id(conn_id: &str) -> Self {
        if conn_id.starts_with("GP_") {
            Self::Shared
        } else if conn_id.starts_with("G_") {
            Self::Global
        } else {
            Self::Project
        }
    }
}

/// 类别文件夹（schema 下的对象分组）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavFolder {
    Tables,
    Views,
    Routines,
    Sequences,
    Triggers,
}

impl NavFolder {
    /// 文件夹标题。
    pub fn label(self) -> &'static str {
        match self {
            Self::Tables => "表",
            Self::Views => "视图",
            Self::Routines => "存储过程 / 函数",
            Self::Sequences => "序列",
            Self::Triggers => "触发器",
        }
    }

    /// 持久化 key 片段。
    pub fn key(self) -> &'static str {
        match self {
            Self::Tables => "tables",
            Self::Views => "views",
            Self::Routines => "routines",
            Self::Sequences => "sequences",
            Self::Triggers => "triggers",
        }
    }

    /// 全部文件夹（固定顺序）。
    pub const ALL: [NavFolder; 5] = [
        NavFolder::Tables,
        NavFolder::Views,
        NavFolder::Routines,
        NavFolder::Sequences,
        NavFolder::Triggers,
    ];
}

/// 导航节点类型（携带展示所需的最小数据）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NavNodeKind {
    /// 连接（数据源）根节点
    Connection {
        source: NavSource,
        driver: String,
        connected: bool,
    },
    /// Catalog（数据库）
    Catalog,
    /// Schema
    Schema,
    /// 类别文件夹
    Folder(NavFolder),
    /// 表
    Table { row_estimate: Option<u64> },
    /// 视图
    View,
    /// 列
    Column {
        data_type: String,
        nullable: bool,
        primary: bool,
        foreign: bool,
    },
    /// 存储过程 / 函数
    Routine { routine_type: String },
    /// 序列
    Sequence,
    /// 触发器
    Trigger,
}

/// 导航树节点。
///
/// `children` 为空且 `loaded=false` 表示尚未懒加载；`error` 承载可读失败原因
/// （不静默吞噬，v1 V10.8 教训）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavNode {
    /// 稳定唯一 key（展开态持久化 / 树 diff 用）
    pub key: String,
    /// 展示名
    pub name: String,
    /// 归属连接 ID
    pub connection_id: String,
    /// 节点类型
    pub kind: NavNodeKind,
    /// 注释（表/列注释等）
    pub comment: Option<String>,
    /// 是否可展开（有子节点）
    pub has_children: bool,
    /// 子节点（懒加载结果）
    pub children: Vec<NavNode>,
    /// 子节点是否已加载
    pub loaded: bool,
    /// 加载失败原因（有值时展示错误占位 + 重试）
    pub error: Option<String>,
    /// 展开该节点时要加载子节点的路径（连接根为 `Connection`；列节点为 None）
    pub expand_path: Option<NavPath>,
    /// 属性面板定位信息（双击 / 右键「查看属性」时使用）
    pub property: Option<PropertyRef>,
}

impl NavNode {
    /// 构造导航节点（子节点默认未加载）。
    pub fn new(
        key: impl Into<String>,
        name: impl Into<String>,
        connection_id: impl Into<String>,
        kind: NavNodeKind,
        has_children: bool,
    ) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            connection_id: connection_id.into(),
            kind,
            comment: None,
            has_children,
            children: Vec::new(),
            loaded: false,
            error: None,
            expand_path: None,
            property: None,
        }
    }

    /// 带注释。
    pub fn with_comment(mut self, comment: Option<String>) -> Self {
        self.comment = comment;
        self
    }

    /// 指定展开时的子节点加载路径。
    pub fn with_expand_path(mut self, path: NavPath) -> Self {
        self.expand_path = Some(path);
        self
    }

    /// 指定属性面板定位信息。
    pub fn with_property(mut self, property: PropertyRef) -> Self {
        self.property = Some(property);
        self
    }

    /// 生成子节点 key：`{conn}/{seg1}/{seg2}/...`。
    pub fn child_key(connection_id: &str, segments: &[&str]) -> String {
        if segments.is_empty() {
            connection_id.to_string()
        } else {
            format!("{}/{}", connection_id, segments.join("/"))
        }
    }
}

/// 加载路径（描述「要加载谁的子节点」）。
///
/// 由视图根据被展开节点构造，服务据此选择内省调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NavPath {
    /// 连接根：加载 catalog
    Connection,
    /// catalog：加载 schema
    Catalog { catalog: String },
    /// schema：加载类别文件夹
    Schema { catalog: String, schema: String },
    /// 类别文件夹：加载对象
    Folder {
        catalog: String,
        schema: String,
        folder: NavFolder,
    },
    /// 表 / 视图：加载列
    Table {
        catalog: String,
        schema: String,
        table: String,
    },
}

/// 属性面板可展示的对象类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyKind {
    Connection,
    Catalog,
    Schema,
    Table,
    View,
    Column,
}

/// 属性面板定位信息（由导航服务在构建节点时填充）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyRef {
    pub conn_id: String,
    pub source: NavSource,
    pub catalog: Option<String>,
    pub schema: Option<String>,
    /// 列节点的所属表（其他类型为 None）
    pub parent: Option<String>,
    pub name: String,
    pub kind: PropertyKind,
}

/// 导航状态（展开态 / 选中 / 过滤），持久化到 `navigator_state`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NavState {
    /// 已展开节点 key
    pub expanded_keys: Vec<String>,
    /// 选中节点 key
    pub selected_key: Option<String>,
    /// 搜索过滤词
    pub filter_text: String,
    /// 格式版本（便于后续迁移）
    pub version: u32,
}

impl Default for NavState {
    fn default() -> Self {
        Self {
            expanded_keys: Vec::new(),
            selected_key: None,
            filter_text: String::new(),
            version: 1,
        }
    }
}

/// 连接条目（数据源列表项，视图输入）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionEntry {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub source: NavSource,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub schema: Option<String>,
    /// 运行时是否已连接（由连接服务维护）
    pub connected: bool,
}

/// 自定义分组（项目级，多对多成员关系见 §2.2）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i64,
    pub expanded: bool,
}

/// 连接标签（多值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionTag {
    pub connection_id: String,
    pub tag: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_from_conn_id() {
        assert_eq!(NavSource::from_conn_id("P_conn_1"), NavSource::Project);
        assert_eq!(NavSource::from_conn_id("G_conn_1"), NavSource::Global);
        assert_eq!(NavSource::from_conn_id("GP_conn_1"), NavSource::Shared);
    }

    #[test]
    fn source_code_and_label() {
        assert_eq!(NavSource::Project.code(), "P");
        assert_eq!(NavSource::Global.code(), "G");
        assert_eq!(NavSource::Shared.code(), "GP");
        assert_eq!(NavSource::Shared.label(), "共享");
    }

    #[test]
    fn child_key_joins_segments() {
        assert_eq!(NavNode::child_key("G_a", &[]), "G_a");
        assert_eq!(
            NavNode::child_key("G_a", &["db", "public", "t"]),
            "G_a/db/public/t"
        );
    }
}

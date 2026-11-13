//! 对象树的行模型：节点身份、行种类与「能不能确认」。
//!
//! 依据 `prototypes/entry/06-object-tree.html`：
//! - §07 步 3 规定行只带 `depth / kind / label / meta / selectable`；
//! - `has_target()` 改为按**选中节点 kind** 判断（`Table` / `File` / `Temp` 可确认，
//!   `Group` / `Folder` / `Conn` 不可）；
//! - §04 要求 `ElementId` 用**业务键**而非下标，否则增删后焦点会串行——故行必须带 `id`；
//! - §03 要求搜索结果行带面包屑，故行带 `breadcrumb`。
//!
//! 比 §07 那五个字段多出的两项（`id`、`breadcrumb`）就是上面两条规范的要求，不是自由发挥。

/// 树的深度上限：3（§01）。列不做节点（否则到 5 层，弹层宽度不够）。
pub const MAX_DEPTH: u8 = 3;

/// 节点身份：业务键，随行携带，用于 ElementId 与选中态。
///
/// 刻意不用下标：数据库增删后 `selected_table: usize` 这类索引会指向错行。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    /// 数据库表：`conn + catalog + schema + table`（`schema` 允许空串，对齐导航侧口径）
    DbTable {
        conn: String,
        catalog: String,
        schema: String,
        table: String,
    },
    /// 文件：草稿箱相对路径 / 外部文件绝对路径 / 资产库归档文件
    Path(String),
    /// DuckDB 临时表：`scope` 决定「本机可见」还是「仅会话可见」
    Temp { scope: TempScope, table: String },
}

/// 临时表的可见范围——正是结果集条目必须用 pill 标注的那件事。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TempScope {
    /// 建在本地单例：本机任何入口都能引用
    Local,
    /// 建在联邦会话：只有该会话能查，会话重建即失效（owner = conn_id）
    Session(String),
}

/// 行种类。`Group` 是视图分组（三个根 / 资产库三段），`Folder` 是目录层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Group,
    Conn,
    Folder,
    Table,
    File,
    Temp,
}

impl NodeKind {
    /// 能否作为「分析对象」被确认（`has_target()` 的真值来源）。
    ///
    /// `Group` / `Conn` / `Folder` 只是导航层；`Table`（数据库表与资产库分析表）、
    /// `File`（草稿 / 外部 / 归档文件）、`Temp`（结果集 / Mock / 插件输出）才是目标。
    pub fn is_confirmable(self) -> bool {
        matches!(self, Self::Table | Self::File | Self::Temp)
    }
}

/// 一行树。由各数据源产出，三个根共用同一套渲染。
#[derive(Debug, Clone)]
pub struct Row {
    pub id: NodeId,
    pub depth: u8,
    pub kind: NodeKind,
    pub label: String,
    /// 右侧元信息 / 行尾 pill（大小、可见范围、驱动名…）
    pub meta: Option<String>,
    /// 是否可点选确认。与 `kind` 分开：全局 `G_` 表 kind 是 `Table`，
    /// 但读取通道未接通前必须呈现为不可用（§09 缺口），而不是假装能选。
    pub selectable: bool,
    /// 搜索结果行用的路径上下文（`数据库 / mysql_src / analytics`）
    pub breadcrumb: Option<String>,
}

impl Row {
    pub fn new(id: NodeId, depth: u8, kind: NodeKind, label: impl Into<String>) -> Self {
        debug_assert!(depth <= MAX_DEPTH, "树深度上限是 {MAX_DEPTH}，实际 {depth}");
        Self {
            id,
            depth,
            kind,
            label: label.into(),
            meta: None,
            selectable: kind.is_confirmable(),
            breadcrumb: None,
        }
    }

    pub fn with_meta(mut self, meta: impl Into<String>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn with_breadcrumb(mut self, crumb: impl Into<String>) -> Self {
        self.breadcrumb = Some(crumb.into());
        self
    }

    /// 覆盖可选择性：不可读的对象（如全局表缺通道）必须显式置 false。
    pub fn disabled(mut self) -> Self {
        self.selectable = false;
        self
    }

    /// 确认按钮是否可用 = 种类可确认 **且** 本行没有被显式禁用。
    pub fn is_confirmable(&self) -> bool {
        self.kind.is_confirmable() && self.selectable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_table_file_temp_are_confirmable_kinds() {
        assert!(NodeKind::Table.is_confirmable());
        assert!(NodeKind::File.is_confirmable());
        assert!(NodeKind::Temp.is_confirmable());
        assert!(!NodeKind::Group.is_confirmable());
        assert!(!NodeKind::Conn.is_confirmable());
        assert!(!NodeKind::Folder.is_confirmable());
    }

    #[test]
    fn row_default_selectable_follows_kind() {
        let t = Row::new(NodeId::Path("/a.parquet".into()), 3, NodeKind::File, "a.parquet");
        assert!(t.is_confirmable());

        let g = Row::new(NodeId::Path("资产库".into()), 2, NodeKind::Group, "资产库");
        assert!(!g.is_confirmable());
    }

    #[test]
    fn disabled_row_keeps_kind_but_cannot_confirm() {
        // §09：全局表 kind 是 Table，但通道未接通前不可选
        let global = Row::new(
            NodeId::Temp {
                scope: TempScope::Local,
                table: "G_orders".into(),
            },
            3,
            NodeKind::Table,
            "G_orders",
        )
        .with_meta("全局 · 跨项目共享")
        .disabled();
        assert_eq!(global.kind, NodeKind::Table);
        assert!(!global.is_confirmable());
    }

    #[test]
    fn temp_scope_distinguishes_local_from_session() {
        let local = NodeId::Temp {
            scope: TempScope::Local,
            table: "tmp_q_result_1".into(),
        };
        let session = NodeId::Temp {
            scope: TempScope::Session("conn_a".into()),
            table: "tmp_q_result_1".into(),
        };
        assert_ne!(local, session, "同名表在两个作用域不能撞身份");
    }

    #[test]
    fn node_id_is_business_key_not_index() {
        let a = NodeId::DbTable {
            conn: "mysql_src".into(),
            catalog: "analytics".into(),
            schema: String::new(),
            table: "orders".into(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}

//! 导航树的**扁平静态**：可见行序列（虚拟列表的 item 源）。
//!
//! ## 为什么要有这一层
//!
//! 树以前是**自绘递归**：`render_nav_tree` 带着 `render_connection_row` → `render_nav_node`
//! 一层层往下画，顺便把「键盘漫游顺序」累加进 `nav_order`。那样有两个后果：
//!
//! 1. **有几行就画几行**——十万张表的 schema 展开一次就得建十万个元素（今天靠
//!    「首屏 200 + 加载更多」限流，不是虚拟滚动）；
//! 2. 顺序是**渲染的副产物**，想拿它做别的（滚动定位、跳转）只能等渲染完。
//!
//! 现在把「哪些行可见、按什么顺序」抽成一次性算出的 `Vec<NavRow>`：
//! 交给虚拟列表当 item 源，渲染只负责「画第 N 行」。
//!
//! ## 与渲染的分工
//!
//! - 本模块只管**顺序与身份**（谁可见、第几行、属于哪个连接 / 分组、缩进多深）；
//! - 「这一行长什么样」仍然归 `nav_view.rs` 的既有 `render_*`（它们读状态自己算
//!   选中 / 展开 / 错误 / 悬停，本模块不复制这些判据）。
//!
//! 所以这里**不做 I/O、不排队**（唯一的例外是「展开但子节点还没到时排一次加载」，
//! 由 `NavView::collect_nav_rows` 在收集过程中做——它拿得到 `cx`）。

use workbench_shell::model::ConnectionItem;

use crate::model::NavNode;

/// 一行的身份与类别（决定渲染时交给哪个 `render_*`）。
///
/// 载荷按类别带：分组头要计数、连接 / 引用行要连接本体、树行要节点与层级。
/// 不共用「大结构 + 一堆 Option 字段」：那样每个调用点都得判「这个字段这行有没有」。
#[derive(Clone)]
pub(crate) enum NavRow {
    /// 分组头（含「未分组」容器）。
    GroupHeader {
        id: String,
        name: String,
        count: usize,
        connected: usize,
        failed: usize,
    },
    /// 连接行（主组全亮；它自己是否展开由渲染时读状态）。
    Connection {
        conn: Box<ConnectionItem>,
        /// 所属容器：分组 id 或「未分组」伪 id（元素 id 与拖拽落点要用）。
        scope_key: String,
    },
    /// 引用行：连接出现在**非主组**时只指路（`∈ 主组名`）。
    Reference {
        conn: Box<ConnectionItem>,
        scope_key: String,
        primary_name: String,
    },
    /// 对象树行（catalog / schema / 文件夹 / 对象 / 列）。
    Tree {
        node: Box<NavNode>,
        depth: usize,
        scope_key: String,
    },
    /// 「加载更多」行（大 schema 分页）。
    More {
        node: Box<NavNode>,
        depth: usize,
        scope_key: String,
    },
    /// 「已定位到第 N 条」行（定位窗口的说明与回头路）。
    Jump {
        node: Box<NavNode>,
        depth: usize,
        scope_key: String,
        position: usize,
    },
}

impl NavRow {
    /// 业务键（选中锚点、键盘漫游、元素 id 都用它）。
    ///
    /// 与旧 `NavOrderItem::key` 同源：连接行是连接 id，树行是节点 key；
    /// 分组头用 `group:{id}`、引用行用 `ref:{分组}:{连接}`、「更多」/「定位」行
    /// 用节点 key 加后缀——**同一行的键在重构前后必须逐字一致**，否则持久化的
    /// 选中态与展开态会在升级那一次对不上。
    pub(crate) fn key(&self) -> String {
        match self {
            NavRow::GroupHeader { id, .. } => format!("group:{id}"),
            NavRow::Connection { conn, .. } => conn.id.clone(),
            NavRow::Reference {
                conn, scope_key, ..
            } => format!("ref:{scope_key}:{}", conn.id),
            NavRow::Tree { node, .. } => node.key.clone(),
            NavRow::More { node, .. } => format!("{}#more", node.key),
            NavRow::Jump { node, .. } => format!("{}#jump", node.key),
        }
    }

    /// 元素 id（GPUI 要求同层唯一；分组容器名进 id 以免同一连接在多组里撞车）。
    pub(crate) fn element_id(&self) -> String {
        match self {
            NavRow::GroupHeader { id, .. } => format!("nav-group-{id}"),
            NavRow::Connection { conn, scope_key } => format!("nav-conn-{scope_key}::{}", conn.id),
            NavRow::Reference {
                conn, scope_key, ..
            } => format!("nav-ref-{scope_key}::{}", conn.id),
            NavRow::Tree {
                node, scope_key, ..
            } => format!("nav-node-{scope_key}::{}", node.key),
            NavRow::More {
                node, scope_key, ..
            } => format!("nav-more-{scope_key}::{}", node.key),
            NavRow::Jump {
                node, scope_key, ..
            } => format!("nav-jumped-{scope_key}::{}", node.key),
        }
    }

    /// 属于哪个连接（分组头没有归属，返回 `None`）。
    ///
    /// 面板头「刷新当前连接」、拖拽落点等要按行找连接；从行上直接取，
    /// 不再让调用方去解析 key（那是第二份口径）。
    pub(crate) fn conn_id(&self) -> Option<&str> {
        match self {
            NavRow::GroupHeader { .. } => None,
            NavRow::Connection { conn, .. } | NavRow::Reference { conn, .. } => {
                Some(conn.id.as_str())
            }
            NavRow::Tree { node, .. }
            | NavRow::More { node, .. }
            | NavRow::Jump { node, .. } => Some(node.connection_id.as_str()),
        }
    }

    /// 能不能被键盘漫游选中（分组头是容器标题，不参与漫游；「更多」「定位」可点可选中）。
    pub(crate) fn selectable(&self) -> bool {
        !matches!(self, NavRow::GroupHeader { .. })
    }

    /// 树行 / 「更多」/「定位」行携带的节点（连接与分组头为 `None`）。
    pub(crate) fn node(&self) -> Option<&NavNode> {
        match self {
            NavRow::Tree { node, .. }
            | NavRow::More { node, .. }
            | NavRow::Jump { node, .. } => Some(node),
            _ => None,
        }
    }

    /// 树行的层级（非树行为 0；缩进按它算）。
    pub(crate) fn depth(&self) -> usize {
        match self {
            NavRow::Tree { depth, .. } | NavRow::More { depth, .. } | NavRow::Jump { depth, .. } => {
                *depth
            }
            _ => 0,
        }
    }
}

/// 两行是否**同一行**（列表要判「选中锚点还在不在」）。
///
/// 按业务键比而不是按整行比：载荷里的 `ConnectionItem` 会随刷新（状态点、标签）
/// 变，那不该被当成「换了一行」——否则每次刷新列表都会丢掉选中与滚动位置。
pub(crate) fn same_row(a: &NavRow, b: &NavRow) -> bool {
    a.key() == b.key()
}

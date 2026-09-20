//! 数据源导航面板（M4）：面板头 / facet 筛选 / 分组-连接-对象树 / 行内编辑 / 拖拽排序。
//!
//! 本 crate（`database`）自带视图；宿主能力经 [`NavHost`] 注入（`NavView::new`），
//! 与 `mock` / `insight` / `analytics_resource` 同形。
//!
//! ## 模块地图（2026-09-20 拆分；总入口仍是本文件）
//!
//! 拆分前是一个 7808 行的单文件（含测试 1426 行）——改一处 UI 要在一屏里找半天。
//! 现在按「这一行长什么样 / 状态怎么变 / 外壳」分家：
//!
//! | 文件 | 职责 | 行数 |
//! | --- | --- | --- |
//! | `nav_view.rs`（本文件） | `NavView` 结构与协议 + 导航锚点（键盘漫游 / 选中判据 / 展开折叠 / 开属性） | 约 664 |
//! | `nav_view/primitives.rs` | 纯函数与视觉原语（徽标映射 / 类别图标 / 展开指示 / 激活条 / 相对时间 / 命中高亮） | 约 666 |
//! | `nav_view/rows.rs` | 可见行扁平化 + 行高 + 七种行的渲染（含搜索结果行） | 约 2442 |
//! | `nav_view/chrome.rs` | 面板外壳（面板头 / 搜索行 / chips / facet 弹层 / 结果区标题行 / 空态 / 底部状态行） | 约 1055 |
//! | `nav_view/editors.rs` | 行内编辑器（归组 / 标签 / 复制为模板）与提交路径 | 约 427 |
//! | `nav_view/actions.rs` | 状态变更（展开 / 刷新 / 定位泵 / 后台回填 / 筛选落库 / 拖拽落点） | 约 1239 |
//! | `nav_view/dnd.rs` | 拖拽载荷、落点与拖拽幽灵 | 约 85 |
//! | `nav_view/tests.rs` | 单测（自本文件整体搬出，逐字未改） | 约 1548 |
//!
//! 两条拆分规则（后续扩展请沿用）：
//! 1. **文件名不变**：保留 `nav_view.rs` 作模块根（Rust 2018 允许 `foo.rs` + `foo/` 并存），
//!    文档与 skills 里 90 余处路径引用因此零改动；
//! 2. **跨模块项加 `pub(super)`**：子模块能看见祖先的私有项，但根与兄弟模块看不见子模块的私有项；
//!    `pub fn` 是**跨 crate 公开面**，拆分时不得降级（`new` / `focus_nav_search` / `render_nav` /
//!    `reveal_hit` / `reveal_ref` 都由 workbench 在调）。状态类型（`NavViewState` / `NavRow` 等）
//!    留在根，字段可见性因此无需改动。
//!
//! 依赖分工（下沉后不再有 `Shared`）：
//! - 连接清单 / 选中 / 项目根 / 提示 / 视图偏好 / 连接生命周期 → [`NavHost`]；
//! - 标签 / 分组 / 排序 / 导航状态落库 → [`crate::nav_store`]（只依 `engine`）；
//! - 后台任务 → [`crate::nav_jobs`]；对象树元数据 → [`crate::navigator_service`]；
//! - 驱动目录 → `engine::persistence::driver_catalog`。
//!
//! 设计：`docs/architecture/database/database-navigator-prototype-design.md`
//! （结构设计：`docs/architecture/layout/panels-coupling-plan.md` §9 A'1~A'3）。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt;
use gpui_kit::base::{VirtualListScrollHandle, v_virtual_list};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::rc::Rc;

use crate::commands::{
    NavClearSearch, NavCollapse, NavDown, NavExpand, NavOpenProperties, NavReorderDown,
    NavReorderUp, NavUp,
};
use crate::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, ObjectKind, ObjectRef, PropertyKind,
    PropertyRef, PropertyRequest,
};
use crate::nav_host::{NavFilters, NavHost};
use crate::nav_rows::{NavRow, search_hit_row_id, search_hit_row_key};
use crate::sql_gen::DmlKind;
use engine::persistence::driver_catalog::DriverMeta;

use crate::nav_jobs;

use workbench_shell::model::{ConnectionItem, GroupFormSeed, QueryRequest, RightPanel};
use workbench_shell::product_tokens;
use workbench_shell::tree;
use workbench_shell::ui;

// ===== 子模块（拆分见 `tools/split_nav_view.py` + `tools/split_nav_view_impl.py`） =====
//
// 为什么保留 `nav_view.rs` 这个文件名并另建同名目录：文档与 skills 里有 90 余处
// `crates/database/src/nav_view.rs` 与函数级引用，改文件名等于同步改几十份文档。
// Rust 2018 允许 `foo.rs` + `foo/` 并存，于是**路径稳定**、拆分零外部改动。
// 两个脚本是**一次性迁移工具**（已执行；保留作追溯，勿重复运行）。

mod actions;
mod chrome;
mod dnd;
mod editors;
mod primitives;
mod rows;

use dnd::*;
pub use dnd::{NavConnDragPayload, NavDragPayload, NavGroupDragPayload};
use primitives::*;

/// 待定位目标（搜索结果 → 树）。
///
/// 只描述「要找什么」，不持有视图句柄：整条定位是**跨多帧**完成的——连接 → catalog →
/// schema → 文件夹逐层异步加载回来，每回来一层才能往下走一步。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RevealTarget {
    conn_id: String,
    /// 内容档命中没有 catalog（FTS 表里没这一列）→ 空串，定位时从连接已加载的 catalog 里取。
    catalog: String,
    schema: String,
    /// `None` = 目标就是 schema 本身（不再往文件夹里走）。
    folder: Option<NavFolder>,
    /// 列命中时的所属表：需要在文件夹里再展开一层。
    parent: Option<String>,
    name: String,
    /// 已发出「定位页」请求（等回执；避免每帧重发）。
    jumping: bool,
}

impl RevealTarget {
    /// 统一引用 → 定位目标。**两个入口（导航结果行 / Quick Open）共用这一处映射**：
    /// 类别 → 文件夹只判一次，不会两边各写一套而后分叉。
    fn from_ref(object: &ObjectRef) -> Option<Self> {
        // 没有 schema 归属的对象定位不了：树的第一层是 catalog，第二层就是 schema。
        let schema = (!object.schema.is_empty()).then(|| object.schema.clone())?;
        let folder = match object.kind {
            ObjectKind::Schema => None,
            ObjectKind::Table | ObjectKind::Column => Some(NavFolder::Tables),
            ObjectKind::View => Some(NavFolder::Views),
            ObjectKind::Routine => Some(NavFolder::Routines),
            ObjectKind::Sequence => Some(NavFolder::Sequences),
            ObjectKind::Trigger => Some(NavFolder::Triggers),
            // catalog 层不做定位：它在树上只有「唯一容器」这一层语义，没有可定位的“那一行”。
            ObjectKind::Catalog => return None,
        };
        Some(Self {
            conn_id: object.conn_id.clone(),
            // 空串 = 该命中没有 catalog（内容档没有这一列），定位时从连接已加载的 catalog 取
            catalog: object.catalog.clone(),
            schema,
            folder,
            parent: (!object.parent.is_empty()).then(|| object.parent.clone()),
            name: object.name.clone(),
            jumping: false,
        })
    }

    /// 搜索命中 → 定位目标（导航面板的搜索结果行）。
    ///
    /// 先把命中归一成统一引用（`ObjectRef::from_index_hit` —— 搜索与导航 / 属性面板之间的
    /// **唯一对接口**），再走 [`Self::from_ref`]：类别 → 文件夹只判一处。
    fn from_hit(hit: &nav_jobs::SearchHit) -> Option<Self> {
        let object = ObjectRef::from_index_hit(
            &hit.conn_id,
            &hit.object_type,
            &hit.object_name,
            hit.parent_name.as_deref(),
            hit.catalog.as_deref(),
            hit.schema.as_deref(),
        )?;
        Self::from_ref(&object)
    }

    /// 目标节点的 key（与 `NavNode::child_key` 同构）。
    fn node_key(&self, catalog: &str) -> String {
        match (&self.parent, self.folder) {
            (Some(parent), _) => {
                NavNode::child_key(&self.conn_id, &[catalog, &self.schema, parent, &self.name])
            }
            (None, Some(_)) => {
                NavNode::child_key(&self.conn_id, &[catalog, &self.schema, &self.name])
            }
            // schema 目标：它本身就是 key 链的最后一节
            (None, None) => NavNode::child_key(&self.conn_id, &[catalog, &self.schema]),
        }
    }

    /// 逐层要**展开并等其子节点到位**的 `(key, 加载路径)`；最后一个的子节点里就有目标。
    ///
    /// 顺序不能少一层：连接根的子节点是 catalog 列表，少了 catalog 那一层，定位会在
    /// 「等一个永远不来的子节点」上卡死（`children` 里永远没有那个 key）。
    fn levels(&self, catalog: &str) -> Vec<(String, NavPath)> {
        let mut levels = vec![
            (self.conn_id.clone(), NavPath::Connection),
            (
                NavNode::child_key(&self.conn_id, &[catalog]),
                NavPath::Catalog {
                    catalog: catalog.to_string(),
                },
            ),
        ];
        if let Some(folder) = self.folder {
            levels.push((
                NavNode::child_key(&self.conn_id, &[catalog, &self.schema]),
                NavPath::Schema {
                    catalog: catalog.to_string(),
                    schema: self.schema.clone(),
                },
            ));
            levels.push((
                NavNode::child_key(&self.conn_id, &[catalog, &self.schema, folder.key()]),
                NavPath::Folder {
                    catalog: catalog.to_string(),
                    schema: self.schema.clone(),
                    folder,
                },
            ));
        }
        levels
    }
}

/// 数据库导航面板状态（M4）。
///
/// 数据来自 `Shared::connections`（连接列表）+ `NavigatorService`（对象树懒加载）。
/// 来源标签页、展开态、加载结果与错误就地保存在此，重启持久化在后续切片接入。
#[derive(Default)]
pub struct NavViewState {
    /// 来源筛选（None = 全部）。
    pub source_filter: Option<NavSource>,
    /// 搜索过滤词（本地筛选）。
    filter: String,
    /// 已展开节点 key。
    expanded: HashSet<String>,
    /// 父节点 key → 已加载子节点。
    children: HashMap<String, Vec<NavNode>>,
    /// 已尝试加载的节点 key（避免重复请求）。
    attempted: HashSet<String>,
    /// 正在后台加载的节点 key（渲染「加载中…」占位）。
    loading: HashSet<String>,
    /// 节点 key → 加载失败原因。
    errors: HashMap<String, String>,
    /// 本会话运行时已连接的连接 ID。
    connected: HashSet<String>,
    /// 已从库中恢复过导航状态的连接 ID。
    state_loaded: HashSet<String>,
    /// 自定义分组（项目级）。
    groups: Vec<engine::persistence::ConnectionGroup>,
    /// 连接 ID → 所属分组 ID 列表（多对多）。
    membership: HashMap<String, Vec<String>>,
    /// 分组 ID → 组内连接 ID（按手动排序优先，未排按连接 ID）。
    group_order: HashMap<String, Vec<String>>,
    /// 「未分组」容器里**已手动排序**的连接 ID（顺序）。未列出的回退到名称升序。
    ungrouped_order: Vec<String>,
    /// 连接 ID → **显式主组** ID（仅用户显式指定过的连接；缺省回退到分组排序推导）。
    primary_group: HashMap<String, String>,
    /// 节点 key → 该路径下的对象总数（来自 `metadata_index` 计数或全量结果长度）。
    ///
    /// 大 schema 只加载首屏时，仅靠 `children` 长度无法判断“还有没有”，必须由数据侧告知。
    ///
    /// 旧的 `page_limit`（「屏上允许出现多少条」的渲染窗口）已退场（2026-09-20）：
    /// 虚拟列表只画视口内那几行，已加载的行全部进列表也不会多花代价；
    /// 于是双轨（`child_total` 与 `page_limit`）与「显示更多（只放大窗口）」半套一起删掉，
    /// 「加载更多」只剩一个意思：**数据侧还有没取的**。
    child_total: HashMap<String, usize>,
    /// 已发给后台的索引搜索词（`None` = 当前无搜索；用于“查询词变了才重搜”）。
    search_query: Option<String>,
    /// 索引搜索命中（跨连接；命中行进列表最前，见 `NavRow::SearchHit`）。
    search_hits: Vec<nav_jobs::SearchHit>,
    /// 上批结果实际搜了几个连接（有缓存的那些；用于结果区的“已搜 N 个连接”）。
    search_searched: usize,
    /// 待定位目标（搜索结果行的「定位」→ 展开链路并选中它）。跨帧推进，见
    /// [`NavView::pump_reveal`]。
    reveal: Option<RevealTarget>,
    /// 已「定位窗口」打开的文件夹 key → 目标在这一类的**全量**里的绝对位次。
    ///
    /// 存它有两个用处：顶上那行「已定位到第 N 条」要说准数；有它时不摆「加载更多」
    /// （那时的行集是**一窗**不是前缀，按条数往后翻会跳错）。
    jumped: HashMap<String, usize>,
    /// 定位失败的一行如实说明（没找到 / 索引没重建）；成功或未开定位时为空。
    reveal_note: Option<String>,
    /// 连接 ID → 标签列表（一次读库缓存，避免渲染期逐条查询）。
    tags: HashMap<String, Vec<String>>,
    /// 分组是否已加载。
    groups_loaded: bool,
    /// 折叠的自定义分组 ID（缺省展开）。
    collapsed_groups: HashSet<String>,
    /// 正在内联编辑分组（归组）的连接 ID（None = 未打开）。
    group_picker_for: Option<String>,
    /// 正在内联编辑**标签**的连接 ID（None = 未打开；`+` 专用）。
    tag_editor_for: Option<String>,
    /// 正在内联「复制为模板」的连接 ID（None = 未打开）。
    copy_for: Option<String>,
    /// 正在内联重命名的分组 ID（None = 未打开）。
    group_rename_for: Option<String>,
    /// 已发起列预取（C2）的类别文件夹 key（避免重复排队）。
    prefetched: HashSet<String>,
    /// 当前选中的节点 key（键盘导航与选中高亮）。
    selected_key: Option<String>,
    /// 附加 facet 筛选：类型（`drivers.type_id`）。
    pub type_filter: Option<String>,
    /// 附加 facet 筛选：驱动 id。
    pub driver_filter: Option<String>,
    /// 附加 facet 筛选：标签。
    pub tag_filter: Option<String>,
    /// 搜索框 facet 语法解析结果（每帧重算，不持久化）。
    search_facets: NavSearchFacets,
    /// 最近一次元数据加载**成功回填**的时间（面板底「元数据更新于 X 分钟前」）。
    ///
    /// 为何不用缓存文件 mtime：render 期禁止 I/O，而这条信息每帧都要用；它回答的是
    /// 「我看到的树是什么时候取的」，与缓存文件写入时间在体感上等价。
    last_loaded_at: Option<SystemTime>,
}

/// 附加 facet 种类（facet 弹层与持久化共用）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum NavFacet {
    /// 数据库类型（`drivers.type_id`）。
    Type,
    /// 驱动 id。
    Driver,
    /// 标签。
    Tag,
}

/// 搜索框 facet 语法解析结果（`scope:` / `type:` / `driver:` / `tag:`）。
///
/// 搜索中的 facet token 作为**额外约束**与面板 chips **叠加（AND）**，不写回 chips；
/// 这样避免“输入框回写 → 重新解析”的反馈环与光标跳动。
#[derive(Default, Clone)]
struct NavSearchFacets {
    /// 自由文本（去除 facet token 后的剩余）。
    free: String,
    /// 搜索是否包含 facet token（用于计数）。
    active: usize,
    /// `scope:` / `source:`（`source` 为历史别名）。
    source: Option<NavSource>,
    /// `type:`。
    db_type: Option<String>,
    /// `driver:`。
    driver: Option<String>,
    /// `tag:`。
    tag: Option<String>,
}

/// 渲染顺序中的可见项（键盘导航用；每帧重建）。
#[derive(Clone)]
pub struct NavOrderItem {
    key: String,
    conn_id: String,
    path: Option<NavPath>,
    property: Option<PropertyRef>,
    has_children: bool,
    expanded: bool,
}

/// 「未分组」固定分组的伪 ID（收纳不属于任何自定义分组的连接）。
///
/// 与引擎侧哨兵同值：`navigator_ungrouped_order` 用它标识「未分组容器」的顺序。
const GROUP_UNGROUPED: &str = engine::persistence::UNGROUPED_SCOPE;

/// 连接行徽标状态（颜色通道；见原型设计 §2.3）。
#[derive(Clone, Copy)]
enum NavBadgeStatus {
    /// 已连接（`success`）。
    Connected,
    /// 建连 / 预热中（`info`）。
    Connecting,
    /// 最近一次连接失败（`danger`）。
    Failed,
    /// 未连接（`muted`，灰）。
    Idle,
}

impl NavBadgeStatus {
    fn color(self, theme: &gpui_kit::component::Theme) -> Hsla {
        match self {
            Self::Connected => theme.colors.success,
            Self::Connecting => theme.colors.info,
            Self::Failed => theme.colors.danger,
            Self::Idle => theme.colors.muted_foreground,
        }
    }

    /// 状态文案（徽标 hover 卡用）。
    fn label(self) -> &'static str {
        match self {
            Self::Connected => "已连接",
            Self::Connecting => "连接中",
            Self::Failed => "连接失败",
            Self::Idle => "未连接",
        }
    }
}

/// 连接级约束快照（归属域 chips + 搜索框 `scope:`/`type:`/`driver:`/`tag:` + 「筛选 ▾」弹层）。
///
/// 三处来源是**叠加**关系（都得满足）：chips 与 `scope:` 同名不同源，`type:` 与弹层里选的
/// 类型同理。若合成「后写的覆盖先写的」，「chips 选项目 + `scope:global`」会从「无匹配」
/// 变成「命中全局」，与屏幕上同时亮着的两个选中态自相矛盾。
///
/// 抽成一份快照是因为树与「索引搜索该搜哪些连接」必须吃**同一份**判据：分成两处写，
/// 以后加一个 facet 只会改到其中一处，症状是「搜索能搜到的连接在树里看不见」。
#[derive(Default)]
struct NavConnFilter {
    /// 归属域（chips 与 `scope:` 各占一条）。
    sources: Vec<NavSource>,
    /// 数据源类型（`type_id`，不是驱动 id）。
    types: Vec<String>,
    /// 驱动 id。
    drivers: Vec<String>,
    /// 标签（每条约束都要命中）。
    tags: Vec<String>,
}

impl NavConnFilter {
    /// 从状态快照一份。`source_filter` 是 chips 的当前值（不在状态里，由调用方给）。
    fn snapshot(view: &NavViewState, source_filter: Option<NavSource>) -> Self {
        let mut out = Self::default();
        out.sources.extend(source_filter);
        out.sources.extend(view.search_facets.source);
        out.types.extend(view.type_filter.clone());
        out.types.extend(view.search_facets.db_type.clone());
        out.drivers.extend(view.driver_filter.clone());
        out.drivers.extend(view.search_facets.driver.clone());
        out.tags.extend(view.tag_filter.clone());
        out.tags.extend(view.search_facets.tag.clone());
        out
    }

    /// 这条连接是否满足全部约束。
    ///
    /// `type_of` 惰性求值（没有类型约束时不必去读驱动目录），由调用方提供
    /// —— 类型 id 只在 `driver_catalog` 里，本结构只管筛。
    fn passes(
        &self,
        conn: &ConnectionItem,
        conn_tags: Option<&Vec<String>>,
        type_of: impl Fn(&str) -> String,
    ) -> bool {
        if !self.sources.is_empty() {
            let source = NavSource::from_conn_id(&conn.id);
            if self.sources.iter().any(|want| *want != source) {
                return false;
            }
        }
        if !self.types.is_empty() {
            let actual = type_of(&conn.driver);
            if self.types.iter().any(|want| *want != actual) {
                return false;
            }
        }
        if self.drivers.iter().any(|want| *want != conn.driver) {
            return false;
        }
        if self.tags.iter().any(|want| {
            !conn_tags
                .map(|ts| ts.iter().any(|t| t == want))
                .unwrap_or(false)
        }) {
            return false;
        }
        true
    }
}

/// 数据源导航面板（M4）。
///
/// 视图归本 crate；宿主能力经 [`NavHost`] 注入（与 `mock` / `insight` /
/// `analytics_resource` 同形，宿主实现在 `crates/workbench/src/components/nav_host.rs`）。
///
/// 状态分两处：**实体字段**（输入框 / 轮询任务 / 驱动目录等渲染资产）+ [`NavViewState`]
/// （展开态、已加载子节点、筛选与行内编辑器标记）。后者用 `Rc<RefCell<…>>` 是因为
/// 若干 `&self` 辅助函数要就地更新（键盘移动 / 后台回填）。
pub struct NavView {
    /// 宿主端口（连接清单 / 选中 / 提示 / 视图偏好 / 连接生命周期 / 对话框）。
    host: Rc<dyn NavHost>,
    focus_handle: FocusHandle,
    /// 视图状态（懒加载对象树）。
    nav: Rc<RefCell<NavViewState>>,
    /// 数据源导航搜索框（懒创建）。
    nav_search: Option<Entity<InputState>>,
    _nav_search_sub: Option<Subscription>,
    /// 连接行内联标签输入框（组织编辑器打开时创建，关闭时销毁）。
    nav_tag_input: Option<Entity<InputState>>,
    /// 当前标签输入框对应的连接 ID（切换连接时重建并重新预填）。
    nav_tag_input_for: Option<String>,
    _nav_tag_sub: Option<Subscription>,
    /// 行内「复制为模板」输入框（打开时创建，关闭时销毁）。
    nav_copy_input: Option<Entity<InputState>>,
    /// 当前复制输入框对应的连接 ID（切换连接时重建）。
    nav_copy_input_for: Option<String>,
    _nav_copy_sub: Option<Subscription>,
    /// 分组名内联重命名输入框（重命名时创建，关闭时销毁）。
    nav_group_input: Option<Entity<InputState>>,
    _nav_group_sub: Option<Subscription>,
    /// 渲染顺序重建的可见项（键盘 ↑↓ 移动 / 展开折叠 / 打开属性）。
    nav_order: Rc<RefCell<Vec<NavOrderItem>>>,
    /// 本次可见行（[`Self::collect_nav_rows`] 在 render 里写入；虚拟列表与漫游序列都读它）。
    ///
    /// 为何存在实体字段而不是每帧传给列表：`v_virtual_list` 的 item 渲染器在**布局期**
    /// 才被调用（拿得到 `&mut Context<Self>`），那时没有再传一次的机会。
    rows: Vec<NavRow>,
    /// 待兑现的滚动意图（行 key）：定位跨帧推进，行集合下一帧才包含目标。
    nav_pending_scroll: Option<String>,
    /// 树区滚动句柄（虚拟列表自带滚动；「滚到眼前」类需求走它，见导航开发方案 §2.5 S5）。
    list_scroll: Rc<VirtualListScrollHandle>,
    /// 正在轮询预热进度的后台任务（避免重复启动）。
    warm_poll: Option<Task<()>>,
    /// 正在轮询导航加载结果的后台任务（避免重复启动；`&self` 路径也要访问）。
    nav_pump: RefCell<Option<Task<()>>>,
    /// Ctrl+F 待聚焦标记：搜索框懒创建，先到位的请求在这里等一帧。
    nav_search_focus_pending: bool,
    /// 驱动 id → 类型 / 显示名（徽标、hover 卡与属性面板共用；随组织数据一次性加载）。
    driver_catalog: RefCell<HashMap<String, DriverMeta>>,
}

impl NavView {
    /// 构造：注入宿主端口，并从宿主恢复 facet 筛选（UI 偏好，跟项目无关）。
    pub fn new(host: Rc<dyn NavHost>, cx: &mut Context<Self>) -> Self {
        let saved = host.nav_filters(cx);
        let mut nav = NavViewState::default();
        nav.source_filter = saved.source.as_deref().and_then(NavSource::from_key);
        nav.type_filter = saved.db_type.clone();
        nav.driver_filter = saved.driver.clone();
        nav.tag_filter = saved.tag.clone();
        Self {
            host,
            focus_handle: cx.focus_handle(),
            nav: Rc::new(RefCell::new(nav)),
            nav_search: None,
            _nav_search_sub: None,
            nav_tag_input: None,
            nav_tag_input_for: None,
            _nav_tag_sub: None,
            nav_copy_input: None,
            nav_copy_input_for: None,
            _nav_copy_sub: None,
            nav_group_input: None,
            _nav_group_sub: None,
            nav_order: Rc::new(RefCell::new(Vec::new())),
            rows: Vec::new(),
            nav_pending_scroll: None,
            list_scroll: Rc::new(VirtualListScrollHandle::new()),
            warm_poll: None,
            nav_pump: RefCell::new(None),
            nav_search_focus_pending: false,
            driver_catalog: RefCell::new(HashMap::new()),
        }
    }
}

impl Focusable for NavView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NavView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_nav(window, cx)
    }
}

impl NavView {
    /// 聚焦数据源导航搜索框（Ctrl+F，由宿主 action 调用）。
    ///
    /// 搜索框懒创建：已存在则立即聚焦，否则置位由 `render_nav` 消费。
    pub fn focus_nav_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.nav_search.clone() {
            let handle = input.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        } else {
            // 搜索框可能上一帧才创建：置面板自己的待聚焦标记，本帧的 `render_nav` 消费。
            self.nav_search_focus_pending = true;
        }
        cx.notify();
    }

    /// 键盘导航：按渲染顺序移动选中项（`delta` 为 ±1）。
    fn nav_move(&self, delta: isize, cx: &mut Context<Self>) {
        let order = self.nav_order.borrow();
        if order.is_empty() {
            return;
        }
        let current = self.nav.borrow().selected_key.clone();
        let idx = current.and_then(|k| order.iter().position(|i| i.key == k));
        let next = match idx {
            None => 0,
            Some(i) => (i as isize + delta).clamp(0, order.len() as isize - 1) as usize,
        };
        let key = order[next].key.clone();
        drop(order);
        self.nav.borrow_mut().selected_key = Some(key.clone());
        // 键盘漫游跟着滚：列表只画视口内的行，不滚的话选中会跑到屏外。
        self.nav_scroll_to_key(&key);
        cx.notify();
    }

    /// 当前选中项（克隆，避免跨借用）。
    fn nav_selected(&self) -> Option<NavOrderItem> {
        let key = self.nav.borrow().selected_key.clone()?;
        self.nav_order
            .borrow()
            .iter()
            .find(|i| i.key == key)
            .cloned()
    }

    /// 某行是否选中（**全树唯一判据**：点击与键盘漫游都写 `selected_key`）。
    ///
    /// 为什么抽出来：「加载更多 / 已定位 / 引用」这些行同样进键盘漫游序列
    /// （`NavRow::selectable`），但原先只有连接行与树行读 `selected_key`
    /// ——↑↓ 走到这些行时画面不动，看起来像键盘失灵。
    fn nav_row_selected(&self, key: &str) -> bool {
        self.nav.borrow().selected_key.as_deref() == Some(key)
    }

    /// 当前选中的连接 id（仅当选中节点是连接根时）。
    ///
    /// 面板头「⟳ 刷新元数据」「断开当前连接」的作用目标（设计 §2.1 / §4.2）。
    fn nav_current_connection(&self) -> Option<String> {
        self.nav_selected()
            .filter(|item| matches!(item.path, Some(NavPath::Connection)))
            .map(|item| item.conn_id)
    }

    /// →：展开选中节点（有子节点且尚未展开）。
    fn nav_expand(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.nav_selected() else {
            return;
        };
        if item.has_children && !item.expanded {
            if let Some(path) = item.path {
                self.toggle_nav_node(&item.conn_id, &item.key, path, cx);
                cx.notify();
            }
        }
    }

    /// ←：折叠选中节点（已展开）。
    fn nav_collapse(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.nav_selected() else {
            return;
        };
        if item.has_children && item.expanded {
            if let Some(path) = item.path {
                self.toggle_nav_node(&item.conn_id, &item.key, path, cx);
                cx.notify();
            }
        }
    }

    /// Enter / F4：打开选中节点的属性面板（无属性定位时为无操作）。
    fn nav_open_properties(&self, cx: &mut Context<Self>) {
        let Some(item) = self.nav_selected() else {
            return;
        };
        let Some(property) = item.property else {
            return;
        };
        let (conn_label, driver) = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == item.conn_id)
            .map(|c| (c.name.clone(), c.driver.clone()))
            .unwrap_or_else(|| (item.conn_id.clone(), String::new()));
        self.host.show_properties(
            PropertyRequest {
                property,
                conn_label,
                driver,
            },
            cx,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests;

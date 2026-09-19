//! 数据源导航面板（M4）：面板头 / facet 筛选 / 分组-连接-对象树 / 行内编辑 / 拖拽排序。
//!
//! 本 crate（`database`）自带视图；宿主能力经 [`NavHost`] 注入（`NavView::new`），
//! 与 `mock` / `insight` / `analytics_resource` 同形。内容分四段：
//! 1. 状态类型与纯函数辅助（`NavViewState` / `NavFacet` / `parse_nav_search` / 徽标映射 / 拖拽载荷）；
//! 2. 键盘移动 / 展开折叠 / 打开属性；
//! 3. 导航渲染与操作实现（树渲染 / 分组与标签 / 拖拽落点 / 连接增删改 / 刷新预热）；
//! 4. 纯函数单测。
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

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::rc::Rc;

use crate::commands::{
    NavCollapse, NavDown, NavExpand, NavOpenProperties, NavReorderDown, NavReorderUp, NavUp,
};
use crate::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, ObjectKind, ObjectRef, PropertyKind,
    PropertyRef, PropertyRequest,
};
use crate::nav_host::{NavFilters, NavHost};
use crate::sql_gen::DmlKind;
use engine::persistence::driver_catalog::DriverMeta;

use crate::nav_jobs;

use workbench_shell::model::{ConnectionItem, GroupFormSeed, QueryRequest, RightPanel};
use workbench_shell::product_tokens;
use workbench_shell::ui;

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
    /// 类别文件夹节点 key → 已渲染条数上限（大 schema 客户端分页）。
    ///
    /// 与 [`Self::child_total`] 配合：`page_limit` 是「屏上允许出现多少条」的渲染窗口，
    /// `child_total` 是「数据侧一共有多少条」；前者可以小于后者（分批拉取中）。
    page_limit: HashMap<String, usize>,
    /// 节点 key → 该路径下的对象总数（来自 `metadata_index` 计数或全量结果长度）。
    ///
    /// 大 schema 只加载首屏时，仅靠 `children` 长度无法判断“还有没有”，必须由数据侧告知。
    child_total: HashMap<String, usize>,
    /// 已发给后台的索引搜索词（`None` = 当前无搜索；用于“查询词变了才重搜”）。
    search_query: Option<String>,
    /// 索引搜索命中（跨连接；结果区渲染在树顶）。
    search_hits: Vec<nav_jobs::SearchHit>,
    /// 上批结果实际搜了几个连接（有缓存的那些；用于结果区的“已搜 N 个连接”）。
    search_searched: usize,
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

/// 解析搜索框文本：拆出 `scope:` / `type:` / `driver:` / `tag:` token，其余为自由文本。
///
/// 值不引号包裹（含空格需自行避免）；`scope` / `source` 支持全名与短码。
/// 无法识别的 token（如空值）原样保留在自由文本里，避免“输入中丢字”。
fn parse_nav_search(raw: &str) -> NavSearchFacets {
    let mut out = NavSearchFacets::default();
    let mut free: Vec<&str> = Vec::new();
    for tok in raw.split_whitespace() {
        let Some((key, value)) = tok.split_once(':') else {
            free.push(tok);
            continue;
        };
        if value.is_empty() {
            free.push(tok);
            continue;
        }
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "scope" | "source" => match NavSource::from_key(value) {
                Some(s) => {
                    out.source = Some(s);
                    out.active += 1;
                }
                None => free.push(tok),
            },
            "type" => {
                out.db_type = Some(value.to_string());
                out.active += 1;
            }
            "driver" => {
                out.driver = Some(value.to_string());
                out.active += 1;
            }
            "tag" => {
                out.tag = Some(value.to_string());
                out.active += 1;
            }
            _ => free.push(tok),
        }
    }
    out.free = free.join(" ");
    out
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

/// 徽标 hover 卡：类型 / 状态 / 驱动（gpui-kit 0.6.1 无通用 `.tooltip` 扩展，故用 `HoverCard`）。
fn nav_badge_hover_card(
    id: SharedString,
    trigger: impl IntoElement + 'static,
    type_label: String,
    status_label: &'static str,
    driver_label: String,
) -> impl IntoElement {
    use gpui_kit::component::hover_card::HoverCard;
    HoverCard::new(id)
        .open_delay(std::time::Duration::from_millis(300))
        .trigger(trigger)
        .content(move |_, _window, cx| {
            let fg = cx.theme().colors.foreground;
            let muted = cx.theme().colors.muted_foreground;
            let type_label = type_label.clone();
            let driver_label = driver_label.clone();
            div()
                .v_flex()
                .gap(rems(0.125))
                .text_xs()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(type_label),
                )
                .child(
                    div()
                        .text_color(muted)
                        .child(format!("状态：{status_label}")),
                )
                .child(
                    div()
                        .text_color(muted)
                        .child(format!("驱动：{driver_label}")),
                )
        })
}

/// 类型文案（徽标 hover 卡用）：已知类型给出「名称（分类）」，否则回退类型 id。
fn nav_type_label(type_id: &str) -> String {
    let known = match type_id {
        "postgresql" => "PostgreSQL（关系型）",
        "mysql" => "MySQL（关系型）",
        "mariadb" => "MariaDB（关系型）",
        "mssql" => "SQL Server（关系型）",
        "oracle" => "Oracle（关系型）",
        "sqlite" => "SQLite（文件型）",
        "duckdb" => "DuckDB（分析型）",
        "clickhouse" => "ClickHouse（分析型）",
        "mongodb" => "MongoDB（文档型）",
        "redis" => "Redis（键值型）",
        _ => "",
    };
    if known.is_empty() {
        type_id.to_string()
    } else {
        known.to_string()
    }
}

/// 类型短名（去掉「（关系型）」等分类后缀），facet 菜单用。
fn nav_type_short_label(type_id: &str) -> String {
    let full = nav_type_label(type_id);
    full.split('（').next().unwrap_or(&full).to_string()
}

/// 类型徽标映射：数据库类型 id →（形状资产路径，2 字母缩写）。
///
/// 形状取自 `gpui-kit-assets` 全量 Lucide（`AllAssets` 已注册，无需新增资产）；
/// **字母是权威识别，形状是冗余强化 + 扫视加速**（原型设计 §2.3）。
/// 目录外类型回退通用形状 + 类型名首 2 字母。
fn nav_type_badge(type_id: &str) -> (&'static str, String) {
    let (path, letters): (&'static str, &'static str) = match type_id {
        "postgresql" => ("icons/database.svg", "PG"),
        "mysql" => ("icons/cylinder.svg", "MY"),
        "mariadb" => ("icons/coins.svg", "MA"),
        "sqlite" => ("icons/file.svg", "SQ"),
        "duckdb" => ("icons/layers.svg", "DK"),
        "mssql" => ("icons/server.svg", "MS"),
        "oracle" => ("icons/hexagon.svg", "OR"),
        "clickhouse" => ("icons/chart-column.svg", "CH"),
        "mongodb" => ("icons/leaf.svg", "MG"),
        "redis" => ("icons/braces.svg", "RD"),
        _ => {
            let upper = type_id.to_uppercase();
            let short: String = upper.chars().take(2).collect();
            return (
                "icons/database.svg",
                if short.is_empty() { "DB".into() } else { short },
            );
        }
    };
    (path, letters.to_string())
}

/// 节点图标色（复用主题标准色，代码零裸色）。
fn nav_kind_color(kind: &NavNodeKind, theme: &gpui_kit::component::Theme) -> Hsla {
    match kind {
        NavNodeKind::Connection { .. } => theme.colors.primary,
        NavNodeKind::Catalog | NavNodeKind::Schema | NavNodeKind::Folder(_) => theme.colors.warning,
        NavNodeKind::Table { .. } => theme.colors.info,
        NavNodeKind::View => theme.colors.success,
        NavNodeKind::Routine { .. } => theme.colors.primary,
        _ => theme.colors.muted_foreground,
    }
}

/// 限定名（`catalog.schema.name`，跳过空段）：用于复制与生成 SELECT。
///
/// 无独立 Schema 层的驱动（MySQL / SQLite / DuckDB）导航把 schema 传成 catalog，
/// 相等时只留一份，避免出现 `db.db.name`。
fn nav_qualified_name(prop: &PropertyRef) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(c) = prop.catalog.as_deref() {
        if !c.is_empty() {
            parts.push(c);
        }
    }
    if let Some(s) = prop.schema.as_deref() {
        if !s.is_empty() && Some(s) != prop.catalog.as_deref() {
            parts.push(s);
        }
    }
    parts.push(prop.name.as_str());
    parts.join(".")
}

/// 「结构洞察」的靶（M8）：表 / 视图用它所在的 schema，schema 节点用自己；其余节点不给。
///
/// 为什么不给 catalog 节点：`table_schema` 的取值各家不同（MySQL 里就是库名、
/// PG 里是 schema），catalog 级的「全部 schema」要拼一套跨方言语义——先不做，
/// 需要时按方言补（施工单见 `insight-dev-plan.md` §10）。
fn insight_schema_target(
    kind: &NavNodeKind,
    path: Option<&NavPath>,
    conn_id: &str,
) -> Option<ObjectRef> {
    match (kind, path) {
        (
            NavNodeKind::Table { .. } | NavNodeKind::View,
            Some(NavPath::Table {
                catalog, schema, ..
            }),
        )
        | (NavNodeKind::Schema, Some(NavPath::Schema { catalog, schema })) => {
            Some(ObjectRef::schema(conn_id, catalog.clone(), schema.clone()))
        }
        _ => None,
    }
}

/// 表 / 视图节点 → Mock · 洞察表入口的引用靶。
///
/// **kind 按节点类型给**：视图必须标成 `View`——引用是跨屏身份，标成表会传染到
/// 属性面板 / 洞察 / 将来的“在树中定位”（曾经就一律标成了 `Table`）。
/// 非数据类节点不给靶。
fn nav_data_target(
    kind: &NavNodeKind,
    conn_id: &str,
    catalog: String,
    schema: String,
    name: String,
) -> Option<ObjectRef> {
    match kind {
        NavNodeKind::Table { .. } => Some(ObjectRef::table(conn_id, catalog, schema, name)),
        NavNodeKind::View => Some(ObjectRef::view(conn_id, catalog, schema, name)),
        _ => None,
    }
}

/// 数据源导航 → 编辑区拖拽载荷（仅表 / 视图行携带）。
///
/// 载荷只带**限定名 + 展示文案**：拖拽本身不承诺语义，落点决定动作
/// （SQL 区插到光标处 / 编辑区其它位置追加），因此不携带连接句柄或执行计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavDragPayload {
    /// 已去重的限定名（`db.table` / `schema.table`）。
    pub qualified: String,
    /// 展示文案（拖拽幽灵 + 落点通知）。
    pub label: String,
}

/// 拖拽幽灵：跟随光标的轻量预览（不参与命中测试）。
struct NavDragGhost {
    label: String,
    /// 类型色点，与树内节点图标同色（`nav_kind_color`）。
    tint: Hsla,
}

impl Render for NavDragGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .h(rems(1.375))
            .px_2()
            .rounded_md()
            .bg(cx.theme().colors.popover)
            .border_1()
            .border_color(cx.theme().colors.border)
            .shadow_md()
            .child(div().w_2().h_2().flex_none().rounded_sm().bg(self.tint))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.foreground)
                    .child(self.label.clone()),
            )
    }
}

/// 连接行拖拽载荷（归组与组内排序共用）。
///
/// 与表 / 视图的拖拽载荷是**不同类型**：两类落点的 `on_drop` 只认自己的类型，
/// 拖到不相干的落点上自然什么都不发生（回调里无需再判类型）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavConnDragPayload {
    /// 被拖动的连接 ID。
    pub conn_id: String,
    /// 展示名（拖拽幽灵 + 落点通知）。
    pub name: String,
}

/// 连接拖拽的落点语义。
#[derive(Clone, Debug, PartialEq, Eq)]
enum ConnDropTarget {
    /// 落到分组头 / 未分组头：只处理归属（已在组内时不改位置）。
    Container,
    /// 落到某一行：归组 + 插到该行之前（落到自己身上 = 位置不变）。
    BeforeRow(String),
}

/// 计算「把 `moving` 放到 `before` 之前」后的容器顺序；无需变更时返回 `None`。
///
/// - `moving` 不在 `ids` 里 → 视为新加入，插到 `before` 之前（`None` 则追加到末尾）；
/// - `moving` 已在 `ids` 里 → 先摘除再插入，因此「拖到自己身上」与「已经就位」都返回 `None`；
/// - `before` 不在 `ids` 里（目标行被过滤掉）→ 追加到末尾。
///
/// 纯函数：不碰存储；落库顺序由调用方一次写 `0..n`（见 `crate::nav_store::set_container_order`）。
fn nav_reorder(ids: &[String], moving: &str, before: Option<&str>) -> Option<Vec<String>> {
    if before == Some(moving) {
        return None;
    }
    let mut next: Vec<String> = ids
        .iter()
        .filter(|id| id.as_str() != moving)
        .cloned()
        .collect();
    let at = before
        .and_then(|b| next.iter().position(|id| id == b))
        .unwrap_or(next.len());
    next.insert(at, moving.to_string());
    (next != ids).then_some(next)
}

/// 分组头拖拽载荷（分组之间的排序）。
///
/// 与连接载荷分开：拖**分组**到分组头 = 给分组排序，拖**连接**到分组头 = 归组，
/// 两者落在同一个元素上但语义不同，用载荷类型区分最省事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavGroupDragPayload {
    /// 被拖动的分组 ID。
    pub group_id: String,
    /// 展示名（拖拽幽灵 + 通知）。
    pub name: String,
}

/// 主组解析：**显式指定**优先（且仍在所属分组内，被移出则忽略），
/// 否则取排序最前的分组；无任何分组 → `None`（即「未分组」）。
///
/// 渲染（多组只全亮呈现一次）与键盘重排（改哪个容器的顺序）共用同一条规则。
fn nav_primary_scope(
    membership: &HashMap<String, Vec<String>>,
    primary_explicit: &HashMap<String, String>,
    conn_id: &str,
) -> Option<String> {
    let groups_of = membership.get(conn_id)?;
    if let Some(p) = primary_explicit.get(conn_id) {
        if groups_of.iter().any(|g| g == p) {
            return Some(p.clone());
        }
    }
    groups_of.first().cloned()
}

/// 容器内移动一步（`delta` = -1 上移 / +1 下移）；已在边界或不在列表里 → `None`。
///
/// 用“摘除后插到目标下标处元素之前”表达（复用 [`nav_reorder`]），
/// 越过末尾则退化为追加；因为 `nav_reorder` 对“已就位”返回 `None`，末位下移自然为 no-op。
fn nav_step(ids: &[String], moving: &str, delta: i32) -> Option<Vec<String>> {
    let i = ids.iter().position(|id| id == moving)? as i32;
    let target = i + delta;
    if target < 0 {
        return None;
    }
    let others: Vec<&String> = ids.iter().filter(|id| id.as_str() != moving).collect();
    match others.get(target as usize) {
        Some(anchor) => nav_reorder(ids, moving, Some(anchor.as_str())),
        // 目标下标刚好等于剩余长度 = 落到末尾；超出则无效。
        None if target as usize == others.len() => nav_reorder(ids, moving, None),
        None => None,
    }
}

/// 容器成员顺序：手动排序过的按存储序号在前，**未排过的按名称升序在后**。
///
/// `stored` 来自引擎的「成员 + 是否手动排序过」列表。显式段在这里再排一次序号
/// （不依赖引擎的返回顺序），未排段按名称排——名称不在组织存储里，只能在这一层做。
/// 排序均稳定，同键保持传入顺序。纯函数，便于单测。
fn nav_order_members(
    stored: &[(String, Option<i64>)],
    name_of: impl Fn(&str) -> String,
) -> Vec<String> {
    let mut ordered: Vec<(i64, String)> = Vec::with_capacity(stored.len());
    let mut unset: Vec<String> = Vec::new();
    for (id, order) in stored {
        match order {
            Some(o) => ordered.push((*o, id.clone())),
            None => unset.push(id.clone()),
        }
    }
    ordered.sort_by_key(|(o, _)| *o);
    // 名称大小写不敏感；同名再按 ID 定序，保证结果稳定。
    unset.sort_by(|a, b| {
        name_of(a)
            .to_lowercase()
            .cmp(&name_of(b).to_lowercase())
            .then_with(|| a.cmp(b))
    });
    let mut out: Vec<String> = ordered.into_iter().map(|(_, id)| id).collect();
    out.extend(unset);
    out
}

/// 搜索过滤：节点名命中，或已加载子节点中任一命中。
fn nav_node_matches(
    children: &HashMap<String, Vec<NavNode>>,
    node: &NavNode,
    filter: &str,
) -> bool {
    if node.name.to_lowercase().contains(filter) {
        return true;
    }
    match children.get(&node.key) {
        Some(kids) => kids.iter().any(|c| nav_node_matches(children, c, filter)),
        None => false,
    }
}

/// 索引命中 → 统一引用（`None` = 该类别不可寻址）。
///
/// 这是「搜索」与「导航树 / 属性面板」的唯一对接口：命中行必须走 [`ObjectRef`]，
/// 否则搜索侧的键与树上的键就会各拼一套。
fn nav_search_hit_ref(hit: &nav_jobs::SearchHit) -> Option<ObjectRef> {
    ObjectRef::from_index_hit(
        &hit.conn_id,
        &hit.object_type,
        &hit.object_name,
        hit.parent_name.as_deref(),
        hit.catalog.as_deref(),
        hit.schema.as_deref(),
    )
}

/// 索引命中 → 属性面板定位（`None` = 该类别暂不支持属性定位）。
///
/// 列命中要带 `parent`（所属表）：属性面板靠它区分「哪张表的列」。
/// 类别映射在 `model::property_ref_of`（导航搜索与 Quick Open 共用一处）。
fn nav_search_hit_property(hit: &nav_jobs::SearchHit) -> Option<PropertyRef> {
    nav_search_hit_ref(hit).map(|object| crate::model::property_ref_of(&object))
}

/// 索引命中 → 合成节点类别（只为取图标颜色，不参与树挂载）。
fn nav_search_hit_kind(hit: &nav_jobs::SearchHit) -> NavNodeKind {
    match nav_search_hit_ref(hit).map(|r| r.kind) {
        Some(ObjectKind::Table) => NavNodeKind::Table { row_estimate: None },
        Some(ObjectKind::View) => NavNodeKind::View,
        Some(ObjectKind::Column) => NavNodeKind::Column {
            data_type: String::new(),
            nullable: true,
            primary: false,
            foreign: false,
        },
        _ => NavNodeKind::Schema,
    }
}

/// 命中行的类别短标签（表 / 视图 / 模式 / 列）。
///
/// 词汇表在 `engine::ObjectKind::label`（与索引串一一对应）；
/// 这里的未知兜底是必需的：索引里可能出现不可寻址的类别（见 `parse_index_str`）。
fn nav_object_type_label(object_type: &str) -> &'static str {
    ObjectKind::parse_index_str(object_type)
        .map(ObjectKind::label)
        .unwrap_or("对象")
}

/// 搜索词是否够格打一次索引搜索（太短时命中面过大，且首字符几乎必然还要改）。
fn nav_search_query_ready(query: &str) -> bool {
    query.trim().chars().count() >= 2
}

/// 把一页结果并入已加载列表，返回**新增**条数（去重后）。
///
/// 抽成纯函数是为了可测：索引翻页理论上不重叠，但刷新 / 结构变更后两次读可能交叠，
/// 重复节点会在树上出现两次（key 相同 → 元素 id 冲突，删除 / 选中都会错位）。
fn nav_merge_page(loaded: &mut Vec<NavNode>, page: Vec<NavNode>) -> usize {
    // 用 owned key 集合（而非 `&str` 视图）：下面要把节点按值移入 `loaded`，
    // 借 `loaded` 里的 key 会与 `push` 的可变借用冲突。每页一次克隆，代价可忽略。
    let mut seen: HashSet<String> = loaded.iter().map(|n| n.key.clone()).collect();
    let before = loaded.len();
    for node in page {
        if seen.insert(node.key.clone()) {
            loaded.push(node);
        }
    }
    loaded.len() - before
}

/// 搜索命中高亮：把 `name` 中与 `filter`（已小写）匹配的一段用命中底色标出。
///
/// 非 ASCII（如 CJK）大小写转换不改变字节长度，故按字节切片安全；
/// 仅当两端都是 char 边界时才切，否则退化为整体文本。
fn nav_name_highlight(name: &str, filter: &str, match_bg: Hsla, fg: Hsla) -> Div {
    if !filter.is_empty() {
        let lower = name.to_lowercase();
        if lower.len() == name.len() {
            if let Some(pos) = lower.find(filter) {
                let end = pos + filter.len();
                if name.is_char_boundary(pos) && name.is_char_boundary(end) {
                    let before = name[..pos].to_string();
                    let hit = name[pos..end].to_string();
                    let after = name[end..].to_string();
                    return div()
                        .h_flex()
                        .items_center()
                        .min_w_0()
                        .text_color(fg)
                        .child(before)
                        .child(div().rounded_sm().bg(match_bg).child(hit))
                        .child(after);
                }
            }
        }
    }
    div().min_w_0().text_color(fg).child(name.to_string())
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
        self.nav.borrow_mut().selected_key = Some(key);
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

impl NavView {
    /// 数据源导航面板（M4）：面板头 + 来源 chips + 搜索 + 分组树。
    pub fn render_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.nav_search.is_none() {
            let state = cx.new(|cx| InputState::new(window, cx));
            state.update(cx, |s, cx| {
                s.set_placeholder("筛选数据源 / 表 / 列 / 标签…", window, cx)
            });
            // 输入变化时通知面板重渲染，否则过滤词不会即时生效。
            let sub = cx.subscribe_in(&state, window, |_this, _e, ev: &InputEvent, _w, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.nav_search = Some(state);
            self._nav_search_sub = Some(sub);
        }
        // Ctrl+F 请求（搜索框可能上一帧才创建，故在此统一消费）。
        if self.nav_search_focus_pending {
            self.nav_search_focus_pending = false;
            if let Some(input) = self.nav_search.clone() {
                let handle = input.read(cx).focus_handle(cx);
                handle.focus(window, cx);
            }
        }

        // 本地 SQLite 一次性读取（分组/标签、各连接展开态）不在 render 做，
        // 推到本帧效果周期之后执行，完成后重绘。
        let need_org = !self.nav.borrow().groups_loaded;
        let need_state: Vec<String> = {
            let view = self.nav.borrow();
            self.host
                .connections()
                .iter()
                .filter(|c| !view.state_loaded.contains(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        if need_org || !need_state.is_empty() {
            let conn_ids = need_state.clone();
            cx.defer_in(window, move |this, _window, cx| {
                let org_pending = need_org && !this.nav.borrow().groups_loaded;
                if org_pending {
                    this.reload_nav_org();
                }
                for cid in &conn_ids {
                    this.ensure_nav_state_loaded(cid);
                }
                cx.notify();
            });
        }
        let raw_search = self
            .nav_search
            .as_ref()
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default();
        // 搜索框 facet 语法（`scope:` / `type:` / `driver:` / `tag:`）拆为额外约束，
        // 其余为自由文本；与面板 chips 叠加（AND）而非写回，避免输入框反馈环。
        {
            let parsed = parse_nav_search(&raw_search);
            let mut view = self.nav.borrow_mut();
            view.filter = parsed.free.clone();
            view.search_facets = parsed;
        }

        // 连接行内联**标签**编辑器（`+` 打开）打开时，按需创建标签输入框
        // 并用当前标签预填；关闭时销毁，保证下次打开重新回填。
        let tag_editor_for = self.nav.borrow().tag_editor_for.clone();
        match &tag_editor_for {
            Some(conn_id) => {
                // 已为**本**连接建过则复用；否则重建并重新预填（曾在 A 开过再切 B 时
                // 会沿用 A 的输入值 → 回车把 A 的标签写到 B）。
                let stale = self.nav_tag_input.is_none()
                    || self.nav_tag_input_for.as_deref() != Some(conn_id.as_str());
                if stale {
                    let current =
                        crate::nav_store::list_tags(conn_id, self.host.project_root().as_deref());
                    let text = current.join(", ");
                    let input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("标签，逗号分隔"));
                    input.update(cx, |s, cx| s.set_value(text, window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_nav_tags(cx),
                            _ => {}
                        },
                    );
                    self.nav_tag_input = Some(input);
                    self.nav_tag_input_for = Some(conn_id.clone());
                    self._nav_tag_sub = Some(sub);
                }
            }
            None => {
                self.nav_tag_input = None;
                self.nav_tag_input_for = None;
                self._nav_tag_sub = None;
            }
        }

        // 连接行内「复制为模板」输入框：打开时创建并预填「原名 副本」，关闭时销毁。
        let copy_for = self.nav.borrow().copy_for.clone();
        match &copy_for {
            Some(conn_id) => {
                let stale = self.nav_copy_input.is_none()
                    || self.nav_copy_input_for.as_deref() != Some(conn_id.as_str());
                if stale {
                    let base = self
                        .host
                        .connections()
                        .iter()
                        .find(|c| c.id == *conn_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| conn_id.clone());
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("新连接名称"));
                    input.update(cx, |s, cx| s.set_value(format!("{base} 副本"), window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_copy_connection(cx),
                            _ => {}
                        },
                    );
                    self.nav_copy_input = Some(input);
                    self.nav_copy_input_for = Some(conn_id.clone());
                    self._nav_copy_sub = Some(sub);
                }
            }
            None => {
                self.nav_copy_input = None;
                self.nav_copy_input_for = None;
                self._nav_copy_sub = None;
            }
        }

        // 分组重命名输入框：打开时按名称预填，关闭时销毁。
        let rename_for = self.nav.borrow().group_rename_for.clone();
        match &rename_for {
            Some(group_id) => {
                if self.nav_group_input.is_none() {
                    let name = self
                        .nav
                        .borrow()
                        .groups
                        .iter()
                        .find(|g| &g.id == group_id)
                        .map(|g| g.name.clone())
                        .unwrap_or_default();
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder("分组名"));
                    input.update(cx, |s, cx| s.set_value(name, window, cx));
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this, _e, ev: &InputEvent, _w, cx| match ev {
                            InputEvent::Change => cx.notify(),
                            InputEvent::PressEnter { .. } => this.commit_group_rename(cx),
                            _ => {}
                        },
                    );
                    self.nav_group_input = Some(input);
                    self._nav_group_sub = Some(sub);
                }
            }
            None => {
                self.nav_group_input = None;
                self._nav_group_sub = None;
            }
        }

        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let accent = cx.theme().colors.primary;
        let source_filter = self.nav.borrow().source_filter;

        // 面板头「⟳ 刷新元数据」「断开当前连接」的作用目标：当前选中的连接。
        let current_conn = self.nav_current_connection();
        let current_connected = current_conn
            .as_deref()
            .map(|id| self.host.is_connected(id) || self.nav.borrow().connected.contains(id))
            .unwrap_or(false);
        let project_root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());

        let header = div()
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.875))
            .pl_2p5()
            .pr_2()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg)
                    .child("数据源"),
            )
            .child(div().flex_1())
            // C1 预热进度（后台任务进行中时显示，可取消）。
            .when(nav_jobs::warm_active(), |h| {
                let (done, total) = nav_jobs::warm_progress();
                h.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("预热 {done}/{total}")),
                )
                .child(
                    div()
                        .id("nav-warm-cancel")
                        .px_1()
                        .rounded_md()
                        .text_xs()
                        .text_color(muted)
                        .cursor_pointer()
                        .hover({
                            let h = cx.theme().colors.list_hover;
                            move |s| s.bg(h)
                        })
                        .child("取消")
                        .on_click(|_, _, _| nav_jobs::cancel_warm()),
                )
            })
            .child(
                // 新建数据源（＋）：与编辑区「新建连接」同一条对话框入口。
                Button::new("nav-new-connection")
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .on_click({
                        let host = self.host.clone();
                        move |_, window, app: &mut App| {
                            // 命令端口：直接开对话框（不再置位请求字段等渲染消费）。
                            host.new_connection(window, app);
                        }
                    }),
            )
            .child(
                // 新建分组（🗂＋）：项目级自定义分组。
                div()
                    .id("nav-new-group")
                    .px_1()
                    .rounded_md()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .hover({
                        let h = cx.theme().colors.list_hover;
                        move |s| s.bg(h)
                    })
                    .child("\u{1f5c2}\u{ff0b}")
                    .on_click({
                        let entity = cx.entity();
                        let host = self.host.clone();
                        let seed = GroupFormSeed::for_new(self.next_group_name());
                        move |_, window, app: &mut App| {
                            let e = entity.clone();
                            let host = host.clone();
                            let seed = seed.clone();
                            host.open_group_form(
                                seed,
                                window,
                                app,
                                Rc::new(move |gid, name, desc, app| {
                                    e.update(app, |this, cx| {
                                        this.save_group_form(gid, name, desc, cx);
                                    });
                                }),
                            );
                        }
                    }),
            )
            .child(
                // 刷新元数据（⟳）：刷新当前选中连接（设计 §4.2「单连接 = 工具栏 ⟳」，
                // 「全部」在「⋯」菜单）；未选中连接时禁用。
                Button::new("nav-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RotateCw)
                    .disabled(current_conn.is_none())
                    .on_click({
                        let entity = cx.entity();
                        let conn = current_conn.clone();
                        move |_, _, app| {
                            let Some(cid) = conn.clone() else {
                                return;
                            };
                            entity.update(app, |this, cx| {
                                this.refresh_node(&cid, &cid, Some(NavPath::Connection), cx);
                            });
                        }
                    }),
            )
            .child(
                // 断开当前连接（设计 §2.1 / 原型 title「断开当前连接」）：仅运行时已连接时
                // 可用；断开只关运行时连接，元数据缓存保留（可离线浏览 / 重连秒开）。
                Button::new("nav-disconnect")
                    .ghost()
                    .small()
                    .icon(
                        Icon::empty()
                            .path("icons/plug.svg")
                            .text_color(cx.theme().colors.danger),
                    )
                    .disabled(!current_connected)
                    .on_click({
                        let entity = cx.entity();
                        let conn = current_conn.clone();
                        let root = project_root.clone();
                        move |_, _, app| {
                            let Some(cid) = conn.clone() else {
                                return;
                            };
                            entity.update(app, |this, cx| {
                                this.toggle_connection(&cid, root.as_deref(), cx);
                            });
                        }
                    }),
            )
            .child(
                // 更多（⋯）：刷新全部元数据 / 缓存管理。
                Button::new("nav-more")
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu({
                        let entity = cx.entity();
                        let host_tags = self.host.clone();
                        let host_scope = self.host.clone();
                        let host_cache = self.host.clone();
                        let show_tags = self.host.show_tags(cx);
                        let show_scope = self.host.show_scope(cx);
                        move |menu, _window, _cx| {
                            let e_refresh = entity.clone();
                            let host_cache = host_cache.clone();
                            menu.item(PopupMenuItem::new("刷新全部元数据").on_click(
                                move |_, _, app| {
                                    e_refresh.update(app, |this, cx| this.refresh_all(cx));
                                },
                            ))
                            .separator()
                            .item(
                                PopupMenuItem::new(if show_tags {
                                    "✓ 显示标签"
                                } else {
                                    "显示标签"
                                })
                                .on_click({
                                    let host_tags = host_tags.clone();
                                    move |_, _, app| {
                                        host_tags.set_show_tags(!show_tags, app);
                                    }
                                }),
                            )
                            .item(
                                PopupMenuItem::new(if show_scope {
                                    "✓ 显示归属域"
                                } else {
                                    "显示归属域"
                                })
                                .on_click({
                                    let host_scope = host_scope.clone();
                                    move |_, _, app| {
                                        host_scope.set_show_scope(!show_scope, app);
                                    }
                                }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("缓存管理…").on_click(move |_, window, app| {
                                    host_cache.open_cache_dialog(window, app);
                                }),
                            )
                        }
                    }),
            );

        let chips = div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_1()
            .pl_2p5()
            .pr_2()
            .pb_1p5()
            .child(self.nav_source_chip("全部", None, source_filter, fg, muted, accent, cx))
            .child(self.nav_source_chip(
                "项目",
                Some(NavSource::Project),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "全局",
                Some(NavSource::Global),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ))
            .child(self.nav_source_chip(
                "共享",
                Some(NavSource::Shared),
                source_filter,
                fg,
                muted,
                accent,
                cx,
            ));

        let body = self.render_nav_tree(source_filter, cx);

        // 搜索行：输入框 + 「筛选 ▾ N」弹层（类型 / 驱动 / 标签；归属域由上方 chips 承担）。
        let active_facets = self.nav_active_facet_count();
        let facet_label = if active_facets > 0 {
            format!("筛选 ▾ {active_facets}")
        } else {
            "筛选 ▾".to_string()
        };
        let filters_active = self.nav_filters_active();
        let free_text = self.nav.borrow().search_facets.free.clone();
        let (cur_type, cur_driver, cur_tag) = {
            let view = self.nav.borrow();
            (
                view.type_filter.clone(),
                view.driver_filter.clone(),
                view.tag_filter.clone(),
            )
        };
        let (type_cands, driver_cands, tag_cands) = self.nav_facet_candidates();
        let tag_pairs: Vec<(String, String)> =
            tag_cands.iter().map(|t| (t.clone(), t.clone())).collect();
        let type_menu_label = match &cur_type {
            Some(t) => format!("类型：{}", nav_type_short_label(t)),
            None => "类型".to_string(),
        };
        let driver_menu_label = match &cur_driver {
            Some(d) => {
                let name = self
                    .driver_catalog
                    .borrow()
                    .get(d)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| d.clone());
                format!("驱动：{name}")
            }
            None => "驱动".to_string(),
        };
        let tag_menu_label = match &cur_tag {
            Some(t) => format!("标签：{t}"),
            None => "标签".to_string(),
        };

        let facet_button = Button::new("nav-facet-filter")
            .ghost()
            .small()
            .label(facet_label)
            .dropdown_menu({
                let entity = cx.entity();
                let input = self.nav_search.clone();
                let free = free_text.clone();
                let types = type_cands.clone();
                let drivers = driver_cands.clone();
                let tags = tag_pairs.clone();
                let ct = cur_type.clone();
                let cd = cur_driver.clone();
                let ctg = cur_tag.clone();
                let tl = type_menu_label.clone();
                let dl = driver_menu_label.clone();
                let gl = tag_menu_label.clone();
                move |menu, window, cx| {
                    let e_clear = entity.clone();
                    let input_clear = input.clone();
                    let free_clear = free.clone();
                    let mut menu = menu.item(
                        PopupMenuItem::new("清除筛选")
                            .disabled(!filters_active)
                            .on_click(move |_, window, app| {
                                if let Some(input) = &input_clear {
                                    let free = free_clear.clone();
                                    input
                                        .update(app, |s, cx| s.set_value(free.clone(), window, cx));
                                }
                                e_clear.update(app, |this, cx| this.clear_nav_filters(cx));
                            }),
                    );
                    menu = menu.separator();
                    menu = menu.submenu(tl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = ct.clone();
                        let c = types.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Type,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    menu = menu.submenu(dl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = cd.clone();
                        let c = drivers.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Driver,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    });
                    menu.submenu(gl.clone(), window, cx, {
                        let e = entity.clone();
                        let cur = ctg.clone();
                        let c = tags.clone();
                        move |m, _w, _c| {
                            Self::build_facet_items(
                                m,
                                e.clone(),
                                NavFacet::Tag,
                                cur.clone(),
                                c.clone(),
                            )
                        }
                    })
                }
            });

        let mut search_row = div().h_flex().items_center().w_full().gap_1().px_2().pb_2();
        if let Some(input) = &self.nav_search {
            search_row = search_row.child(div().flex_1().min_w_0().child(Input::new(input)));
        }
        search_row = search_row.child(facet_button);

        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .pt_1()
            .child(header)
            .child(search_row)
            .child(chips)
            .child(div().h(ui::HAIRLINE).w_full().bg(border))
            .child(body)
            .key_context("database-nav")
            .track_focus(&self.focus_handle)
            .on_action({
                let entity = cx.entity();
                move |_: &NavUp, _window, app| {
                    entity.update(app, |this, cx| this.nav_move(-1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavDown, _window, app| {
                    entity.update(app, |this, cx| this.nav_move(1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavExpand, _window, app| {
                    entity.update(app, |this, cx| this.nav_expand(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavCollapse, _window, app| {
                    entity.update(app, |this, cx| this.nav_collapse(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavOpenProperties, _window, app| {
                    entity.update(app, |this, cx| this.nav_open_properties(cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavReorderUp, _window, app| {
                    entity.update(app, |this, cx| this.nav_step_selected(-1, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &NavReorderDown, _window, app| {
                    entity.update(app, |this, cx| this.nav_step_selected(1, cx));
                }
            })
    }

    /// 来源筛选 chip（全部 / 项目 / 全局 / 共享）；点击切换筛选，不占一级结构。
    #[allow(clippy::too_many_arguments)]
    fn nav_source_chip(
        &self,
        label: &str,
        target: Option<NavSource>,
        current: Option<NavSource>,
        _fg: Hsla,
        muted: Hsla,
        accent: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = target == current;
        let entity = cx.entity();
        let (bg, text) = if active {
            (accent, cx.theme().colors.primary_foreground)
        } else {
            // 未选中：无底色（透明），靠 hover 灰层给出可点反馈。
            (transparent_black(), muted)
        };
        let weight = if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        // 未选中态在 hover 时给一层浅底，保持“可点”的反馈。
        let hover_bg = if active {
            accent
        } else {
            cx.theme().colors.list_hover
        };
        div()
            .id(format!(
                "nav-source-{}",
                target.map(|s| s.code()).unwrap_or("all")
            ))
            .h_flex()
            .items_center()
            .px_1p5()
            .py_0p5()
            .rounded_full()
            .bg(bg)
            .text_xs()
            .font_weight(weight)
            .text_color(text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .child(label.to_string())
            .on_click(move |_, _, app| {
                entity.update(app, |this, cx| {
                    this.nav.borrow_mut().source_filter = target;
                    this.write_nav_filters(cx);
                    cx.notify();
                });
            })
    }

    /// 写回 facet 筛选到 `settings.json`（chips 状态为准；搜索 token 不持久化）。
    fn write_nav_filters(&self, cx: &mut Context<Self>) {
        let filters = {
            let view = self.nav.borrow();
            NavFilters {
                source: view.source_filter.map(|s| s.key().to_string()),
                db_type: view.type_filter.clone(),
                driver: view.driver_filter.clone(),
                tag: view.tag_filter.clone(),
            }
        };
        self.host.set_nav_filters(filters, cx);
    }

    /// 应用某个 facet 值（`None` = 清除该项），并持久化 + 重渲染。
    fn apply_facet(&mut self, facet: NavFacet, value: Option<String>, cx: &mut Context<Self>) {
        {
            let mut view = self.nav.borrow_mut();
            match facet {
                NavFacet::Type => view.type_filter = value,
                NavFacet::Driver => view.driver_filter = value,
                NavFacet::Tag => view.tag_filter = value,
            }
        }
        self.write_nav_filters(cx);
        cx.notify();
    }

    /// 清除全部 facet 筛选（附加 facet + 归属域）。
    fn clear_nav_filters(&mut self, cx: &mut Context<Self>) {
        {
            let mut view = self.nav.borrow_mut();
            view.type_filter = None;
            view.driver_filter = None;
            view.tag_filter = None;
            view.source_filter = None;
        }
        self.write_nav_filters(cx);
        cx.notify();
    }

    /// 已生效的附加 facet 数（类型 / 驱动 / 标签；chips 与搜索 token 取并）。
    fn nav_active_facet_count(&self) -> usize {
        let view = self.nav.borrow();
        let mut n = 0;
        if view.type_filter.is_some() || view.search_facets.db_type.is_some() {
            n += 1;
        }
        if view.driver_filter.is_some() || view.search_facets.driver.is_some() {
            n += 1;
        }
        if view.tag_filter.is_some() || view.search_facets.tag.is_some() {
            n += 1;
        }
        n
    }

    /// 是否存在任何生效筛选（含归属域与搜索 token）；用于「清除筛选」可用性。
    fn nav_filters_active(&self) -> bool {
        let view = self.nav.borrow();
        view.source_filter.is_some()
            || view.type_filter.is_some()
            || view.driver_filter.is_some()
            || view.tag_filter.is_some()
            || view.search_facets.active > 0
    }

    /// 构建单个 facet 子菜单（「全部」+ 候选项，单选）。
    fn build_facet_items(
        menu: PopupMenu,
        entity: Entity<Self>,
        facet: NavFacet,
        current: Option<String>,
        candidates: Vec<(String, String)>,
    ) -> PopupMenu {
        let mut menu = menu.item(
            PopupMenuItem::new("全部")
                .checked(current.is_none())
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| {
                        entity.update(app, |this, cx| this.apply_facet(facet, None, cx));
                    }
                }),
        );
        for (value, label) in candidates {
            let checked = current.as_deref() == Some(value.as_str());
            let entity = entity.clone();
            menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                move |_, _, app| {
                    let value = value.clone();
                    entity.update(app, |this, cx| {
                        this.apply_facet(facet, Some(value.clone()), cx);
                    });
                },
            ));
        }
        menu
    }

    /// 计算 facet 候选清单（类型 / 驱动 / 标签），值 → 展示名，已排序去重。
    ///
    /// 类型 / 驱动以驱动目录为主、连接实际使用值为兜底（目录未就绪时不落空）；
    /// 标签来自已缓存的组织数据。
    fn nav_facet_candidates(&self) -> (Vec<(String, String)>, Vec<(String, String)>, Vec<String>) {
        use std::collections::{BTreeMap, BTreeSet};
        let mut types: BTreeMap<String, String> = BTreeMap::new();
        let mut drivers: BTreeMap<String, String> = BTreeMap::new();
        {
            let catalog = self.driver_catalog.borrow();
            for (id, meta) in catalog.iter() {
                drivers
                    .entry(id.clone())
                    .or_insert_with(|| meta.name.clone());
                types
                    .entry(meta.type_id.clone())
                    .or_insert_with(|| nav_type_short_label(&meta.type_id));
            }
        }
        let conns: Vec<ConnectionItem> = self.host.connections();
        {
            let catalog = self.driver_catalog.borrow();
            for c in &conns {
                let tid = catalog
                    .get(&c.driver)
                    .map(|m| m.type_id.clone())
                    .unwrap_or_else(|| c.driver.clone());
                types
                    .entry(tid.clone())
                    .or_insert_with(|| nav_type_short_label(&tid));
                drivers
                    .entry(c.driver.clone())
                    .or_insert_with(|| c.driver.clone());
            }
        }
        let tags: BTreeSet<String> = self
            .nav
            .borrow()
            .tags
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();
        let mut types: Vec<(String, String)> = types.into_iter().collect();
        types.sort_by(|a, b| a.1.cmp(&b.1));
        let mut drivers: Vec<(String, String)> = drivers.into_iter().collect();
        drivers.sort_by(|a, b| a.1.cmp(&b.1));
        (types, drivers, tags.into_iter().collect())
    }

    /// 树主体：一级为自定义分组（含「未分组」），下设连接节点。
    ///
    /// 连接可属于多个分组（多对多）：只在**主组**全亮呈现，其余分组以**引用行**出现
    /// （`∈ 主组名`），避免多对多线性撑高树；归属无任何分组的连接收进「未分组」。
    fn render_nav_tree(&self, source_filter: Option<NavSource>, cx: &mut Context<Self>) -> Div {
        // 键盘导航的可见序列每帧重建（渲染是顺序权威来源）。
        self.nav_order.borrow_mut().clear();
        let muted = cx.theme().colors.muted_foreground;
        let fg = cx.theme().colors.foreground;
        // 分组 / 成员 / 标签尚未就绪（首次渲染由 `render_nav` 的 defer 加载）。
        if !self.nav.borrow().groups_loaded {
            return div()
                .v_flex()
                .w_full()
                .min_h_0()
                .px_1()
                .pt_1()
                .child(div().text_xs().text_color(muted).child("加载中…"));
        }
        let (
            filter,
            groups,
            membership,
            group_order,
            ungrouped_order,
            tags,
            type_filter,
            driver_filter,
            tag_filter,
        ) = {
            let view = self.nav.borrow();
            (
                view.filter.to_lowercase(),
                view.groups.clone(),
                view.membership.clone(),
                view.group_order.clone(),
                view.ungrouped_order.clone(),
                view.tags.clone(),
                view.type_filter.clone(),
                view.driver_filter.clone(),
                view.tag_filter.clone(),
            )
        };
        let conns: Vec<ConnectionItem> = self.host.connections();
        let by_id: HashMap<&str, &ConnectionItem> =
            conns.iter().map(|c| (c.id.as_str(), c)).collect();

        // 搜索框 facet 语法（`scope:` / `type:` / `driver:` / `tag:`）作为额外约束叠加。
        let search_facets = self.nav.borrow().search_facets.clone();
        // 显式主组（连接 ID → 分组 ID）。
        let primary_explicit = self.nav.borrow().primary_group.clone();

        // 主组：用户**显式指定**优先；未指定时回退到分组排序最靠前的一个
        // （`membership` 按分组排序构建）。显式值若已不在所属分组（被移出）则忽略。
        // 主组用于「多组只全亮呈现一次，其余组以引用行出现」。
        let primary_gid =
            |conn_id: &str| nav_primary_scope(&membership, &primary_explicit, conn_id);

        // 连接级约束：归属域 chips + 附加 facet（类型 / 驱动 / 标签）。
        // 索引搜索也吃这一套（它回答“在哪些连接里找”），**不吃**搜索词——
        // 搜索词是**对象级**查询，若拿它去筛连接，搜表名时会因连接名不匹配而搜不到任何东西。
        let passes_facets = |conn: &ConnectionItem| -> bool {
            if let Some(src) = source_filter {
                if NavSource::from_conn_id(&conn.id) != src {
                    return false;
                }
            }
            if let Some(src) = search_facets.source {
                if NavSource::from_conn_id(&conn.id) != src {
                    return false;
                }
            }
            if let Some(want_type) = &type_filter {
                let actual = self
                    .driver_catalog
                    .borrow()
                    .get(&conn.driver)
                    .map(|m| m.type_id.clone())
                    .unwrap_or_else(|| conn.driver.clone());
                if &actual != want_type {
                    return false;
                }
            }
            if let Some(want_type) = &search_facets.db_type {
                let actual = self
                    .driver_catalog
                    .borrow()
                    .get(&conn.driver)
                    .map(|m| m.type_id.clone())
                    .unwrap_or_else(|| conn.driver.clone());
                if &actual != want_type {
                    return false;
                }
            }
            if let Some(want_driver) = &driver_filter {
                if &conn.driver != want_driver {
                    return false;
                }
            }
            if let Some(want_driver) = &search_facets.driver {
                if &conn.driver != want_driver {
                    return false;
                }
            }
            if let Some(want_tag) = &tag_filter {
                let hit = tags
                    .get(&conn.id)
                    .map(|ts| ts.iter().any(|t| t == want_tag))
                    .unwrap_or(false);
                if !hit {
                    return false;
                }
            }
            if let Some(want_tag) = &search_facets.tag {
                let hit = tags
                    .get(&conn.id)
                    .map(|ts| ts.iter().any(|t| t == want_tag))
                    .unwrap_or(false);
                if !hit {
                    return false;
                }
            }
            true
        };

        // 树里的连接行：在连接级约束之上，搜索词也顺手命中连接名 / 标签
        // （否则搜连接名时整棵树会被清空，看起来像坏了）。
        let passes = |conn: &ConnectionItem| -> bool {
            if !passes_facets(conn) {
                return false;
            }
            if filter.is_empty() {
                return true;
            }
            if conn.name.to_lowercase().contains(&filter) {
                return true;
            }
            tags.get(&conn.id)
                .map(|ts| ts.iter().any(|t| t.to_lowercase().contains(&filter)))
                .unwrap_or(false)
        };

        // ——— 索引搜索（树顶结果区）———
        // 本地过滤命中的是**已加载**节点（零往返、即时）；索引搜索补的是「尚未展开到的那部分」。
        // 只在查询词变化时排一次后台任务；过期批次在回填处丢弃。
        //
        // 方向（已拍板）：搜索 / 命令这类“先输入再选”的交互后续要**独立成一个 crate**
        // （类 VS Code 的 Quick Open / 命令面板），导航面板只保留这个“够用”版本；
        // 新特性不要再往这里加。
        let search_started = {
            let mut view = self.nav.borrow_mut();
            let query = filter.trim().to_string();
            if !nav_search_query_ready(&query) {
                view.search_query = None;
                view.search_hits.clear();
                view.search_searched = 0;
                None
            } else if view.search_query.as_deref() == Some(query.as_str()) {
                None
            } else {
                view.search_query = Some(query.clone());
                view.search_hits.clear();
                view.search_searched = 0;
                Some(query)
            }
        };
        if let Some(query) = search_started {
            let targets: Vec<nav_jobs::SearchTarget> = conns
                .iter()
                .filter(|c| passes_facets(c))
                .map(|c| nav_jobs::SearchTarget {
                    conn_id: c.id.clone(),
                    label: c.name.clone(),
                    driver: c.driver.clone(),
                })
                .collect();
            let root = self
                .host
                .project_root()
                .map(|p| p.to_string_lossy().to_string());
            nav_jobs::enqueue_search(
                nav_jobs::SearchConsumer::Navigator,
                nav_jobs::SearchKind::Name,
                &query,
                root.as_deref(),
                targets,
            );
            self.ensure_nav_pump(cx);
        }

        let mut column = div()
            .v_flex()
            .w_full()
            .min_h_0()
            .gap_0p5()
            .px_1()
            .pt_0p5()
            .pb_1();
        let mut shown = 0usize;

        // 运行时连接状态与错误集（分组头聚合健康度用；一次算完避免逐条查询）。
        let connected_set: HashSet<String> = {
            let mut set: HashSet<String> = self.nav.borrow().connected.iter().cloned().collect();
            for c in &conns {
                if c.connected {
                    set.insert(c.id.clone());
                }
            }
            set
        };
        let error_set: HashSet<String> = self.nav.borrow().errors.keys().cloned().collect();

        // 搜索结果区（树顶）：索引里搜到的对象，含尚未展开到的 schema / 连接。
        if let Some(section) = self.render_search_section(cx) {
            column = column.child(section);
        }

        for group in &groups {
            // 组内顺序以存储的手动排序为准（缺省无成员）。
            let members: Vec<&ConnectionItem> = group_order
                .get(&group.id)
                .map(|ids| {
                    ids.iter()
                        .filter_map(|id| by_id.get(id.as_str()).copied())
                        .filter(|c| passes(c))
                        .collect()
                })
                .unwrap_or_default();
            if members.is_empty() {
                continue;
            }
            shown += members.len();
            let ok_count = members
                .iter()
                .filter(|c| connected_set.contains(&c.id))
                .count();
            let fail_count = members.iter().filter(|c| error_set.contains(&c.id)).count();
            column = column.child(self.render_group_header(
                &group.id,
                &group.name,
                members.len(),
                ok_count,
                fail_count,
                cx,
            ));
            if !self.group_collapsed(&group.id) {
                for conn in members {
                    if primary_gid(&conn.id).as_deref() == Some(group.id.as_str()) {
                        column = column.child(self.render_connection_row(conn, &group.id, cx));
                    } else {
                        // 引用行：全亮行在主组，这里只指路。
                        let primary_name = primary_gid(&conn.id)
                            .and_then(|gid| {
                                groups.iter().find(|g| g.id == gid).map(|g| g.name.clone())
                            })
                            .unwrap_or_else(|| "未分组".to_string());
                        column = column.child(self.render_reference_row(
                            conn,
                            &group.id,
                            &primary_name,
                            cx,
                        ));
                    }
                }
            }
        }

        // 「未分组」固定分组：收纳不属于任何自定义分组的连接。
        // 顺序与分组内同一条规则（`nav_order_members`）：手动排序在前，未排过的按名称升序。
        let candidates: Vec<&ConnectionItem> = conns
            .iter()
            .filter(|c| {
                membership
                    .get(&c.id)
                    .map(|gs| gs.is_empty())
                    .unwrap_or(true)
                    && passes(c)
            })
            .collect();
        let ranked: HashMap<&str, i64> = ungrouped_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i as i64))
            .collect();
        let stored: Vec<(String, Option<i64>)> = candidates
            .iter()
            .map(|c| (c.id.clone(), ranked.get(c.id.as_str()).copied()))
            .collect();
        let ungrouped: Vec<&ConnectionItem> = nav_order_members(&stored, |id| {
            by_id
                .get(id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        })
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect();
        // 已有自定义分组时也渲染（空）未分组头：它是「拖拽移出分组」的常驻落点。
        let groups_shown = shown > 0;
        if !ungrouped.is_empty() || (!groups.is_empty() && groups_shown) {
            shown += ungrouped.len();
            let ok_count = ungrouped
                .iter()
                .filter(|c| connected_set.contains(&c.id))
                .count();
            let fail_count = ungrouped
                .iter()
                .filter(|c| error_set.contains(&c.id))
                .count();
            column = column.child(self.render_group_header(
                GROUP_UNGROUPED,
                "未分组",
                ungrouped.len(),
                ok_count,
                fail_count,
                cx,
            ));
            if !self.group_collapsed(GROUP_UNGROUPED) {
                for conn in ungrouped {
                    column = column.child(self.render_connection_row(conn, GROUP_UNGROUPED, cx));
                }
            }
        }

        if shown == 0 {
            if conns.is_empty() {
                // 空态引导（设计 §2.4）：标题 + 说明 + 面板内「新建连接」按钮。
                let host = self.host.clone();
                column = column.child(
                    div()
                        .w_full()
                        .pt_5()
                        .px_3()
                        .v_flex()
                        .items_start()
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(fg)
                                .child("还没有数据源"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("当前项目与全局库均无连接。点击右上角「＋」新建。"),
                        )
                        .child(
                            Button::new("nav-empty-new-connection")
                                .primary()
                                .small()
                                .icon(IconName::Plus)
                                .label("新建连接")
                                .on_click(move |_, window, app: &mut App| {
                                    host.new_connection(window, app);
                                }),
                        ),
                );
            } else if self.nav.borrow().search_hits.is_empty() {
                // 有命中时不说这句：结果区在树顶已经给了答案，
                // 同时出现“没有匹配的数据源”与一批命中会自相矛盾。
                column = column.child(
                    div()
                        .w_full()
                        .pt_5()
                        .px_3()
                        .text_xs()
                        .text_color(muted)
                        .child("没有匹配的数据源。"),
                );
            }
        }
        column
    }

    /// 分组头：左侧统一色条 + 略深底 + **健康度**（已连/总）+ 计数 + 全折叠；右键菜单（重命名 / 新建 / 删除）。
    fn render_group_header(
        &self,
        group_id: &str,
        name: &str,
        count: usize,
        connected_count: usize,
        failed_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let danger = cx.theme().colors.danger;
        let bar = cx.theme().colors.primary;
        let bg = cx.theme().colors.sidebar_accent;
        let hover = cx.theme().colors.list_hover;
        let collapsed = self.group_collapsed(group_id);
        let entity = cx.entity();
        let gid = group_id.to_string();
        let gname = name.to_string();
        let is_ungrouped = group_id == GROUP_UNGROUPED;
        let health_text = format!("{connected_count}/{count}");
        // 分组表单初值：新建用自动去重默认名，编辑预填现有名称与描述。
        let default_group_name = self.next_group_name();
        let group_seed = self.group_form_seed(group_id);
        // 上移 / 下移分组：边界项禁用（拖拽是主路径，菜单项是键盘 / 无鼠标的替代）。
        let group_ids_now = self.group_ids();
        let group_ix = group_ids_now.iter().position(|id| id == group_id);
        let can_up = group_ix.is_some_and(|i| i > 0);
        let can_down = group_ix.is_some_and(|i| i + 1 < group_ids_now.len());

        let mut header = div()
            .id(format!("nav-group-{group_id}"))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.5))
            .px_1()
            .gap_1()
            .rounded_md()
            .bg(bg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .on_click({
                let entity = entity.clone();
                let gid = gid.clone();
                move |_, _, app| {
                    let gid = gid.clone();
                    entity.update(app, |this, cx| {
                        {
                            let mut view = this.nav.borrow_mut();
                            if view.collapsed_groups.contains(&gid) {
                                view.collapsed_groups.remove(&gid);
                            } else {
                                view.collapsed_groups.insert(gid.clone());
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .w(ui::NAV_GROUP_BAR_WIDTH)
                    .h(rems(0.875))
                    .flex_none()
                    .rounded_full()
                    .bg(bar),
            )
            .child(
                div()
                    .w_2p5()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(if collapsed { "\u{25b8}" } else { "\u{25be}" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_ellipsis()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child(name.to_string()),
            )
            // 聚合健康度（v5）：已连接/总数；有失败时附 danger 计数点。
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if failed_count > 0 { danger } else { muted })
                    .child(if failed_count > 0 {
                        format!("{health_text} · {failed_count} 失败")
                    } else {
                        health_text
                    }),
            )
            .child(div().text_xs().text_color(muted).child(count.to_string()))
            // 全折叠：一键折叠 / 展开全部同层分组（v5）。
            .child(
                div()
                    .id(format!("nav-group-foldall-{group_id}"))
                    .w_2p5()
                    .h_2p5()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover))
                    .child("\u{21c5}")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app: &mut App| {
                            entity.update(app, |this, cx| {
                                let ids: Vec<String> = this
                                    .nav
                                    .borrow()
                                    .groups
                                    .iter()
                                    .map(|g| g.id.clone())
                                    .collect();
                                {
                                    let mut view = this.nav.borrow_mut();
                                    let all_collapsed = !ids.is_empty()
                                        && ids.iter().all(|id| view.collapsed_groups.contains(id));
                                    if all_collapsed {
                                        view.collapsed_groups.clear();
                                    } else {
                                        for id in ids {
                                            view.collapsed_groups.insert(id);
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }),
            );

        // 分组之间可拖排序（「未分组」是伪分组，不可拖）。
        if !is_ungrouped {
            header = header.on_drag(
                NavGroupDragPayload {
                    group_id: gid.clone(),
                    name: gname.clone(),
                },
                {
                    let tint = bar;
                    move |payload, _, _, cx| {
                        let label = payload.name.clone();
                        cx.new(|_| NavDragGhost { label, tint })
                    }
                },
            );
        }

        // 后续链接返回的是 `ContextMenu<..>`（不再与 `Stateful<Div>` 同型），故遮罩重绑。
        let header = header
            // 落点①（分组拖拽）：排到该分组之前；「未分组」头 = 排到最后。
            .drag_over::<NavGroupDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let entity = entity.clone();
                let before = if is_ungrouped {
                    None
                } else {
                    Some(gid.clone())
                };
                move |payload: &NavGroupDragPayload, _, app| {
                    let before = before.clone();
                    entity.update(app, |this, cx| {
                        this.apply_group_drop(payload, before.as_deref(), cx)
                    });
                }
            })
            // 落点②（组归拖拽）：拖到分组头 = 归组；拖到「未分组」头 = 移出全部分组。
            .drag_over::<NavConnDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let entity = entity.clone();
                let gid = gid.clone();
                move |payload: &NavConnDragPayload, _, app| {
                    let gid = gid.clone();
                    entity.update(app, |this, cx| {
                        this.apply_conn_drop(payload, &gid, ConnDropTarget::Container, cx)
                    });
                }
            })
            .context_menu({
                let entity = entity.clone();
                let host = self.host.clone();
                let gid = gid.clone();
                let gname = gname.clone();
                move |menu, _window, _cx| {
                    if is_ungrouped {
                        return menu.item(PopupMenuItem::new("新建分组").on_click({
                            let entity = entity.clone();
                            let host = host.clone();
                            let seed = GroupFormSeed::for_new(default_group_name.clone());
                            move |_, window, app| {
                                let e = entity.clone();
                                let seed = seed.clone();
                                host.open_group_form(
                                    seed,
                                    window,
                                    app,
                                    Rc::new(move |gid, name, desc, app| {
                                        e.update(app, |this, cx| {
                                            this.save_group_form(gid, name, desc, cx);
                                        });
                                    }),
                                );
                            }
                        }));
                    }
                    let e_rename = entity.clone();
                    let gid_rename = gid.clone();
                    let e_new = entity.clone();
                    let seed_new = GroupFormSeed::for_new(default_group_name.clone());
                    let e_desc = entity.clone();
                    let seed_desc = group_seed.clone();
                    let e_del = entity.clone();
                    let gid_del = gid.clone();
                    let gname_del = gname.clone();
                    menu.item(PopupMenuItem::new("重命名分组").on_click(move |_, _, app| {
                        let gid = gid_rename.clone();
                        e_rename.update(app, |this, cx| {
                            this.nav.borrow_mut().group_rename_for = Some(gid.clone());
                            cx.notify();
                        });
                    }))
                    // 名称 + 描述一次编辑（行内重命名只改名称，这里补上描述）。
                    .item(PopupMenuItem::new("编辑分组…").on_click({
                        let host = host.clone();
                        move |_, window, app| {
                            let e = e_desc.clone();
                            let seed = seed_desc.clone();
                            host.open_group_form(
                                seed,
                                window,
                                app,
                                Rc::new(move |gid, name, desc, app| {
                                    e.update(app, |this, cx| {
                                        this.save_group_form(gid, name, desc, cx);
                                    });
                                }),
                            );
                        }
                    }))
                    .item(PopupMenuItem::new("新建分组").on_click({
                        let host = host.clone();
                        move |_, window, app| {
                            let e = e_new.clone();
                            let seed = seed_new.clone();
                            host.open_group_form(
                                seed,
                                window,
                                app,
                                Rc::new(move |gid, name, desc, app| {
                                    e.update(app, |this, cx| {
                                        this.save_group_form(gid, name, desc, cx);
                                    });
                                }),
                            );
                        }
                    }))
                    .separator()
                    // 分组排序：拖拽是主路径，这两项是键盘 / 无鼠标时的替代（边界置灰）。
                    .item(PopupMenuItem::new("上移分组").disabled(!can_up).on_click({
                        let e = entity.clone();
                        let gid = gid.clone();
                        move |_, _, app| {
                            let gid = gid.clone();
                            e.update(app, |this, cx| this.step_group(&gid, -1, cx));
                        }
                    }))
                    .item(
                        PopupMenuItem::new("下移分组")
                            .disabled(!can_down)
                            .on_click({
                                let e = entity.clone();
                                let gid = gid.clone();
                                move |_, _, app| {
                                    let gid = gid.clone();
                                    e.update(app, |this, cx| this.step_group(&gid, 1, cx));
                                }
                            }),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("删除分组").on_click(move |_, window, app| {
                            let entity = e_del.clone();
                            let gid = gid_del.clone();
                            let gname = gname_del.clone();
                            window.open_alert_dialog(app, move |alert, _window, _cx| {
                                let entity = entity.clone();
                                let gid = gid.clone();
                                alert
                                    .confirm()
                                    .title("删除分组")
                                    .description(format!(
                                        "确定删除分组「{gname}」？成员连接与缓存不会被删除。"
                                    ))
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text("删除")
                                            .ok_variant(ButtonVariant::Danger)
                                            .show_cancel(true),
                                    )
                                    .on_ok(move |_, _window, app| {
                                        entity.update(app, |this, cx| this.delete_group(&gid, cx));
                                        true
                                    })
                            });
                        }),
                    )
                }
            });

        let mut wrap = div().v_flex().w_full().gap_0p5();
        wrap = wrap.child(header);
        // 重命名内联输入（打开时创建，见 `render_nav`）。
        if self.nav.borrow().group_rename_for.as_deref() == Some(group_id) {
            if let Some(input) = &self.nav_group_input {
                wrap = wrap.child(div().px_1().pb_0p5().child(Input::new(input)));
            }
        }
        wrap
    }

    /// 引用行（v6）：连接已在其**主组**全亮呈现，此分组下只做指路。
    ///
    /// 不重复状态 / 徽标 / 操作位（避免重复向）；点击展开主组并选中该连接。
    /// `group_id` 为**当前包含它的分组**（用于唯一元素 ID），`primary_name` 为主组名。
    fn render_reference_row(
        &self,
        conn: &ConnectionItem,
        group_id: &str,
        primary_name: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let entity = cx.entity();
        let conn_id = conn.id.clone();
        let tint = muted;
        let drag_payload = NavConnDragPayload {
            conn_id: conn.id.clone(),
            name: conn.name.clone(),
        };
        let drop_scope = group_id.to_string();
        let drop_before = conn.id.clone();
        let primary_gid = self
            .nav
            .borrow()
            .membership
            .get(&conn.id)
            .and_then(|gs| gs.first().cloned());
        let label = format!("\u{2208} {primary_name}");
        div()
            .id(format!("nav-ref-{}::{}", group_id, conn.id))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.625))
            .px_1()
            .gap_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(div().w_2p5().flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .text_ellipsis()
                    .child(conn.name.clone()),
            )
            .child(div().flex_none().text_xs().text_color(muted).child(label))
            // 引用行同样可拖（否则“全组成员都是引用行”的分组无法重排）
            // 与可落：落到引用行 = 加入该分组并插到它之前。
            .on_drag(drag_payload, move |payload, _, _, cx| {
                let label = payload.name.clone();
                cx.new(|_| NavDragGhost { label, tint })
            })
            .drag_over::<NavConnDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let entity = entity.clone();
                let scope = drop_scope.clone();
                let before = drop_before.clone();
                move |payload: &NavConnDragPayload, _, app| {
                    let scope = scope.clone();
                    let before = before.clone();
                    entity.update(app, |this, cx| {
                        this.apply_conn_drop(payload, &scope, ConnDropTarget::BeforeRow(before), cx)
                    });
                }
            })
            .on_click(move |_, _, app| {
                let cid = conn_id.clone();
                entity.update(app, |this, cx| {
                    {
                        let mut view = this.nav.borrow_mut();
                        // 展开主组（若已折叠）并选中该连接，使全亮行可见。
                        if let Some(gid) = &primary_gid {
                            view.collapsed_groups.remove(gid);
                        }
                        view.selected_key = Some(cid.clone());
                    }
                    cx.notify();
                });
            })
    }

    /// 连接节点行（徽标（色=状态·形=类型） + 名称 + 归属域列 + 行尾操作）。
    ///
    /// `scope_key` 为其所属分组标识：同一连接可出现在多个分组，用于生成唯一元素 ID。
    fn render_connection_row(
        &self,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let danger = cx.theme().colors.danger;

        let expanded = {
            let view = self.nav.borrow();
            view.expanded.contains(&conn.id)
        };
        let (connected, error, children) = {
            let view = self.nav.borrow();
            (
                view.connected.contains(&conn.id) || conn.connected,
                view.errors.get(&conn.id).cloned(),
                view.children.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        // 仅在**运行时已连接**时才在渲染期排后台加载。
        // 为何加这道门：展开态会跨重启从 `navigator_state` 恢复，但运行时连接不跨重启；
        // 若此时仍排队，`NavigatorService` → `MetadataService` 取不到句柄，
        // 会冒泡为用户看到的 `[CONN_NOT_FOUND]`。
        if expanded && connected {
            self.ensure_nav_loaded(&conn.id, &conn.id, NavPath::Connection, false, cx);
        }

        let source = NavSource::from_conn_id(&conn.id);
        // 来源标识：短码 `P/G/GP` 或文字（设置项，默认短码）。
        let source_text = if self.host.source_short_code(cx) {
            source.code().to_string()
        } else {
            source.label().to_string()
        };
        let filter = self.nav.borrow().filter.to_lowercase();
        let match_bg = product_tokens::get(cx).search_match_background(cx.theme());
        let project_root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        let conn_id = conn.id.clone();
        let scope_key = scope_key.to_string();
        let selected = self.nav.borrow().selected_key.as_deref() == Some(conn.id.as_str());
        let selected_bg = cx.theme().colors.list_active;
        // 键盘导航序列（父在前，子随渲染加入）。
        self.nav_order.borrow_mut().push(NavOrderItem {
            key: conn.id.clone(),
            conn_id: conn.id.clone(),
            path: Some(NavPath::Connection),
            property: Some(PropertyRef {
                conn_id: conn.id.clone(),
                source,
                catalog: None,
                schema: None,
                parent: None,
                name: conn.name.clone(),
                kind: PropertyKind::Connection,
            }),
            has_children: true,
            expanded,
        });

        // ---- v7：行内只常驻「徽标 + 名称 + 归属域列」；`+` 与行操作仅 hover / 选中显 ----
        let nav_view = self
            .driver_catalog
            .borrow()
            .get(&conn.driver)
            .map(|m| (m.type_id.clone(), m.name.clone()));
        let type_id = nav_view
            .as_ref()
            .map(|(t, _)| t.clone())
            .unwrap_or_else(|| conn.driver.clone());
        let driver_name = nav_view.map(|(_, n)| n);
        let nav_view = self.nav.borrow();
        let badge_status = if nav_view.loading.contains(&conn.id) {
            NavBadgeStatus::Connecting
        } else if error.is_some() {
            NavBadgeStatus::Failed
        } else if connected {
            NavBadgeStatus::Connected
        } else {
            NavBadgeStatus::Idle
        };
        let tag_list: Vec<String> = nav_view.tags.get(&conn.id).cloned().unwrap_or_default();
        drop(nav_view);

        let (badge_path, badge_letters) = nav_type_badge(&type_id);
        let badge_color = badge_status.color(cx.theme());
        // 双通道徽标：颜色 = 状态（能不能用），形状 = 类型（是什么库，内叠 2 字母）。
        let badge = div()
            .relative()
            .w(rems(ui::NAV_BADGE_SIZE))
            .h(rems(ui::NAV_BADGE_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::empty()
                            .path(badge_path)
                            .size(rems(ui::NAV_BADGE_SIZE))
                            .text_color(badge_color),
                    ),
            )
            .child(
                div()
                    .relative()
                    .text_size(rems(0.4))
                    .font_weight(FontWeight::BOLD)
                    .text_color(badge_color)
                    .child(badge_letters),
            );

        // 徽标 hover 卡：类型 / 状态 / 驱动的完整事实（gpui-kit 0.6.1 无通用 `.tooltip` 扩展，
        // 故用 `HoverCard` 承载；行内仍只显颜色 + 形状）。
        let badge = {
            let type_label = nav_type_label(&type_id);
            let status_label = badge_status.label();
            let driver_label = driver_name
                .clone()
                .map(|n| format!("{n} · {}", conn.driver))
                .unwrap_or_else(|| conn.driver.clone());
            let hover_id = SharedString::from(format!("nav-badge-{}::{}", scope_key, conn.id));
            nav_badge_hover_card(hover_id, badge, type_label, status_label, driver_label)
        };

        // 标签 chip（可选显示，`⋯ → 显示标签`）：默认关；开启后「≤2 chip + `+N`」。
        let tag_chips = if self.host.show_tags(cx) && !tag_list.is_empty() {
            let chip_bg = cx.theme().colors.list_hover;
            // 标签字号用最初版小字 `text_xs`（与行内文字同尺寸，不因换行而变大）。
            let mut chips = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .text_xs();
            for t in tag_list.iter().take(2) {
                chips = chips.child(
                    div()
                        .px_1()
                        .rounded_sm()
                        .bg(chip_bg)
                        .text_color(muted)
                        .child(t.clone()),
                );
            }
            if tag_list.len() > 2 {
                chips = chips.child(
                    div()
                        .text_color(muted)
                        .child(format!("+{}", tag_list.len() - 2)),
                );
            }
            Some(chips)
        } else {
            None
        };

        // 归属域短码：右对齐固定列（可在 `⋯ → 显示归属域` 关闭）。
        let scope_visible = self.host.show_scope(cx);
        let scope_col = if self.host.source_short_code(cx) {
            ui::NAV_SCOPE_COL_SHORT
        } else {
            ui::NAV_SCOPE_COL_TEXT
        };
        let scope_color = match source {
            NavSource::Project => cx.theme().colors.info,
            NavSource::Global => muted,
            NavSource::Shared => cx.theme().colors.primary,
        };

        // 行尾操作（v8）：`+` 加标签 · `✎` 编辑；仅 hover / 选中显（连接 / 断开走右键菜单）。
        let ops = {
            let entity = cx.entity();
            let host = self.host.clone();
            let cid = conn_id.clone();
            let hover_bg = cx.theme().colors.list_hover;
            let mut ops = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .opacity(if selected { 1.0 } else { 0.0 })
                .group_hover("nav-conn-row", |s| s.opacity(1.0));
            // `+`：仅**标签**行内编辑（`+` 只处理标签；归组走右键「移动到分组…」）。
            ops = ops.child(
                div()
                    .id(format!("nav-conn-addtag-{}::{}", scope_key, conn.id))
                    .w(rems(ui::NAV_ADD_TAG_SIZE))
                    .h(rems(ui::NAV_ADD_TAG_SIZE))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover_bg))
                    .child(Icon::empty().path("icons/plus.svg").size(rems(0.75)))
                    .on_click({
                        let entity = entity.clone();
                        let cid = cid.clone();
                        move |_, _, app: &mut App| {
                            let cid = cid.clone();
                            entity.update(app, |this, cx| {
                                let mut view = this.nav.borrow_mut();
                                view.tag_editor_for =
                                    if view.tag_editor_for.as_deref() == Some(cid.as_str()) {
                                        None
                                    } else {
                                        Some(cid.clone())
                                    };
                                cx.notify();
                            });
                        }
                    }),
            );
            // `✎`：在对话框中编辑连接。
            ops = ops.child(
                div()
                    .id(format!("nav-conn-edit-{}::{}", scope_key, conn.id))
                    .px_1()
                    .text_xs()
                    .text_color(muted)
                    .cursor_pointer()
                    .rounded_md()
                    .hover(move |s| s.bg(hover_bg))
                    .child("\u{270e}")
                    .on_click({
                        let host = host.clone();
                        let cid = cid.clone();
                        move |_, window, app: &mut App| {
                            host.edit_connection(&cid, window, app);
                        }
                    }),
            );
            // 连接 / 断开不进 hover 行操作：仅右键菜单提供（避免误触与视觉噪声）。
            ops
        };

        // 徽标 tooltip 材料（接入 `Tooltip` 组件前，事实统一在属性面板展示）。
        let _ = (&driver_name, danger);

        // 「设为主组」子菜单数据（仅归组的连接出现）：所属分组 + 当前主组 + 是否显式。
        let (menu_groups, menu_primary, menu_primary_explicit) = {
            let view = self.nav.borrow();
            let gids = view.membership.get(&conn.id).cloned().unwrap_or_default();
            let explicit = view
                .primary_group
                .get(&conn.id)
                .filter(|p| gids.iter().any(|g| g == *p))
                .cloned();
            let primary = explicit.clone().or_else(|| gids.first().cloned());
            let names: Vec<(String, String)> = gids
                .iter()
                .map(|gid| {
                    let name = view
                        .groups
                        .iter()
                        .find(|g| &g.id == gid)
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| gid.clone());
                    (gid.clone(), name)
                })
                .collect();
            (names, primary, explicit.is_some())
        };

        let mut block = div().v_flex().w_full();
        block = block.child(
            div()
                .id(format!("nav-conn-{}::{}", scope_key, conn.id))
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(1.625))
                .px_1()
                .gap_1()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |s| s.bg(selected_bg))
                .hover(move |s| s.bg(hover))
                .on_click({
                    let entity = cx.entity();
                    let conn_id = conn_id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    let focus = self.focus_handle.clone();
                    move |ev, window, app| {
                        focus.focus(window, app);
                        // 单击选中（键盘导航基准）；双击打开属性；再次点击展开 / 折叠。
                        let sel_key = conn_id.clone();
                        entity.update(app, |this, cx| {
                            this.nav.borrow_mut().selected_key = Some(sel_key.clone());
                            cx.notify();
                        });
                        if ev.click_count() >= 2 {
                            let req = PropertyRequest {
                                property: PropertyRef {
                                    conn_id: conn_id.clone(),
                                    source,
                                    catalog: None,
                                    schema: None,
                                    parent: None,
                                    name: conn_name.clone(),
                                    kind: PropertyKind::Connection,
                                },
                                conn_label: conn_name.clone(),
                                driver: conn_driver.clone(),
                            };
                            entity.update(app, |this, cx| {
                                this.host.show_properties(req, cx);
                                cx.notify();
                            });
                            return;
                        }
                        let conn_id = conn_id.clone();
                        entity.update(app, |this, cx| {
                            this.toggle_nav_node(&conn_id, &conn_id, NavPath::Connection, cx);
                            cx.notify();
                        });
                    }
                })
                .child(
                    div()
                        .w_2p5()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(if expanded { "\u{25be}" } else { "\u{25b8}" }),
                )
                .group("nav-conn-row")
                .child(badge)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(fg)
                        .text_ellipsis()
                        .child(nav_name_highlight(&conn.name, &filter, match_bg, fg)),
                )
                .when(scope_visible, |s| {
                    s.child(
                        div()
                            .w(rems(scope_col))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(scope_color)
                            .child(source_text.clone()),
                    )
                })
                // 行尾操作组（`+` / `✎` / 连接·断开）：仅 hover / 选中显（v8）。
                .child(ops)
                // 拖拽（归组 + 组内排序）：拖起本行；落点 = 行（插到该行之前）/ 分组头（见 `render_group_header`）。
                .on_drag(
                    NavConnDragPayload {
                        conn_id: conn.id.clone(),
                        name: conn.name.clone(),
                    },
                    {
                        let tint = badge_color;
                        move |payload, _, _, cx| {
                            let label = payload.name.clone();
                            cx.new(|_| NavDragGhost { label, tint })
                        }
                    },
                )
                .drag_over::<NavConnDragPayload>(|style, _, _, cx| {
                    style.bg(cx.theme().colors.list_active)
                })
                .on_drop({
                    let entity = cx.entity();
                    let scope = scope_key.clone();
                    let before = conn.id.clone();
                    move |payload: &NavConnDragPayload, _, app| {
                        let scope = scope.clone();
                        let before = before.clone();
                        entity.update(app, |this, cx| {
                            this.apply_conn_drop(
                                payload,
                                &scope,
                                ConnDropTarget::BeforeRow(before),
                                cx,
                            )
                        });
                    }
                })
                // 右键菜单（连接节点）：连接/断开、编辑、查看属性、分组/标签、复制、刷新。
                .context_menu({
                    let entity = cx.entity();
                    let host = self.host.clone();
                    let conn_id = conn.id.clone();
                    let conn_name = conn.name.clone();
                    let conn_driver = conn.driver.clone();
                    let root = project_root.clone();
                    let prop = PropertyRef {
                        conn_id: conn_id.clone(),
                        source,
                        catalog: None,
                        schema: None,
                        parent: None,
                        name: conn_name.clone(),
                        kind: PropertyKind::Connection,
                    };
                    let is_connected = connected;
                    move |menu, window, cx| {
                        let e_connect = entity.clone();
                        let cid_connect = conn_id.clone();
                        let root_connect = root.clone();
                        let e_edit = entity.clone();
                        let cid_edit = conn_id.clone();
                        let e_prop = entity.clone();
                        let prop_own = prop.clone();
                        let label_prop = conn_name.clone();
                        let drv_prop = conn_driver.clone();
                        let e_org = entity.clone();
                        let cid_org = conn_id.clone();
                        let e_copy = entity.clone();
                        let host_copy = host.clone();
                        let name_copy = conn_name.clone();
                        let e_refresh = entity.clone();
                        let cid_refresh = conn_id.clone();
                        let name_refresh = conn_name.clone();
                        let mut menu = menu
                            .item(
                                PopupMenuItem::new(if is_connected { "断开" } else { "连接" })
                                    .on_click(move |_, _, app| {
                                        let cid = cid_connect.clone();
                                        let root = root_connect.clone();
                                        e_connect.update(app, |this, cx| {
                                            this.toggle_connection(&cid, root.as_deref(), cx)
                                        });
                                    }),
                            )
                            .item(PopupMenuItem::new("测试连接").on_click({
                                let e = entity.clone();
                                let cid = conn_id.clone();
                                let root = root.clone();
                                let name = conn_name.clone();
                                // 探测入口是函数指针；句柄在这里克隆一份，避免把 `self` 借进 'static 闭包。
                                let host = host.clone();
                                move |_, _, app| {
                                    // 独立会话探测（不注册连接池 / 不写库）；结果落面板提示。
                                    // 探测入口是函数指针（可跨到工作线程）；视图搬入 `database`
                                    // 后改由宿主端口供给（`NavHost::connection_probe`）。
                                    nav_jobs::enqueue_test_connection(
                                        &cid,
                                        root.as_deref(),
                                        &name,
                                        host.connection_probe(),
                                    );
                                    e.update(app, |this, cx| this.ensure_nav_pump(cx));
                                }
                            }))
                            .item(PopupMenuItem::new("编辑连接…").on_click(move |_, window, app| {
                                let cid = cid_edit.clone();
                                e_edit.update(app, |this, cx| {
                                    // 命令端口：直接开对话框（不再置位请求字段等渲染消费）。
                                    this.host.edit_connection(&cid, window, cx);
                                });
                            }))
                            .separator()
                            .item(PopupMenuItem::new("查看属性").on_click(move |_, _, app| {
                                let prop = prop_own.clone();
                                let label = label_prop.clone();
                                let drv = drv_prop.clone();
                                e_prop.update(app, |this, cx| {
                                    this.host.show_properties(
                                        PropertyRequest {
                                            property: prop.clone(),
                                            conn_label: label.clone(),
                                            driver: drv.clone(),
                                        },
                                        cx,
                                    );
                                    cx.notify();
                                });
                            }))
                            .item(
                                PopupMenuItem::new("移动到分组…").on_click(move |_, _, app| {
                                    let cid = cid_org.clone();
                                    e_org.update(app, |this, cx| {
                                        this.nav.borrow_mut().group_picker_for =
                                            Some(cid.clone());
                                        cx.notify();
                                    });
                                }),
                            );
                        // 「设为主组 ▸」：仅在该连接已归组时出现（单选 + 「自动」回退）。
                        if !menu_groups.is_empty() {
                            let e_p = entity.clone();
                            let root_p = root.clone();
                            let cid_p = conn_id.clone();
                            let groups_for = menu_groups.clone();
                            let cur = menu_primary.clone();
                            let explicit = menu_primary_explicit;
                            menu = menu.submenu("设为主组", window, cx, move |m, _w, _c| {
                                let mut m = m.item(
                                    PopupMenuItem::new("自动（按分组排序）")
                                        .checked(!explicit)
                                        .on_click({
                                            let e = e_p.clone();
                                            let root = root_p.clone();
                                            let cid = cid_p.clone();
                                            move |_, _, app| {
                                                let root = root.clone();
                                                let cid = cid.clone();
                                                let root = root.as_deref().map(std::path::Path::new);
                                                e.update(app, |this, cx| {
                                                    let _ = crate::nav_store::clear_primary_group(
                                                        root,
                                                        &cid,
                                                    );
                                                    this.reload_nav_org();
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                );
                                for (gid, gname) in groups_for.clone() {
                                    let checked = cur.as_deref() == Some(gid.as_str());
                                    let e = e_p.clone();
                                    let root = root_p.clone();
                                    let cid = cid_p.clone();
                                    m = m.item(
                                        PopupMenuItem::new(gname).checked(checked).on_click(
                                            move |_, _, app| {
                                                let gid = gid.clone();
                                                let root = root.clone();
                                                let cid = cid.clone();
                                                let root = root.as_deref().map(std::path::Path::new);
                                                e.update(app, |this, cx| {
                                                    let _ =
                                                        crate::nav_store::set_primary_group(
                                                            root,
                                                            &cid,
                                                            &gid,
                                                        );
                                                    this.reload_nav_org();
                                                    cx.notify();
                                                });
                                            },
                                        ),
                                    );
                                }
                                m
                            });
                        }
                        // 复制（模板）/ 共享 / 删除：连接自身的管理动作。
                        // 共享快照（GP_）本身即副本，不提供复制。
                        if source != NavSource::Shared {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new("复制连接（模板）…").on_click(
                                    move |_, _, app| {
                                        let cid = cid.clone();
                                        e.update(app, |this, cx| {
                                            this.nav.borrow_mut().copy_for =
                                                Some(cid.clone());
                                            cx.notify();
                                        });
                                    },
                                ),
                            );
                        }
                        if source == NavSource::Global {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.item(
                                PopupMenuItem::new("共享至项目").on_click(move |_, _, app| {
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    e.update(app, |this, cx| {
                                        this.share_connection_to_project(&cid, &name, cx)
                                    });
                                }),
                            );
                        }
                        if source == NavSource::Shared {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.item(
                                PopupMenuItem::new("取消共享").on_click(move |_, _, app| {
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    e.update(app, |this, cx| {
                                        this.unshare_connection_from_project(&cid, &name, cx)
                                    });
                                }),
                            );
                        }
                        {
                            let e = entity.clone();
                            let cid = conn_id.clone();
                            let name = conn_name.clone();
                            menu = menu.separator().item(
                                PopupMenuItem::new("删除连接").on_click(move |_, window, app| {
                                    let e = e.clone();
                                    let cid = cid.clone();
                                    let name = name.clone();
                                    window.open_alert_dialog(app, move |alert, _window, _cx| {
                                        let e = e.clone();
                                        let cid = cid.clone();
                                        // 内层 `on_ok` 是 move 闭包：先在外层建一份新绑定，
                                        // 否则会把外层闭包环境里的 `name` 移出（E0507）。
                                        let name_for_ok = name.clone();
                                        alert
                                            .confirm()
                                            .title("删除连接")
                                            .description(format!(
                                                "确定删除连接「{name}」？元数据缓存会保留（可在「缓存管理」清理）。"
                                            ))
                                            .button_props(
                                                DialogButtonProps::default()
                                                    .ok_text("删除")
                                                    .ok_variant(ButtonVariant::Danger)
                                                    .show_cancel(true),
                                            )
                                            .on_ok(move |_, _window, app| {
                                                let cid = cid.clone();
                                                let name = name_for_ok.clone();
                                                e.update(app, |this, cx| {
                                                    this.delete_connection(&cid, &name, cx)
                                                });
                                                true
                                            })
                                    });
                                }),
                            );
                        }
                        menu.item(PopupMenuItem::new("复制名称").on_click(move |_, _, app| {
                            let name = name_copy.clone();
                            app.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                            host_copy.notice(format!("已复制：{name}"), app);
                            e_copy.update(app, |_, cx| cx.notify());
                        }))
                        .item(PopupMenuItem::new("刷新元数据").on_click(move |_, _, app| {
                            let cid = cid_refresh.clone();
                            let name = name_refresh.clone();
                            e_refresh.update(app, |this, cx| {
                                this.refresh_node(&cid, &cid, Some(NavPath::Connection), cx);
                                this.host.notice(format!("已刷新：{name}"), cx);
                            });
                        }))
                        // 通用模块入口（与连接状态无关）：SQL 编辑器 / 洞察。
                        // Mock 只针对表 / 视图，见 `render_nav_node` 的对象菜单。
                        .separator()
                        .item(PopupMenuItem::new("在 SQL 编辑器中打开").on_click({
                            // `host` 是上层块里的局部（`self` 不能进 'static 闭包）
                            let host = host.clone();
                            let cid = conn_id.clone();
                            move |_, _, app| {
                                // B11：只入队「打开查询」请求，开文档在宿主 render 里做
                                // （菜单回调拿不到 `Window`；绑定该连接 + 聚焦都由宿主完成）
                                host.open_query(QueryRequest {
                                    conn_id: Some(cid.clone()),
                                    sql: String::new(),
                                    run: false,
                                }, app);
                                                            }
                        }))
                        .item(PopupMenuItem::new("查看洞察").on_click({
                            let e = entity.clone();
                            move |_, _, app| {
                                e.update(app, |this, cx| {
this.host.open_right_panel(RightPanel::Insight, cx);
            });
                            }
                        }))
                    }
                }),
        );

        // 后台加载占位（避免展开后空白，误导为已加载完）。
        if self.nav.borrow().loading.contains(&conn.id) {
            block = block.child(
                div()
                    .pl_6()
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child("加载中…"),
            );
        }

        // 标签（v7 修订）：显示在连接名**下一行**（不在名称行内），仅 `⋯ → 显示标签`
        // 开启时；以「≤2 chip + `+N`」呈现，避免撑爆名称行 / 挤掉归属域列。
        if let Some(chips) = tag_chips {
            block = block.child(
                div()
                    .pl_6()
                    .pb_0p5()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .child(chips),
            );
        }

        // 行内编辑器：归组（右键「移动到分组…」）与标签（行尾 `+`）分开，各司其职。
        let picker_open = self.nav.borrow().group_picker_for.as_deref() == Some(conn.id.as_str());
        if picker_open {
            block = block.child(self.render_group_editor(conn, &scope_key, cx));
        }
        let tag_open = self.nav.borrow().tag_editor_for.as_deref() == Some(conn.id.as_str());
        if tag_open {
            block = block.child(self.render_tag_editor(cx));
        }
        let copy_open = self.nav.borrow().copy_for.as_deref() == Some(conn.id.as_str());
        if copy_open {
            block = block.child(self.render_copy_editor(cx));
        }

        // 展开但未连接（如上次会话遗留的展开态）：不报错，给明下一步指引。
        let loading_here = self.nav.borrow().loading.contains(&conn.id);
        if expanded && !connected && children.is_empty() && error.is_none() && !loading_here {
            block = block.child(
                div()
                    .pl_6()
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child("未连接 · 右键「连接」或再次展开"),
            );
        }
        if let Some(err) = error {
            block = block.child(div().pl_6().pb_1().text_xs().text_color(danger).child(err));
        }

        if expanded {
            for child in children {
                block = block.child(self.render_nav_node(&child, 1, &scope_key, cx));
            }
        }

        block
    }

    /// 连接行内联组织编辑器：分组多选（多对多）+「新建分组」+ 标签输入。
    /// 行内**归组**编辑器（右键「移动到分组…」打开）：多选切换 + 新建分组。
    ///
    /// 只处理分组；标签由 [`Self::render_tag_editor`]（行尾 `+`）单独负责。
    fn render_group_editor(
        &self,
        conn: &ConnectionItem,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let hover = cx.theme().colors.list_hover;
        let accent = cx.theme().colors.primary;
        let bg = cx.theme().colors.popover;

        let (groups, membership) = {
            let view = self.nav.borrow();
            (
                view.groups.clone(),
                view.membership.get(&conn.id).cloned().unwrap_or_default(),
            )
        };
        let root = self.host.project_root();

        let mut panel = div()
            .v_flex()
            .w_full()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(div().text_xs().text_color(muted).child("归组"));

        for group in &groups {
            let checked = membership.iter().any(|g| g == &group.id);
            let gid = group.id.clone();
            let cid = conn.id.clone();
            let root = root.clone();
            panel = panel.child(
                div()
                    .id(format!(
                        "nav-org-g-{}::{}::{}",
                        scope_key, group.id, conn.id
                    ))
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .w_3()
                            .flex_none()
                            .text_xs()
                            .text_color(if checked { accent } else { muted })
                            .child(if checked { "\u{2713}" } else { "" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(fg)
                            .child(group.name.clone()),
                    )
                    .on_click({
                        let entity = cx.entity();
                        move |_, _, app: &mut App| {
                            let gid = gid.clone();
                            let cid = cid.clone();
                            let root = root.clone();
                            entity.update(app, |this, cx| {
                                let in_group = this
                                    .nav
                                    .borrow()
                                    .membership
                                    .get(&cid)
                                    .map(|gs| gs.iter().any(|g| g == &gid))
                                    .unwrap_or(false);
                                let result = if in_group {
                                    crate::nav_store::remove_from_group(root.as_deref(), &gid, &cid)
                                } else {
                                    crate::nav_store::add_to_group(root.as_deref(), &gid, &cid)
                                };
                                match result {
                                    Ok(()) => this.reload_nav_org(),
                                    Err(e) => {
                                        this.host.notice(format!("更新分组失败: {e}"), cx);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }),
            );
        }

        // 新建分组并直接归入当前连接。
        let cid_new = conn.id.clone();
        panel = panel.child(
            div()
                .id(format!("nav-org-new::{}::{}", scope_key, conn.id))
                .h_flex()
                .items_center()
                .gap_1()
                .px_1()
                .py_0p5()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(
                    div()
                        .w_3()
                        .flex_none()
                        .text_xs()
                        .text_color(accent)
                        .child("+"),
                )
                .child(div().text_xs().text_color(accent).child("新建分组"))
                .on_click({
                    let entity = cx.entity();
                    let host = self.host.clone();
                    let seed = GroupFormSeed::for_new(self.next_group_name());
                    move |_, window, app: &mut App| {
                        let e = entity.clone();
                        let host = host.clone();
                        let seed = seed.clone();
                        let cid = cid_new.clone();
                        host.open_group_form(
                            seed,
                            window,
                            app,
                            Rc::new(move |gid, name, desc, app| {
                                // 组内联编辑器：新建后直接把当前连接归入该组。
                                let cid = cid.clone();
                                e.update(app, |this, cx| {
                                    let Some(new_gid) = this.save_group_form(gid, name, desc, cx)
                                    else {
                                        return;
                                    };
                                    let root = this.host.project_root();
                                    if let Err(err) = crate::nav_store::add_to_group(
                                        root.as_deref(),
                                        &new_gid,
                                        &cid,
                                    ) {
                                        this.host.notice(format!("归组失败: {err}"), cx);
                                    }
                                    this.reload_nav_org();
                                    cx.notify();
                                });
                            }),
                        );
                    }
                }),
        );

        panel
    }

    /// 行内**标签**编辑器（行尾 `+` 打开）：仅标签输入，回车保存。
    fn render_tag_editor(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.popover;
        let mut panel = div()
            .v_flex()
            .w_full()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("标签（逗号分隔，回车保存）"),
            );
        if let Some(input) = &self.nav_tag_input {
            panel = panel.child(Input::new(input));
        }
        panel
    }

    /// 行内「复制为模板」编辑器（右键「复制连接（模板）…」打开）：输入新名，回车提交。
    fn render_copy_editor(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let border = cx.theme().colors.border;
        let bg = cx.theme().colors.popover;
        let entity = cx.entity();
        let mut panel = div()
            .v_flex()
            .w_full()
            .ml_6()
            .mr_1()
            .mb_1()
            .p_2()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("复制为模板（不带密码；回车提交）"),
            );
        if let Some(input) = &self.nav_copy_input {
            panel = panel.child(Input::new(input));
        }
        panel.child(
            div().h_flex().justify_end().w_full().child(
                Button::new("nav-copy-cancel")
                    .ghost()
                    .small()
                    .label("取消")
                    .on_click(move |_, _, app| {
                        entity.update(app, |this, cx| this.cancel_copy_connection(cx));
                    }),
            ),
        )
    }

    /// 对象树节点行（懒加载；叶子不可展开）。
    ///
    /// `scope_key` 为所属分组标识：同一连接可出现在多个分组，用于生成唯一元素 ID。
    fn render_nav_node(
        &self,
        node: &NavNode,
        depth: usize,
        scope_key: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let pri = cx.theme().colors.primary;
        let danger = cx.theme().colors.danger;
        let icon = {
            let theme = cx.theme();
            nav_kind_color(&node.kind, theme)
        };

        let filter = self.nav.borrow().filter.to_lowercase();
        let expanded = {
            let view = self.nav.borrow();
            view.expanded.contains(&node.key)
        };
        if expanded && node.has_children {
            if let Some(p) = node.expand_path.clone() {
                self.ensure_nav_loaded(&node.connection_id, &node.key, p, false, cx);
            }
        }
        let (skip, expanded_eff, error, children) = {
            let view = self.nav.borrow();
            let children = view.children.get(&node.key).cloned().unwrap_or_default();
            // 分页未拉完时，本地过滤只能覆盖**已加载**部分：此时不能因“已加载的都不匹配”
            // 就把整支隐藏——那会让用户连「加载更多」都点不到，看上去像对象不存在。
            let pending_more = view
                .child_total
                .get(&node.key)
                .map(|total| *total > children.len())
                .unwrap_or(false);
            let skip = !filter.is_empty()
                && !pending_more
                && !nav_node_matches(&view.children, node, &filter);
            let expanded_now = view.expanded.contains(&node.key);
            (
                skip,
                expanded_now || !filter.is_empty(),
                view.errors.get(&node.key).cloned(),
                children,
            )
        };
        if skip {
            return div();
        }

        let entity = cx.entity();
        let n_conn_id = node.connection_id.clone();
        let n_key = node.key.clone();
        let n_path = node.expand_path.clone();
        let n_property = node.property.clone();
        let n_conn_label = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == node.connection_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| node.connection_id.clone());
        let n_driver = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == node.connection_id)
            .map(|c| c.driver.clone())
            .unwrap_or_default();
        // 供右键菜单使用（双击处理器已消费 `n_conn_label` / `n_driver`）。
        let m_conn_label = n_conn_label.clone();
        let m_driver = n_driver.clone();

        let right_meta: Option<(String, bool)> = match &node.kind {
            NavNodeKind::Column {
                data_type,
                primary,
                foreign,
                ..
            } => {
                let mut t = data_type.clone();
                if *primary {
                    t.push_str("  PK");
                }
                if *foreign {
                    t.push_str("  FK");
                }
                Some((t, *primary))
            }
            _ => None,
        };

        // 拖拽载荷：仅表 / 视图（原型 §6.1「拖拽表到编辑器」）。
        let drag_payload = match &node.kind {
            NavNodeKind::Table { .. } | NavNodeKind::View => {
                node.property.as_ref().map(|p| NavDragPayload {
                    qualified: nav_qualified_name(p),
                    label: node.name.clone(),
                })
            }
            _ => None,
        };

        // 缩进 = 基础内边距 + 层级 × 步长（设计 §2.1）。两者都是 rem 倍率，
        // 直接交给 `rems()` 换算（其基准是主题字号而非 4px，写 `/ 4.` 会放大 4 倍）。
        let indent = ui::TREE_BASE_PADDING + depth as f32 * ui::TREE_INDENT;
        let selected = self.nav.borrow().selected_key.as_deref() == Some(node.key.as_str());
        let selected_bg = cx.theme().colors.list_active;
        // 键盘导航序列（父在前，子随递归渲染加入）。
        self.nav_order.borrow_mut().push(NavOrderItem {
            key: node.key.clone(),
            conn_id: node.connection_id.clone(),
            path: node.expand_path.clone(),
            property: node.property.clone(),
            has_children: node.has_children,
            expanded: expanded_eff,
        });
        let mut block = div().v_flex().w_full();
        let mut row =
            div()
                .id(format!("nav-node-{}::{}", scope_key, node.key))
                .h_flex()
                .items_center()
                .w_full()
                .h(rems(1.375))
                .pr_1()
                .pl(rems(indent))
                .gap_1()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |s| s.bg(selected_bg))
                .hover(move |s| s.bg(hover))
                .child(div().w_2p5().flex_none().text_xs().text_color(muted).child(
                    if node.has_children {
                        if expanded_eff { "\u{25be}" } else { "\u{25b8}" }
                    } else {
                        ""
                    },
                ))
                .child(div().w_2().h_2().flex_none().rounded_sm().bg(icon))
                .child(div().flex_1().min_w_0().text_xs().text_color(fg).child(
                    nav_name_highlight(
                        &node.name,
                        &filter,
                        product_tokens::get(cx).search_match_background(cx.theme()),
                        fg,
                    ),
                ));
        if let Some((meta, is_pk)) = right_meta {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(if is_pk { pri } else { muted })
                    .child(meta),
            );
        }
        if let Some(payload) = drag_payload {
            // 类型色点沿用节点图标色；`Hsla` 是 `Copy`，可直接进闭包。
            let tint = icon;
            row = row.on_drag(payload, move |payload, _offset, _window, cx| {
                // 幽灵显示短名（与用户抓住的东西一致），插入的是限定名。
                let label = payload.label.clone();
                cx.new(|_| NavDragGhost { label, tint })
            });
        }
        row = row.on_click({
            let focus = self.focus_handle.clone();
            move |ev, window, app| {
                focus.focus(window, app);
                let sel_key = n_key.clone();
                entity.update(app, |this, cx| {
                    this.nav.borrow_mut().selected_key = Some(sel_key.clone());
                    cx.notify();
                });
                if ev.click_count() >= 2 {
                    if let Some(p) = n_property.clone() {
                        let req = PropertyRequest {
                            property: p,
                            conn_label: n_conn_label.clone(),
                            driver: n_driver.clone(),
                        };
                        entity.update(app, |this, cx| {
                            this.host.show_properties(req, cx);
                            cx.notify();
                        });
                        return;
                    }
                }
                if let Some(p) = n_path.clone() {
                    let conn_id = n_conn_id.clone();
                    let key = n_key.clone();
                    entity.update(app, |this, cx| {
                        this.toggle_nav_node(&conn_id, &key, p, cx);
                        cx.notify();
                    });
                }
            }
        });
        // 右键菜单（对象节点）：查看数据 / 属性 / 复制 / 刷新。
        block = block.child(row.context_menu({
            let entity = cx.entity();
            let host = self.host.clone();
            let conn_id = node.connection_id.clone();
            let nkey = node.key.clone();
            let dname = node.name.clone();
            let menu_path = node.expand_path.clone();
            let menu_prop = node.property.clone();
            let qualified = node.property.as_ref().map(nav_qualified_name);
            let conn_label = m_conn_label.clone();
            let driver = m_driver.clone();
            let data_like = matches!(&node.kind, NavNodeKind::Table { .. } | NavNodeKind::View);
            // 表 / 视图的「生成 SQL」需要列：由后台任务取（命中 L2 不发查询）。
            let dml_target = match &menu_path {
                Some(NavPath::Table {
                    catalog,
                    schema,
                    table,
                }) if data_like => Some((catalog.clone(), schema.clone(), table.clone())),
                _ => None,
            };
            let dml_root = if dml_target.is_some() {
                self.host
                    .project_root()
                    .map(|p| p.to_string_lossy().to_string())
            } else {
                None
            };
            // 结构洞察的靶要在闭包**外**算好（`node` 的借用活不过 `'static` 闭包）
            let insight_schema = insight_schema_target(&node.kind, menu_path.as_ref(), &conn_id);
            // Mock / 洞察表入口的引用靶同理；**kind 按节点类型给**：视图不能标成表
            // ——引用是跨屏身份，标错会传染到属性面板 / 洞察 / 将来的“在树中定位”。
            let data_target = dml_target
                .clone()
                .and_then(|(catalog, schema, name)| {
                    nav_data_target(&node.kind, &conn_id, catalog, schema, name)
                });
            move |menu, window, cx| {
                let mut menu = menu;
                if let Some(prop0) = menu_prop.clone() {
                    let e = entity.clone();
                    let label0 = conn_label.clone();
                    let drv0 = driver.clone();
                    menu = menu.item(PopupMenuItem::new("查看属性").on_click(move |_, _, app| {
                        let prop = prop0.clone();
                        let label = label0.clone();
                        let drv = drv0.clone();
                        e.update(app, |this, cx| {
                            this.host.show_properties(
                                PropertyRequest {
                                    property: prop.clone(),
                                    conn_label: label.clone(),
                                    driver: drv.clone(),
                                },
                                cx,
                            );
                            cx.notify();
                        });
                    }));
                }
                if data_like {
                    if let Some(q) = qualified.clone() {
                        let sql = format!("SELECT * FROM {q} LIMIT 200;");
                        let host_sql = host.clone();
                        let cid_view = conn_id.clone();
                        menu = menu.item(PopupMenuItem::new("查看数据（LIMIT 200）").on_click(
                            move |_, _, app| {
                                // B11：打开一份绑定该连接的查询并**自动执行**
                                // （M4 遗留的“查看数据不自动执行”在此关闭）
                                host_sql.open_query(
                                    QueryRequest {
                                        conn_id: Some(cid_view.clone()),
                                        sql: sql.clone(),
                                        run: true,
                                    },
                                    app,
                                );
                            },
                        ));
                    }
                    // 「生成 SQL ▸」：由列信息生成 INSERT / UPDATE / DELETE 模板（只注入不执行）。
                    if let Some((catalog, schema, table)) = dml_target.clone() {
                        let q = crate::sql_gen::qualified_name(
                            Some(catalog.as_str()),
                            Some(schema.as_str()),
                            &table,
                        );
                        let e = entity.clone();
                        let key = nkey.clone();
                        let cid = conn_id.clone();
                        let root = dml_root.clone();
                        menu = menu.submenu("生成 SQL", window, cx, move |m, _w, _c| {
                            let mut m = m;
                            for kind in [DmlKind::Insert, DmlKind::Update, DmlKind::Delete] {
                                let e = e.clone();
                                let key = key.clone();
                                let cid = cid.clone();
                                let root = root.clone();
                                let catalog = catalog.clone();
                                let schema = schema.clone();
                                let table = table.clone();
                                let q = q.clone();
                                m = m.item(PopupMenuItem::new(kind.label()).on_click(
                                    move |_, _, app| {
                                        nav_jobs::enqueue_generate_dml(
                                            &key,
                                            &cid,
                                            root.as_deref(),
                                            &catalog,
                                            &schema,
                                            &table,
                                            &q,
                                            kind,
                                        );
                                        e.update(app, |this, cx| this.ensure_nav_pump(cx));
                                    },
                                ));
                            }
                            m
                        });
                    }
                }
                {
                    let e = entity.clone();
                    let host_copy = host.clone();
                    let name_copy = dname.clone();
                    menu = menu.item(PopupMenuItem::new("复制名称").on_click(move |_, _, app| {
                        let name = name_copy.clone();
                        app.write_to_clipboard(ClipboardItem::new_string(name.clone()));
                        host_copy.notice(format!("已复制：{name}"), app);
                        e.update(app, |_, cx| cx.notify());
                    }));
                }
                if let Some(q) = qualified.clone() {
                    let e = entity.clone();
                    let host_copy = host.clone();
                    menu =
                        menu.item(PopupMenuItem::new("复制限定名").on_click(move |_, _, app| {
                            let q = q.clone();
                            app.write_to_clipboard(ClipboardItem::new_string(q.clone()));
                            host_copy.notice(format!("已复制：{q}"), app);
                            e.update(app, |_, cx| cx.notify());
                        }));
                }
                if let Some(p) = menu_path.clone() {
                    let e = entity.clone();
                    let cid = conn_id.clone();
                    let key = nkey.clone();
                    menu =
                        menu.item(PopupMenuItem::new("刷新元数据").on_click(move |_, _, app| {
                            let cid = cid.clone();
                            let key = key.clone();
                            let p = p.clone();
                            e.update(app, |this, cx| {
                                this.refresh_node(&cid, &key, Some(p.clone()), cx);
                            });
                        }));
                }
                // 通用模块入口（所有对象节点都有，与节点类型 / 连接状态无关）：
                // SQL 编辑器 / 洞察；Mock 只针对表 / 视图（`data_like`）。
                menu = menu.separator().item({
                    // `host` 是上层块里的局部（`self` 不能进 'static 闭包）
                    let host = host.clone();
                    let cid = conn_id.clone();
                    PopupMenuItem::new("在 SQL 编辑器中打开").on_click(move |_, _, app| {
                        // B11：同上方连接菜单——只入队，宿主开文档并聚焦
                        host.open_query(
                            QueryRequest {
                                conn_id: Some(cid.clone()),
                                sql: String::new(),
                                run: false,
                            },
                            app,
                        );
                    })
                });
                if data_like {
                    let e = entity.clone();
                    // 定向请求：连接 + 源库对象（catalog / schema / 名字）——Mock 面板据此
                    // 读源库结构并预填目标表名（v1 主路径：源库结构 → 造新数据）。
                    let request = data_target.clone();
                    // 洞察那份先克隆出来：Mock 的闭包会把 `request` move 进去
                    let request_for_insight = request.clone();
                    menu = menu.item(PopupMenuItem::new("生成 Mock 数据").on_click(
                        move |_, _, app| {
                            let request = request.clone();
                            e.update(app, |this, cx| {
                                this.host.open_mock_panel(request, cx);
                            });
                        },
                    ));
                    // 【M8】看这张源表的统计：取样（LIMIT 500 → 分析临时表）由洞察侧完成，
                    // 这里只给「哪条连接 + 哪张表」（D58 的统一入口）。
                    if let Some(source) = request_for_insight {
                        let e = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new("查看统计").on_click(move |_, _, app| {
                                let source = source.clone();
                                e.update(app, |this, cx| this.host.open_insight_table(source, cx));
                            }));
                    }
                }
                // 【M8】结构洞察（Schema 级）：表 / 视图用它所在的 schema，schema 节点用自己。
                // 与「查看统计」分工：那个看**数据**（取样），这个看**结构**（源库内省）。
                if let Some(target) = insight_schema.clone() {
                    let e = entity.clone();
                    menu = menu.item(PopupMenuItem::new("结构洞察").on_click(move |_, _, app| {
                        let target = target.clone();
                        e.update(app, |this, cx| this.host.open_insight_schema(target, cx));
                    }));
                }
                menu = menu.item({
                    let e = entity.clone();
                    PopupMenuItem::new("查看洞察").on_click(move |_, _, app| {
                        e.update(app, |this, cx| {
                            this.host.open_right_panel(RightPanel::Insight, cx);
                        });
                    })
                });
                menu
            }
        }));

        if self.nav.borrow().loading.contains(&node.key) {
            block = block.child(
                div()
                    .pl(rems(indent + ui::TREE_INDENT))
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child("加载中…"),
            );
        }
        if let Some(err) = error {
            block = block.child(
                div()
                    .pl(rems(indent + ui::TREE_INDENT))
                    .pb_1()
                    .text_xs()
                    .text_color(danger)
                    .child(err),
            );
        }
        if expanded_eff {
            // 类别文件夹分页：大 schema 只渲染首批，其余用「加载更多」逐页展开。
            let is_folder = matches!(&node.kind, NavNodeKind::Folder(_));
            let loaded = children.len();
            let total = if is_folder {
                self.node_total(&node.key, loaded)
            } else {
                loaded
            };
            let limit = if is_folder {
                self.folder_limit(&node.key)
            } else {
                usize::MAX
            };
            for child in children.iter().take(limit) {
                block = block.child(self.render_nav_node(child, depth + 1, scope_key, cx));
            }
            // 两种「还有更多」：数据侧的（要取）与渲染窗口的（已到手，只放大窗口）。
            if is_folder && (limit < loaded || loaded < total) {
                block = block.child(self.render_more_row(node, loaded, total, limit, depth, cx));
            }
        }
        block
    }

    /// 「加载更多」/「显示更多」行（大 schema 分页）。
    ///
    /// 两种语义分开（用户点击的代价不同，必须区分）：
    /// - `total > loaded`：数据侧还有没取的（索引分页）→ 点击**排队取下一页**；
    /// - 否则：数据已到手，只是超出渲染窗口 → 点击**只放大窗口**，不重查也不排队。
    fn render_more_row(
        &self,
        node: &NavNode,
        loaded: usize,
        total: usize,
        window: usize,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pri = cx.theme().colors.primary;
        let indent = ui::TREE_BASE_PADDING + (depth as f32 + 1.0) * ui::TREE_INDENT;
        let entity = cx.entity();
        let key = node.key.clone();
        let to_fetch = total.saturating_sub(loaded);
        let hidden = loaded.saturating_sub(window);
        // 带筛选时，本地过滤只能命中**已加载**的那些：这一点必须说出口，
        // 否则用户会以为“搜不到 = 库里没有”（索引/FTS 搜索尚未接，见文档 §4.5）。
        let filtering = !self.nav.borrow().filter.is_empty();
        let label = if to_fetch > 0 {
            let scope = if filtering {
                "；筛选仅覆盖已加载"
            } else {
                ""
            };
            format!("加载更多（余 {to_fetch} / 共 {total}{scope}）")
        } else {
            format!("显示更多（余 {hidden}）")
        };
        let conn_id = node.connection_id.clone();
        let path = node.expand_path.clone();
        div()
            .id(format!("nav-more-{key}"))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.375))
            .pl(rems(indent))
            .pr_1()
            .rounded_md()
            .cursor_pointer()
            .text_xs()
            .text_color(pri)
            .child(label)
            .on_click(move |_, _, app| {
                let k = key.clone();
                let conn_id = conn_id.clone();
                let path = path.clone();
                entity.update(app, |this, cx| {
                    // 已在取数就别重复排队：后台任务是单线程串行的，重复点会叠成一串。
                    if this.nav.borrow().loading.contains(&k) {
                        return;
                    }
                    if to_fetch > 0 {
                        let Some(path) = path.clone() else { return };
                        let root = this
                            .host
                            .project_root()
                            .map(|p| p.to_string_lossy().to_string());
                        this.nav.borrow_mut().loading.insert(k.clone());
                        nav_jobs::enqueue_load_page(
                            &conn_id,
                            root.as_deref(),
                            &k,
                            path,
                            false,
                            loaded,
                            nav_jobs::PAGE_SIZE,
                        );
                        this.ensure_nav_pump(cx);
                    } else {
                        let mut view = this.nav.borrow_mut();
                        view.page_limit
                            .insert(k.clone(), window + ui::NAV_FOLDER_PAGE_SIZE);
                        drop(view);
                    }
                    cx.notify();
                });
            })
    }

    /// 展开 / 折叠节点；首次展开时排队后台懒加载，并持久化展开态。
    fn toggle_nav_node(&mut self, conn_id: &str, key: &str, path: NavPath, cx: &mut Context<Self>) {
        let now_expanded = {
            let mut view = self.nav.borrow_mut();
            if view.expanded.contains(key) {
                view.expanded.remove(key);
                false
            } else {
                view.expanded.insert(key.to_string());
                true
            }
        };
        if now_expanded {
            // 连接根展开：未建连则先建连。否则 `NavigatorService` → `MetadataService`
            // 取不到运行时句柄，冒泡为 `[CONN_NOT_FOUND]`（用户看到的“连不上”）。
            if matches!(path, NavPath::Connection) && !self.ensure_connected_for_browse(conn_id, cx)
            {
                self.save_nav_state_for(conn_id);
                return;
            }
            self.ensure_nav_loaded(conn_id, key, path, false, cx);
        }
        self.save_nav_state_for(conn_id);
    }

    /// 展开前的隐式建连：未连接时先建连；返回是否可用（已连接 或 建连成功）。
    ///
    /// 失败时写面板提示（与 `toggle_connection` 同文案），不阻后续可重试。
    fn ensure_connected_for_browse(&mut self, conn_id: &str, cx: &mut Context<Self>) -> bool {
        let already =
            self.host.is_connected(conn_id) || self.nav.borrow().connected.contains(conn_id);
        if already {
            return true;
        }
        let root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        match self.host.connect(conn_id) {
            Ok(()) => {
                {
                    let mut view = self.nav.borrow_mut();
                    view.connected.insert(conn_id.to_string());
                    view.prefetched.clear();
                    // 清掉上一次的负载错误（如 CONN_NOT_FOUND），以便重试加载。
                    view.errors.remove(conn_id);
                }
                nav_jobs::warm_after_connect(conn_id, root.as_deref());
                self.ensure_warm_poll(cx);
                true
            }
            Err(e) => {
                self.host.notice(format!("连接失败: {e}"), cx);
                false
            }
        }
    }

    /// 排队后台懒加载子节点（已请求过则跳过；render 路径不做 I/O）。
    ///
    /// `fresh`（刷新模式）跳过 L2 读缓存并重写。结果由 [`Self::apply_load_results`] 回填。
    fn ensure_nav_loaded(
        &self,
        conn_id: &str,
        key: &str,
        path: NavPath,
        fresh: bool,
        cx: &mut Context<Self>,
    ) {
        {
            let view = self.nav.borrow();
            if view.attempted.contains(key) {
                return;
            }
        }
        {
            let mut view = self.nav.borrow_mut();
            view.attempted.insert(key.to_string());
            view.loading.insert(key.to_string());
        }
        let project_root = self
            .host
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        nav_jobs::enqueue_load(conn_id, project_root.as_deref(), key, path, fresh);
        self.ensure_nav_pump(cx);
    }

    /// 搜索结果区（树顶；有查询词时出现）。
    ///
    /// 与本地过滤的分工（两者同时生效，互不替代）：
    /// - 本地过滤：命中**已加载**的节点（零往返、即时，改一个字就变）；
    /// - 索引搜索：命中**索引里的全部对象**（含尚未展开到的 schema），跨连接。
    fn render_search_section(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (query, hits, searched) = {
            let view = self.nav.borrow();
            (
                view.search_query.clone()?,
                view.search_hits.clone(),
                view.search_searched,
            )
        };
        let pri = cx.theme().colors.primary;
        let muted = cx.theme().colors.muted_foreground;
        let max = ui::NAV_SEARCH_MAX_ROWS;

        let title = if nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator)
            && hits.is_empty()
        {
            format!("索引搜索：{query}（搜索中…）")
        } else if hits.is_empty() {
            format!("索引搜索：{query}（无命中；已搜 {searched} 个有缓存的连接）")
        } else {
            format!(
                "索引搜索：{query}（{} 条，覆盖 {searched} 个连接）",
                hits.len()
            )
        };

        let mut block = div().v_flex().w_full().gap_0p5().pb_1().child(
            div()
                .w_full()
                .px_1()
                .pb_0p5()
                .text_xs()
                .text_color(pri)
                .child(title),
        );
        for hit in hits.iter().take(max) {
            block = block.child(self.render_search_hit(hit, cx));
        }
        if hits.len() > max {
            block = block.child(
                div()
                    .w_full()
                    .px_1()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("仅显示前 {max} 条（缩小搜索词可看到更多）")),
            );
        }
        Some(block)
    }

    /// 一条搜索命中行：色点 + 名称（高亮命中）+ 类别 / 归属（schema · 连接）。
    ///
    /// 单击 → 打开属性面板：搜索结果的价值就在「还没展开到那层时也能立刻看结构」。
    fn render_search_hit(
        &self,
        hit: &nav_jobs::SearchHit,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fg = cx.theme().colors.foreground;
        let muted = cx.theme().colors.muted_foreground;
        let hover = cx.theme().colors.list_hover;
        let match_bg = product_tokens::get(cx).search_match_background(cx.theme());
        let tint = nav_kind_color(&nav_search_hit_kind(hit), cx.theme());
        let filter = self.nav.borrow().filter.to_lowercase();

        // 归属：`schema · 连接`（列再带上所属表）
        let mut scope = Vec::new();
        if let Some(parent) = &hit.parent_name {
            scope.push(parent.clone());
        }
        if let Some(schema) = &hit.schema {
            scope.push(schema.clone());
        }
        scope.push(hit.conn_label.clone());
        let scope = scope.join(" · ");
        let kind_label = nav_object_type_label(&hit.object_type);

        let entity = cx.entity();
        let object = nav_search_hit_ref(hit);
        let property = nav_search_hit_property(hit);
        let conn_label = hit.conn_label.clone();
        let driver = hit.driver.clone();
        // 元素 id 用**统一引用 key**（连接 + 类别段一路到底）：同一对象在树与结果区
        // 得到同一个 key，而前缀 `nav-search-` 保证它不会与树上那行的 id 撞车。
        let id = match &object {
            Some(object) => format!("nav-search-{}", object.key()),
            None => format!(
                "nav-search-{}-{}-{}",
                hit.conn_id, hit.object_type, hit.object_name
            ),
        };

        div()
            .id(id)
            .h_flex()
            .items_center()
            .gap_1p5()
            .w_full()
            .h(rems(1.375))
            .px_1()
            .rounded_md()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(div().size(rems(0.5)).rounded_full().bg(tint))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(fg)
                    .child(nav_name_highlight(&hit.object_name, &filter, match_bg, fg)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{kind_label} · {scope}")),
            )
            .on_click(move |_, _, app| {
                let Some(property) = property.clone() else {
                    return;
                };
                let req = PropertyRequest {
                    property,
                    conn_label: conn_label.clone(),
                    driver: driver.clone(),
                };
                entity.update(app, |this, cx| {
                    this.host.show_properties(req, cx);
                    cx.notify();
                });
            })
    }

    /// 回填索引搜索结果（过期批次直接丢弃：用户在等待期间已经把词改了）。
    fn apply_search_results(
        &mut self,
        results: Vec<nav_jobs::SearchResult>,
        cx: &mut Context<Self>,
    ) {
        {
            let mut view = self.nav.borrow_mut();
            for r in results {
                if view.search_query.as_deref() != Some(r.query.as_str()) {
                    continue;
                }
                view.search_hits = r.hits;
                view.search_searched = r.searched;
            }
        }
        cx.notify();
    }

    /// 启动加载结果轮询（已有存活任务时不重复启动）。
    fn ensure_nav_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.nav_pump.borrow().as_ref() {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let results = nav_jobs::drain_load_results();
                let had = !results.is_empty();
                if had
                    && weak
                        .update(cx, |this, cx| this.apply_load_results(results, cx))
                        .is_err()
                {
                    return;
                }
                // 生成 SQL / 测试连接：同一轮询泵回填（两者都可能在菜单触发）。
                let sql_results = nav_jobs::drain_sql_results();
                if !sql_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_sql_results(sql_results, cx))
                        .is_err()
                {
                    return;
                }
                let test_results = nav_jobs::drain_test_results();
                if !test_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_test_results(test_results, cx))
                        .is_err()
                {
                    return;
                }
                // 搜索：同一轮询泵回填（搜索框的跨连接索引搜索）。
                let search_results =
                    nav_jobs::drain_search_results(nav_jobs::SearchConsumer::Navigator);
                if !search_results.is_empty()
                    && weak
                        .update(cx, |this, cx| this.apply_search_results(search_results, cx))
                        .is_err()
                {
                    return;
                }
                let idle = !nav_jobs::has_pending_loads()
                    && !nav_jobs::has_pending_sql()
                    && !nav_jobs::has_pending_test()
                    && !nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator);
                if idle {
                    // 多等一拍确认没有新任务（render 可能刚入队）。
                    executor.timer(std::time::Duration::from_millis(120)).await;
                    if !nav_jobs::has_pending_loads()
                        && !nav_jobs::has_pending_sql()
                        && !nav_jobs::has_pending_test()
                        && !nav_jobs::has_pending_search(nav_jobs::SearchConsumer::Navigator)
                    {
                        break;
                    }
                }
            }
        });
        *self.nav_pump.borrow_mut() = Some(task);
    }

    /// 回填后台加载结果（主线程）。
    fn apply_load_results(&mut self, results: Vec<nav_jobs::LoadResult>, cx: &mut Context<Self>) {
        for r in results {
            let key = r.key.clone();
            let is_tables_folder = matches!(
                &r.path,
                NavPath::Folder {
                    folder: NavFolder::Tables,
                    ..
                }
            );
            let (catalog, schema) = match &r.path {
                NavPath::Folder {
                    catalog, schema, ..
                } => (catalog.clone(), schema.clone()),
                _ => (String::new(), String::new()),
            };
            let conn_id = r.conn_id.clone();
            let project_root = r.project_root.clone();
            {
                let mut view = self.nav.borrow_mut();
                view.loading.remove(&key);
                match r.result {
                    Ok(page) => {
                        view.errors.remove(&key);
                        if r.offset == 0 {
                            // 首屏：整体替换（刷新 / 重新展开都走这里）。
                            view.children.insert(key.clone(), page.nodes);
                            view.child_total.insert(key.clone(), page.total);
                        } else {
                            // 追加页：去重后并入（见 `nav_merge_page`）。
                            let entry = view.children.entry(key.clone()).or_default();
                            let appended = nav_merge_page(entry, page.nodes);
                            let loaded = entry.len();
                            // 索引计数与实际行数可能不一致（索引陈旧）：翻到底（空块）就收敛到
                            // 实际已加载数，否则「加载更多」会永远挂在树上、点了没反应。
                            let total = if appended == 0 { loaded } else { page.total };
                            view.child_total.insert(key.clone(), total);
                            // 追加页到位后把渲染窗口抬到已加载数，否则新到的行落在窗口外看不见。
                            view.page_limit.insert(key.clone(), loaded);
                        }
                    }
                    Err(e) => {
                        view.errors.insert(key.clone(), e);
                    }
                }
            }
            // C2：「表」文件夹首次加载成功后，排队预取前 N 张表的列。
            if is_tables_folder {
                self.maybe_prefetch(&conn_id, &key, &catalog, &schema, project_root.as_deref());
            }
        }
        cx.notify();
    }

    /// 回填「生成 SQL」结果：成功注入编辑区（打开一份绑定该连接的草稿），失败落提示。
    ///
    /// **不自动执行**：模板是给人改的（写语句更不该替用户跑）。
    fn apply_sql_results(&mut self, results: Vec<nav_jobs::SqlGenResult>, cx: &mut Context<Self>) {
        for r in results {
            match r.result {
                Ok(sql) => {
                    let conn_id = self
                        .host
                        .selected_index()
                        .and_then(|i| self.host.connections().get(i).map(|item| item.id.clone()));
                    self.host.open_query(
                        QueryRequest {
                            conn_id,
                            sql,
                            run: false,
                        },
                        cx,
                    );
                    self.host.notify_host(cx);
                }
                Err(e) => {
                    self.host.notice(format!("生成 SQL 失败：{e}"), cx);
                }
            }
        }
        cx.notify();
    }

    /// 回填「测试连接」结果（结果文案加连接名前缀，直接落面板提示）。
    fn apply_test_results(
        &mut self,
        results: Vec<nav_jobs::TestConnResult>,
        cx: &mut Context<Self>,
    ) {
        for r in results {
            let msg = match r.result {
                Ok(m) => format!("{}：{m}", r.name),
                Err(e) => format!("{}：{e}", r.name),
            };
            self.host.notice(msg, cx);
        }
        cx.notify();
    }

    /// C2：对刚加载完的「表」文件夹排队列预取（一次性）。
    fn maybe_prefetch(
        &self,
        conn_id: &str,
        key: &str,
        catalog: &str,
        schema: &str,
        project_root: Option<&str>,
    ) {
        let first_time = self.nav.borrow_mut().prefetched.insert(key.to_string());
        if !first_time {
            return;
        }
        let targets: Vec<nav_jobs::ColumnTarget> = {
            let view = self.nav.borrow();
            view.children
                .get(key)
                .map(|kids| {
                    kids.iter()
                        .filter(|n| matches!(n.kind, NavNodeKind::Table { .. }))
                        .take(nav_jobs::PREFETCH_BATCH)
                        .map(|n| nav_jobs::ColumnTarget {
                            catalog: catalog.to_string(),
                            schema: schema.to_string(),
                            table: n.name.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        nav_jobs::prefetch_columns(conn_id, project_root, targets);
    }

    /// 当前项目根（项目 / 共享连接的导航状态落项目库）。

    /// 首次渲染某连接时，从库中恢复其展开态。
    fn ensure_nav_state_loaded(&self, conn_id: &str) {
        {
            let view = self.nav.borrow();
            if view.state_loaded.contains(conn_id) {
                return;
            }
        }
        let state = crate::nav_store::load_nav_state(conn_id, self.host.project_root().as_deref());
        let prefix = format!("{conn_id}/");
        let mut view = self.nav.borrow_mut();
        for key in state.expanded_keys {
            if key == conn_id || key.starts_with(&prefix) {
                view.expanded.insert(key);
            }
        }
        view.state_loaded.insert(conn_id.to_string());
    }

    /// 持久化某连接的展开态。
    fn save_nav_state_for(&self, conn_id: &str) {
        let prefix = format!("{conn_id}/");
        let keys: Vec<String> = {
            let view = self.nav.borrow();
            view.expanded
                .iter()
                .filter(|k| *k == conn_id || k.starts_with(&prefix))
                .cloned()
                .collect()
        };
        let state = crate::model::NavState {
            expanded_keys: keys,
            ..Default::default()
        };
        let _ =
            crate::nav_store::save_nav_state(conn_id, self.host.project_root().as_deref(), &state);
    }

    /// 容器展示名（分组名；`GROUP_UNGROUPED` → 「未分组」）：拖拽通知文案用。
    fn container_label(&self, scope_id: &str) -> String {
        if scope_id == GROUP_UNGROUPED {
            return "未分组".to_string();
        }
        self.nav
            .borrow()
            .groups
            .iter()
            .find(|g| g.id == scope_id)
            .map(|g| g.name.clone())
            .unwrap_or_else(|| scope_id.to_string())
    }

    /// 容器当前的**全部成员**顺序（不做搜索 / facet 筛选）。
    ///
    /// 排序落库要覆盖容器的全部成员：用渲染过的（已筛选）列表写库，会把被过滤掉的
    /// 行在下次写库时丢掉位置。
    fn container_order(&self, scope_id: &str) -> Vec<String> {
        if scope_id != GROUP_UNGROUPED {
            return self
                .nav
                .borrow()
                .group_order
                .get(scope_id)
                .cloned()
                .unwrap_or_default();
        }
        // 未分组：成员由“不属于任何分组”推导；顺序与分组内同一条规则。
        let view = self.nav.borrow();
        let conns = self.host.connections();
        let ranked: HashMap<&str, i64> = view
            .ungrouped_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i as i64))
            .collect();
        let stored: Vec<(String, Option<i64>)> = conns
            .iter()
            .filter(|c| {
                view.membership
                    .get(&c.id)
                    .map(|gs| gs.is_empty())
                    .unwrap_or(true)
            })
            .map(|c| (c.id.clone(), ranked.get(c.id.as_str()).copied()))
            .collect();
        nav_order_members(&stored, |id| {
            conns
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        })
    }

    /// 拖拽落点动作：把连接移到 `scope_id` 容器的指定位置。
    ///
    /// 归属与顺序分两步：先改归属（未分组 = 移出全部分组；分组 = 加入并**保留**其它归属，
    /// 多对多），再以**变更后**的真实成员算新顺序——拿拖拽前的快照会漏掉刚加入的成员。
    /// 已在目标容器且落点未造成位移时不写库（`nav_reorder` 返回 `None`）。
    fn apply_conn_drop(
        &mut self,
        payload: &NavConnDragPayload,
        scope_id: &str,
        target: ConnDropTarget,
        cx: &mut Context<Self>,
    ) {
        let root = self.host.project_root();
        let conn_id = payload.conn_id.clone();
        let name = payload.name.clone();
        let label = self.container_label(scope_id);
        let groups_of: Vec<String> = self
            .nav
            .borrow()
            .membership
            .get(&conn_id)
            .cloned()
            .unwrap_or_default();
        let was_member = if scope_id == GROUP_UNGROUPED {
            groups_of.is_empty()
        } else {
            groups_of.iter().any(|g| g == scope_id)
        };

        // 1) 归属。
        let membership = if scope_id == GROUP_UNGROUPED {
            crate::nav_store::remove_from_all_groups(root.as_deref(), &conn_id)
        } else {
            crate::nav_store::add_to_group(root.as_deref(), scope_id, &conn_id)
        };
        if let Err(e) = membership {
            self.host.notice(format!("移动失败: {e}"), cx);
            cx.notify();
            return;
        }

        // 2) 顺序。落到容器头且已是成员时不改位置，避免“只是归组”把行拽到末尾。
        self.reload_nav_org();
        let next = match &target {
            ConnDropTarget::Container if was_member => None,
            ConnDropTarget::Container => {
                let current = self.container_order(scope_id);
                nav_reorder(&current, &conn_id, None)
            }
            ConnDropTarget::BeforeRow(before) => {
                let current = self.container_order(scope_id);
                nav_reorder(&current, &conn_id, Some(before.as_str()))
            }
        };
        if let Some(next) = next {
            if let Err(e) = crate::nav_store::set_container_order(root.as_deref(), scope_id, &next)
            {
                self.host.notice(format!("保存排序失败: {e}"), cx);
                cx.notify();
                return;
            }
            self.reload_nav_org();
        }

        // 3) 通知：先说归属变化（更重的动作），再说位置。
        self.host.notice(
            if !was_member {
                if scope_id == GROUP_UNGROUPED {
                    format!("已把「{name}」移出分组")
                } else {
                    format!("已把「{name}」加入「{label}」")
                }
            } else {
                format!("已调整「{name}」在「{label}」中的位置")
            },
            cx,
        );
        cx.notify();
    }

    /// 当前分组顺序（ID 列表，按 `sort_order` → 名称）。
    fn group_ids(&self) -> Vec<String> {
        self.nav
            .borrow()
            .groups
            .iter()
            .map(|g| g.id.clone())
            .collect()
    }

    /// 分组拖拽落点：把分组排到 `before` 之前（`None` = 排到最后）。
    fn apply_group_drop(
        &mut self,
        payload: &NavGroupDragPayload,
        before: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let ids = self.group_ids();
        let Some(next) = nav_reorder(&ids, &payload.group_id, before) else {
            return;
        };
        let root = self.host.project_root();
        match crate::nav_store::set_group_order(root.as_deref(), &next) {
            Ok(()) => {
                self.reload_nav_org();
                self.host
                    .notice(format!("已移动分组「{}」", payload.name), cx);
            }
            Err(e) => self.host.notice(format!("保存分组顺序失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 分组上移 / 下移一位（`delta` = -1 / +1）；边界为 no-op。
    fn step_group(&mut self, group_id: &str, delta: i32, cx: &mut Context<Self>) {
        let ids = self.group_ids();
        let Some(next) = nav_step(&ids, group_id, delta) else {
            return;
        };
        let root = self.host.project_root();
        match crate::nav_store::set_group_order(root.as_deref(), &next) {
            Ok(()) => self.reload_nav_org(),
            Err(e) => self.host.notice(format!("保存分组顺序失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 键盘重排：把选中连接在它的**主容器**内上移 / 下移一位（`delta` = -1 / +1）。
    ///
    /// 只作用于连接行——对象节点（库 / 表 / 列…）的顺序由后端内省给出，不能重排。
    /// 容器取「主组解析」（与渲染同一套规则），无任何分组时就是「未分组」。
    fn nav_step_selected(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(conn_id) = self.nav.borrow().selected_key.clone() else {
            return;
        };
        let Some(name) = self
            .host
            .connections()
            .iter()
            .find(|c| c.id == conn_id)
            .map(|c| c.name.clone())
        else {
            return;
        };
        let scope = {
            let view = self.nav.borrow();
            nav_primary_scope(&view.membership, &view.primary_group, &conn_id)
                .unwrap_or_else(|| GROUP_UNGROUPED.to_string())
        };
        let current = self.container_order(&scope);
        let Some(next) = nav_step(&current, &conn_id, delta) else {
            return;
        };
        let root = self.host.project_root();
        match crate::nav_store::set_container_order(root.as_deref(), &scope, &next) {
            Ok(()) => {
                self.reload_nav_org();
                let how = if delta < 0 { "上移" } else { "下移" };
                self.host.notice(format!("已{how}「{name}」"), cx);
            }
            Err(e) => self.host.notice(format!("保存排序失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 重载分组 / 成员关系 / 标签映射（组织变更后调用）。
    fn reload_nav_org(&self) {
        let root = self.host.project_root();
        let groups = crate::nav_store::list_groups(root.as_deref());
        let mut membership: HashMap<String, Vec<String>> = HashMap::new();
        let mut group_order: HashMap<String, Vec<String>> = HashMap::new();
        // 名称表：未手动排序的成员要按名称升序，而名称不在组织存储里。
        let names: HashMap<String, String> = self
            .host
            .connections()
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect();
        for group in &groups {
            let stored = crate::nav_store::list_group_members_detailed(root.as_deref(), &group.id);
            let ids = nav_order_members(&stored, |id| {
                names.get(id).cloned().unwrap_or_else(|| id.to_string())
            });
            for cid in &ids {
                membership
                    .entry(cid.clone())
                    .or_default()
                    .push(group.id.clone());
            }
            group_order.insert(group.id.clone(), ids);
        }
        let tags = crate::nav_store::list_all_tags(root.as_deref());
        let primary_group = crate::nav_store::list_primary_groups(root.as_deref());
        let ungrouped_order = crate::nav_store::list_ungrouped_order(root.as_deref());
        let driver_catalog = engine::persistence::load_driver_catalog();
        *self.driver_catalog.borrow_mut() = driver_catalog;
        let mut view = self.nav.borrow_mut();
        view.groups = groups;
        view.membership = membership;
        view.group_order = group_order;
        view.ungrouped_order = ungrouped_order;
        view.primary_group = primary_group;
        view.tags = tags;
        view.groups_loaded = true;
    }

    /// 分组是否折叠（缺省展开）。
    fn group_collapsed(&self, group_id: &str) -> bool {
        self.nav.borrow().collapsed_groups.contains(group_id)
    }

    /// 类别文件夹当前渲染条数上限（缺省 `NAV_FOLDER_PAGE_SIZE`）。
    fn folder_limit(&self, key: &str) -> usize {
        self.nav
            .borrow()
            .page_limit
            .get(key)
            .copied()
            .unwrap_or(ui::NAV_FOLDER_PAGE_SIZE)
    }

    /// 该节点的对象总数（数据侧）；未知时回落到已加载条数。
    fn node_total(&self, key: &str, loaded: usize) -> usize {
        self.nav
            .borrow()
            .child_total
            .get(key)
            .copied()
            .unwrap_or(loaded)
    }

    /// 生成不与现有分组重名的默认分组名。
    fn next_group_name(&self) -> String {
        let groups = self.nav.borrow().groups.clone();
        if !groups.iter().any(|g| g.name == "新建分组") {
            return "新建分组".to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("新建分组 {n}");
            if !groups.iter().any(|g| g.name == candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// 提交连接行内联标签输入（逗号分隔 → 覆盖式保存）。
    fn commit_nav_tags(&mut self, cx: &mut Context<Self>) {
        let Some(conn_id) = self.nav.borrow().tag_editor_for.clone() else {
            return;
        };
        let Some(input) = self.nav_tag_input.clone() else {
            return;
        };
        let text = input.read(cx).value().to_string();
        let tags: Vec<String> = text
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let root = self.host.project_root();
        match crate::nav_store::set_tags(&conn_id, root.as_deref(), &tags) {
            Ok(()) => self.reload_nav_org(),
            Err(e) => self.host.notice(format!("保存标签失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 分组表单初值（编辑时需带上描述；分组头渲染只拿到名称）。
    fn group_form_seed(&self, group_id: &str) -> GroupFormSeed {
        let view = self.nav.borrow();
        view.groups
            .iter()
            .find(|g| g.id == group_id)
            .map(|g| {
                GroupFormSeed::for_existing(g.id.clone(), g.name.clone(), g.description.clone())
            })
            .unwrap_or_else(|| GroupFormSeed::for_new(self.next_group_name()))
    }

    /// 提交分组表单（新建 / 编辑），成功时返回分组 ID。
    ///
    /// 名称唯一性不在这里拦：同名分组允许存在（排序 / 描述已经能区分），
    /// 但重名会让「移动到分组…」难以辨认，所以只在面板提示里点出来。
    fn save_group_form(
        &mut self,
        group_id: Option<String>,
        name: String,
        description: Option<String>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let root = self.host.project_root();
        let result = match group_id {
            Some(id) => {
                crate::nav_store::update_group(root.as_deref(), &id, &name, description.as_deref())
                    .map(|()| id)
            }
            None => {
                crate::nav_store::create_group_with(root.as_deref(), &name, description.as_deref())
            }
        };
        let saved = match result {
            Ok(id) => {
                self.reload_nav_org();
                let duplicated = self
                    .nav
                    .borrow()
                    .groups
                    .iter()
                    .filter(|g| g.name == name)
                    .count()
                    > 1;
                self.host.notice(
                    if duplicated {
                        format!("已保存分组「{name}」（存在同名分组）")
                    } else {
                        format!("已保存分组「{name}」")
                    },
                    cx,
                );
                Some(id)
            }
            Err(e) => {
                self.host.notice(format!("保存分组失败: {e}"), cx);
                None
            }
        };
        cx.notify();
        saved
    }

    /// 删除分组（仅解除关系，不删成员连接与缓存）。
    fn delete_group(&mut self, group_id: &str, cx: &mut Context<Self>) {
        let root = self.host.project_root();
        match crate::nav_store::delete_group(root.as_deref(), group_id) {
            Ok(()) => {
                self.reload_nav_org();
                self.host
                    .notice("分组已删除（成员连接保留）".to_string(), cx);
            }
            Err(e) => self.host.notice(format!("删除分组失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 提交行内「复制为模板」：成功后重载连接列表，并提示新名（不含密码）。
    fn commit_copy_connection(&mut self, cx: &mut Context<Self>) {
        let Some(from_id) = self.nav.borrow().copy_for.clone() else {
            return;
        };
        let Some(input) = self.nav_copy_input.clone() else {
            return;
        };
        let new_name = input.read(cx).value().trim().to_string();
        if new_name.is_empty() {
            return;
        }
        let result = self.host.copy_connection(&from_id, &new_name);
        self.nav.borrow_mut().copy_for = None;
        match result {
            Ok(()) => {
                self.reload_connections(cx);
                self.host
                    .notice(format!("已复制为模板：「{new_name}」（不含密码）"), cx);
            }
            Err(e) => self.host.notice(format!("复制失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 取消行内「复制为模板」。
    fn cancel_copy_connection(&mut self, cx: &mut Context<Self>) {
        self.nav.borrow_mut().copy_for = None;
        cx.notify();
    }

    /// 共享至当前项目（`G_` → 项目侧 `GP_` 快照）。
    fn share_connection_to_project(&mut self, conn_id: &str, name: &str, cx: &mut Context<Self>) {
        match self.host.share_connection(conn_id) {
            Ok(()) => {
                self.reload_connections(cx);
                self.host.notice(format!("「{name}」已共享至当前项目"), cx);
            }
            Err(e) => self.host.notice(format!("共享失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 取消共享：删除项目侧 `GP_` 快照（全局定义保留）。
    fn unshare_connection_from_project(
        &mut self,
        conn_id: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) {
        let result = self.host.delete_connection(conn_id).map(|_| ());
        match result {
            Ok(()) => {
                // `delete` 对项目侧 `GP_` 即「取消共享」；运行时连接一并断开（缓存保留）。
                self.host.disconnect(conn_id).ok();
                self.reload_connections(cx);
                self.host
                    .notice(format!("已取消共享：「{name}」（全局定义保留）"), cx);
            }
            Err(e) => self.host.notice(format!("取消共享失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 删除连接（物理删除；元数据缓存保留）。
    fn delete_connection(&mut self, conn_id: &str, name: &str, cx: &mut Context<Self>) {
        let result = self.host.delete_connection(conn_id);
        // 运行时连接一并断开（缓存保留，可在「缓存管理」清理）。
        self.host.disconnect(conn_id).ok();
        match result {
            Ok(msg) => {
                self.reload_connections(cx);
                self.host.notice(format!("「{name}」：{msg}"), cx);
            }
            Err(e) => self.host.notice(format!("删除连接失败：{e}"), cx),
        }
        cx.notify();
    }

    /// 重载当前作用域可见连接（增 / 删 / 共享后调用）。
    ///
    /// 连接清单与选中下标修正都在宿主侧（那是宿主自持状态），这里只广播一次重载。
    fn reload_connections(&mut self, cx: &mut Context<Self>) {
        self.host.reload_connections(cx);
        cx.notify();
    }

    /// 提交分组内联重命名。
    fn commit_group_rename(&mut self, cx: &mut Context<Self>) {
        let Some(group_id) = self.nav.borrow().group_rename_for.clone() else {
            return;
        };
        let Some(input) = self.nav_group_input.clone() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        if !name.is_empty() {
            let root = self.host.project_root();
            match crate::nav_store::rename_group(root.as_deref(), &group_id, &name) {
                Ok(()) => self.reload_nav_org(),
                Err(e) => self.host.notice(format!("重命名失败: {e}"), cx),
            }
        }
        self.nav.borrow_mut().group_rename_for = None;
        cx.notify();
    }

    /// 连接 / 断开运行时连接（保留缓存）。连接状态取运行时真值。
    fn toggle_connection(
        &mut self,
        conn_id: &str,
        project_root: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let already =
            self.host.is_connected(conn_id) || self.nav.borrow().connected.contains(conn_id);
        let outcome = if already {
            self.host.disconnect(conn_id).map(|_| false)
        } else {
            self.host.connect(conn_id).map(|_| true)
        };
        match outcome {
            Ok(is_connected) => {
                {
                    let mut view = self.nav.borrow_mut();
                    if is_connected {
                        view.connected.insert(conn_id.to_string());
                        // 刷新模式下预取过的标记清空（重新连接后重新预取）。
                        view.prefetched.clear();
                    } else {
                        view.connected.remove(conn_id);
                    }
                }
                if is_connected {
                    // C1：连接成功后提交后台预热（仅 catalogs/schemas），不阻塞 UI。
                    nav_jobs::warm_after_connect(conn_id, project_root);
                    self.ensure_warm_poll(cx);
                }
            }
            Err(e) => self.host.notice(format!("连接操作失败: {e}"), cx),
        }
        cx.notify();
    }

    /// 启动预热进度轮询（已有存活任务时不重复启动）。
    ///
    /// 预热在工作线程上跑，进度是原子量；这里用主线程 async 任务每 300ms 轮询并重绘，
    /// 结束后再重绘一次（让「预热中」指示消失）。
    fn ensure_warm_poll(&mut self, cx: &mut Context<Self>) {
        if let Some(task) = &self.warm_poll {
            if !task.is_ready() {
                return;
            }
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            // 预热任务在队列里异步启动，`warm_active` 置位有延迟；
            // 因此用「连续两次非活动才退出」的判据，同时覆盖启动与结束。
            let mut idle = 0;
            loop {
                executor.timer(std::time::Duration::from_millis(300)).await;
                if nav_jobs::warm_active() {
                    idle = 0;
                } else {
                    idle += 1;
                    if idle >= 2 {
                        break;
                    }
                }
                if weak.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        self.warm_poll = Some(task);
    }

    /// 刷新某节点（含连接根）的元数据：清掉已缓存子节点后重新加载当前层级。
    ///
    /// 保留展开态：子节点重新渲染时会根据最新 `expand_path` 再次懒加载。缓存文件不删。
    fn refresh_node(
        &self,
        conn_id: &str,
        key: &str,
        path: Option<NavPath>,
        cx: &mut Context<Self>,
    ) {
        let prefix = format!("{key}/");
        {
            let mut view = self.nav.borrow_mut();
            view.children
                .retain(|k, _| k != key && !k.starts_with(&prefix));
            view.attempted
                .retain(|k| k != key && !k.starts_with(&prefix));
            view.errors
                .retain(|k, _| k != key && !k.starts_with(&prefix));
            // 分页簿记也要清：孩子清了而总数留着的话，「加载更多」会按旧总数
            // 报出已不存在的余量（甚至对着空列表显示“余 5000”）。
            view.child_total
                .retain(|k, _| k != key && !k.starts_with(&prefix));
            view.page_limit
                .retain(|k, _| k != key && !k.starts_with(&prefix));
        }
        if let Some(p) = path {
            self.ensure_nav_loaded(conn_id, key, p, true, cx);
        }
        self.host.notice("已刷新元数据".to_string(), cx);
        cx.notify();
    }

    /// 刷新全部连接的元数据（清掉已加载子节点后重载仍展开的连接根）。
    fn refresh_all(&self, cx: &mut Context<Self>) {
        let roots: Vec<String> = {
            let view = self.nav.borrow();
            self.host
                .connections()
                .iter()
                .filter(|c| view.expanded.contains(&c.id))
                .map(|c| c.id.clone())
                .collect()
        };
        {
            let mut view = self.nav.borrow_mut();
            view.children.clear();
            view.attempted.clear();
            view.errors.clear();
            view.child_total.clear();
            view.page_limit.clear();
        }
        for cid in roots {
            self.ensure_nav_loaded(&cid, &cid, NavPath::Connection, true, cx);
        }
        self.host.notice("已刷新全部元数据".to_string(), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`use gpui_kit::*` 会把 gpui 的 `test` 宏带入作用域）。
    use super::nav_type_badge;
    use super::{
        insight_schema_target, nav_data_target, nav_merge_page, nav_object_type_label,
        nav_order_members, nav_reorder, nav_search_hit_property, nav_search_hit_ref,
        nav_search_query_ready, nav_step, nav_type_short_label, parse_nav_search,
    };
    use crate::model::{
        NavNode, NavNodeKind, NavPath, NavSource, ObjectKind, ObjectRef, PropertyKind,
    };

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// 「结构洞察」的靶：表 / 视图用它所在的 schema，schema 节点用自己；列 / 例行与
    /// catalog 节点不给（后者要跨方言的“全部 schema”语义，未定）。
    #[test]
    fn schema_insight_target_covers_tables_views_and_schemas() {
        let table_path = NavPath::Table {
            catalog: "shop".into(),
            schema: "public".into(),
            table: "orders".into(),
        };
        let expect = |target: Option<ObjectRef>| {
            let t = target.expect("应当给靶");
            assert_eq!(t.conn_id, "G_1");
            assert_eq!(t.catalog, "shop");
            assert_eq!(t.schema, "public");
            assert_eq!(t.kind, ObjectKind::Schema, "结构洞察的靶一律是 Schema 引用");
        };

        expect(insight_schema_target(
            &NavNodeKind::Table { row_estimate: None },
            Some(&table_path),
            "G_1",
        ));
        expect(insight_schema_target(
            &NavNodeKind::View,
            Some(&table_path),
            "G_1",
        ));
        expect(insight_schema_target(
            &NavNodeKind::Schema,
            Some(&NavPath::Schema {
                catalog: "shop".into(),
                schema: "public".into(),
            }),
            "G_1",
        ));

        // 列 / 例行（没有 schema 归属）/ catalog（跨方言语义未定）都不给
        assert!(
            insight_schema_target(
                &NavNodeKind::Column {
                    data_type: "int".into(),
                    nullable: false,
                    primary: false,
                    foreign: false,
                },
                None,
                "G_1",
            )
            .is_none()
        );
        assert!(
            insight_schema_target(
                &NavNodeKind::Catalog,
                Some(&NavPath::Catalog {
                    catalog: "shop".into(),
                }),
                "G_1",
            )
            .is_none()
        );
    }

    /// 索引命中 → 属性定位：表带 catalog/schema，列带所属表（parent），未知类别不接。
    #[test]
    fn search_hit_maps_to_property_ref() {
        let hit =
            |object_type: &str, name: &str, parent: Option<&str>| crate::nav_jobs::SearchHit {
                conn_id: "P_conn_hit".to_string(),
                conn_label: "本地库".to_string(),
                driver: "duckdb".to_string(),
                object_type: object_type.to_string(),
                object_name: name.to_string(),
                parent_name: parent.map(str::to_string),
                catalog: Some("main".to_string()),
                schema: Some("public".to_string()),
                snippet: None,
            };

        let table = nav_search_hit_property(&hit("table", "orders", None)).expect("表命中应可定位");
        assert_eq!(table.kind, PropertyKind::Table);
        assert_eq!(table.name, "orders");
        assert_eq!(table.catalog.as_deref(), Some("main"));
        assert_eq!(table.schema.as_deref(), Some("public"));
        assert_eq!(table.parent, None);
        assert_eq!(table.source, NavSource::from_conn_id("P_conn_hit"));

        let col = nav_search_hit_property(&hit("column", "order_id", Some("orders")))
            .expect("列命中应可定位");
        assert_eq!(col.kind, PropertyKind::Column);
        assert_eq!(
            col.parent.as_deref(),
            Some("orders"),
            "列必须带所属表（属性面板靠它区分是哪张表的列）"
        );

        // 未知类别（如索引里可能出现的 index / routine）不提供属性定位
        assert!(nav_search_hit_property(&hit("index", "idx_a", None)).is_none());
    }

    /// 搜索词门槛：≥ 2 个字符（含空白裁剪）；单字符不打搜索（命中面过大且几乎必然还要改）。
    #[test]
    fn search_query_requires_two_chars() {
        assert!(!nav_search_query_ready(""));
        assert!(!nav_search_query_ready("o"));
        assert!(!nav_search_query_ready("  o  "));
        assert!(nav_search_query_ready("or"));
        assert!(nav_search_query_ready(" 订单 "));
    }

    /// 命中行的类别短标签（给用户看的中文；未知类别兜到「对象」）。
    #[test]
    fn object_type_labels_are_localized() {
        assert_eq!(nav_object_type_label("table"), "表");
        assert_eq!(nav_object_type_label("view"), "视图");
        assert_eq!(nav_object_type_label("schema"), "模式");
        assert_eq!(nav_object_type_label("column"), "列");
        assert_eq!(
            nav_object_type_label("whatever"),
            "对象",
            "未知类别不得显示成空白"
        );
    }

    /// Mock / 洞察表入口的引用靶：视图必须标成 `View`（曾经一律标成 `Table`）。
    #[test]
    fn data_target_kind_follows_the_node() {
        let target = |kind: NavNodeKind| {
            nav_data_target(&kind, "P_1", "shop".into(), "public".into(), "orders".into())
        };
        assert_eq!(
            target(NavNodeKind::Table { row_estimate: None })
                .expect("表应给靶")
                .kind,
            ObjectKind::Table
        );
        assert_eq!(
            target(NavNodeKind::View).expect("视图应给靶").kind,
            ObjectKind::View,
            "视图不能标成表（跨屏身份靠 kind 区分）"
        );
        assert!(
            target(NavNodeKind::Schema).is_none(),
            "非数据类节点不给靶"
        );
    }

    /// `ObjectRef` 的 key 与导航树节点 key **同构**——这是搜索命中与树节点
    /// 判定“是不是同一个对象”的契约（将来「在树中定位」直接靠它，不再各拼一套）。
    #[test]
    fn object_ref_key_matches_nav_node_key() {
        // 表：树上的 key = child_key(conn, [catalog, schema, name])
        let table = ObjectRef::table("P_1", "shop", "public", "orders");
        assert_eq!(
            table.key(),
            NavNode::child_key("P_1", &["shop", "public", "orders"])
        );

        // 列：树上的 key = child_key(conn, [catalog, schema, 表, 列])
        let column = ObjectRef::column("P_1", "shop", "public", "orders", "order_id");
        assert_eq!(
            column.key(),
            NavNode::child_key("P_1", &["shop", "public", "orders", "order_id"])
        );

        // schema：树上的 key = child_key(conn, [catalog, schema])
        let schema = ObjectRef::schema("G_1", "shop", "public");
        assert_eq!(schema.key(), NavNode::child_key("G_1", &["shop", "public"]));

        // 无 Catalog 层的驱动：导航侧把 catalog 同时当 schema（`load_folders(conn, c, c)`），
        // 因此两边都拿到重复段——关键是一致，不是好看。
        let no_schema = ObjectRef::table("G_1", "mall", "mall", "order");
        assert_eq!(
            no_schema.key(),
            NavNode::child_key("G_1", &["mall", "mall", "order"])
        );
    }

    /// 索引命中 → 统一引用 → 属性定位：走完这条通路后，字段与树节点那条路完全一致。
    #[test]
    fn search_hit_ref_agrees_with_tree_side_ref() {
        let hit = crate::nav_jobs::SearchHit {
            conn_id: "P_1".to_string(),
            conn_label: "本地库".to_string(),
            driver: "postgres".to_string(),
            object_type: "column".to_string(),
            object_name: "order_id".to_string(),
            parent_name: Some("orders".to_string()),
            catalog: Some("shop".to_string()),
            schema: Some("public".to_string()),
            snippet: None,
        };
        let from_hit = nav_search_hit_ref(&hit).expect("列命中应可寻址");
        let from_tree = ObjectRef::column("P_1", "shop", "public", "orders", "order_id");
        assert_eq!(
            from_hit.key(),
            from_tree.key(),
            "搜索与树必须给出同一个 key"
        );
        assert_eq!(from_hit, from_tree);
    }

    /// 分页追加：按 key 去重，并返回新增条数（追加方靠它判断“是否翻到底”）。
    #[test]
    fn merge_page_dedupes_by_key_and_reports_appended() {
        let node = |key: &str| {
            NavNode::new(
                key.to_string(),
                key.to_string(),
                "P_conn_merge",
                NavNodeKind::Table { row_estimate: None },
                false,
            )
        };

        let mut loaded = vec![node("a"), node("b")];
        // 与已有重叠一条（b）+ 新的一条（c）
        let appended = nav_merge_page(&mut loaded, vec![node("b"), node("c")]);
        assert_eq!(appended, 1, "只有 c 是新增");
        assert_eq!(
            loaded.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"],
            "顺序保持：已加载在前、新页在后"
        );

        // 整页重复（刷新后二次读交叠）：新增 0，长度不变
        assert_eq!(nav_merge_page(&mut loaded, vec![node("a")]), 0);
        assert_eq!(loaded.len(), 3, "重复节点不得再入列（元素 id 会冲突）");
    }

    #[test]
    fn nav_reorder_moves_and_appends() {
        let base = ids(&["a", "b", "c"]);
        // 已在列表里：摘除后插到目标之前。
        assert_eq!(
            nav_reorder(&base, "c", Some("a")),
            Some(ids(&["c", "a", "b"]))
        );
        assert_eq!(
            nav_reorder(&base, "a", Some("c")),
            Some(ids(&["b", "a", "c"]))
        );
        // 不在列表里（新归组）：插到目标之前；无目标则追加。
        assert_eq!(
            nav_reorder(&base, "z", Some("b")),
            Some(ids(&["a", "z", "b", "c"]))
        );
        assert_eq!(
            nav_reorder(&base, "z", None),
            Some(ids(&["a", "b", "c", "z"]))
        );
        // 空容器：首个成员落在末尾。
        assert_eq!(nav_reorder(&[], "z", None), Some(ids(&["z"])));
    }

    #[test]
    fn nav_step_swaps_neighbours_and_stops_at_bounds() {
        let base = ids(&["a", "b", "c"]);
        // 中间项上下各一步。
        assert_eq!(nav_step(&base, "b", -1), Some(ids(&["b", "a", "c"])));
        assert_eq!(nav_step(&base, "b", 1), Some(ids(&["a", "c", "b"])));
        // 末项上移 / 首项下移。
        assert_eq!(nav_step(&base, "c", -1), Some(ids(&["a", "c", "b"])));
        assert_eq!(nav_step(&base, "a", 1), Some(ids(&["b", "a", "c"])));
        // 边界：首项上移 / 末项下移 → 无变化。
        assert_eq!(nav_step(&base, "a", -1), None);
        assert_eq!(nav_step(&base, "c", 1), None);
        // 不在容器里（例如对象节点被过滤掉）→ 无变化。
        assert_eq!(nav_step(&base, "z", 1), None);
        assert_eq!(nav_step(&[], "z", 1), None);
    }

    #[test]
    fn nav_order_members_puts_manual_first_then_names() {
        let name = |id: &str| match id {
            "P_b" => "zeta".to_string(),
            "P_c" => "Alpha".to_string(),
            "P_d" => "beta".to_string(),
            other => other.to_string(),
        };
        // 手动排序的两个按序号在前；未排的三个按名称（大小写不敏感）升序在后。
        let stored = vec![
            ("P_a".to_string(), Some(1)),
            ("P_b".to_string(), None),
            ("P_d".to_string(), Some(0)),
            ("P_c".to_string(), None),
            ("P_e".to_string(), None),
        ];
        assert_eq!(
            nav_order_members(&stored, name),
            ids(&["P_d", "P_a", "P_c", "P_e", "P_b"])
        );
        // 全未排：纯名称序。
        let stored = vec![("P_b".to_string(), None), ("P_c".to_string(), None)];
        assert_eq!(nav_order_members(&stored, name), ids(&["P_c", "P_b"]));
        // 全已排：按序号（与本传入顺序无关）。
        let stored = vec![("P_b".to_string(), Some(1)), ("P_a".to_string(), Some(0))];
        assert_eq!(nav_order_members(&stored, name), ids(&["P_a", "P_b"]));
        assert!(nav_order_members(&[], name).is_empty());
    }

    #[test]
    fn nav_reorder_skips_no_op_moves() {
        let base = ids(&["a", "b", "c"]);
        // 落到自己身上。
        assert_eq!(nav_reorder(&base, "b", Some("b")), None);
        // 已经正好在目标之前。
        assert_eq!(nav_reorder(&base, "a", Some("b")), None);
        // 已在末尾且要追加到末尾。
        assert_eq!(nav_reorder(&base, "c", None), None);
        // 目标不在列表里（被并发删除）：退化为追加；已在末尾则不写库。
        assert_eq!(
            nav_reorder(&base, "a", Some("gone")),
            Some(ids(&["b", "c", "a"]))
        );
        assert_eq!(nav_reorder(&base, "c", Some("gone")), None);
    }

    #[test]
    fn type_badge_maps_known_types_and_falls_back() {
        // 已知类型：形状与 2 字母按映射表（原型设计 §2.3）。
        assert_eq!(
            nav_type_badge("postgresql"),
            ("icons/database.svg", "PG".into())
        );
        assert_eq!(nav_type_badge("sqlite"), ("icons/file.svg", "SQ".into()));
        assert_eq!(nav_type_badge("redis"), ("icons/braces.svg", "RD".into()));
        // 目录外类型：回退通用形状 + 类型名首 2 字母（大写）。
        let (path, letters) = nav_type_badge("snowflake");
        assert_eq!(path, "icons/database.svg");
        assert_eq!(letters, "SN");
        // 空类型 id：不做空字母，回退 `DB`。
        assert_eq!(nav_type_badge("").1, "DB");
    }

    #[test]
    fn search_facets_parse_tokens_and_free_text() {
        let p = parse_nav_search("prod scope:global type:mysql tag:核心");
        assert_eq!(p.free, "prod");
        assert_eq!(p.source, Some(NavSource::Global));
        assert_eq!(p.db_type.as_deref(), Some("mysql"));
        assert_eq!(p.tag.as_deref(), Some("核心"));
        assert_eq!(p.active, 3);
        // `source:` 为 `scope:` 历史别名，短码亦可。
        let p = parse_nav_search("source:P driver:postgres_native");
        assert_eq!(p.source, Some(NavSource::Project));
        assert_eq!(p.driver.as_deref(), Some("postgres_native"));
        assert!(p.free.is_empty());
        // 空值 / 未识别前缀不吞字（保留在自由文本）。
        let p = parse_nav_search("tag: foo:bar");
        assert!(p.tag.is_none());
        assert_eq!(p.free, "tag: foo:bar");
        assert_eq!(p.active, 0);
    }

    #[test]
    fn type_short_label_strips_category_suffix() {
        assert_eq!(nav_type_short_label("postgresql"), "PostgreSQL");
        // 未知类型回退原 id。
        assert_eq!(nav_type_short_label("snowflake"), "snowflake");
    }
}

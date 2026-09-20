//! 导航面板的纯函数与视觉原语（无 `self`、无 I/O、可单测）。
//!
//! 为什么要独立成模块：类型徽标映射 / 类别图标 / 展开指示 / 激活条 / 相对时间 /
//! 命中高亮这些都是**纯计算**，与面板状态无关；它们此前混在 7800 行的单一文件里，
//! 改一处要在一屏里找半天。搬出后根文件只留「状态 + 协议 + 导航锚点」。
//!
//! 可见性：搬出的项加 `pub(super)`（= 在 `crate::nav_view` 及其子孙模块可见），
//! 根模块用 `use self::primitives::*;` 收回来，**调用点一字未改**。

use super::*;

/// 解析搜索框文本：拆出 `scope:` / `type:` / `driver:` / `tag:` token，其余为自由文本。
///
/// 值不引号包裹（含空格需自行避免）；`scope` / `source` 支持全名与短码。
/// 无法识别的 token（如空值）原样保留在自由文本里，避免“输入中丢字”。
pub(super) fn parse_nav_search(raw: &str) -> NavSearchFacets {
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

/// 徽标 hover 卡：类型 / 状态 / 驱动（gpui-kit 0.6.1 无通用 `.tooltip` 扩展，故用 `HoverCard`）。
/// 归属域列 tooltip（原型设计 §2.3 要求）：把 `P` / `G` / `GP` 展开成一句人话。
///
/// 为何必须有：短码是**系统事实**（记录存在哪个域、谁能看到），但两个字母对用户不是自解释的；
/// 属性面板只接对象，不接「这条连接存在哪」，所以这句解释的稳态去处在行内——与徽标 hover 卡
/// （类型 / 状态 / 驱动）同一套 300ms 延迟，避免鼠标扫过整棵树时弹层乱闪。
pub(super) fn nav_scope_hover_card(
    id: SharedString,
    trigger: impl IntoElement + 'static,
    text: String,
) -> impl IntoElement {
    use gpui_kit::component::hover_card::HoverCard;
    HoverCard::new(id)
        .open_delay(std::time::Duration::from_millis(300))
        .trigger(trigger)
        .content(move |_, _window, cx| {
            let fg = cx.theme().colors.foreground;
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(fg)
                .child(text.clone())
        })
}

/// 归属域短码 / 文字的 tooltip 文案（§2.3 的三档）。
pub(super) fn nav_scope_tooltip(source: NavSource) -> String {
    match source {
        NavSource::Project => "归属域：项目连接（仅本项目可见）".to_string(),
        NavSource::Global => "归属域：全局连接（所有项目可见）".to_string(),
        NavSource::Shared => "归属域：项目共享（全局定义 + 当前项目快照）".to_string(),
    }
}

pub(super) fn nav_badge_hover_card(
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

/// 类型文案（徽标 hover 卡 / facet 菜单用）：**目录优先**，硬编码表降为兜底。
///
/// `from_catalog` = `(类型显示名, 类型分类键)`，来自驱动目录（`data_source_types`）。
/// 为什么目录优先：新增一个库族只需往库里加一行，UI 不必改代码；
/// 且同一个库在连接对话框（读目录）与导航（曾硬编码）不再出现两套名字。
pub(super) fn nav_type_label(type_id: &str, from_catalog: Option<(&str, &str)>) -> String {
    if let Some((name, category)) = from_catalog {
        let name = name.trim();
        if !name.is_empty() {
            let cat = nav_type_category_label(category);
            return if cat.is_empty() {
                name.to_string()
            } else {
                format!("{name}（{cat}）")
            };
        }
    }
    // 兜底：目录未就绪 / 旧数据（类型不在目录里）时用内置表，仍认不出就原样显示 id。
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

/// 类型分类键 → 中文（视图层词汇；`data_source_types.category` 的已知取值）。
///
/// 未知分类返回空字符串（调用方就不加后缀，不编造分类）。
pub(super) fn nav_type_category_label(category: &str) -> &'static str {
    match category {
        "relational" => "关系型",
        "file-based" => "文件型",
        "analytics" => "分析型",
        "nosql" => "非关系型",
        _ => "",
    }
}

/// 类型短名（无分类后缀），facet 菜单 / 筛选药丸用。
pub(super) fn nav_type_short_label(type_id: &str, from_catalog: Option<(&str, &str)>) -> String {
    if let Some((name, _)) = from_catalog {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    let full = nav_type_label(type_id, None);
    full.split('（').next().unwrap_or(&full).to_string()
}

/// 类型徽标映射：数据库类型 id →（形状资产路径，2 字母缩写）。
///
/// 形状取自 `gpui-kit-assets` 全量 Lucide（`AllAssets` 已注册，无需新增资产）；
/// **字母是权威识别，形状是冗余强化 + 扫视加速**（原型设计 §2.3）。
/// 目录外类型回退通用形状 + 类型名首 2 字母。
pub(super) fn nav_type_badge(type_id: &str) -> (&'static str, String) {
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

/// 内联图标（统一走资产路径）。
///
/// 为何不用 `IconName`：`gpui_kit::component::IconName` 是**组件兼容枚举**，
/// 只含 default-icons 那 ~100 个；本面板要的类别形状（`table` / `columns-3` /
/// `square-function` / `pencil` …）都不在其中。全量枚举在 `gpui_kit::assets::IconName`，
/// 而 `Icon::new` 只收兼容枚举——所以按路径取（与本文件 `nav_type_badge` 同法；
/// 路径写错时资产源返回 Err，日志里看得见，不会静默空白）。
pub(super) fn nav_icon(path: &'static str, size: f32, color: Hsla) -> Icon {
    Icon::empty()
        .path(path)
        .size(rems(size))
        .flex_none()
        .text_color(color)
}

/// 节点图标（**形状 = 类别**）：原型设计 §3「图标角色」表逐项对应。
///
/// 为什么类别不用颜色：颜色在本模块是稀缺通道，已由「连接徽标色 = 状态」占用
/// （§1.1 心智模型 / v7 决策：颜色给可操作性，不给静态身份）。此前用 8px 色块表达类别时，
/// 同一个 `info` 在连接行是「连接中」、在表行是「这是表」——同屏同色两义；
/// 改成形状后颜色只留给失败态（见 `render_nav_node`）。
///
/// 资产取自 `gpui-kit-assets` 全量 Lucide（`AllAssets` 已注册，无需新增）。
/// `expanded` 只影响文件夹的开 / 合形状，其余类别恒定。
pub(super) fn nav_kind_icon(kind: &NavNodeKind, expanded: bool) -> &'static str {
    match kind {
        NavNodeKind::Connection { .. } | NavNodeKind::Catalog => "icons/database.svg",
        NavNodeKind::Schema => "icons/folder-tree.svg",
        NavNodeKind::Folder(_) => {
            if expanded {
                "icons/folder-open.svg"
            } else {
                "icons/folder.svg"
            }
        }
        NavNodeKind::Table { .. } => "icons/table.svg",
        NavNodeKind::View => "icons/eye.svg",
        NavNodeKind::Column { .. } => "icons/columns-3.svg",
        NavNodeKind::Routine { .. } => "icons/square-function.svg",
        NavNodeKind::Sequence => "icons/list-ordered.svg",
        NavNodeKind::Trigger => "icons/zap.svg",
    }
}

/// 展开指示（`chevron-down` / `chevron-right`）。
///
/// 槽宽固定 `w_2p5`（10px）——**与字符 `▾/▸` 时期同宽**：树行内容 = `pl` + 槽 + `gap_1`
/// 恰好一个 `TREE_INDENT` 步长，「加载更多 / 已定位」行的 `pl` 靠这个对齐，
/// 换图标不得让它漂移（图标居中，溢出槽宽 1px，落在既有的 `gap_1` 里）。
/// 无子节点时留同宽空位，保证同层行标题左边缘对齐。
pub(super) fn nav_disclosure(has_children: bool, expanded: bool, color: Hsla) -> Div {
    let slot = tree::disclosure_slot();
    if !has_children {
        return slot;
    }
    slot.child(tree::disclosure_icon(expanded, color))
}

/// 限定名（`catalog.schema.name`，跳过空段）：用于复制与生成 SELECT。
///
/// 无独立 Schema 层的驱动（MySQL / SQLite / DuckDB）导航把 schema 传成 catalog，
/// 相等时只留一份，避免出现 `db.db.name`。
pub(super) fn nav_qualified_name(prop: &PropertyRef) -> String {
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

/// 徽标是否挂呼吸动画（原型设计 §2.3：`连接中` = `info` + 脉冲）。
///
/// 为何抽成纯函数：这条规则出错的表现是「窗口永远在每帧重绘」（循环动画每帧申请一帧），
/// 肉眼与截图都看不出来，只能靠测试钉住。
///
/// 为何只给暂态：建连 / 预热是几秒就过去的过程，脉冲说的是「还在动」；
/// 已连 / 未连 / 失败都是稳态，挂上去就变成**环境装饰**（上游动效规范：
/// 动效解释变化，不做常驻装饰）。
pub(super) fn nav_badge_pulses(status: NavBadgeStatus) -> bool {
    matches!(status, NavBadgeStatus::Connecting)
}

/// 「结构洞察」的靶：表 / 视图用它所在的 schema，schema 节点用自己；列 / 例行与
///
/// 为什么不给 catalog 节点：`table_schema` 的取值各家不同（MySQL 里就是库名、
/// PG 里是 schema），catalog 级的「全部 schema」要拼一套跨方言语义——先不做，
/// 需要时按方言补（施工单见 `insight-dev-plan.md` §10）。
pub(super) fn insight_schema_target(
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
pub(super) fn nav_data_target(
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

/// 计算「把 `moving` 放到 `before` 之前」后的容器顺序；无需变更时返回 `None`。
///
/// - `moving` 不在 `ids` 里 → 视为新加入，插到 `before` 之前（`None` 则追加到末尾）；
/// - `moving` 已在 `ids` 里 → 先摘除再插入，因此「拖到自己身上」与「已经就位」都返回 `None`；
/// - `before` 不在 `ids` 里（目标行被过滤掉）→ 追加到末尾。
///
/// 纯函数：不碰存储；落库顺序由调用方一次写 `0..n`（见 `crate::nav_store::set_container_order`）。
pub(super) fn nav_reorder(
    ids: &[String],
    moving: &str,
    before: Option<&str>,
) -> Option<Vec<String>> {
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

/// 主组解析：**显式指定**优先（且仍在所属分组内，被移出则忽略），
/// 否则取排序最前的分组；无任何分组 → `None`（即「未分组」）。
///
/// 渲染（多组只全亮呈现一次）与键盘重排（改哪个容器的顺序）共用同一条规则。
pub(super) fn nav_primary_scope(
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
pub(super) fn nav_step(ids: &[String], moving: &str, delta: i32) -> Option<Vec<String>> {
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
pub(super) fn nav_order_members(
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
pub(super) fn nav_node_matches(
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
pub(super) fn nav_search_hit_ref(hit: &nav_jobs::SearchHit) -> Option<ObjectRef> {
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
pub(super) fn nav_search_hit_property(hit: &nav_jobs::SearchHit) -> Option<PropertyRef> {
    nav_search_hit_ref(hit).map(|object| crate::model::property_ref_of(&object))
}

/// 索引命中 → 合成节点类别（只为取图标颜色，不参与树挂载）。
pub(super) fn nav_search_hit_kind(hit: &nav_jobs::SearchHit) -> NavNodeKind {
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
pub(super) fn nav_object_type_label(object_type: &str) -> &'static str {
    ObjectKind::parse_index_str(object_type)
        .map(ObjectKind::label)
        .unwrap_or("对象")
}

/// 搜索词是否够格打一次索引搜索（太短时命中面过大，且首字符几乎必然还要改）。
pub(super) fn nav_search_query_ready(query: &str) -> bool {
    query.trim().chars().count() >= 2
}

/// 选中的这一行**值不值得跨重启记住**。
///
/// 只记**树上真有的行**：
/// - 连接行 / 树行的 key 是 `{连接 id}` 或 `{连接 id}/…`（与 `NavNode::child_key` 同构）；
/// - 搜索命中行（`search:{位次}:…`）、分组头（`group:`）、引用行（`ref:`）、
///   「更多」/「已定位」（`#more` / `#jump`）都是**视图临时行**：重启后要么不存在、
///   要么位置会变（搜索行还带位次），恢复它们只会让选中跑到一个看上去无关的行上。
pub(super) fn nav_selection_is_persistable(key: &str) -> bool {
    const TEMPORARY_PREFIXES: [&str; 3] = ["search:", "group:", "ref:"];
    if TEMPORARY_PREFIXES.iter().any(|p| key.starts_with(p)) {
        return false;
    }
    !key.contains("#more") && !key.contains("#jump")
}

/// 相对时间文案（面板底状态行）：`刚刚` / `N 分钟前` / `N 小时前` / `N 天前`。
///
/// 与 `analytics_resource::present::format_relative_time` 同一口径（那边显示文件时间，
/// 这边显示元数据加载时间）；时钟回拨（`duration_since` 报错）按「刚刚」处理，
/// 不显示负数。
pub(super) fn nav_relative_time(now: SystemTime, then: SystemTime) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    let Ok(elapsed) = now.duration_since(then) else {
        return "刚刚".to_string();
    };
    let seconds = elapsed.as_secs();
    if seconds < MINUTE {
        "刚刚".to_string()
    } else if seconds < HOUR {
        format!("{} 分钟前", seconds / MINUTE)
    } else if seconds < DAY {
        format!("{} 小时前", seconds / HOUR)
    } else {
        format!("{} 天前", seconds / DAY)
    }
}

/// 把一页结果并入已加载列表，返回**新增**条数（去重后）。
///
/// 抽成纯函数是为了可测：索引翻页理论上不重叠，但刷新 / 结构变更后两次读可能交叠，
/// 重复节点会在树上出现两次（key 相同 → 元素 id 冲突，删除 / 选中都会错位）。
pub(super) fn nav_merge_page(loaded: &mut Vec<NavNode>, page: Vec<NavNode>) -> usize {
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
pub(super) fn nav_name_highlight(name: &str, filter: &str, match_bg: Hsla, fg: Hsla) -> Div {
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
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(fg)
                        .child(before)
                        .child(div().rounded_sm().bg(match_bg).child(hit))
                        .child(after);
                }
            }
        }
    }
    div()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .text_color(fg)
        .child(name.to_string())
}

/// 树行下的附加行（「加载中…」/ 错误 / 未连接提示），带缩进。
///
/// 高度**钉成常量**（`ui::NAV_SUBLINE`）：虚拟列表按估算高度给每行分槽位，
/// 内容高过槽位就会压到下一行——附加行的高度不能交给排版自己定。
pub(super) fn nav_subline_at(text: &str, color: Hsla, indent: f32) -> Div {
    div()
        .h_flex()
        .items_center()
        .w_full()
        .h(rems(ui::NAV_SUBLINE))
        .pl(rems(indent))
        .pr_1()
        .text_xs()
        .text_color(color)
        .overflow_hidden()
        .child(text.to_string())
}

/// 固定缩进的附加行（连接行用；1.5rem 与旧的 `pl_6` 同值）。
pub(super) fn nav_subline(text: &str, color: Hsla) -> Div {
    nav_subline_at(text, color, 1.5)
}

/// 一行的估算高度（rem → px）。
///
/// 必须与渲染里那几处的显式高度**成对维护**（`ui::NAV_ROW_*` / `ui::NAV_SUBLINE` /
/// `ui::NAV_EDITOR_*`）：虚拟列表按估算高度给每行分槽位，内容高过槽位就会压到下一行。
/// 不写成 `NavView` 方法：它只读状态（借用一份即可，不必拿整个面板）。
pub(super) fn nav_row_height(
    row: &NavRow,
    view: &NavViewState,
    show_tags: bool,
) -> tree::RowHeight {
    // 基础行高按行型取（三档，与 `ui.rs` 的 `NAV_ROW_*` 成对），附加块逐个链上：
    // 链式写法让「多算了哪一块」一眼看得出（虚拟列表不回写实测高度，估算与渲染是硬契约）。
    let base = match row {
        NavRow::GroupHeader { .. } => ui::NAV_ROW_GROUP,
        NavRow::Connection { .. } | NavRow::Reference { .. } => ui::NAV_ROW_CONNECTION,
        NavRow::Tree { .. }
        | NavRow::More { .. }
        | NavRow::Jump { .. }
        | NavRow::SearchHit { .. } => ui::NAV_ROW_TREE,
    };
    let mut height = tree::RowHeight::new(base);
    match row {
        NavRow::Connection { conn, .. } => {
            let connected = view.connected.contains(&conn.id) || conn.connected;
            let loading = view.loading.contains(&conn.id);
            let error = view.errors.contains_key(&conn.id);
            // 「未连接 · 右键连接」提示：展开、没连上、也没孩子、没报错、没在加载。
            let expanded = view.expanded.contains(&conn.id);
            let has_children = view.children.get(&conn.id).is_some_and(|c| !c.is_empty());
            height = height
                .add_if(loading, ui::NAV_SUBLINE)
                .add_if(
                    show_tags && view.tags.get(&conn.id).is_some_and(|t| !t.is_empty()),
                    ui::NAV_SUBLINE,
                )
                .add_if(
                    view.group_picker_for.as_deref() == Some(conn.id.as_str()),
                    ui::NAV_EDITOR_GROUP,
                )
                .add_if(
                    view.tag_editor_for.as_deref() == Some(conn.id.as_str()),
                    ui::NAV_EDITOR_TAG,
                )
                .add_if(
                    view.copy_for.as_deref() == Some(conn.id.as_str()),
                    ui::NAV_EDITOR_COPY,
                )
                .add_if(
                    expanded && !connected && !has_children && !error && !loading,
                    ui::NAV_SUBLINE,
                )
                .add_if(error, ui::NAV_SUBLINE);
        }
        NavRow::Tree { node, .. } => {
            height = height
                .add_if(view.loading.contains(&node.key), ui::NAV_SUBLINE)
                .add_if(view.errors.contains_key(&node.key), ui::NAV_SUBLINE);
        }
        _ => {}
    }
    height
}

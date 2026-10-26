//! 导航面板的纯函数与视觉原语（无 `self`、无 I/O、可单测）。
//!
//! 为什么要独立成模块：类型徽标映射 / 类别图标 / 展开指示 / 激活条 / 相对时间 /
//! 命中高亮这些都是**纯计算**，与面板状态无关；它们此前混在 7800 行的单一文件里，
//! 改一处要在一屏里找半天。搬出后根文件只留「状态 + 协议 + 导航锚点」。
//!
//! 可见性：搬出的项加 `pub(super)`（= 在 `crate::nav_view` 及其子孙模块可见），
//! 根模块用 `use self::primitives::*;` 收回来，**调用点一字未改**。

use super::*;
use crate::nav::capability::NavOp;
use crate::nav::family_profile;
use crate::nav_host::ObjectMaskRule;
use regex::Regex;

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

/// 徽标 hover 卡：类型 / 状态 / 驱动（gpui-kit 0.7.1 无通用 `.tooltip` 扩展，故用 `HoverCard`）。
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
        NavSource::Shared => "归属域：项目 + 全局区域（共享连接同时出现在「项目」与「全局」两处）".to_string(),
    }
}

/// 剥离连接名里的区域前缀（`[项目] / [全局] / [共享] `）。
/// 区域信息由「归属域」列承载；名称里的重复前缀只挤占宽度（设置项可开关）。
pub(super) fn strip_nav_scope_prefix(name: &str) -> String {
    let name = name.trim_start();
    for prefix in ["[项目] ", "[全局] ", "[共享] "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    name.to_string()
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
        "github" => "GitHub（仓库源）",
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

/// S3-④ 行估计徽标文本：千分位 + 超长截断（B 档文档 §1.4.4）。
///
/// - `0`（驱动给 0 / 空表）→ `"0"`（如实，不隐藏；它也是信息）；
/// - `≥ 1_000_000_000` → `"{:.1}B"`（如 `1.2B`）；
/// - `≥ 1_000_000` → `"{:.1}M"`（如 `3.4M`）；
/// - 其余 → 千分位（`1,234,567`）。
///
/// 前缀 `≈` 由渲染处统一加（表行徽标与属性面板共用本函数时各自加，不写死在文本里）。
pub(super) fn format_row_estimate(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else {
        // 千分位：分组从低位起每 3 位插 `,`（u64 无负数，直接按位组）。
        let digits = n.to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, ch) in digits.chars().enumerate() {
            let from_left = i;
            let remaining = digits.len() - from_left;
            out.push(ch);
            if remaining > 1 && remaining % 3 == 1 {
                out.push(',');
            }
        }
        out
    }
}

/// D12：表数据段大小（字节）→ 紧凑徽标文本，二进制 1024 档位（B / KB / MB / GB）。
///
/// 与 [`format_row_estimate`] 的行数档位不同：行数按 1000 千进制 B/M，大小是字节
/// 单位，按 1024 二进制进制；`< 1024` 走整数 B（`0` → `0 B`，如实不隐藏），其余
/// 档位保留 1 位小数。本函数只格式化 `Some` 数值；`None`（未回填 / 驱动不支持）
/// 由渲染处决定「显 … 占位」还是「徽标安静消失」，不在此把 `None` 文本化。
pub(super) fn format_data_length(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => format!("{:.1} KB", b as f64 / KB as f64),
        b if b < GB => format!("{:.1} MB", b as f64 / MB as f64),
        b => format!("{:.1} GB", b as f64 / GB as f64),
    }
}

/// D12 连接行主机名副标文本（纯函数）：档案 `show_host_name` 开启且 host 非空才返回；
/// port 非空拼 `host:port`，空则只显 host；host 为 `None` / 空串 → `None`（整槽不挂）。
pub(super) fn host_subline_text(
    profile: &family_profile::InlineStatsProfile,
    host: Option<&str>,
    port: Option<i32>,
) -> Option<String> {
    if !profile.show_host_name {
        return None;
    }
    let host = host?.trim();
    if host.is_empty() {
        return None;
    }
    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// D12 对象行注释副标文本（纯函数）：档案 `show_description` 开启 + 描述类对象
/// （表 / 视图 / 例程 / 序列 / 触发器；列 / 文件夹 / 连接等不显）+ 注释非空才返回。
pub(super) fn comment_subline_text(
    profile: &family_profile::InlineStatsProfile,
    kind: &NavNodeKind,
    comment: Option<&str>,
) -> Option<String> {
    if !profile.show_description {
        return None;
    }
    let visible = matches!(
        kind,
        NavNodeKind::Table { .. }
            | NavNodeKind::View
            | NavNodeKind::Routine { .. }
            | NavNodeKind::Sequence
            | NavNodeKind::Trigger
    );
    if !visible {
        return None;
    }
    let comment = comment?.trim();
    if comment.is_empty() {
        return None;
    }
    Some(comment.to_string())
}

/// D12 表大小徽标文本（纯函数）：档案 `show_size` 开启 + 已回填才返回；未回填 / 驱动
/// 不支持安静消失。在途「…」占位由行估计槽统一承担（两查询同批次、同一在途集合），
/// 本函数不重复占位。
pub(super) fn size_badge_text(
    profile: &family_profile::InlineStatsProfile,
    data_length: Option<u64>,
) -> Option<String> {
    if !profile.show_size {
        return None;
    }
    data_length.map(format_data_length)
}

/// D12 文件夹子计数徽标文本（纯函数）：档案 `show_child_count` 开启 + 文件夹类型 +
/// 计数大于 0 才返回；候选总数优先（`child_total`，分页前由数据侧告知），缺省回退已
/// 加载子节点数；零个不显。
pub(super) fn folder_count_label(
    profile: &family_profile::InlineStatsProfile,
    kind: &NavNodeKind,
    total: Option<usize>,
    loaded: usize,
) -> Option<String> {
    if !profile.show_child_count {
        return None;
    }
    if !matches!(kind, NavNodeKind::Folder(_)) {
        return None;
    }
    let count = total.unwrap_or(loaded);
    (count > 0).then(|| count.to_string())
}

/// 「加载更多」行文案（纯函数；DBeaver `visible/total` 口径，D:13）。
///
/// - `fetching`：本轮点过「加载更多」、后台在途 → 取数中文案，禁用其余计数口径；
/// - `filtering`：本地筛选词非空且分页未拉完 → 追加「筛选仅覆盖已加载」，避免用户误以为
///   “搜不到 = 库里没有”；
/// - 常规：`可见 {loaded} / 共 {total}`。
pub(super) fn more_row_label(loaded: usize, total: usize, filtering: bool, fetching: bool) -> String {
    if fetching {
        return "加载中…".to_string();
    }
    let base = format!("可见 {loaded} / 共 {total}");
    if filtering {
        format!("{base}；筛选仅覆盖已加载")
    } else {
        base
    }
}

/// D8 合并窗口内六类结果的回填顺序（与泵现状顺序一致）：
/// load → sql → test → row_estimate → connect → search。
/// core 拆分后合并窗口内按此顺序调用；单测断言顺序表，调乱顺序会被钉死。
pub(super) const PUMP_BATCH_ORDER: [&str; 6] = [
    "load",
    "sql",
    "test",
    "row_estimate",
    "connect",
    "search",
];

/// D8 合并窗口判定：本轮六类结果任一非空即需要一次状态写入 + 重绘；
/// 全空则跳过本轮 `weak.update`，直接进入 idle 判定。
pub(super) fn batches_present(
    load: &[nav_jobs::LoadResult],
    sql: &[nav_jobs::SqlGenResult],
    test: &[nav_jobs::TestConnResult],
    row_estimates: &[nav_jobs::RowEstimateResult],
    connect: &[nav_jobs::ConnectResult],
    search: &[nav_jobs::SearchResult],
) -> bool {
    !load.is_empty()
        || !sql.is_empty()
        || !test.is_empty()
        || !row_estimates.is_empty()
        || !connect.is_empty()
        || !search.is_empty()
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
        // GitHub 连接（仓库源 / GitHub 联邦数据）：Lucide 官方 github 图标 + GH。
        "github" => ("icons/github.svg", "GH"),
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
    // M3：五个叶子对象的图标 key 来自 L1 族档案；连接根 / Schema / 文件夹开合 / 列 /
    // 列溢出提示属展示特例，不属于数据库对象身份，由下方特例分支兜底。
    let object_kind = match kind {
        NavNodeKind::Table { .. } => Some(ObjectKind::Table),
        NavNodeKind::View => Some(ObjectKind::View),
        NavNodeKind::Routine { .. } => Some(ObjectKind::Routine),
        NavNodeKind::Sequence => Some(ObjectKind::Sequence),
        NavNodeKind::Trigger => Some(ObjectKind::Trigger),
        _ => None,
    };
    if let Some(icon) = object_kind.and_then(family_profile::object_kind_icon) {
        return icon;
    }
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
        NavNodeKind::Column { .. } => "icons/columns-3.svg",
        NavNodeKind::ColumnOverflow => "icons/info.svg",
        // 档案已覆盖对象类别不会落到这里；`_` 仅满足 match 穷尽。
        _ => "icons/info.svg",
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
///
/// **2026-10-27（表格无数据根因）**：执行类 SQL 的限定名对 `catalog_is_connection_scoped`
/// 的族（PostgreSQL）跳过 catalog 段（catalog = 当前连接 database，拼进 SQL 即跨库三段式）。
/// 经 [`crate::sql_gen::sql_qualified_name`] 按驱动分派。
pub(super) fn nav_qualified_name(prop: &PropertyRef, driver: &str) -> String {
    crate::sql_gen::sql_qualified_name(
        driver,
        prop.catalog.as_deref(),
        prop.schema.as_deref(),
        &prop.name,
    )
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

/// 容器成员顺序：手动排序过的按存储序号在前，**未排过的按「最近使用 → 名称」在后**。
///
/// `stored` 来自引擎的「成员 + 是否手动排序过」列表。显式段在这里再排一次序号
/// （不依赖引擎的返回顺序）；未排段里：`recent`（新→旧）中的成员按最近顺序在前，
/// 其余按名称——「手动排序优先」原则不变（用户显式排的序不被最近使用覆盖）。
/// 排序均稳定，同键保持传入顺序。纯函数，便于单测。
pub(super) fn nav_order_members(
    stored: &[(String, Option<i64>)],
    name_of: impl Fn(&str) -> String,
    recent: &[String],
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
    // 未排段分区：最近使用过的（保持 `recent` 的新→旧顺序）与非最近（按名称）。
    let recent_set: std::collections::HashSet<&str> =
        recent.iter().map(|s| s.as_str()).collect();
    let recent_used: Vec<String> = recent
        .iter()
        .filter(|id| unset.iter().any(|u| u == *id))
        .cloned()
        .collect();
    let mut others: Vec<String> = unset
        .into_iter()
        .filter(|id| !recent_set.contains(id.as_str()))
        .collect();
    // 名称大小写不敏感；同名再按 ID 定序，保证结果稳定。
    others.sort_by(|a, b| {
        name_of(a)
            .to_lowercase()
            .cmp(&name_of(b).to_lowercase())
            .then_with(|| a.cmp(b))
    });
    let mut out: Vec<String> = ordered.into_iter().map(|(_, id)| id).collect();
    out.extend(recent_used);
    out.extend(others);
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
    // M5：ObjectKind → 展示节点形状，查表自 L1 族档案；无引用回退 Schema 形状。
    nav_search_hit_ref(hit)
        .map(|r| family_profile::search_hit_node_kind(r.kind))
        .unwrap_or(NavNodeKind::Schema)
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

/// 命中维度徽标的语义角色：注释 / 定义两类（名称命中是默认通道，不标注）。
/// 颜色在渲染侧按角色取主题 token，纯函数不碰 theme。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavHitBadgeRole {
    Comment,
    Definition,
}

/// 命中维度 → 徽标文案与角色：名称命中不额外标注；注释 / 定义命中给「注释」「定义」
/// 小标签，让用户一眼看出结果是按哪个维度命中的（对齐 DBeaver 搜注释 / 搜定义口径）。
pub(super) fn nav_hit_source_badge(
    source: nav_jobs::HitSource,
) -> Option<(&'static str, NavHitBadgeRole)> {
    match source {
        nav_jobs::HitSource::Name => None,
        nav_jobs::HitSource::Comment => Some(("注释", NavHitBadgeRole::Comment)),
        nav_jobs::HitSource::Definition => Some(("定义", NavHitBadgeRole::Definition)),
    }
}

/// 搜索词是否够格打一次索引搜索（太短时命中面过大，且首字符几乎必然还要改）。
pub(super) fn nav_search_query_ready(query: &str) -> bool {
    query.trim().chars().count() >= 2
}

/// 树内空态占位判定（纯函数；在途加载中 / 已有可见行时不摆占位）。
///
/// 参数口径：
/// - `filter_active`：本地筛选词非空；
/// - `search_active`：索引搜索词非空；
/// - `search_done`：一批索引搜索已回填（`search_searched > 0`；进行中不摆空态）；
/// - `hits_empty`：索引命中为空；
/// - `tree_rows_rendered`：当前作用域内已有树行（连接壳 / 分组内对象行；筛选空态用）；
/// - `object_rows_rendered`：已有对象行（表 / 视图等叶子与容器；搜索空态用——搜索模式下
///   连接壳不算「对象」，见 `.rds/tmp/b5-design.md` §4.5）；
/// - `loading`：当前作用域仍在后台取数。
pub(super) fn placeholder_decision(
    filter_active: bool,
    search_active: bool,
    search_done: bool,
    hits_empty: bool,
    tree_rows_rendered: bool,
    object_rows_rendered: bool,
    loading: bool,
) -> Option<PlaceholderKind> {
    if loading {
        return None;
    }
    let search_missing = search_active && search_done && hits_empty;
    match (filter_active, search_missing) {
        (true, false) => (!tree_rows_rendered).then_some(PlaceholderKind::FilterEmpty),
        (false, true) => (!object_rows_rendered).then_some(PlaceholderKind::SearchEmpty),
        (true, true) => (!object_rows_rendered).then_some(PlaceholderKind::FilterSearchEmpty),
        (false, false) => None,
    }
}

/// 占位行文案（索引空态带上原词；其余两类为固定说明）。
pub(super) fn placeholder_message(kind: PlaceholderKind, query: &str) -> String {
    match kind {
        PlaceholderKind::FilterEmpty => "当前筛选无匹配对象".to_string(),
        PlaceholderKind::SearchEmpty => format!("索引中未找到「{query}」"),
        PlaceholderKind::FilterSearchEmpty => "筛选与索引搜索均无匹配对象".to_string(),
    }
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

/// 高亮分段（纯函数，可单测）：把 `name` 中与 `filter`（已小写）匹配的每一段
/// 标成 `(文本, 是否命中)`。渲染层拿它摆 span，测试层断言分段——不断言 Div 结构。
///
/// 非 ASCII（如 CJK）大小写转换不改变字节长度，故按字节切片安全；
/// 仅当两端都是 char 边界时才切，否则跳过该段（循环里也要推进游标防死循环）。
pub(super) fn nav_highlight_segments(name: &str, filter: &str) -> Vec<(String, bool)> {
    if filter.is_empty() {
        return vec![(name.to_string(), false)];
    }
    let lower = name.to_lowercase();
    if lower.len() != name.len() {
        return vec![(name.to_string(), false)];
    }
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    let mut lower_cursor = 0usize;
    while let Some(rel) = lower[lower_cursor..].find(filter) {
        let start = lower_cursor + rel;
        let end = start + filter.len();
        if name.is_char_boundary(start) && name.is_char_boundary(end) {
            if start > cursor {
                segments.push((name[cursor..start].to_string(), false));
            }
            segments.push((name[start..end].to_string(), true));
            cursor = end;
        }
        // 无论本段是否可切都推进：命中段已消费，避免同一段无限循环。
        lower_cursor = end;
    }
    if cursor < name.len() {
        segments.push((name[cursor..].to_string(), false));
    }
    if segments.is_empty() {
        return vec![(name.to_string(), false)];
    }
    segments
}

/// B-1 对象名 glob 匹配（v1 仅 `*` / `?`，大小写不敏感）。
///
/// 递归实现：`*` 匹配任意多字符（含空）、`?` 匹配单字符、其余字面全串匹配。
/// 与 `content_search::glob_matches`（路径语义、大小写敏感）只是递归结构同源，
/// 不跨 crate 调用；对象名场景没有路径分隔符，`?` 不做分隔排除。
pub(super) fn nav_glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    fn rec(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => {
                // 贪心不必要：`*` 之后的内容尝试在任意截断点续配，任一成功即整体匹配。
                let rest = &p[1..];
                (0..=n.len()).any(|take| rec(rest, &n[take..]))
            }
            Some('?') => !n.is_empty() && rec(&p[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && rec(&p[1..], &n[1..]),
        }
    }
    rec(&p, &n)
}

/// 规则编译产物：include / exclude 两路分开承载，`use_regex` 决定匹配分支。
///
/// - 正则模式：编译成功的模式进 `*_re`；**编译失败的条目不进向量，视为永不命中**
///   （不 panic、不使整规则失效，单行坏正则只废掉它自己）。
/// - glob 模式：原始模式串进 `*_glob`，匹配期逐条走 [`nav_glob_match`]。
/// - `include_is_empty` 记录**原始**规则列表是否为空：不能用编译后的正则向量判定——
///   编译失败会丢弃条目，若拿「正则向量非空」当「规则非空」，一条全坏的 include 会被
///   误判为空规则而放行全部对象。
pub(super) struct CompiledMask {
    use_regex: bool,
    include_is_empty: bool,
    include_re: Vec<Regex>,
    exclude_re: Vec<Regex>,
    include_glob: Vec<String>,
    exclude_glob: Vec<String>,
}

/// 编译一条 mask 规则（纯函数、无副作用、不缓存）。
///
/// 正则逐条 `Regex::new("(?i){pattern}")`：`(?i)` 大小写不敏感，与 glob 现状口径一致；
/// 编译失败的条目丢弃，匹配期等价于「该条永不命中」。glob 模式原样留存模式串。
pub(super) fn compile_mask(rule: &ObjectMaskRule) -> CompiledMask {
    if rule.use_regex {
        let compile = |patterns: &[String]| {
            patterns
                .iter()
                .filter_map(|p| Regex::new(&format!("(?i){p}")).ok())
                .collect()
        };
        CompiledMask {
            use_regex: true,
            include_is_empty: rule.include.is_empty(),
            include_re: compile(&rule.include),
            exclude_re: compile(&rule.exclude),
            include_glob: Vec::new(),
            exclude_glob: Vec::new(),
        }
    } else {
        CompiledMask {
            use_regex: false,
            include_is_empty: rule.include.is_empty(),
            include_re: Vec::new(),
            exclude_re: Vec::new(),
            include_glob: rule.include.clone(),
            exclude_glob: rule.exclude.clone(),
        }
    }
}

/// 编译产物判定：exclude 任一命中 → 隐藏；include 原始为空 → 全量候选；
/// include 任一命中 → 保留。**exclude 优先于 include**。
pub(super) fn mask_passes(compiled: &CompiledMask, name: &str) -> bool {
    let excluded = if compiled.use_regex {
        compiled.exclude_re.iter().any(|r| r.is_match(name))
    } else {
        compiled.exclude_glob.iter().any(|p| nav_glob_match(p, name))
    };
    if excluded {
        return false;
    }
    if compiled.include_is_empty {
        return true;
    }
    if compiled.use_regex {
        compiled.include_re.iter().any(|r| r.is_match(name))
    } else {
        compiled.include_glob.iter().any(|p| nav_glob_match(p, name))
    }
}

/// B-1 单条规则判定：先 [`compile_mask`] 再委托 [`mask_passes`]，保留既有签名供调用点不动。
///
/// 规则集合在规则变更（`set_object_mask` / `set_global_mask` 事件路径）时才会变；
/// 渲染期每次调用重编译代价可接受，暂不在视图状态内缓存编译产物（若 profile 显示高频
/// 编译开销，再按规则快照缓存，见设计备忘 R6）。
pub(super) fn nav_mask_passes(rule: &ObjectMaskRule, name: &str) -> bool {
    mask_passes(&compile_mask(rule), name)
}

/// B-1 组合判定（per-conn + 全局，AND）：任一**启用**的规则不通过 = 隐藏。
///
/// 规则不启用（空）不参与判断；只在**对象层**调用（catalog / schema / 列不进 mask）。
pub(super) fn nav_object_masked(
    per_conn: Option<&ObjectMaskRule>,
    global: Option<&ObjectMaskRule>,
    name: &str,
) -> bool {
    if let Some(r) = per_conn {
        if r.is_active() && !nav_mask_passes(r, name) {
            return true;
        }
    }
    if let Some(r) = global {
        if r.is_active() && !nav_mask_passes(r, name) {
            return true;
        }
    }
    false
}

/// B-1 该节点类型是否参与对象名 mask（只对象层；列 / 结构层不进 mask）。
pub(super) fn nav_object_kind_masked(kind: &crate::model::NavNodeKind) -> bool {
    matches!(
        kind,
        crate::model::NavNodeKind::Table { .. }
            | crate::model::NavNodeKind::View
            | crate::model::NavNodeKind::Routine { .. }
            | crate::model::NavNodeKind::Sequence
            | crate::model::NavNodeKind::Trigger
    )
}

/// 搜索命中高亮：把 `name` 中与 `filter`（已小写）匹配的**每一段**用命中底色标出。
///
/// `strike` = true（B6 待提交删除态）时，父段与各分段统一挂删除线：删除线是父元素
/// text_style 位、不会穿透到内层分段 Div（与文字色继承不是同一通道），故分段显式补挂，
/// 避免命中段漏掉删除线（风险 R2 落码闭环，不靠首帧视觉核对）。
pub(super) fn nav_name_highlight(
    name: &str,
    filter: &str,
    match_bg: Hsla,
    fg: Hsla,
    strike: bool,
) -> Div {
    let segments = nav_highlight_segments(name, filter);
    let has_hit = segments.iter().any(|(_, hit)| *hit);
    if !has_hit {
        return div()
            .min_w_0()
            .overflow_hidden()
            .text_ellipsis()
            .when(strike, |s| s.line_through())
            .text_color(fg)
            .child(name.to_string());
    }
    div()
        .h_flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .when(strike, |s| s.line_through())
        .text_color(fg)
        .children(segments.into_iter().map(|(text, hit)| {
            let segment = div().when(strike, |s| s.line_through());
            if hit {
                segment.rounded_sm().bg(match_bg).child(text)
            } else {
                segment.child(text)
            }
        }))
}

/// B6 待提交破坏性动作的菜单项文案（纯函数，供菜单装配与单测钉死）。
///
/// - 已在待提交清单（`staged_hit`）→ 「从待提交移除」（单项撤销，不弹确认）；
/// - 未入队：按动作 × 对象类别给「删除表…/删除视图…/删除序列…/清空表…」；
/// - 动作与类别不匹配（如 Truncate 挂到视图 / 序列）→ `None`，菜单不渲染该项。
pub(super) fn staged_menu_label(op: NavOp, kind: ObjectKind, staged_hit: bool) -> Option<&'static str> {
    if staged_hit {
        return Some("从待提交移除");
    }
    match (op, kind) {
        (NavOp::Drop, ObjectKind::Table) => Some("删除表…"),
        (NavOp::Drop, ObjectKind::View) => Some("删除视图…"),
        (NavOp::Drop, ObjectKind::Sequence) => Some("删除序列…"),
        (NavOp::Truncate, ObjectKind::Table) => Some("清空表…"),
        _ => None,
    }
}

/// B6 单项确认对话框文案（纯函数）：返回 `(标题, 描述, OK 按钮文案)`。
///
/// 描述内限定名用展示形态（`nav_qualified_name`，无引号转义）；执行 DDL 的引号转义
/// 由 L3 方言 provider 在提交执行时完成（§4.4）。动作 × 类别不匹配时返回 `None`。
pub(super) fn staged_dialog_copy(
    op: NavOp,
    kind: ObjectKind,
    qualified: &str,
) -> Option<(&'static str, String, &'static str)> {
    let ok_text = "确认加入待提交";
    let (title, description) = match (op, kind) {
        (NavOp::Drop, ObjectKind::Table) => (
            "删除表",
            format!("确定删除表 `{qualified}`？表及其数据将被永久删除，提交后不可恢复。"),
        ),
        (NavOp::Truncate, ObjectKind::Table) => (
            "清空表",
            format!("确定清空表 `{qualified}`？表内全部数据将被删除，提交后不可恢复。"),
        ),
        (NavOp::Drop, ObjectKind::View) => (
            "删除视图",
            format!("确定删除视图 `{qualified}`？提交后不可恢复。"),
        ),
        (NavOp::Drop, ObjectKind::Sequence) => (
            "删除序列",
            format!("确定删除序列 `{qualified}`？提交后不可恢复。"),
        ),
        _ => return None,
    };
    Some((title, description, ok_text))
}

/// B6 汇总确认清单 / 失败提示里的**动作标签**（纯函数，无菜单省略号「…」）。
///
/// 与 [`staged_menu_label`] 的区别：菜单文案带 `…` 表示会弹对话框；清单行只陈述要执行的
/// 动作，故用「删除表 / 清空表 / 删除视图 / 删除序列」。动作 × 类别不匹配时返回 `None`
/// （注册表 `applies` 同源，正常路径不会出现）。
pub(super) fn staged_action_label(op: NavOp, kind: ObjectKind) -> Option<&'static str> {
    match (op, kind) {
        (NavOp::Drop, ObjectKind::Table) => Some("删除表"),
        (NavOp::Drop, ObjectKind::View) => Some("删除视图"),
        (NavOp::Drop, ObjectKind::Sequence) => Some("删除序列"),
        (NavOp::Truncate, ObjectKind::Table) => Some("清空表"),
        _ => None,
    }
}

/// 待提交条目的展示限定名（`catalog.schema.name`，无引号转义；执行 DDL 的引号转义由
/// L3 方言 provider 完成）。与 [`nav_qualified_name`] 同口径，入参是自存身份字段的条目。
pub(super) fn staged_qualified_name(op: &StagedOp) -> String {
    crate::sql_gen::qualified_name(op.catalog.as_deref(), op.schema.as_deref(), &op.object_name)
}

/// 批量提交汇总提示文案（纯函数；成功 / 失败计数 + 失败明细，超 5 条折叠）。
///
/// - 全成功：单行「已提交 X 项变更」；
/// - 有失败：首行「成功 X 项、失败 Y 项」，随后逐条 `· 限定名：错误`；失败 > 5 条只列前 5，
///   末行折叠为「…等 Y 项失败，详见日志」（§4.8 D8：失败明细进汇总 notice，v1 不做行级失败气泡）。
pub(super) fn staged_commit_summary(ok: usize, failed: &[(String, String)]) -> String {
    let failed_total = failed.len();
    if failed_total == 0 {
        return format!("已提交 {ok} 项变更");
    }
    let mut lines = vec![format!("成功 {ok} 项、失败 {failed_total} 项")];
    const DETAIL_LIMIT: usize = 5;
    let shown = failed.iter().take(DETAIL_LIMIT);
    for (qualified, error) in shown {
        lines.push(format!("· {qualified}：{error}"));
    }
    if failed_total > DETAIL_LIMIT {
        lines.push(format!("…等 {failed_total} 项失败，详见日志"));
    }
    lines.join("\n")
}

/// 汇总确认对话框清单行（纯函数；动作标签弱色固定宽列 + 限定名省略号截断）。
///
/// 面板头「提交 N」按钮与 `Ctrl+S` 键盘入口共用同一份行构造，避免两处各写一遍布局；
/// 标签由 [`staged_action_label`] 兜底 `None`（注册表 `applies` 同源，正常路径不出现）。
pub(super) fn staged_commit_rows(plan: &[StagedOp], muted: Hsla) -> Vec<AnyElement> {
    plan.iter()
        .filter_map(|entry| {
            staged_action_label(entry.op, entry.object_kind).map(|label| {
                div()
                    .h_flex()
                    .gap_2()
                    .text_xs()
                    .items_start()
                    .child(
                        div()
                            .w_12()
                            .flex_none()
                            .text_color(muted)
                            .child(label),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .child(staged_qualified_name(entry)),
                    )
                    .into_any_element()
            })
        })
        .collect()
}

/// 内容档 snippet 的分段（纯函数，可单测）：解析 `<mark>…</mark>` 标记，
/// 返回 `(文本, 是否命中)` 序列。无标记 / 标记残缺时整体为未命中段。
pub(super) fn nav_snippet_segments(snippet: &str) -> Vec<(String, bool)> {
    let mut segments = Vec::new();
    let mut rest = snippet;
    while let Some(start) = rest.find("<mark>") {
        let body_start = start + "<mark>".len();
        let Some(rel_end) = rest[body_start..].find("</mark>") else {
            break;
        };
        let body_end = body_start + rel_end;
        if start > 0 {
            segments.push((rest[..start].to_string(), false));
        }
        segments.push((rest[body_start..body_end].to_string(), true));
        rest = &rest[body_end + "</mark>".len()..];
    }
    if !rest.is_empty() {
        segments.push((rest.to_string(), false));
    }
    if segments.is_empty() {
        return vec![(snippet.to_string(), false)];
    }
    segments
}

/// 内容档（`#` 前缀）命中片段高亮：把 FTS 的 `<mark>…</mark>` 标记解析成命中底色，
/// 其余文本 muted——用户看到「为什么命中」时，命中词必须一眼可辨。
///
/// 无标记（普通片段 / 异常残缺）时整体按 muted 文本渲染，不 panic。
pub(super) fn nav_snippet_highlight(snippet: &str, match_bg: Hsla, muted: Hsla) -> Div {
    let segments = nav_snippet_segments(snippet);
    if !segments.iter().any(|(_, hit)| *hit) {
        return div().text_color(muted).child(snippet.to_string());
    }
    div()
        .h_flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .text_ellipsis()
        .children(segments.into_iter().map(|(text, hit)| {
            if hit {
                div().rounded_sm().bg(match_bg).child(text)
            } else {
                div().text_color(muted).child(text)
            }
        }))
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

/// 失败行（错误文案 + 尾部「重试」）。
///
/// 为什么左段截断、右段固定：错误文案长度不可控（PG 报错带列名与建议），
/// 而「重试」必须**钉在固定位置**才点得到。整行都可点是这条行的本意，
/// 末尾那两个字只是把它说出来——所以高度仍与普通附加行一致（`ui::NAV_SUBLINE`）。
pub(super) fn nav_error_row_at(text: &str, color: Hsla, action: Hsla, indent: f32) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .w_full()
        .h(rems(ui::NAV_SUBLINE))
        .pl(rems(indent))
        .pr_1()
        .rounded_md()
        .text_xs()
        .overflow_hidden()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_ellipsis()
                .text_color(color)
                .child(text.to_string()),
        )
        .child(div().flex_none().text_color(action).child("重试"))
}

/// 这次展开是不是「重试」：该节点上次失败，且手里还没有它的子节点。
///
/// 为何要这条判据：`attempted` 是「已请求过」的拦截标记，只看它的话，
/// 一个失败的节点**收起再展开也不会重试**（红字像焊死在树上），用户只剩右键「刷新元数据」。
/// 为何只认「手里没有子节点」：已经有子节点的那种失败是**翻页**失败（「加载更多」），
/// 它的重试在那一行自己身上，不该顺手把已加载的前缀换掉。
///
/// 只由**用户动作**（展开 / 点红字）消费：渲染路径（`collect_node_rows`）不查它，
/// 否则一个失败节点会被每帧重排一次加载。
pub(super) fn nav_load_is_retry(view: &NavViewState, key: &str) -> bool {
    view.errors.contains_key(key) && !view.children.contains_key(key)
}

/// 「重试」前的清理：抹掉该节点的失败与请求痕迹，让它回到「没请求过」的状态。
pub(super) fn nav_forget_attempt(view: &mut NavViewState, key: &str) {
    view.errors.remove(key);
    view.attempted.remove(key);
    view.loading.remove(key);
}

/// 判断 key 是否落在被刷新子树内：等于根 key，或位于根 key 的 `root/` 前缀之下。
pub(super) fn key_in_subtree(key: &str, root: &str, prefix: &str) -> bool {
    key == root || key.starts_with(prefix)
}

/// 刷新某节点子树：回收集合中落在该子树范围内的 key，保留其余部分。
/// 供 `refresh_node` 清 `loading` 等 `HashSet` 簿记复用（与 children/attempted 同口径 retain）。
pub(super) fn prune_subtree_keys(
    set: &HashSet<String>,
    root: &str,
    prefix: &str,
) -> HashSet<String> {
    set.iter()
        .filter(|k| !key_in_subtree(k.as_str(), root, prefix))
        .cloned()
        .collect()
}

/// 当前选中锚点是否落在被刷新子树内；是则刷新后清空，避免指向已被清掉子树的锚点残留。
pub(super) fn selected_key_in_subtree(selected: Option<&str>, root: &str, prefix: &str) -> bool {
    selected.is_some_and(|k| key_in_subtree(k, root, prefix))
}

/// 首屏整体替换后回收 `expanded` 残留：保留白名单 = 现存容器/节点 key 全集 ∪ 连接根 key，
/// 剔除新回填树里已不存在的旧节点 key——避免状态集长期膨胀、恢复展开态时对不存在 key 空排队。
/// 父链容器 key 已在现存 key 全集内（自身既是上层子节点、又是下层容器），不会被误清。
pub(super) fn prune_expanded(
    expanded: &HashSet<String>,
    existing_keys: &HashSet<String>,
    root_keys: &HashSet<String>,
) -> HashSet<String> {
    expanded
        .iter()
        .filter(|k| existing_keys.contains(k.as_str()) || root_keys.contains(k.as_str()))
        .cloned()
        .collect()
    }

/// 加载失败是否为「连接类」错误（可自动重试一次）。
///
/// 背景（app_run57/58 铁证）：池连接被网络层静默丢弃后，`test_before_acquire`
/// 验证/查询挂在 TCP 半开上超时——这类失败重入队后 sqlx 已把死连接回收并新建，
/// 第二次秒过；权限/对象不存在等业务失败重试无意义，交给红字 + 用户重试。
pub(super) fn nav_error_is_connection(result: &Result<crate::navigator_service::NavPage, String>) -> bool {
    let Err(err) = result else {
        return false;
    };
    [
        "pool timed out",
        "connection closed",
        "connection refused",
        "server closed",
        "PoolClosed",
        "acquire",
        "not connected",
        "连接已断开",
        "连接被关闭",
    ]
    .iter()
    .any(|k| err.contains(k))
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
        | NavRow::SearchHit { .. }
        | NavRow::Placeholder { .. } => ui::NAV_ROW_TREE,
    };
    let mut height = tree::RowHeight::new(base);
    match row {
        NavRow::Connection { conn, .. } => {
            let loading = view.loading.contains(&conn.id);
            let error = view.errors.contains_key(&conn.id);
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
                .add_if(error, ui::NAV_SUBLINE);
        }
        NavRow::Tree { node, .. } => {
            height = height
                .add_if(view.loading.contains(&node.key), ui::NAV_SUBLINE)
                .add_if(view.errors.contains_key(&node.key), ui::NAV_SUBLINE);
        }
        // 内容档（`#` 前缀）命中带 snippet：附加一行「为什么命中」。
        NavRow::SearchHit { hit, .. } => {
            height = height.add_if(hit.snippet.is_some(), ui::NAV_SUBLINE);
        }
        _ => {}
    }
    height
}

/// 标签名 → 色档下标（纯函数，可单测；哈希取模保证同名稳定、异名分散）。
pub(super) fn nav_tag_palette_index(name: &str, len: usize) -> usize {
    let mut h = 0u64;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(u64::from(b));
    }
    (h as usize) % len
}

/// 标签 chip 色点：按名称稳定映射到主题语义色（规格 §14 color→TagVariant 映射，
/// 第 5 步导航接入统一做；不直渲 hex——标签颜色统一走语义 token，避免裸色值）。
pub(super) fn nav_tag_color(name: &str, theme: &gpui_kit::component::Theme) -> Hsla {
    let palette = [
        theme.colors.info,
        theme.colors.success,
        theme.colors.warning,
        theme.colors.primary,
        theme.colors.muted_foreground,
        theme.colors.info,
        theme.colors.success,
        theme.colors.warning,
    ];
    palette[nav_tag_palette_index(name, palette.len())]
}

#[cfg(test)]
mod tests {
    // 不 `use super::*`：通配会把 gpui 的 `test` 宏带进作用域（gpui-kit-dev §窗口测试）。
    use std::collections::{HashMap, HashSet};

    use super::{
        batches_present, compile_mask, comment_subline_text, folder_count_label, format_data_length,
        format_row_estimate, host_subline_text, key_in_subtree, mask_passes, more_row_label,
        nav_glob_match, nav_highlight_segments, nav_hit_source_badge, nav_mask_passes, nav_node_matches,
        nav_object_masked, nav_snippet_segments, nav_tag_palette_index, placeholder_decision,
        placeholder_message, prune_expanded, prune_subtree_keys, selected_key_in_subtree,
        staged_action_label, staged_commit_summary, staged_dialog_copy, staged_menu_label,
        staged_qualified_name, size_badge_text, NavHitBadgeRole, PUMP_BATCH_ORDER, StagedOp,
    };
    use crate::model::{NavFolder, NavNode, NavNodeKind, NavPath, ObjectKind};
    use crate::nav::capability::NavOp;
    use crate::nav::family_profile::InlineStatsProfile;
    use crate::nav_host::ObjectMaskRule;
    use crate::nav_rows::PlaceholderKind;
    use crate::navigator_service::NavPage;
    use crate::nav_jobs;

    /// 同名同档（跨调用稳定）；不同名分布在不同档（至少不完全扎堆）。
    #[test]
    fn tag_palette_is_stable_and_spread() {
        assert_eq!(nav_tag_palette_index("报表", 8), nav_tag_palette_index("报表", 8));
        let names = ["报表", "核心", "待整理", "月报", "prod", "archive"];
        let mut seen = std::collections::HashSet::new();
        for n in names {
            seen.insert(nav_tag_palette_index(n, 8));
        }
        assert!(seen.len() >= 2, "不同标签应落在不同色档（至少两档）");
    }

    /// 单段命中：整词中缀只标一段，前后文本未命中。
    #[test]
    fn highlight_single_hit_segment() {
        let segs = nav_highlight_segments("order_items", "order");
        assert_eq!(
            segs,
            vec![
                ("order".to_string(), true),
                ("_items".to_string(), false),
            ]
        );
    }

    /// 多段命中：每一处都标（不只第一段——旧实现只高亮第一段）。
    #[test]
    fn highlight_all_matching_segments() {
        let segs = nav_highlight_segments("orders_by_orders", "orders");
        assert_eq!(
            segs,
            vec![
                ("orders".to_string(), true),
                ("_by_".to_string(), false),
                ("orders".to_string(), true),
            ]
        );
    }

    /// 空过滤器 = 整段未命中（渲染回退为普通文本）。
    #[test]
    fn highlight_empty_filter_means_no_hit() {
        let segs = nav_highlight_segments("order_items", "");
        assert_eq!(segs, vec![("order_items".to_string(), false)]);
    }

    /// 大小写不敏感：`ORDER` 命中 `order`（LIKE 超集口径一致）。
    #[test]
    fn highlight_is_case_insensitive() {
        let segs = nav_highlight_segments("ORDER_ITEMS", "order");
        assert_eq!(
            segs,
            vec![
                ("ORDER".to_string(), true),
                ("_ITEMS".to_string(), false),
            ]
        );
    }

    /// snippet 解析：`<mark>` 命中段、前后文本未命中；无标记整体未命中。
    #[test]
    fn snippet_marks_become_hit_segments() {
        let segs = nav_snippet_segments("…下单<mark>渠道</mark>…");
        assert_eq!(
            segs,
            vec![
                ("…下单".to_string(), false),
                ("渠道".to_string(), true),
                ("…".to_string(), false),
            ]
        );
        assert_eq!(
            nav_snippet_segments("无标记片段"),
            vec![("无标记片段".to_string(), false)]
        );
    }

    /// 多个 mark：逐段解析，不丢不叠。
    #[test]
    fn snippet_multiple_marks() {
        let segs = nav_snippet_segments("<mark>a</mark>x<mark>b</mark>");
        assert_eq!(
            segs,
            vec![
                ("a".to_string(), true),
                ("x".to_string(), false),
                ("b".to_string(), true),
            ]
        );
    }

    /// 搜索过滤命中：节点名中缀命中，或已加载子节点中任一命中。
    /// 第三个用例锁定「祖先链保留」——搜索态自动展开的树形态下，
    /// 父节点名不匹配但子命中时父必须显示（否则用户看不到命中的位置）。
    #[test]
    fn node_matches_self_or_any_loaded_child() {
        let mut children: HashMap<String, Vec<NavNode>> = HashMap::new();
        let schema = NavNode::new(
            "conn::sales",
            "sales",
            "conn",
            NavNodeKind::Schema,
            true,
        );
        let table = NavNode::new(
            "conn::sales::orders",
            "orders",
            "conn",
            NavNodeKind::Table { row_estimate: None },
            false,
        );
        children.insert("conn::sales".to_string(), vec![table]);

        // 节点名中缀命中（filter 由调用方预先小写，函数内直接 contains）。
        assert!(nav_node_matches(&children, &schema, "sales"));
        assert!(nav_node_matches(&children, &schema, "sale"));
        // 子节点命中 → 父保留（祖先链）。
        assert!(nav_node_matches(&children, &schema, "orders"));
        // 都不匹配 → 隐藏。
        assert!(!nav_node_matches(&children, &schema, "customers"));
    }

    /// S3-④ 行估计格式：千分位 / M / B 三档 + 0。
    #[test]
    fn row_estimate_formatting() {
        assert_eq!(format_row_estimate(0), "0");
        assert_eq!(format_row_estimate(9), "9");
        assert_eq!(format_row_estimate(999), "999");
        assert_eq!(format_row_estimate(1_000), "1,000");
        assert_eq!(format_row_estimate(12_345), "12,345");
        assert_eq!(format_row_estimate(999_999), "999,999");
        assert_eq!(format_row_estimate(1_234_567), "1.2M");
        assert_eq!(format_row_estimate(3_400_000), "3.4M");
        assert_eq!(format_row_estimate(1_000_000), "1.0M");
        assert_eq!(format_row_estimate(1_200_000_000), "1.2B");
        assert_eq!(format_row_estimate(1_000_000_000), "1.0B");
    }

    /// D12 大小格式：B / KB / MB / GB 二进制 1024 档位 + 0 如实显 B。
    #[test]
    fn data_length_formatting() {
        assert_eq!(format_data_length(0), "0 B");
        assert_eq!(format_data_length(512), "512 B");
        assert_eq!(format_data_length(1_023), "1023 B");
        assert_eq!(format_data_length(1_024), "1.0 KB");
        assert_eq!(format_data_length(1_536), "1.5 KB");
        assert_eq!(format_data_length(1_048_576), "1.0 MB");
        assert_eq!(format_data_length(1_572_864), "1.5 MB");
        assert_eq!(format_data_length(1_073_741_824), "1.0 GB");
        assert_eq!(format_data_length(1_610_612_736), "1.5 GB");
    }

    // ---- D12 行内统计元素门控纯函数 ----

    /// 全开档案位测试夹具。
    fn inline_all_on() -> InlineStatsProfile {
        InlineStatsProfile {
            show_description: true,
            show_size: true,
            show_host_name: true,
            show_child_count: true,
        }
    }

    #[test]
    fn host_subline_gated_by_profile_and_host_presence() {
        let on = inline_all_on();
        let off = InlineStatsProfile { show_host_name: false, ..on };

        // 档案关闭：即便有 host 也不显。
        assert_eq!(host_subline_text(&off, Some("db.local"), Some(5432)), None);
        // host 缺失 / 空白串：不显。
        assert_eq!(host_subline_text(&on, None, Some(5432)), None);
        assert_eq!(host_subline_text(&on, Some("  "), Some(5432)), None);
        // port 非空：拼 `host:port`。
        assert_eq!(
            host_subline_text(&on, Some("db.local"), Some(5432)).as_deref(),
            Some("db.local:5432")
        );
        // port 缺失：只显 host；host 两侧空白被修剪。
        assert_eq!(host_subline_text(&on, Some("db.local"), None).as_deref(), Some("db.local"));
        assert_eq!(
            host_subline_text(&on, Some("  db.local  "), None).as_deref(),
            Some("db.local")
        );
    }

    #[test]
    fn comment_subline_gated_by_profile_kind_and_comment() {
        let on = inline_all_on();
        let off = InlineStatsProfile { show_description: false, ..on };

        // 档案关闭：描述类对象也不显。
        assert_eq!(comment_subline_text(&off, &NavNodeKind::View, Some("备注")), None);

        // 描述类白名单：表 / 视图 / 例程 / 序列 / 触发器显。
        let visible = [
            NavNodeKind::Table { row_estimate: None },
            NavNodeKind::View,
            NavNodeKind::Routine { routine_type: "function".into() },
            NavNodeKind::Sequence,
            NavNodeKind::Trigger,
        ];
        for kind in &visible {
            assert_eq!(
                comment_subline_text(&on, kind, Some("备注")).as_deref(),
                Some("备注"),
                "{kind:?}"
            );
        }

        // 非描述类：列 / 文件夹 / Schema / Catalog 不显。
        let hidden = [
            NavNodeKind::Column {
                data_type: "int".into(),
                nullable: true,
                primary: false,
                foreign: false,
            },
            NavNodeKind::Folder(NavFolder::Tables),
            NavNodeKind::Schema,
            NavNodeKind::Catalog,
        ];
        for kind in &hidden {
            assert_eq!(comment_subline_text(&on, kind, Some("备注")), None, "{kind:?}");
        }

        // 注释缺失 / 空白串：不显。
        let table = NavNodeKind::Table { row_estimate: None };
        assert_eq!(comment_subline_text(&on, &table, None), None);
        assert_eq!(comment_subline_text(&on, &table, Some("  ")), None);
    }

    #[test]
    fn size_badge_gated_by_profile_and_presence() {
        let on = inline_all_on();
        let off = InlineStatsProfile { show_size: false, ..on };

        // 档案关闭：即便有大小也不显。
        assert_eq!(size_badge_text(&off, Some(1_048_576)), None);
        // 未回填：安静消失（在途「…」由行估计槽统一承担，本函数不重复占位）。
        assert_eq!(size_badge_text(&on, None), None);
        // 已回填：走二进制档位格式化。
        assert_eq!(size_badge_text(&on, Some(1_048_576)).as_deref(), Some("1.0 MB"));
    }

    #[test]
    fn folder_count_label_gated_by_profile_kind_and_zero() {
        let on = inline_all_on();
        let off = InlineStatsProfile { show_child_count: false, ..on };

        // 档案关闭：不显。
        assert_eq!(
            folder_count_label(&off, &NavNodeKind::Folder(NavFolder::Tables), Some(7), 3),
            None
        );
        // 非文件夹：不显。
        assert_eq!(folder_count_label(&on, &NavNodeKind::View, Some(7), 3), None);
        // 候选总数优先于已加载数。
        assert_eq!(
            folder_count_label(&on, &NavNodeKind::Folder(NavFolder::Tables), Some(7), 3)
                .as_deref(),
            Some("7")
        );
        // 缺省回退已加载子节点数。
        assert_eq!(
            folder_count_label(&on, &NavNodeKind::Folder(NavFolder::Tables), None, 3).as_deref(),
            Some("3")
        );
        // 零个不显。
        assert_eq!(
            folder_count_label(&on, &NavNodeKind::Folder(NavFolder::Tables), Some(0), 0),
            None
        );
    }

    // ==================== B-1 对象名 mask 规则矩阵 ====================

    /// glob：`*` 任意多字符（含空）、`?` 单字符、大小写不敏感、字面全串。
    #[test]
    fn glob_matches_wildcards_and_case() {
        assert!(nav_glob_match("orders*", "orders_2024"));
        assert!(nav_glob_match("orders*", "orders"));
        assert!(nav_glob_match("*", "anything"));
        assert!(nav_glob_match("order?_items", "orderX_items"));
        assert!(!nav_glob_match("order?_items", "orderXY_items"));
        assert!(nav_glob_match("ORDERS*", "orders_2024"));
        assert!(nav_glob_match("*temp*", "x_temp_x"));
        assert!(!nav_glob_match("orders*", "customers"));
        assert!(!nav_glob_match("", "orders")); // 空 pattern 只匹配空名
    }

    /// exclude 优先于 include；include 空 = 全量候选。
    #[test]
    fn mask_exclude_beats_include_and_empty_include_passes_all() {
        // 空规则 = 不过滤。
        assert!(nav_mask_passes(&ObjectMaskRule::default(), "orders"));
        // include 命中 + exclude 未命中 → 保留。
        let r = ObjectMaskRule {
            include: vec!["orders*".into()],
            exclude: vec!["orders_old*".into()],
            ..Default::default()
        };
        assert!(nav_mask_passes(&r, "orders_2024"));
        assert!(!nav_mask_passes(&r, "orders_old_2024")); // exclude 优先
        // 只 exclude：其余全保留。
        let r2 = ObjectMaskRule {
            include: vec![],
            exclude: vec!["temp_*".into()],
            ..Default::default()
        };
        assert!(nav_mask_passes(&r2, "orders"));
        assert!(!nav_mask_passes(&r2, "temp_table"));
        // include 非空且不命中 → 隐藏。
        let r3 = ObjectMaskRule {
            include: vec!["orders*".into()],
            exclude: vec![],
            ..Default::default()
        };
        assert!(!nav_mask_passes(&r3, "customers"));
    }

    /// 正则分支：精确（`^…$`）/ 前缀 / 后缀 / 子串锚点语义，`(?i)` 大小写不敏感。
    #[test]
    fn mask_regex_matches_anchors_and_is_case_insensitive() {
        let regex = |include: Vec<&str>, exclude: Vec<&str>| ObjectMaskRule {
            include: include.into_iter().map(str::to_string).collect(),
            exclude: exclude.into_iter().map(str::to_string).collect(),
            use_regex: true,
        };
        // 精确整串（锚点由模式自带；裸正则是子串 search，与 DBeaver 正则语义一致）。
        let exact = regex(vec!["^orders$"], vec![]);
        assert!(nav_mask_passes(&exact, "orders"));
        assert!(!nav_mask_passes(&exact, "orders_2024"));
        // 前缀 `^ord`。
        let prefix = regex(vec!["^ord"], vec![]);
        assert!(nav_mask_passes(&prefix, "orders"));
        assert!(!nav_mask_passes(&prefix, "customers"));
        // 后缀 `id$`。
        let suffix = regex(vec!["id$"], vec![]);
        assert!(nav_mask_passes(&suffix, "order_id"));
        assert!(!nav_mask_passes(&suffix, "idle"));
        // 子串 `.*order.*`。
        let contains = regex(vec![".*order.*"], vec![]);
        assert!(nav_mask_passes(&contains, "order_items"));
        assert!(!nav_mask_passes(&contains, "customers"));
        // `(?i)` 前缀：大写模式命中小写名，小写模式同样命中大写名。
        let insensitive = regex(vec!["^ORD"], vec![]);
        assert!(nav_mask_passes(&insensitive, "orders"));
        assert!(nav_mask_passes(&insensitive, "ORDERS"));
    }

    /// 正则分支 exclude 优先于 include。
    #[test]
    fn mask_regex_exclude_still_beats_include() {
        let rule = ObjectMaskRule {
            include: vec!["order.*".into()],
            exclude: vec![".*_bak".into()],
            use_regex: true,
        };
        assert!(nav_mask_passes(&rule, "order_2024"));
        assert!(!nav_mask_passes(&rule, "order_bak"));
    }

    /// 编译失败的正则条目丢弃、永不命中、不 panic；include 全坏不被误判为空而放行。
    #[test]
    fn mask_regex_bad_pattern_never_matches_and_does_not_panic() {
        // exclude 坏正则：该条不命中，其余名字不被排除（不 panic）。
        let bad_exclude = ObjectMaskRule {
            include: vec![],
            exclude: vec!["[未闭合".into()],
            use_regex: true,
        };
        assert!(nav_mask_passes(&bad_exclude, "orders"));
        // include 一条坏 + 一条好：坏的丢弃，好的照常参与。
        let mixed = ObjectMaskRule {
            include: vec!["[bad".into(), "^good".into()],
            exclude: vec![],
            use_regex: true,
        };
        assert!(nav_mask_passes(&mixed, "good_table"));
        assert!(!nav_mask_passes(&mixed, "other_table"));
        // include 全坏：原始规则非空但编译后无可用正则，仍按「非空 include」判定隐藏，
        // 不被 `include_is_empty` 误判为空规则放行全部。
        let all_bad_include = ObjectMaskRule {
            include: vec!["[bad".into(), "(unclosed".into()],
            exclude: vec![],
            use_regex: true,
        };
        assert!(!nav_mask_passes(&all_bad_include, "orders"));
        // 直接走编译产物：坏条被丢弃，include_is_empty 仍反映原始规则非空。
        let compiled = compile_mask(&all_bad_include);
        assert!(!compiled.include_is_empty);
        assert!(compiled.include_re.is_empty());
        assert!(!mask_passes(&compiled, "orders"));
    }

    /// 组合判定：per-conn + 全局 AND，任一启用规则不通过即隐藏；空规则不参与。
    #[test]
    fn object_masked_combines_layers_and_ignores_inactive() {
        let per_conn = ObjectMaskRule {
            include: vec!["orders*".into()],
            ..Default::default()
        };
        let global = ObjectMaskRule {
            exclude: vec!["*_archive".into()],
            ..Default::default()
        };
        // 两层都过 → 显示。
        assert!(!nav_object_masked(Some(&per_conn), Some(&global), "orders_2024"));
        // 全局 exclude 命中 → 隐藏（即使 per-conn include 命中）。
        assert!(nav_object_masked(Some(&per_conn), Some(&global), "orders_archive"));
        // per-conn 不命中 → 隐藏。
        assert!(nav_object_masked(Some(&per_conn), Some(&global), "customers"));
        // 空 per-conn（不启用）→ 只看全局。
        assert!(!nav_object_masked(None, Some(&global), "orders"));
        // 两层都空 → 显示。
        assert!(!nav_object_masked(None, None, "orders"));
    }

    /// 三态占位判定：本地筛选空 / 索引搜索空 / 两者叠加；其余条件一律不摆。
    #[test]
    fn placeholder_decision_picks_the_right_empty_state() {
        // 本地筛选无匹配（索引未触发；树行全空）。
        assert_eq!(
            placeholder_decision(true, false, false, true, false, false, false),
            Some(PlaceholderKind::FilterEmpty)
        );
        // 索引搜索已完成且无命中，树内也无对象行（连接壳不算对象行）。
        assert_eq!(
            placeholder_decision(false, true, true, true, true, false, false),
            Some(PlaceholderKind::SearchEmpty)
        );
        // 两者叠加。
        assert_eq!(
            placeholder_decision(true, true, true, true, true, false, false),
            Some(PlaceholderKind::FilterSearchEmpty)
        );
        // 已有可见行：不摆任何占位。
        assert!(placeholder_decision(true, false, false, true, true, false, false).is_none());
        assert!(placeholder_decision(false, true, true, true, false, true, false).is_none());
        assert!(placeholder_decision(true, true, true, true, false, true, false).is_none());
        // 在途加载中：先不摆空态，避免结果回填瞬间空态闪现。
        assert!(placeholder_decision(true, false, false, true, false, false, true).is_none());
        assert!(placeholder_decision(false, true, true, true, true, false, true).is_none());
        // 索引搜索进行中（未回填）：不摆搜索空态。
        assert!(placeholder_decision(false, true, false, true, true, false, false).is_none());
        // 索引已有命中：不摆搜索空态。
        assert!(placeholder_decision(false, true, true, false, true, false, false).is_none());
        // 两个通道都不活跃：不摆。
        assert!(placeholder_decision(false, false, false, true, false, false, false).is_none());
    }

    /// 占位文案：索引空态带上原词，其余两类为固定说明。
    #[test]
    fn placeholder_message_mentions_the_query_for_search_empty() {
        assert_eq!(
            placeholder_message(PlaceholderKind::FilterEmpty, ""),
            "当前筛选无匹配对象"
        );
        assert_eq!(
            placeholder_message(PlaceholderKind::SearchEmpty, "orders"),
            "索引中未找到「orders」"
        );
        assert_eq!(
            placeholder_message(PlaceholderKind::FilterSearchEmpty, ""),
            "筛选与索引搜索均无匹配对象"
        );
    }

    /// 「加载更多」行文案：常规可见/共；筛选追加覆盖范围提示；取数中固定文案。
    #[test]
    fn more_row_label_follows_filter_and_fetching_state() {
        assert_eq!(more_row_label(2, 5, false, false), "可见 2 / 共 5");
        assert_eq!(
            more_row_label(2, 5, true, false),
            "可见 2 / 共 5；筛选仅覆盖已加载"
        );
        assert_eq!(more_row_label(2, 5, false, true), "加载中…");
        assert_eq!(more_row_label(2, 5, true, true), "加载中…");
    }

    /// D8：六类全空 → 不需要本轮状态写入与重绘（跳过 `weak.update`，直接进 idle 判定）。
    #[test]
    fn batches_present_all_empty_is_false() {
        let load: Vec<nav_jobs::LoadResult> = Vec::new();
        let sql: Vec<nav_jobs::SqlGenResult> = Vec::new();
        let test: Vec<nav_jobs::TestConnResult> = Vec::new();
        let row_estimates: Vec<nav_jobs::RowEstimateResult> = Vec::new();
        let connect: Vec<nav_jobs::ConnectResult> = Vec::new();
        let search: Vec<nav_jobs::SearchResult> = Vec::new();
        assert!(!batches_present(
            &load,
            &sql,
            &test,
            &row_estimates,
            &connect,
            &search
        ));
    }

    /// D8：六类任一非空即触发本轮合并提交（逐一覆盖六个分支，防止 OR 折叠漏类）。
    #[test]
    fn batches_present_each_nonempty_category_triggers() {
        let load = vec![nav_jobs::LoadResult {
            key: "k".into(),
            conn_id: "c".into(),
            project_root: None,
            path: NavPath::Connection,
            offset: 0,
            jumped_to: None,
            conn_fingerprint: None,
            result: Ok(NavPage {
                nodes: vec![],
                total: 0,
                offset: 0,
                has_more: false,
            }),
        }];
        let sql = vec![nav_jobs::SqlGenResult {
            key: "k".into(),
            result: Ok(String::new()),
        }];
        let test = vec![nav_jobs::TestConnResult {
            conn_id: "c".into(),
            name: "n".into(),
            result: Ok(String::new()),
        }];
        let row_estimates = vec![nav_jobs::RowEstimateResult {
            conn_id: "c".into(),
            catalog: String::new(),
            schema: String::new(),
            seq: 0,
            estimates: vec![],
            sizes: vec![],
        }];
        let connect = vec![nav_jobs::ConnectResult {
            conn_id: "c".into(),
            ok: true,
            message: String::new(),
        }];
        let search = vec![nav_jobs::SearchResult {
            consumer: nav_jobs::SearchConsumer::Navigator,
            seq: 0,
            query: String::new(),
            searched: 0,
            hits: vec![],
        }];

        // 逐一替换为非空，其余全空 → 每一类都应触发。
        let cases: [(&str, bool); 6] = [
            ("load", batches_present(&load, &[], &[], &[], &[], &[])),
            ("sql", batches_present(&[], &sql, &[], &[], &[], &[])),
            ("test", batches_present(&[], &[], &test, &[], &[], &[])),
            (
                "row_estimate",
                batches_present(&[], &[], &[], &row_estimates, &[], &[]),
            ),
            (
                "connect",
                batches_present(&[], &[], &[], &[], &connect, &[]),
            ),
            (
                "search",
                batches_present(&[], &[], &[], &[], &[], &search),
            ),
        ];
        for (name, present) in cases {
            assert!(present, "类别 {name} 非空应触发合并提交");
        }
    }

    /// D8：合并窗口 core 调用顺序与现状泵顺序一致（load → sql → test → row_estimate
    /// → connect → search），顺序表供回归钉死，core 拆分不得调乱。
    #[test]
    fn pump_batch_order_follows_load_to_search() {
        assert_eq!(
            PUMP_BATCH_ORDER,
            ["load", "sql", "test", "row_estimate", "connect", "search"]
        );
    }

    /// D9：子树判定——根 key 与前缀内命中，前缀外保留；前缀与根不混判。
    #[test]
    fn key_in_subtree_matches_root_and_prefix_only() {
        let root = "C_1/shop";
        let prefix = "C_1/shop/";
        assert!(key_in_subtree(root, root, prefix));
        assert!(key_in_subtree("C_1/shop/public", root, prefix));
        assert!(key_in_subtree("C_1/shop/public/orders", root, prefix));
        assert!(!key_in_subtree("C_1/other", root, prefix));
        assert!(!key_in_subtree("C_1", root, prefix));
    }

    /// D9：`prune_subtree_keys` 回收集合内根与前缀 key，保留其余（refresh_node 清
    /// jumped/loading 同口径）。
    #[test]
    fn prune_subtree_keys_drops_root_and_descendants_only() {
        let set: HashSet<String> = [
            "C_1/shop".to_string(),
            "C_1/shop/public".to_string(),
            "C_1/other".to_string(),
            "C_2".to_string(),
        ]
        .into_iter()
        .collect();
        let kept = prune_subtree_keys(&set, "C_1/shop", "C_1/shop/");
        assert_eq!(kept, ["C_1/other".to_string(), "C_2".to_string()].into_iter().collect());
    }

    /// D9：选中锚点落子树内命中清空；子树外保持；None 不命中。
    #[test]
    fn selected_key_in_subtree_detects_inside_and_outside() {
        let root = "C_1/shop";
        let prefix = "C_1/shop/";
        assert!(selected_key_in_subtree(Some("C_1/shop"), root, prefix));
        assert!(selected_key_in_subtree(Some("C_1/shop/public/orders"), root, prefix));
        assert!(!selected_key_in_subtree(Some("C_1/other"), root, prefix));
        assert!(!selected_key_in_subtree(None, root, prefix));
    }

    /// D9：`prune_expanded` 保留现存 key 与连接根白名单，剔除已删节点残留；
    /// 父链容器 key 在现存集合内不会被误清（R2 风险反证）。
    #[test]
    fn prune_expanded_keeps_existing_and_roots_drops_orphans() {
        let expanded: HashSet<String> = [
            "C_1".to_string(),                 // 连接根：root 白名单保留
            "C_1/shop".to_string(),            // 现存容器：保留
            "C_1/shop/public".to_string(),      // 现存容器：保留
            "C_1/shop/public/orders".to_string(), // 现存节点：保留
            "C_1/shop/public/deleted".to_string(), // 已删节点：剔除
            "C_2".to_string(),                 // 另一连接根：root 白名单保留
        ]
        .into_iter()
        .collect();
        let existing: HashSet<String> = [
            "C_1/shop".to_string(),
            "C_1/shop/public".to_string(),
            "C_1/shop/public/orders".to_string(),
        ]
        .into_iter()
        .collect();
        let roots: HashSet<String> = ["C_1".to_string(), "C_2".to_string()].into_iter().collect();
        let kept = prune_expanded(&expanded, &existing, &roots);
        assert!(kept.contains("C_1"));
        assert!(kept.contains("C_1/shop"));
        assert!(kept.contains("C_1/shop/public"));
        assert!(kept.contains("C_1/shop/public/orders"));
        assert!(kept.contains("C_2"));
        assert!(!kept.contains("C_1/shop/public/deleted"));
    }

    /// D11 why 三态：名称命中不标注；注释 / 定义命中给对应文案与角色（定义分支仅由
    /// 档案位与本测试钉住，本批生产序列不产生定义候选）。
    #[test]
    fn hit_source_badge_labels_comment_and_definition_only() {
        assert_eq!(nav_hit_source_badge(nav_jobs::HitSource::Name), None);
        assert_eq!(
            nav_hit_source_badge(nav_jobs::HitSource::Comment),
            Some(("注释", NavHitBadgeRole::Comment))
        );
        assert_eq!(
            nav_hit_source_badge(nav_jobs::HitSource::Definition),
            Some(("定义", NavHitBadgeRole::Definition))
        );
    }

    /// B6 菜单标签矩阵：已入队统一切「从待提交移除」；未入队按动作 × 类别给细分文案；
    /// 不适用组合返回 `None`（菜单不渲染该项，类型白名单与注册表 `applies` 同源）。
    #[test]
    fn staged_menu_label_follows_action_kind_and_staged_state() {
        // 已入队：适用类别一律切移除臂，不再区分 Drop / Truncate。
        for (op, kind) in [
            (NavOp::Drop, ObjectKind::Table),
            (NavOp::Drop, ObjectKind::View),
            (NavOp::Drop, ObjectKind::Sequence),
            (NavOp::Truncate, ObjectKind::Table),
        ] {
            assert_eq!(
                staged_menu_label(op, kind, true),
                Some("从待提交移除"),
                "{op:?}/{kind:?} 已入队应切移除臂"
            );
        }
        // 未入队：动作 × 类别细分文案。
        assert_eq!(staged_menu_label(NavOp::Drop, ObjectKind::Table, false), Some("删除表…"));
        assert_eq!(staged_menu_label(NavOp::Drop, ObjectKind::View, false), Some("删除视图…"));
        assert_eq!(staged_menu_label(NavOp::Drop, ObjectKind::Sequence, false), Some("删除序列…"));
        assert_eq!(staged_menu_label(NavOp::Truncate, ObjectKind::Table, false), Some("清空表…"));
        // 不适用：Truncate 只挂表；其余类别不产生破坏性菜单项。
        for (op, kind) in [
            (NavOp::Truncate, ObjectKind::View),
            (NavOp::Truncate, ObjectKind::Sequence),
            (NavOp::Truncate, ObjectKind::Routine),
            (NavOp::Drop, ObjectKind::Routine),
            (NavOp::Drop, ObjectKind::Trigger),
            (NavOp::Drop, ObjectKind::Column),
            (NavOp::Drop, ObjectKind::Schema),
        ] {
            assert_eq!(
                staged_menu_label(op, kind, false),
                None,
                "{op:?}/{kind:?} 不应出现菜单项"
            );
        }
    }

    /// B6 单项确认对话框文案矩阵：标题按动作 × 类别分派；描述含限定名与对应不可恢复警告；
    /// OK 按钮文案统一「确认加入待提交」；不适用组合返回 `None`。
    #[test]
    fn staged_dialog_copy_follows_action_kind_with_qualified_name() {
        let (title, desc, ok) = staged_dialog_copy(NavOp::Drop, ObjectKind::Table, "shop.orders")
            .expect("删除表应给出文案");
        assert_eq!(title, "删除表");
        assert!(desc.contains("shop.orders"), "描述应含限定名");
        assert!(desc.contains("表及其数据将被永久删除"), "表删除警告口径");
        assert_eq!(ok, "确认加入待提交");

        let (title, desc, ok) =
            staged_dialog_copy(NavOp::Truncate, ObjectKind::Table, "shop.orders")
                .expect("清空表应给出文案");
        assert_eq!(title, "清空表");
        assert!(desc.contains("shop.orders"));
        assert!(desc.contains("表内全部数据将被删除"), "清空表警告口径");
        assert_eq!(ok, "确认加入待提交");

        let (title, desc, ok) = staged_dialog_copy(NavOp::Drop, ObjectKind::View, "shop.v_orders")
            .expect("删除视图应给出文案");
        assert_eq!(title, "删除视图");
        assert!(desc.contains("shop.v_orders"));
        assert!(desc.contains("提交后不可恢复"));
        assert_eq!(ok, "确认加入待提交");

        let (title, desc, ok) =
            staged_dialog_copy(NavOp::Drop, ObjectKind::Sequence, "shop.seq_id")
                .expect("删除序列应给出文案");
        assert_eq!(title, "删除序列");
        assert!(desc.contains("shop.seq_id"));
        assert!(desc.contains("提交后不可恢复"));
        assert_eq!(ok, "确认加入待提交");

        // 不适用组合不产出对话框。
        assert!(staged_dialog_copy(NavOp::Truncate, ObjectKind::View, "v").is_none());
        assert!(staged_dialog_copy(NavOp::Drop, ObjectKind::Routine, "f").is_none());
    }

    #[test]
    fn staged_action_label_maps_four_families_and_rejects_mismatch() {
        // 清单行动作标签不带菜单省略号「…」（与 `staged_menu_label` 区分）。
        assert_eq!(
            staged_action_label(NavOp::Drop, ObjectKind::Table),
            Some("删除表")
        );
        assert_eq!(
            staged_action_label(NavOp::Drop, ObjectKind::View),
            Some("删除视图")
        );
        assert_eq!(
            staged_action_label(NavOp::Drop, ObjectKind::Sequence),
            Some("删除序列")
        );
        assert_eq!(
            staged_action_label(NavOp::Truncate, ObjectKind::Table),
            Some("清空表")
        );
        // 不适用组合不产出标签（注册表 `applies` 同源，正常路径不出现）。
        assert!(staged_action_label(NavOp::Truncate, ObjectKind::View).is_none());
        assert!(staged_action_label(NavOp::Drop, ObjectKind::Routine).is_none());
    }

    #[test]
    fn staged_qualified_name_builds_catalog_schema_name_by_availability() {
        let full = StagedOp {
            key: "k1".to_string(),
            conn_id: "G_1".to_string(),
            driver: "postgresql".to_string(),
            catalog: Some("shop".to_string()),
            schema: Some("public".to_string()),
            parent: None,
            object_name: "orders".to_string(),
            object_kind: ObjectKind::Table,
            op: NavOp::Drop,
            error: None,
        };
        assert_eq!(staged_qualified_name(&full), "shop.public.orders");

        let no_catalog = StagedOp {
            catalog: None,
            ..full.clone()
        };
        assert_eq!(staged_qualified_name(&no_catalog), "public.orders");

        let no_schema = StagedOp {
            schema: None,
            ..full
        };
        assert_eq!(staged_qualified_name(&no_schema), "shop.orders");
    }

    #[test]
    fn staged_commit_summary_single_line_when_all_succeed() {
        let text = staged_commit_summary(3, &[]);
        assert_eq!(text, "已提交 3 项变更");
    }

    #[test]
    fn staged_commit_summary_lists_failures_up_to_five_then_collapses() {
        let two: Vec<(String, String)> = vec![
            ("shop.public.orders".to_string(), "连接已断开".to_string()),
            ("shop.public.items".to_string(), "关系不存在".to_string()),
        ];
        let text = staged_commit_summary(1, &two);
        assert!(text.starts_with("成功 1 项、失败 2 项"));
        assert!(text.contains("· shop.public.orders：连接已断开"));
        assert!(text.contains("· shop.public.items：关系不存在"));
        assert!(!text.contains("等 2 项失败"), "≤5 条不得折叠");

        let seven: Vec<(String, String)> = (0..7)
            .map(|i| (format!("shop.public.t{i}"), "执行超时".to_string()))
            .collect();
        let text = staged_commit_summary(2, &seven);
        assert!(text.starts_with("成功 2 项、失败 7 项"));
        assert!(text.contains("· shop.public.t0：执行超时"));
        assert!(text.contains("· shop.public.t4：执行超时"), "前 5 条全列");
        assert!(!text.contains("t5"), "第 6 条起折叠不展开");
        assert!(!text.contains("t6"));
        assert!(text.contains("…等 7 项失败，详见日志"), "超 5 条折叠行");
    }
}

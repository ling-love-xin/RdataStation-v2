//! 数据源导航设置 model（struct + 默认值 + 默认函数）。
use std::collections::BTreeMap;
use super::default_true;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NavigatorFilters {
    /// 归属域筛选（`project` / `global` / `shared`；`None` = 全部）。
    #[serde(default)]
    pub source: Option<String>,
    /// 数据库类型筛选（`drivers.type_id`）。
    #[serde(default)]
    pub db_type: Option<String>,
    /// 驱动 id 筛选。
    #[serde(default)]
    pub driver: Option<String>,
    /// 标签筛选。
    #[serde(default)]
    pub tag: Option<String>,
}

/// 数据源导航：来源标识展示形式、显示开关与属性面板宽度（UI 偏好，跨项目）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Navigator {
    /// 来源标识用短码（`P` / `G` / `GP`）；false 时用文字（项目 / 全局 / 共享）。
    #[serde(default = "default_true")]
    pub source_short_code: bool,
    /// 连接行是否显示标签 chip（默认关；开启后「≤2 chip + `+N`」）。
    #[serde(default)]
    pub show_tags: bool,
    /// 连接行是否显示归属域列（默认开）。
    #[serde(default = "default_true")]
    pub show_scope: bool,
    /// 面板是否显示来源筛选 chips 行（全部/项目/全局/共享，默认开）。
    #[serde(default = "default_true")]
    pub show_chips: bool,
    /// 面板是否显示搜索行（输入框 + 模式切换，默认开）。
    #[serde(default = "default_true")]
    pub show_search: bool,
    /// 启动时是否自动连接上次打开的连接（默认关）。
    #[serde(default)]
    pub auto_connect_on_startup: bool,
    /// 连接名称前是否以 `[简称]` 显示类型简称（如 `[MY] 192.168.3.138`）。默认开。
    /// 简称来自 `type_short_map`（「类型简称映射…」弹窗维护），缺省回退徽标缩写。
    #[serde(default = "default_true")]
    pub show_name_type_short: bool,
    /// 数据库类型 → 简称 映射表（`type_id` → 简称）。默认同徽标缩写
    /// （PG/MY/MA/SQ/DK/MS/OR/CH/MG/RD/GH）；未登记类型回退类型名前 2 字母。
    #[serde(default)]
    pub type_short_map: std::collections::HashMap<String, String>,
    /// facet 筛选（类型 / 驱动 / 标签 / 归属域）。
    #[serde(default)]
    pub filters: NavigatorFilters,
    /// 已保存视图（命名 facet 快照，跨项目 UI 偏好）。
    ///
    /// 视图名为键（同名唯一、天然拦截）；`BTreeMap` 按名有序，子菜单与管理对话框
    /// 按此顺序展示。读写走导航面板自己的入口。
    #[serde(default)]
    pub saved_views: BTreeMap<String, NavigatorFilters>,
    /// 每连接的可见库白名单（S3-①）：`conn_id → 可见集合`。
    ///
    /// 键不存在 = 默认模式（树隐藏系统库 / 系统 schema）；键存在 = 白名单模式
    /// （原样展示集合里的名字，含系统对象）。集合里可同时含 catalog 名与 schema 名。
    /// 空集合 = 显式「全部隐藏」，等价于默认模式但对用户可见（弹框里全部取消勾选）。
    #[serde(default)]
    pub visible_databases: BTreeMap<String, Vec<String>>,
    /// 表/视图行是否显示行估计徽标（S3-④，默认开）：PG `reltuples` / MySQL
    /// `TABLE_ROWS` 后台回填，行尾「≈ N」弱色徽标；关闭后不排估计任务、不显示徽标。
    #[serde(default = "default_true")]
    pub row_estimate: bool,
    /// 表/视图行右侧统计徽标**整体**开关（需求④，默认开）：分区 / 继承 / ≈行估计 /
    /// 大小(B/KB) / 注释副标 一并开或关。关时即使驱动档案默认开也不显示；
    /// 开时仍受各驱动档案（`family_inline_profile`）的 show_size / show_description 门控。
    /// 「为何只有 PG 有」：分区/继承/大小/行估计由驱动档案 + 方言查询支持度共同决定
    /// （PG 档案默认开 size、方言支持行估计；SQLite/DuckDB 档案关 size 且不支持行估计）。
    #[serde(default = "default_true")]
    pub show_inline_stats: bool,
    /// 表行「注释副标」子开关（需求②，默认开）：总开关 `show_inline_stats` 开时仍受
    /// 此项门控；关时表/视图行的注释副标不显示（其余统计项不受影响）。
    #[serde(default = "default_true")]
    pub show_inline_comment: bool,
    /// 表行「大小徽标」子开关（需求②，默认开）：总开关开时仍受此项门控；关时
    /// 表行的 `≈ KB` 大小徽标不显示。「行数」子项由既有 `row_estimate` 独立承担。
    #[serde(default = "default_true")]
    pub show_inline_size: bool,
    /// 每连接的对象名 mask（B-1）：`conn_id → 规则`。
    ///
    /// 键不存在 = 该连接不过滤；键存在 = 按规则过滤**已加载**的对象行（表 / 视图 /
    /// 例程 / 序列 / 触发器，只作用于对象层，catalog / schema 层不进 mask）。
    /// 过滤是纯内存操作：只滤已加载进 `children` 的子集，`child_total`（分页判断）
    /// 保持原始候选计数不变。断连连接的缓存对象照常过滤（B-1-E）。
    #[serde(default)]
    pub object_masks: BTreeMap<String, ObjectMaskRule>,
    /// 面板全局对象名 mask（B-1）：对面板内全部可见连接的对象名兜底过滤。
    /// 空规则 = 不启用；与 per-conn mask、facet 均为 AND。
    #[serde(default)]
    pub global_mask: ObjectMaskRule,
}

/// 对象名过滤规则（B-1，glob / 正则双档，由 `use_regex` 切换）。
///
/// 匹配语义：include 为空 = 全部候选进入候选集；exclude 为空 = 不排除；**exclude 优先于
/// include**（命中 include 且未命中 exclude 的对象保留）。`use_regex=false` 走 glob
///（`*` 任意多字符 / `?` 单字符，大小写不敏感）；`use_regex=true` 走正则（`(?i)`
/// 大小写不敏感，编译失败条目视为永不命中）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ObjectMaskRule {
    /// include 模式列表（每行一条，空行忽略；空 = 全量候选；glob / 正则由 `use_regex` 决定）。
    #[serde(default)]
    pub include: Vec<String>,
    /// exclude 模式列表（每行一条，空行忽略；空 = 不排除；glob / 正则由 `use_regex` 决定）。
    #[serde(default)]
    pub exclude: Vec<String>,
    /// `true` = 正则匹配；`false` = glob 匹配。
    #[serde(default)]
    pub use_regex: bool,
}

impl ObjectMaskRule {
    /// 规则是否「启用」：include 或 exclude 任一非空（空规则 = 该层不过滤）。
    pub fn is_active(&self) -> bool {
        !self.include.is_empty() || !self.exclude.is_empty()
    }
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            source_short_code: true,
            show_tags: false,
            show_scope: true,
            show_chips: false,
            show_search: true,
            auto_connect_on_startup: false,
            show_name_type_short: true,
            type_short_map: default_type_short_map(),
            filters: NavigatorFilters::default(),
            saved_views: BTreeMap::new(),
            visible_databases: BTreeMap::new(),
            row_estimate: true,
            show_inline_stats: true,
            show_inline_comment: true,
            show_inline_size: true,
            object_masks: BTreeMap::new(),
            global_mask: ObjectMaskRule::default(),
        }
    }
}

/// 一个已保存视图（访问器返回载体：`name` 来自 map 键，`filters` 是快照）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    /// 视图名（map 键，同名唯一）。
    pub name: String,
    /// 保存时的筛选快照（四项均可为 `None`；全空视图禁止保存）。
    pub filters: NavigatorFilters,
}

/// 类型简称映射默认值：与导航徽标缩写（`nav_type_badge`）同源，
/// 设置层独立持有种子（不依赖 database crate，依赖只向下）。
/// 需要新增类型时，同步维护 `primitives.rs::nav_type_badge` 与此表。
pub fn default_type_short_map() -> std::collections::HashMap<String, String> {
    [
        ("postgresql", "PG"),
        ("mysql", "MY"),
        ("mariadb", "MA"),
        ("sqlite", "SQ"),
        ("duckdb", "DK"),
        ("mssql", "MS"),
        ("oracle", "OR"),
        ("clickhouse", "CH"),
        ("mongodb", "MG"),
        ("redis", "RD"),
        ("github", "GH"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

//! L1 族档案：族静态元模型 + 族无关身份字典 + 内置四族档案注册表。
//!
//! 身份基准是 `engine::ObjectKind`（八类可寻址闭集，方案稿 D5 保持闭集、档案层开放扩展）。
//! 本模块的字典函数输出与现状七处 `match` 逐字等价（B1 等价迁移，零行为变化）；
//! 四族档案的类型全集 / 层级形态当前仅作为静态元数据建档，运行时显隐判定归 B2 能力矩阵。

use std::sync::OnceLock;

use engine::ObjectKind;
use property::PropertyKind;

use crate::model::{NavFolder, NavNodeKind};

/// 文件夹对象的展示变体形状（M1 档案查表结果）。
///
/// 只表达「该文件夹装哪一类叶子对象」；运行时携带的数据（如例程的
/// `routine_type`、表的 `row_estimate`）由装配方填充，档案不承担运行时状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeShape {
    Table,
    View,
    Routine,
    Sequence,
    Trigger,
}

/// 对象类型档案条目：身份基准 `ObjectKind` ↔ 文件夹 / 图标 / 容器性 / 树内可见性。
#[derive(Clone, Copy, Debug)]
pub struct TypeProfile {
    pub kind: ObjectKind,
    /// 所属文件夹分组；Catalog / Schema 无文件夹归属 → `None`。
    pub folder: Option<NavFolder>,
    /// 对象类型图标 key；展示特例（连接根 / 文件夹开合 / 列 / 列溢出提示节点）不在此表。
    pub icon: Option<&'static str>,
    /// 是否容器（可展开下一层：Catalog / Schema）。
    pub is_container: bool,
    /// 是否在树一级出现；`false` = 仅作为详情子节点（Column）。
    pub shown_in_tree: bool,
}

/// 文件夹分组档案：标题与排序。
#[derive(Clone, Copy, Debug)]
pub struct FolderProfile {
    pub key: NavFolder,
    pub label: &'static str,
    pub order: u8,
}

/// 一族数据库在「注释 / 定义」两个搜索维度上支持的对象类型集合（D11 档案位）。
///
/// 口径对齐 DBeaver `supportsSearchInCommentsFor/DefinitionsFor`：注释 / 定义开关开启后，
/// 并入名称档的 FTS 候选只保留本族在该维度声明支持的类型；未声明类型不进榜。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchScopeProfile {
    /// 注释搜索覆盖的对象类型。
    pub comments: &'static [ObjectKind],
    /// 定义搜索覆盖的对象类型。
    ///
    /// 当前批量内省链路尚未采集视图 / 例程定义（`save_view` 写空串、`save_routine`
    /// definition 为 `None`），本批档案位先建档、定义候选为空；采集链路接通后即生效。
    pub definitions: &'static [ObjectKind],
}

/// 一族数据库在「行内统计元素」上的静态默认开关（D12 档案位）。
///
/// 口径对齐 DBeaver `UINavigatorPreferencesInitializer` 默认值：主机名 / 对象统计（大小）
/// 默认开，对象描述 / 子节点计数默认关。档案只表达「该族默认显不显示」；运行时是否
/// 真有数据可给由方言 / 数据源兜底（方言 `table_size_sql` 返回 `None` 时徽标安静消失）。
/// 用户偏好开关归宿主 settings，本批只落档案静态默认位 + 渲染接线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineStatsProfile {
    /// 对象行尾是否显示注释描述（斜体弱色副标；仅表 / 视图 / 例程 / 序列 / 触发器）。
    pub show_description: bool,
    /// 对象行尾是否显示表数据段大小徽标。
    pub show_size: bool,
    /// 连接行是否显示主机名副标（本地库 SQLite / DuckDB 无主机，默认关）。
    pub show_host_name: bool,
    /// 文件夹行尾是否显示子节点总数徽标。
    pub show_child_count: bool,
}

/// 一族数据库的静态档案（层级形态 + 类型全集 + 文件夹顺序 + 搜索范围）。
#[derive(Clone, Copy, Debug)]
pub struct DbFamilyProfile {
    /// 族 id（归一后）：`postgresql` / `mysql` / `sqlite` / `duckdb`。
    pub family: &'static str,
    /// 族别名（归一命中键集合；驱动 id 与历史写法都归到本族）。
    pub aliases: &'static [&'static str],
    /// 是否有独立 Schema 层（静态默认值；B2 运行时 trait 可覆写，本批只建档不接线）。
    pub has_schema_level: bool,
    /// Catalog 是否连接作用域内（无独立库层时为 false）。
    pub catalog_is_connection_scoped: bool,
    /// 类型全集（顺序即文件夹内默认排序；B1 仅档案元数据，不参与运行时显隐）。
    pub types: &'static [TypeProfile],
    /// 文件夹顺序与标题。
    pub folders: &'static [FolderProfile],
    /// 注释 / 定义搜索维度的类型集合（D11）。
    pub search: SearchScopeProfile,
    /// 行内统计元素静态默认开关（D12）。
    pub inline: InlineStatsProfile,
}

// =====================================================================================
// 族无关身份字典（B1 七处等价迁移落点；四族一致，与族档案解耦）
// =====================================================================================

/// M1：文件夹 → 展示变体形状 + 是否有子节点（`navigator_service.rs:717-728` 等价迁移）。
///
/// 表 / 视图可展开列；例程 / 序列 / 触发器为叶子。
pub fn folder_to_shape(folder: NavFolder) -> (NodeShape, bool) {
    match folder {
        NavFolder::Tables => (NodeShape::Table, true),
        NavFolder::Views => (NodeShape::View, true),
        NavFolder::Routines => (NodeShape::Routine, false),
        NavFolder::Sequences => (NodeShape::Sequence, false),
        NavFolder::Triggers => (NodeShape::Trigger, false),
    }
}

/// M2：文件夹 → 属性面板类别（`navigator_service.rs:742-748` 等价迁移）。
pub fn folder_to_property_kind(folder: NavFolder) -> PropertyKind {
    match folder {
        NavFolder::Tables => PropertyKind::Table,
        NavFolder::Views => PropertyKind::View,
        NavFolder::Routines => PropertyKind::Routine,
        NavFolder::Sequences => PropertyKind::Sequence,
        NavFolder::Triggers => PropertyKind::Trigger,
    }
}

/// M7：可寻址类别 → 属性面板类别（`model.rs:333-343` 等价迁移；八类同名一一对应）。
pub fn object_kind_to_property_kind(kind: ObjectKind) -> PropertyKind {
    match kind {
        ObjectKind::Catalog => PropertyKind::Catalog,
        ObjectKind::Schema => PropertyKind::Schema,
        ObjectKind::Table => PropertyKind::Table,
        ObjectKind::View => PropertyKind::View,
        ObjectKind::Column => PropertyKind::Column,
        ObjectKind::Routine => PropertyKind::Routine,
        ObjectKind::Sequence => PropertyKind::Sequence,
        ObjectKind::Trigger => PropertyKind::Trigger,
    }
}

/// M6：属性面板类别 → 文件夹分组（`chrome.rs:1047-1054` 等价迁移）。
///
/// 连接 / Catalog / Schema / 列不属于可展开文件夹，返回 `None`（重命名入口直接关闭）。
pub fn property_kind_to_folder(kind: PropertyKind) -> Option<NavFolder> {
    match kind {
        PropertyKind::Table => Some(NavFolder::Tables),
        PropertyKind::View => Some(NavFolder::Views),
        PropertyKind::Routine => Some(NavFolder::Routines),
        PropertyKind::Sequence => Some(NavFolder::Sequences),
        PropertyKind::Trigger => Some(NavFolder::Triggers),
        PropertyKind::Connection | PropertyKind::Catalog | PropertyKind::Schema
        | PropertyKind::Column => None,
    }
}

/// M9：可寻址类别 → 定位目标文件夹（`nav_view.rs:129-138` 等价迁移）。
///
/// 列命中归并到表文件夹（再展开一层父表）；Schema / Catalog 不进入文件夹层，返回 `None`。
/// 注意与 [`property_kind_to_folder`] 的差别：输入是 `ObjectKind`，且 Column 映射到 Tables。
pub fn object_kind_to_folder(kind: ObjectKind) -> Option<NavFolder> {
    match kind {
        ObjectKind::Table | ObjectKind::Column => Some(NavFolder::Tables),
        ObjectKind::View => Some(NavFolder::Views),
        ObjectKind::Routine => Some(NavFolder::Routines),
        ObjectKind::Sequence => Some(NavFolder::Sequences),
        ObjectKind::Trigger => Some(NavFolder::Triggers),
        ObjectKind::Schema | ObjectKind::Catalog => None,
    }
}

/// M9 反向：文件夹分组 → 身份类别（B3 方言 SPI `rename_sql` 入参用，Q1-A 与 L2/L4 同源 `ObjectKind`）。
///
/// 五个文件夹一一对应五类叶子对象；Catalog / Schema / Column 不属于文件夹层，不在此映射内。
pub fn folder_to_object_kind(folder: NavFolder) -> ObjectKind {
    match folder {
        NavFolder::Tables => ObjectKind::Table,
        NavFolder::Views => ObjectKind::View,
        NavFolder::Routines => ObjectKind::Routine,
        NavFolder::Sequences => ObjectKind::Sequence,
        NavFolder::Triggers => ObjectKind::Trigger,
    }
}

/// M5：索引命中类别 → 合成展示节点（`primitives.rs:549-561` 等价迁移；只为取图标 / 形状）。
///
/// 列节点用默认字段兜底（命中行不内联列细节）；其余不可直接挂载的类别回退 Schema 形状。
pub fn search_hit_node_kind(kind: ObjectKind) -> NavNodeKind {
    match kind {
        ObjectKind::Table => NavNodeKind::Table { row_estimate: None },
        ObjectKind::View => NavNodeKind::View,
        ObjectKind::Column => NavNodeKind::Column {
            data_type: String::new(),
            nullable: true,
            primary: false,
            foreign: false,
        },
        ObjectKind::Catalog
        | ObjectKind::Schema
        | ObjectKind::Routine
        | ObjectKind::Sequence
        | ObjectKind::Trigger => NavNodeKind::Schema,
    }
}

/// M3：对象类型图标 key（`primitives.rs:302-308` 等价迁移，仅五个叶子对象）。
///
/// 连接根 / Catalog / Schema / 文件夹开合 / 列 / 列溢出提示属展示特例，由视图层特例分支兜底；
/// 本函数对这些类别返回 `None`。
pub fn object_kind_icon(kind: ObjectKind) -> Option<&'static str> {
    match kind {
        ObjectKind::Table => Some("icons/table.svg"),
        ObjectKind::View => Some("icons/eye.svg"),
        ObjectKind::Routine => Some("icons/square-function.svg"),
        ObjectKind::Sequence => Some("icons/list-ordered.svg"),
        ObjectKind::Trigger => Some("icons/zap.svg"),
        ObjectKind::Catalog | ObjectKind::Schema | ObjectKind::Column => None,
    }
}

// =====================================================================================
// 内置四族档案 + 未知族回退通用档案
// =====================================================================================

const TYPE_TABLE: TypeProfile = TypeProfile {
    kind: ObjectKind::Table,
    folder: Some(NavFolder::Tables),
    icon: Some("icons/table.svg"),
    is_container: false,
    shown_in_tree: true,
};
const TYPE_VIEW: TypeProfile = TypeProfile {
    kind: ObjectKind::View,
    folder: Some(NavFolder::Views),
    icon: Some("icons/eye.svg"),
    is_container: false,
    shown_in_tree: true,
};
const TYPE_COLUMN: TypeProfile = TypeProfile {
    kind: ObjectKind::Column,
    folder: Some(NavFolder::Tables),
    icon: None,
    is_container: false,
    shown_in_tree: false,
};
const TYPE_ROUTINE: TypeProfile = TypeProfile {
    kind: ObjectKind::Routine,
    folder: Some(NavFolder::Routines),
    icon: Some("icons/square-function.svg"),
    is_container: false,
    shown_in_tree: true,
};
const TYPE_SEQUENCE: TypeProfile = TypeProfile {
    kind: ObjectKind::Sequence,
    folder: Some(NavFolder::Sequences),
    icon: Some("icons/list-ordered.svg"),
    is_container: false,
    shown_in_tree: true,
};
const TYPE_TRIGGER: TypeProfile = TypeProfile {
    kind: ObjectKind::Trigger,
    folder: Some(NavFolder::Triggers),
    icon: Some("icons/zap.svg"),
    is_container: false,
    shown_in_tree: true,
};
const TYPE_CATALOG: TypeProfile = TypeProfile {
    kind: ObjectKind::Catalog,
    folder: None,
    icon: None,
    is_container: true,
    shown_in_tree: true,
};
const TYPE_SCHEMA: TypeProfile = TypeProfile {
    kind: ObjectKind::Schema,
    folder: None,
    icon: None,
    is_container: true,
    shown_in_tree: true,
};

const FOLDER_TABLES: FolderProfile = FolderProfile {
    key: NavFolder::Tables,
    label: "表",
    order: 0,
};
const FOLDER_VIEWS: FolderProfile = FolderProfile {
    key: NavFolder::Views,
    label: "视图",
    order: 1,
};
const FOLDER_ROUTINES: FolderProfile = FolderProfile {
    key: NavFolder::Routines,
    label: "存储过程 / 函数",
    order: 2,
};
const FOLDER_SEQUENCES: FolderProfile = FolderProfile {
    key: NavFolder::Sequences,
    label: "序列",
    order: 3,
};
const FOLDER_TRIGGERS: FolderProfile = FolderProfile {
    key: NavFolder::Triggers,
    label: "触发器",
    order: 4,
};

// ---- D11 注释 / 定义搜索范围静态集合（口径见 `SearchScopeProfile` 文档） ----

const PG_SEARCH: SearchScopeProfile = SearchScopeProfile {
    comments: &[ObjectKind::Table, ObjectKind::View, ObjectKind::Column, ObjectKind::Routine],
    definitions: &[ObjectKind::Routine, ObjectKind::View],
};
const MYSQL_SEARCH: SearchScopeProfile = SearchScopeProfile {
    comments: &[ObjectKind::Table, ObjectKind::Column, ObjectKind::Routine],
    definitions: &[ObjectKind::Routine],
};
const SQLITE_SEARCH: SearchScopeProfile = SearchScopeProfile {
    comments: &[ObjectKind::Table, ObjectKind::Column],
    definitions: &[],
};
const DUCKDB_SEARCH: SearchScopeProfile = SearchScopeProfile {
    comments: &[ObjectKind::Table, ObjectKind::Column],
    definitions: &[],
};
const FALLBACK_SEARCH: SearchScopeProfile = SearchScopeProfile {
    comments: &[],
    definitions: &[],
};

// ---- D12 行内统计元素静态默认开关（口径见 `InlineStatsProfile` 文档） ----
// 对齐 DBeaver 默认：主机名 / 大小开，描述 / 子计数关；本地库无主机名。

const PG_INLINE: InlineStatsProfile = InlineStatsProfile {
    show_description: false,
    show_size: true,
    show_host_name: true,
    show_child_count: false,
};
const MYSQL_INLINE: InlineStatsProfile = InlineStatsProfile {
    show_description: false,
    show_size: true,
    show_host_name: true,
    show_child_count: false,
};
const SQLITE_INLINE: InlineStatsProfile = InlineStatsProfile {
    show_description: false,
    show_size: true,
    show_host_name: false,
    show_child_count: false,
};
const DUCKDB_INLINE: InlineStatsProfile = InlineStatsProfile {
    show_description: false,
    show_size: true,
    show_host_name: false,
    show_child_count: false,
};
const FALLBACK_INLINE: InlineStatsProfile = InlineStatsProfile {
    show_description: false,
    show_size: false,
    show_host_name: false,
    show_child_count: false,
};

/// PostgreSQL：独立 Schema 层，Catalog 连接作用域内，八类全量。
static POSTGRESQL: DbFamilyProfile = DbFamilyProfile {
    family: "postgresql",
    aliases: &["postgres", "pg", "postgres_native"],
    has_schema_level: true,
    catalog_is_connection_scoped: true,
    types: &[
        TYPE_CATALOG,
        TYPE_SCHEMA,
        TYPE_TABLE,
        TYPE_VIEW,
        TYPE_COLUMN,
        TYPE_ROUTINE,
        TYPE_SEQUENCE,
        TYPE_TRIGGER,
    ],
    folders: &[
        FOLDER_TABLES,
        FOLDER_VIEWS,
        FOLDER_ROUTINES,
        FOLDER_SEQUENCES,
        FOLDER_TRIGGERS,
    ],
    search: PG_SEARCH,
    inline: PG_INLINE,
};

/// MySQL：无独立 Schema 层（库即 Schema），不支持序列内省。
static MYSQL: DbFamilyProfile = DbFamilyProfile {
    family: "mysql",
    aliases: &["mysql_native"],
    has_schema_level: false,
    catalog_is_connection_scoped: false,
    types: &[
        TYPE_CATALOG,
        TYPE_SCHEMA,
        TYPE_TABLE,
        TYPE_VIEW,
        TYPE_COLUMN,
        TYPE_ROUTINE,
        TYPE_TRIGGER,
    ],
    folders: &[FOLDER_TABLES, FOLDER_VIEWS, FOLDER_ROUTINES, FOLDER_TRIGGERS],
    search: MYSQL_SEARCH,
    inline: MYSQL_INLINE,
};

/// SQLite：单库、无例程与序列内省。
static SQLITE: DbFamilyProfile = DbFamilyProfile {
    family: "sqlite",
    aliases: &[],
    has_schema_level: false,
    catalog_is_connection_scoped: false,
    types: &[TYPE_TABLE, TYPE_VIEW, TYPE_COLUMN, TYPE_TRIGGER],
    folders: &[FOLDER_TABLES, FOLDER_VIEWS, FOLDER_TRIGGERS],
    search: SQLITE_SEARCH,
    inline: SQLITE_INLINE,
};

/// DuckDB：固定 main、无例程与触发器内省。
static DUCKDB: DbFamilyProfile = DbFamilyProfile {
    family: "duckdb",
    aliases: &[],
    has_schema_level: false,
    catalog_is_connection_scoped: false,
    types: &[TYPE_TABLE, TYPE_VIEW, TYPE_COLUMN, TYPE_SEQUENCE],
    folders: &[FOLDER_TABLES, FOLDER_VIEWS, FOLDER_SEQUENCES],
    search: DUCKDB_SEARCH,
    inline: DUCKDB_INLINE,
};

/// 未知族回退通用档案：保底不藏对象，层级形态取宽口径（有 Schema 层）。
static FALLBACK: DbFamilyProfile = DbFamilyProfile {
    family: "fallback",
    aliases: &[],
    has_schema_level: true,
    catalog_is_connection_scoped: false,
    types: &[
        TYPE_CATALOG,
        TYPE_SCHEMA,
        TYPE_TABLE,
        TYPE_VIEW,
        TYPE_COLUMN,
        TYPE_ROUTINE,
        TYPE_SEQUENCE,
        TYPE_TRIGGER,
    ],
    folders: &[
        FOLDER_TABLES,
        FOLDER_VIEWS,
        FOLDER_ROUTINES,
        FOLDER_SEQUENCES,
        FOLDER_TRIGGERS,
    ],
    search: FALLBACK_SEARCH,
    inline: FALLBACK_INLINE,
};

/// 族档案注册表：内置四族 + 未知族回退档案。
#[derive(Default)]
pub struct FamilyRegistry {
    families: Vec<&'static DbFamilyProfile>,
}

impl FamilyRegistry {
    /// 注册一份族档案（新增数据库族在初始化时调用一次）。
    pub fn register(&mut self, profile: &'static DbFamilyProfile) {
        self.families.push(profile);
    }

    /// 族 id 归一：大小写不敏感；别名归并到规范族 id。
    fn normalize(type_id: &str) -> String {
        match type_id.trim().to_ascii_lowercase().as_str() {
            "postgres" | "pg" | "postgres_native" => "postgresql".to_string(),
            "mysql_native" => "mysql".to_string(),
            other => other.to_string(),
        }
    }

    /// 按族 id 查档案；别名归一命中，未注册族回退 [`FALLBACK`] 通用档案。
    pub fn family_of(&self, type_id: &str) -> &'static DbFamilyProfile {
        let key = Self::normalize(type_id);
        self.families
            .iter()
            .find(|profile| {
                profile.family == key || profile.aliases.contains(&key.as_str())
            })
            .copied()
            .unwrap_or(&FALLBACK)
    }

    /// 在指定族档案内查类型条目；未声明返回 `None`（B2 能力矩阵 / B4 动作门控消费）。
    pub fn profile_of(&self, type_id: &str, kind: ObjectKind) -> Option<&'static TypeProfile> {
        self.family_of(type_id)
            .types
            .iter()
            .find(|entry| entry.kind == kind)
    }

    /// 按族 id 查注释 / 定义搜索范围档案；别名归一命中，未注册族回退 FALLBACK（空集合）。
    pub fn family_search_scope(&self, type_id: &str) -> &'static SearchScopeProfile {
        &self.family_of(type_id).search
    }

    /// 按族 id 查行内统计元素开关档案；别名归一命中，未注册族回退 FALLBACK（全关）。
    pub fn family_inline_profile(&self, type_id: &str) -> &'static InlineStatsProfile {
        &self.family_of(type_id).inline
    }
}

/// 全局族档案注册表（首次访问时注册全部内置族）。
pub fn registry() -> &'static FamilyRegistry {
    static REG: OnceLock<FamilyRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut registry = FamilyRegistry::default();
        registry.register(&POSTGRESQL);
        registry.register(&MYSQL);
        registry.register(&SQLITE);
        registry.register(&DUCKDB);
        registry
    })
}

/// 族 × 注释 / 定义搜索范围（D11）：按族 id 查档案位；别名归一、未注册族回退空集合。
pub fn family_search_scope(type_id: &str) -> &'static SearchScopeProfile {
    &registry().family_search_scope(type_id)
}

/// 族 × 行内统计元素开关（D12）：按族 id 查档案位；别名归一、未注册族回退全关。
pub fn family_inline_profile(type_id: &str) -> &'static InlineStatsProfile {
    &registry().family_inline_profile(type_id)
}

/// 族 × catalog 是否连接作用域（2026-10-27 表格无数据根因修复）：
/// 为 true 的族（PostgreSQL）catalog = 当前连接 database，**SQL 标识符不能含 catalog 段**
/// —— 拼进去就是跨库引用（PG 无跨库，报「未实现跨数据库关联」）。
/// 导航把 database 放 catalog 层、schema 独立；执行 SQL 的限定名应跳过 catalog，只到
/// `schema.name`。为 false 的族（MySQL 库即 schema 等）保留 catalog 段。
pub fn family_catalog_is_connection_scoped(type_id: &str) -> bool {
    registry().family_of(type_id).catalog_is_connection_scoped
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 注册表归一与回退 ----

    #[test]
    fn family_of_normalizes_aliases_and_cases() {
        let reg = registry();
        assert_eq!(reg.family_of("postgres").family, "postgresql");
        assert_eq!(reg.family_of("PG").family, "postgresql");
        assert_eq!(reg.family_of("postgres_native").family, "postgresql");
        assert_eq!(reg.family_of("postgresql").family, "postgresql");
        assert_eq!(reg.family_of("mysql").family, "mysql");
        assert_eq!(reg.family_of("mysql_native").family, "mysql");
        assert_eq!(reg.family_of("sqlite").family, "sqlite");
        assert_eq!(reg.family_of("duckdb").family, "duckdb");
    }

    #[test]
    fn family_of_unknown_falls_back_to_general_profile() {
        let reg = registry();
        assert_eq!(reg.family_of("oracle").family, "fallback");
        assert!(reg.family_of("oracle").has_schema_level);
    }

    #[test]
    fn profile_of_hits_declared_types_only() {
        let reg = registry();
        // PostgreSQL 八类全量可查。
        for kind in [
            ObjectKind::Catalog,
            ObjectKind::Schema,
            ObjectKind::Table,
            ObjectKind::View,
            ObjectKind::Column,
            ObjectKind::Routine,
            ObjectKind::Sequence,
            ObjectKind::Trigger,
        ] {
            assert!(reg.profile_of("postgresql", kind).is_some(), "PG 应声明 {kind:?}");
        }
        // MySQL 档案不声明序列（内省能力位归 B2，此处仅档案事实）。
        assert!(reg.profile_of("mysql", ObjectKind::Sequence).is_none());
        assert!(reg.profile_of("mysql", ObjectKind::Table).is_some());
    }

    /// M9 反向映射：五个文件夹一一对应五类叶子身份（B3 方言 rename_sql 入参用）。
    #[test]
    fn folder_to_object_kind_matches_each_leaf_folder() {
        let cases = [
            (NavFolder::Tables, ObjectKind::Table),
            (NavFolder::Views, ObjectKind::View),
            (NavFolder::Routines, ObjectKind::Routine),
            (NavFolder::Sequences, ObjectKind::Sequence),
            (NavFolder::Triggers, ObjectKind::Trigger),
        ];
        for (folder, kind) in cases {
            assert_eq!(folder_to_object_kind(folder), kind, "{folder:?} 应映射到 {kind:?}");
            // 与正向映射闭环：叶子类别经 folder 再回 kind 一致（Column 归并 Tables 的特例除外）。
            assert_eq!(object_kind_to_folder(kind), Some(folder));
        }
    }

    // ---- M1 文件夹 → 形状 / 子节点 ----

    #[test]
    fn folder_to_shape_matches_legacy_match() {
        let cases = [
            (NavFolder::Tables, NodeShape::Table, true),
            (NavFolder::Views, NodeShape::View, true),
            (NavFolder::Routines, NodeShape::Routine, false),
            (NavFolder::Sequences, NodeShape::Sequence, false),
            (NavFolder::Triggers, NodeShape::Trigger, false),
        ];
        for (folder, shape, has_children) in cases {
            assert_eq!(folder_to_shape(folder), (shape, has_children));
        }
    }

    // ---- M2 / M7 / M6 / M9 身份字典 ----

    #[test]
    fn folder_to_property_kind_matches_legacy_match() {
        let cases = [
            (NavFolder::Tables, PropertyKind::Table),
            (NavFolder::Views, PropertyKind::View),
            (NavFolder::Routines, PropertyKind::Routine),
            (NavFolder::Sequences, PropertyKind::Sequence),
            (NavFolder::Triggers, PropertyKind::Trigger),
        ];
        for (folder, kind) in cases {
            assert_eq!(folder_to_property_kind(folder), kind);
        }
    }

    #[test]
    fn object_kind_to_property_kind_is_same_name_bijection() {
        let cases = [
            (ObjectKind::Catalog, PropertyKind::Catalog),
            (ObjectKind::Schema, PropertyKind::Schema),
            (ObjectKind::Table, PropertyKind::Table),
            (ObjectKind::View, PropertyKind::View),
            (ObjectKind::Column, PropertyKind::Column),
            (ObjectKind::Routine, PropertyKind::Routine),
            (ObjectKind::Sequence, PropertyKind::Sequence),
            (ObjectKind::Trigger, PropertyKind::Trigger),
        ];
        for (kind, prop) in cases {
            assert_eq!(object_kind_to_property_kind(kind), prop);
        }
    }

    #[test]
    fn property_kind_to_folder_covers_only_leaf_folders() {
        let some_cases = [
            (PropertyKind::Table, Some(NavFolder::Tables)),
            (PropertyKind::View, Some(NavFolder::Views)),
            (PropertyKind::Routine, Some(NavFolder::Routines)),
            (PropertyKind::Sequence, Some(NavFolder::Sequences)),
            (PropertyKind::Trigger, Some(NavFolder::Triggers)),
        ];
        for (kind, folder) in some_cases {
            assert_eq!(property_kind_to_folder(kind), folder);
        }
        for kind in [
            PropertyKind::Connection,
            PropertyKind::Catalog,
            PropertyKind::Schema,
            PropertyKind::Column,
        ] {
            assert_eq!(property_kind_to_folder(kind), None);
        }
    }

    #[test]
    fn object_kind_to_folder_merges_column_into_tables() {
        assert_eq!(
            object_kind_to_folder(ObjectKind::Table),
            Some(NavFolder::Tables)
        );
        assert_eq!(
            object_kind_to_folder(ObjectKind::Column),
            Some(NavFolder::Tables)
        );
        assert_eq!(
            object_kind_to_folder(ObjectKind::View),
            Some(NavFolder::Views)
        );
        assert_eq!(
            object_kind_to_folder(ObjectKind::Routine),
            Some(NavFolder::Routines)
        );
        assert_eq!(
            object_kind_to_folder(ObjectKind::Sequence),
            Some(NavFolder::Sequences)
        );
        assert_eq!(
            object_kind_to_folder(ObjectKind::Trigger),
            Some(NavFolder::Triggers)
        );
        assert_eq!(object_kind_to_folder(ObjectKind::Schema), None);
        assert_eq!(object_kind_to_folder(ObjectKind::Catalog), None);
    }

    // ---- M5 命中类别 → 展示节点 ----

    #[test]
    fn search_hit_node_kind_matches_legacy_match() {
        assert_eq!(
            search_hit_node_kind(ObjectKind::Table),
            NavNodeKind::Table { row_estimate: None }
        );
        assert_eq!(search_hit_node_kind(ObjectKind::View), NavNodeKind::View);
        assert_eq!(
            search_hit_node_kind(ObjectKind::Column),
            NavNodeKind::Column {
                data_type: String::new(),
                nullable: true,
                primary: false,
                foreign: false,
            }
        );
        for kind in [
            ObjectKind::Catalog,
            ObjectKind::Schema,
            ObjectKind::Routine,
            ObjectKind::Sequence,
            ObjectKind::Trigger,
        ] {
            assert_eq!(search_hit_node_kind(kind), NavNodeKind::Schema);
        }
    }

    // ---- M3 对象类型图标 ----

    #[test]
    fn object_kind_icon_covers_leaf_objects_only() {
        assert_eq!(object_kind_icon(ObjectKind::Table), Some("icons/table.svg"));
        assert_eq!(object_kind_icon(ObjectKind::View), Some("icons/eye.svg"));
        assert_eq!(
            object_kind_icon(ObjectKind::Routine),
            Some("icons/square-function.svg")
        );
        assert_eq!(
            object_kind_icon(ObjectKind::Sequence),
            Some("icons/list-ordered.svg")
        );
        assert_eq!(object_kind_icon(ObjectKind::Trigger), Some("icons/zap.svg"));
        for kind in [
            ObjectKind::Catalog,
            ObjectKind::Schema,
            ObjectKind::Column,
        ] {
            assert_eq!(object_kind_icon(kind), None);
        }
    }

    // ---- D11 注释 / 定义搜索范围档案位 ----

    #[test]
    fn family_search_scope_matches_pinned_matrix() {
        let reg = registry();
        let contains = |scope: &[ObjectKind], kind: ObjectKind| scope.contains(&kind);

        // PostgreSQL：注释覆盖表 / 视图 / 列 / 例程；定义覆盖例程 / 视图。
        let pg = reg.family_search_scope("postgresql");
        for kind in [ObjectKind::Table, ObjectKind::View, ObjectKind::Column, ObjectKind::Routine] {
            assert!(contains(pg.comments, kind), "PG 注释应覆盖 {kind:?}");
        }
        for kind in [ObjectKind::Routine, ObjectKind::View] {
            assert!(contains(pg.definitions, kind), "PG 定义应覆盖 {kind:?}");
        }
        assert!(!contains(pg.definitions, ObjectKind::Table));

        // MySQL：注释覆盖表 / 列 / 例程；定义仅例程（视图不进定义集合）。
        let mysql = reg.family_search_scope("mysql");
        for kind in [ObjectKind::Table, ObjectKind::Column, ObjectKind::Routine] {
            assert!(contains(mysql.comments, kind), "MySQL 注释应覆盖 {kind:?}");
        }
        assert!(contains(mysql.definitions, ObjectKind::Routine));
        assert!(!contains(mysql.definitions, ObjectKind::View));

        // SQLite / DuckDB：注释覆盖表 / 列；定义集合为空（内省未采集定义）。
        for family in ["sqlite", "duckdb"] {
            let scope = reg.family_search_scope(family);
            assert!(contains(scope.comments, ObjectKind::Table));
            assert!(contains(scope.comments, ObjectKind::Column));
            assert!(scope.definitions.is_empty(), "{family} 定义集合应为空");
        }

        // 未知族回退：注释 / 定义集合均为空（不默认开放未验证维度）。
        let fallback = reg.family_search_scope("oracle");
        assert!(fallback.comments.is_empty());
        assert!(fallback.definitions.is_empty());
    }

    #[test]
    fn family_search_scope_resolves_aliases() {
        // 别名归一后查到对应族范围（与 family_of 归一同一套规则）。
        assert_eq!(
            registry().family_search_scope("postgres_native").comments,
            registry().family_search_scope("postgresql").comments
        );
        assert_eq!(
            registry().family_search_scope("mysql_native").definitions,
            registry().family_search_scope("mysql").definitions
        );
    }

    // ---- D12 行内统计元素开关档案位 ----

    #[test]
    fn family_inline_profile_matches_pinned_matrix() {
        let reg = registry();

        // PostgreSQL / MySQL：主机名、大小默认开；描述、子计数默认关（对齐 DBeaver 默认）。
        for family in ["postgresql", "mysql"] {
            let inline = reg.family_inline_profile(family);
            assert!(inline.show_host_name, "{family} 主机名应默认开");
            assert!(inline.show_size, "{family} 大小应默认开");
            assert!(!inline.show_description, "{family} 描述应默认关");
            assert!(!inline.show_child_count, "{family} 子计数应默认关");
        }

        // SQLite / DuckDB：本地库无主机名（关）；大小默认开；描述、子计数关。
        for family in ["sqlite", "duckdb"] {
            let inline = reg.family_inline_profile(family);
            assert!(!inline.show_host_name, "{family} 无主机名应默认关");
            assert!(inline.show_size, "{family} 大小应默认开");
            assert!(!inline.show_description, "{family} 描述应默认关");
            assert!(!inline.show_child_count, "{family} 子计数应默认关");
        }

        // 未知族回退：四项全关（不默认开放未验证维度）。
        let fallback = reg.family_inline_profile("oracle");
        assert!(!fallback.show_host_name);
        assert!(!fallback.show_size);
        assert!(!fallback.show_description);
        assert!(!fallback.show_child_count);
    }

    #[test]
    fn family_inline_profile_resolves_aliases() {
        // 别名归一后查到对应族开关（与 family_of 归一同一套规则）。
        assert_eq!(
            registry().family_inline_profile("postgres_native"),
            registry().family_inline_profile("postgresql")
        );
        assert_eq!(
            registry().family_inline_profile("mysql_native"),
            registry().family_inline_profile("mysql")
        );
    }
}

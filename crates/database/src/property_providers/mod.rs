//! 属性源注册（M4 数据生产者侧）——各数据库类型 × 对象类型的 [`PropertyProvider`] 实现。
//!
//! 对齐 DBeaver 插件机制：`ext.postgresql` / `ext.mysql` 各实现 `DBPPropertySource`，
//! 属性面板 UI 零分支。RDS 对应物 = 本目录每数据库类型一个子模块，实现 `PropertyProvider`，
//! 经 [`global_registry`] 注册——**新增数据库只加一个 provider 子模块**，
//! `property` crate 模型 / `property_panel` 通用骨架 / workbench 渲染层零改动。
//!
//! 分派键是**数据库类型 id**（`drivers.type_id`，如 `postgresql`），不是驱动 id——
//! 同一个数据库族可有多个驱动实现（`postgres_native` / JDBC…），一族共享一个 provider。
//!
//! 布局：`docs/architecture/property/property-module-spec.md` §16（本目录为 §16.2）。

pub mod duckdb;
pub mod mysql;
pub mod pg;
pub mod sqlite;

use property::{
    ObjectOverview, OverviewCard, OverviewStatus, PageId, PropertyRow, PropertySection,
    PropertyTargetRef, PropertyValueKind, ProviderInput, ProviderRegistry,
};
use std::sync::OnceLock;

/// 全局属性源注册表（首次访问时注册全部内置数据库类型 provider）。
///
/// 新增数据库类型 provider = 在 `register_all` 里加一行；**不需要改任何消费方**。
pub fn global_registry() -> &'static ProviderRegistry {
    static REG: OnceLock<ProviderRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut registry = ProviderRegistry::default();
        pg::register_all(&mut registry);
        sqlite::register_all(&mut registry);
        duckdb::register_all(&mut registry);
        mysql::register_all(&mut registry);
        registry
    })
}

/// 只读文本行（provider 属性行辅助）。
pub(crate) fn row(label: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow::read_only(label, value)
}

/// 可编辑属性行（id = 提交映射 SQL 的键）。
pub(crate) fn editable_row(
    id: &str,
    label: &str,
    value: impl Into<String>,
    kind: PropertyValueKind,
    options: Vec<String>,
) -> PropertyRow {
    PropertyRow {
        id: id.to_string(),
        label: label.to_string(),
        category: None,
        value: value.into(),
        editable: true,
        value_kind: kind,
        select_options: options,
    }
}

/// 空表（灰置类别 / 无内容类别用）。
pub(crate) fn empty_table() -> property::PropertyTable {
    property::PropertyTable {
        headers: Vec::new(),
        rows: Vec::new(),
    }
}

/// 目标对象名（限定名：catalog.schema.name，缺段跳过）。
pub(crate) fn qualified_name(t: &PropertyTargetRef) -> String {
    crate::sql_gen::qualified_name(t.catalog.as_deref(), t.schema.as_deref(), &t.name)
}

/// 基础连接信息行（各驱动通用：名称 / 数据库类型 / 归属域 / 驱动）。
pub(crate) fn connection_base_rows(
    input: &ProviderInput,
    driver_label: &str,
) -> Vec<PropertyRow> {
    let mut rows = vec![row("名称", input.conn_label)];
    if let Some(t) = input.db_type {
        if !t.is_empty() {
            rows.push(row("数据库类型", t));
        }
    }
    rows.push(row("归属域", &input.target.source));
    rows.push(row("驱动", driver_label));
    rows
}

/// 基础连接洞察（状态 + 驱动 / 作用域 chips；各驱动通用）。
pub(crate) fn connection_base_overview(input: &ProviderInput, driver_label: &str) -> ObjectOverview {
    // 状态：调用方 counts 带 `connected` 则按值，否则按「已加载」乐观展示；
    // 失败路径（error）由渲染层展示，不进 overview。
    let (status, status_text) = match input.count("connected") {
        Some("true") => (OverviewStatus::Connected, "已连接".to_string()),
        Some("false") => (OverviewStatus::Disconnected, "已断开".to_string()),
        _ => (OverviewStatus::Connected, "已连接".to_string()),
    };
    ObjectOverview {
        title: input.conn_label.to_string(),
        subtitle: format!("{driver_label} 数据库连接"),
        status,
        status_text,
        cards: vec![],
        kv: vec![
            ("驱动".to_string(), driver_label.to_string()),
            ("作用域".to_string(), input.target.source.clone()),
            ("驱动模式".to_string(), "直连（空闲回收）".to_string()),
        ],
        note: "对象统计随导航树展开懒加载。".to_string(),
        meta: vec![
            ("驱动".to_string(), driver_label.to_string()),
            ("作用域".to_string(), input.target.source.clone()),
        ],
    }
}

/// 连接级灰置分区（权限 / 事件触发器——先绘制，能力未实现）。
///
/// 2026-10-24 起按 [`DriverCapabilities`] 生成：驱动差异（是否声明该类别、是否已
/// 实现）只体现在能力位，类别结构与顺序只写一份。
pub(crate) fn connection_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    section_list(&caps.connection, &[
        ("存储", PageId::Storage),
        ("权限", PageId::Privileges),
        ("事件触发器", PageId::EventTrigger),
    ])
}

/// 表级类别：可用核心（列 / 索引 / 约束 / DDL / 数据）+ 按能力位声明的灰置 / 已启用类别。
///
/// 收敛来源（2026-10-24，竞品分析必做 #3）：旧实现各 provider `categories()` 手写
/// `PropertySection::disabled(...)`，pg / sqlite / mysql / duckdb 结构漂移（触发器在
/// pg 挂 `Source`、sqlite 挂 `Dependencies`，连接「存储 / 事件触发器」都挂
/// `Dependencies`——类别 id 错位）；统一为声明池 + 能力位，新增驱动填一份 caps 即可。
pub(crate) fn table_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    let mut v = vec![
        PropertySection { id: PageId::Columns, label: "列".into(), enabled: true, table: empty_table() },
        PropertySection { id: PageId::Indexes, label: "索引".into(), enabled: true, table: empty_table() },
        PropertySection { id: PageId::Constraints, label: "约束".into(), enabled: true, table: empty_table() },
        PropertySection { id: PageId::Ddl, label: "DDL".into(), enabled: true, table: empty_table() },
        PropertySection { id: PageId::Data, label: "数据".into(), enabled: true, table: empty_table() },
    ];
    v.extend(section_list(&caps.table, &[
        ("触发器", PageId::Trigger),
        ("权限", PageId::Privileges),
        ("依赖", PageId::Dependencies),
        ("规则", PageId::Routines),
        ("分区", PageId::Partitions),
    ]));
    v
}

/// 视图级类别：可用核心（列 / DDL / 数据）+ 按能力位声明的灰置 / 已启用类别。
pub(crate) fn view_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    let mut v = vec![
        PropertySection { id: PageId::Columns, label: "列".into(), enabled: true, table: empty_table() },
        PropertySection { id: PageId::Ddl, label: "DDL".into(), enabled: true, table: empty_table() },
        PropertySection { id: PageId::Data, label: "数据".into(), enabled: true, table: empty_table() },
    ];
    v.extend(section_list(&caps.view, &[
        ("权限", PageId::Privileges),
    ]));
    v
}

/// Schema 级类别：仅按能力位声明的灰置 / 已启用类别（内容批次未接入前，
/// Schema 的表 / 视图 / 例程等子项类别不声明，见 pg.rs 的说明）。
pub(crate) fn schema_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    section_list(&caps.schema, &[
        ("权限", PageId::Privileges),
        ("依赖", PageId::Dependencies),
    ])
}

/// 按能力位生成一组类别：`Some(true)` = 已实现（enabled）；`Some(false)` = 灰置声明；
/// `None` = 该驱动不声明此类别。
fn section_list(
    caps: &CapSet,
    decls: &[(&'static str, PageId)],
) -> Vec<PropertySection> {
    let mut v = Vec::new();
    for (label, id) in decls {
        match caps.get(*id) {
            Some(true) => v.push(PropertySection {
                id: *id,
                label: (*label).to_string(),
                enabled: true,
                table: empty_table(),
            }),
            Some(false) => v.push(PropertySection::disabled(*id, label)),
            None => {}
        }
    }
    v
}

/// 一组「类别 → 能力」的映射（`Option<bool>`：已实现 / 灰置 / 不声明）。
///
/// 收敛设计：不按对象层级拆多个 struct（触发器只在表级、存储只在连接级，跨层级
/// 复用的权限 / 依赖也不重复声明），统一一张表按 [`PageId`] 查。
#[derive(Debug, Clone, Default)]
pub struct CapSet {
    map: std::collections::HashMap<PageId, bool>,
}

impl CapSet {
    fn set(mut self, id: PageId, enabled: bool) -> Self {
        self.map.insert(id, enabled);
        self
    }
    fn get(&self, id: PageId) -> Option<bool> {
        self.map.get(&id).copied()
    }
}

/// 驱动能力位——收敛 provider 里散落的布尔判断（类别是否可用 / 列展示是否支持），
/// 统一从这里读。新增数据库类型 = 填一份 caps；类别结构与顺序见各 `*_categories`
/// 生成函数，不随 provider 重复手写。
#[derive(Debug, Clone, Default)]
pub struct DriverCapabilities {
    /// 列「标识」列（PG `attidentity`、MySQL `EXTRA auto_increment`；SQLite /
    /// DuckDB 无原生概念 → false，`columns_table` 按此裁剪列）。
    pub identity_column: bool,
    /// 列「排序规则」列（PG `attcollation` → `pg_collation.collname`、MySQL
    /// `COLLATION_NAME`；SQLite / DuckDB → false）。
    pub collation_column: bool,
    /// 表级类别能力（触发器 / 权限 / 依赖 / 规则 / 分区——见 [`table_categories`]）。
    pub table: CapSet,
    /// 视图级类别能力（权限）。
    pub view: CapSet,
    /// Schema 级类别能力（权限 / 依赖）。
    pub schema: CapSet,
    /// 连接级类别能力（存储 / 权限 / 事件触发器）。
    pub connection: CapSet,
}

impl DriverCapabilities {
    /// PostgreSQL：唯一有「标识 / 排序规则」真值的驱动；表级 触发器 / 规则 / 分区、
    /// 连接级 存储 / 事件触发器、Schema 级 权限 / 依赖 等对象存在但**内容批次未接入**
    /// → 声明为灰置（`Some(false)`，展示策略保持现状：灰置先绘制，用户拍板后再定
    /// 灰置 vs 隐藏）。
    pub fn pg() -> Self {
        Self {
            identity_column: true,
            collation_column: true,
            table: CapSet::default()
                .set(PageId::Trigger, false)
                .set(PageId::Privileges, false)
                .set(PageId::Dependencies, false)
                .set(PageId::Routines, false)
                .set(PageId::Partitions, false),
            view: CapSet::default().set(PageId::Privileges, false),
            schema: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::Dependencies, false),
            connection: CapSet::default()
                .set(PageId::Storage, false)
                .set(PageId::Privileges, false)
                .set(PageId::EventTrigger, false),
        }
    }

    /// MySQL：有「标识 / 排序规则」真值；表级 权限 / 依赖 灰置，触发器 / 规则 /
    /// 分区等类别不声明。
    pub fn mysql() -> Self {
        Self {
            identity_column: true,
            collation_column: true,
            table: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::Dependencies, false),
            view: CapSet::default(),
            schema: CapSet::default(),
            connection: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::EventTrigger, false),
        }
    }

    /// SQLite：无「标识 / 排序规则」列；表级 权限 / 触发器 灰置（文件库无
    /// 权限体系，权限类别沿原型声明，语义标注见旧实现）；其余类别不声明。
    pub fn sqlite() -> Self {
        Self {
            identity_column: false,
            collation_column: false,
            table: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::Trigger, false),
            view: CapSet::default().set(PageId::Privileges, false),
            schema: CapSet::default(),
            connection: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::EventTrigger, false),
        }
    }

    /// DuckDB：无「标识 / 排序规则」列；表级 权限 / 依赖 灰置。
    pub fn duckdb() -> Self {
        Self {
            identity_column: false,
            collation_column: false,
            table: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::Dependencies, false),
            view: CapSet::default().set(PageId::Privileges, false),
            schema: CapSet::default(),
            connection: CapSet::default()
                .set(PageId::Privileges, false)
                .set(PageId::EventTrigger, false),
        }
    }
}

/// 按数据库类型 id 取能力位（`columns_table` / `sections_of` 的裁剪入口）。
/// 未登记的类型（oracle 等桥接 / 未来驱动）→ 全能力关（保守，不展示标识 / 排序规则列）。
pub(crate) fn caps_for(db_type: Option<&str>) -> DriverCapabilities {
    match db_type {
        Some("postgresql") => DriverCapabilities::pg(),
        Some("mysql") => DriverCapabilities::mysql(),
        Some("sqlite") => DriverCapabilities::sqlite(),
        Some("duckdb") => DriverCapabilities::duckdb(),
        _ => DriverCapabilities::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 能力矩阵收敛：表级类别声明集按驱动区分——pg 声明 触发器/权限/依赖/规则/分区
    /// 全灰置；mysql / duckdb 只声明 权限/依赖；sqlite 只声明 权限/触发器。类别结构与
    /// 顺序由生成函数统一维护，驱动差异只体现在 caps。
    #[test]
    fn table_categories_declaration_sets_follow_caps() {
        let pg: Vec<PageId> = table_categories(&DriverCapabilities::pg())
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(pg.contains(&PageId::Trigger), "pg 表级声明触发器（原型类别）");
        assert!(pg.contains(&PageId::Routines), "pg 表级声明规则（原型类别）");
        assert!(pg.contains(&PageId::Partitions), "pg 表级声明分区（原型类别）");

        let my: Vec<PageId> = table_categories(&DriverCapabilities::mysql())
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(my.contains(&PageId::Privileges) && my.contains(&PageId::Dependencies));
        assert!(!my.contains(&PageId::Trigger), "mysql 不声明触发器类别");
        assert!(!my.contains(&PageId::Partitions));

        let sq: Vec<PageId> = table_categories(&DriverCapabilities::sqlite())
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(sq.contains(&PageId::Trigger), "sqlite 表级声明触发器（沿旧实现）");
        assert!(!sq.contains(&PageId::Routines), "sqlite 不声明规则类别");
        assert!(!sq.contains(&PageId::Partitions));

        // 未接入的类别全部灰置（enabled=false）；可用核心（列/索引/约束/DDL/数据）常驻。
        let cats = table_categories(&DriverCapabilities::pg());
        assert!(cats.iter().any(|c| c.id == PageId::Columns && c.enabled));
        assert!(cats.iter().all(|c| !c.enabled || matches!(c.id, PageId::Columns
            | PageId::Indexes | PageId::Constraints | PageId::Ddl | PageId::Data)));
    }

    /// 连接级：pg 声明 存储/权限/事件触发器 三灰置；sqlite / mysql / duckdb 无「存储」类别。
    #[test]
    fn connection_categories_follow_caps() {
        let pg: Vec<PageId> = connection_categories(&DriverCapabilities::pg())
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(pg.contains(&PageId::Storage), "pg 连接级声明存储");
        assert!(pg.contains(&PageId::EventTrigger), "pg 连接级声明事件触发器");

        let sq: Vec<PageId> = connection_categories(&DriverCapabilities::sqlite())
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(!sq.contains(&PageId::Storage), "sqlite 不声明存储类别");
        assert!(sq.contains(&PageId::EventTrigger));
    }

    /// 能力位驱动列展示：pg / mysql 有标识 / 排序规则；sqlite / duckdb 无。
    #[test]
    fn column_capabilities_per_driver() {
        assert!(DriverCapabilities::pg().identity_column);
        assert!(DriverCapabilities::pg().collation_column);
        assert!(DriverCapabilities::mysql().identity_column);
        assert!(!DriverCapabilities::sqlite().identity_column);
        assert!(!DriverCapabilities::duckdb().collation_column);
        // 未登记驱动（oracle 等桥接）→ 全能力关（保守）。
        assert!(!caps_for(Some("oracle")).identity_column);
        assert!(!caps_for(None).collation_column);
    }
}

/// 对象计数卡（stats 有值才显示）。
pub(crate) fn stats_cards(input: &ProviderInput) -> Vec<OverviewCard> {
    let mut cards = Vec::new();
    if input.stats.column_count > 0 {
        cards.push(OverviewCard {
            label: "列".to_string(),
            value: input.stats.column_count.to_string(),
            unit: String::new(),
            delta: None,
        });
    }
    if input.stats.index_count > 0 {
        cards.push(OverviewCard {
            label: "索引".to_string(),
            value: input.stats.index_count.to_string(),
            unit: String::new(),
            delta: None,
        });
    }
    if input.stats.constraint_count > 0 {
        cards.push(OverviewCard {
            label: "约束".to_string(),
            value: input.stats.constraint_count.to_string(),
            unit: String::new(),
            delta: None,
        });
    }
    cards
}

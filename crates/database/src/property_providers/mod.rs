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
    ObjectOverview, OverviewCard, OverviewStatus, PageId, PropertyKind, PropertyRow,
    PropertySection, PropertyTable, PropertyTargetRef, PropertyValueKind, ProviderInput,
    ProviderRegistry, ProviderStats,
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

/// 空表（无内容类别用）。
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

/// 基础连接信息行（各驱动通用，2026-10-24 起接真值）：
/// 名称 / 数据库类型 / 归属域 / 驱动 + 调用方 counts 带值才加的
/// 版本 / 文件路径 / 连接串 / 描述。
///
/// 认证 / 网络 / 环境自 2026-10-25（P0-3 深化）起由 [`connection_meta_sections`]
/// 独立分节承载（方法 / 引用名 / 策略覆盖），不再混入常规行。
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
    if let Some(v) = input.count("version") {
        if !v.is_empty() {
            rows.push(row("版本", v));
        }
    }
    if let Some(v) = input.count("path") {
        if !v.is_empty() {
            rows.push(row("文件路径", v));
        }
    }
    if let Some(v) = input.count("url") {
        if !v.is_empty() {
            rows.push(row("连接串", v));
        }
    }
    if let Some(v) = input.count("description") {
        if !v.is_empty() {
            rows.push(row("描述", v));
        }
    }
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

/// 连接级类别（按 [`DriverCapabilities`] 生成：已实现的产出分区，未实现不产出）。
///
/// 2026-10-24 起按 [`DriverCapabilities`] 生成：驱动差异（是否声明该类别、是否已
/// 实现）只体现在能力位，类别结构与顺序只写一份。
/// 2026-10-25（P0-3 深化）：认证 / 网络 / 环境分节由 [`connection_meta_sections`]
/// 按 counts（引用存在与否）追加，非能力位驱动。
pub(crate) fn connection_categories(
    caps: &DriverCapabilities,
    input: &ProviderInput,
) -> Vec<PropertySection> {
    let mut v = section_list(&caps.connection, &[
        ("存储", PageId::Storage),
        ("权限", PageId::Privileges),
        ("事件触发器", PageId::EventTrigger),
    ]);
    v.extend(connection_meta_sections(input));
    v
}

/// 连接对象「认证 / 网络 / 环境」分节（P0-3 深化，2026-10-25）。
///
/// 引用字段语义对齐 DBeaver：显示来源**名称**（解析失败时回落引用 ID）且只读；
/// 有对应 counts 才产出分节（无引用的密码认证只有认证方法一行）。
pub(crate) fn connection_meta_sections(input: &ProviderInput) -> Vec<PropertySection> {
    let mut v = Vec::new();
    // 认证：方法 + 引用档案（无档案时仅方法）。
    if input.count("auth_method").is_some() || input.count("auth_ref").is_some() {
        let mut rows: Vec<Vec<String>> = Vec::new();
        if let Some(m) = input.count("auth_method") {
            if !m.is_empty() {
                rows.push(vec!["认证方法".into(), m.to_string()]);
            }
        }
        if let Some(r) = input.count("auth_ref") {
            if !r.is_empty() {
                rows.push(vec!["引用档案".into(), input.count("auth_name").unwrap_or(r).to_string()]);
            }
        }
        v.push(PropertySection {
            id: PageId::Auth,
            label: "认证".into(),
            table: PropertyTable {
                headers: vec!["项".into(), "值".into()],
                rows,
            },
        });
    }
    // 网络：档案名 + 类型。
    if let Some(r) = input.count("network_ref") {
        if !r.is_empty() {
            let mut rows: Vec<Vec<String>> = Vec::new();
            rows.push(vec![
                "网络档案".into(),
                input.count("network_name").unwrap_or(r).to_string(),
            ]);
            if let Some(t) = input.count("network_type") {
                if !t.is_empty() {
                    rows.push(vec!["网络类型".into(), t.to_string()]);
                }
            }
            v.push(PropertySection {
                id: PageId::Network,
                label: "网络".into(),
                table: PropertyTable {
                    headers: vec!["项".into(), "值".into()],
                    rows,
                },
            });
        }
    }
    // 环境：名称 + 标识色 + 策略覆盖标记。
    if let Some(r) = input.count("env_ref") {
        if !r.is_empty() {
            let mut rows: Vec<Vec<String>> = Vec::new();
            rows.push(vec![
                "环境名称".into(),
                input.count("env_name").unwrap_or(r).to_string(),
            ]);
            if let Some(c) = input.count("env_color") {
                if !c.is_empty() {
                    rows.push(vec!["标识色".into(), c.to_string()]);
                }
            }
            if let Some(p) = input.count("env_policies") {
                if !p.is_empty() {
                    rows.push(vec!["策略覆盖".into(), p.to_string()]);
                }
            }
            v.push(PropertySection {
                id: PageId::Environment,
                label: "环境".into(),
                table: PropertyTable {
                    headers: vec!["项".into(), "值".into()],
                    rows,
                },
            });
        }
    }
    v
}

/// 表级类别：可用核心（列 / 索引 / 约束 / DDL / 数据）+ 按能力位声明的已实现类别。
///
/// 收敛来源（2026-10-24，竞品分析必做 #3）：旧实现各 provider `categories()` 手写
/// `PropertySection::disabled(...)`，pg / sqlite / mysql / duckdb 结构漂移（触发器在
/// pg 挂 `Source`、sqlite 挂 `Dependencies`，连接「存储 / 事件触发器」都挂
/// `Dependencies`——类别 id 错位）；统一为声明池 + 能力位，新增驱动填一份 caps 即可。
///
/// 展示策略（2026-10-24 拍板）：**未实现类别不产出**——`Some(false)` 只作接线位
/// （caps 注释说明未来接入点），不进 `sections`；渲染层不再有灰置分支。
pub(crate) fn table_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    let mut v = vec![
        PropertySection { id: PageId::Columns, label: "列".into(), table: empty_table() },
        PropertySection { id: PageId::Indexes, label: "索引".into(), table: empty_table() },
        PropertySection { id: PageId::Constraints, label: "约束".into(), table: empty_table() },
        PropertySection { id: PageId::Ddl, label: "DDL".into(), table: empty_table() },
        PropertySection { id: PageId::Data, label: "数据".into(), table: empty_table() },
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

/// 视图级类别：可用核心（列 / DDL / 数据）+ 按能力位声明的已实现类别。
pub(crate) fn view_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    let mut v = vec![
        PropertySection { id: PageId::Columns, label: "列".into(), table: empty_table() },
        PropertySection { id: PageId::Ddl, label: "DDL".into(), table: empty_table() },
        PropertySection { id: PageId::Data, label: "数据".into(), table: empty_table() },
    ];
    v.extend(section_list(&caps.view, &[
        ("权限", PageId::Privileges),
    ]));
    v
}

/// Schema 级类别：仅按能力位声明的已实现类别（内容批次未接入前，
/// Schema 的表 / 视图 / 例程等子项类别不声明，见 pg.rs 的说明）。
pub(crate) fn schema_categories(caps: &DriverCapabilities) -> Vec<PropertySection> {
    section_list(&caps.schema, &[
        ("权限", PageId::Privileges),
        ("依赖", PageId::Dependencies),
    ])
}

/// 按能力位生成一组类别：`Some(true)` = 已实现（产出分区）；`Some(false)` = 未实现
/// **不产出**（接线位，仅 caps 注释承载）；`None` = 该驱动不声明此类别。
fn section_list(
    caps: &CapSet,
    decls: &[(&'static str, PageId)],
) -> Vec<PropertySection> {
    let mut v = Vec::new();
    for (label, id) in decls {
        if caps.get(*id) == Some(true) {
            v.push(PropertySection {
                id: *id,
                label: (*label).to_string(),
                table: empty_table(),
            });
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
    /// → 声明 `Some(false)`（接线位：展示策略 2026-10-24 拍板隐藏——未实现类别不产出
    /// 页签，能力接入时把对应位翻 `true` 即自动出现）。
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

    /// 展示策略（2026-10-24 拍板隐藏式）：未实现类别**不产出**页签——pg 表级只剩
    /// 可用核心（列/索引/约束/DDL/数据），触发器/权限/依赖/规则/分区都不出现；
    /// 能力接线位保留在 caps（`get` 返回 `Some(false)`，翻 `true` 即自动产出）。
    #[test]
    fn table_categories_hide_unimplemented_and_keep_caps() {
        let pg = table_categories(&DriverCapabilities::pg());
        let ids: Vec<PageId> = pg.iter().map(|c| c.id).collect();
        assert!(!ids.contains(&PageId::Trigger), "触发器未实现应隐藏");
        assert!(!ids.contains(&PageId::Routines));
        assert!(!ids.contains(&PageId::Partitions));
        assert!(!ids.contains(&PageId::Privileges));
        assert!(ids.contains(&PageId::Columns) && ids.contains(&PageId::Data));
        // 接线位不丢：声明仍在 caps 里。
        let caps = DriverCapabilities::pg();
        assert_eq!(caps.table.get(PageId::Trigger), Some(false));
        assert_eq!(caps.table.get(PageId::Routines), Some(false));
        // 驱动差异仍体现在声明集：mysql 表级不声明触发器；sqlite 声明触发器但未实现。
        assert_eq!(DriverCapabilities::mysql().table.get(PageId::Trigger), None);
        assert_eq!(DriverCapabilities::sqlite().table.get(PageId::Trigger), Some(false));
        // 已实现声明（Some(true)）才产出——当前无任何 Some(true)，断言总数为核心数。
        assert_eq!(ids.len(), 5, "pg 表级只有 5 个可用核心: {ids:?}");
    }

    /// 连接级：pg 声明 存储/权限/事件触发器 但未实现 → 全部隐藏（空）；接线位保留。
    #[test]
    fn connection_categories_hide_unimplemented_and_keep_caps() {
        let target = test_target();
        let empty = meta_input(&target, &[]);
        let pg = connection_categories(&DriverCapabilities::pg(), &empty);
        assert!(pg.is_empty(), "pg 连接级子项未实现应全部隐藏");
        let caps = DriverCapabilities::pg();
        assert_eq!(caps.connection.get(PageId::Storage), Some(false));
        assert_eq!(caps.connection.get(PageId::EventTrigger), Some(false));
        // sqlite 无「存储」声明；事件触发器声明未实现 → 隐藏。
        assert_eq!(DriverCapabilities::sqlite().connection.get(PageId::Storage), None);
        assert!(connection_categories(&DriverCapabilities::sqlite(), &empty).is_empty());
    }

    /// P0-3 深化（2026-10-25）：认证 / 网络 / 环境分节按引用 counts 产出，
    /// 名称优先（`auth_name` 等）、解析失败回落引用 ID；无引用不产出。
    #[test]
    fn connection_meta_sections_follow_reference_counts() {
        let target = test_target();
        // 全引用 + 名称：三分节齐全，显示解析后的名称。
        let counts = &[
            ("auth_method", "password".into()),
            ("auth_ref", "auth-1".into()),
            ("auth_name", "生产库凭据".into()),
            ("network_ref", "net-1".into()),
            ("network_name", "跳板隧道".into()),
            ("network_type", "ssh".into()),
            ("env_ref", "env-1".into()),
            ("env_name", "prod".into()),
            ("env_policies", "只读模式, 阻断DML".into()),
        ];
        let input = meta_input(&target, counts);
        let sections = connection_meta_sections(&input);
        let ids: Vec<PageId> = sections.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![PageId::Auth, PageId::Network, PageId::Environment]);
        // 认证分节：方法 + 引用档案名（非裸 ID）。
        let auth = &sections[0];
        assert_eq!(auth.label, "认证");
        let auth_rows = auth.table.rows.clone();
        assert!(auth_rows.contains(&vec!["认证方法".into(), "password".into()]));
        assert!(auth_rows.contains(&vec!["引用档案".into(), "生产库凭据".into()]));
        // 网络分节：档案名 + 类型。
        let net = &sections[1];
        assert!(net.table.rows.contains(&vec!["网络档案".into(), "跳板隧道".into()]));
        assert!(net.table.rows.contains(&vec!["网络类型".into(), "ssh".into()]));
        // 环境分节：名称 + 策略覆盖。
        let env = &sections[2];
        assert!(env.table.rows.contains(&vec!["环境名称".into(), "prod".into()]));
        assert!(env.table.rows.contains(&vec!["策略覆盖".into(), "只读模式, 阻断DML".into()]));

        // 有引用但名称缺失：回落引用 ID（不显示空行）。
        let counts = &[
            ("auth_method", "password".into()),
            ("auth_ref", "auth-1".into()),
            ("env_ref", "env-1".into()),
            ("network_ref", "net-1".into()),
        ];
        let input = meta_input(&target, counts);
        let sections = connection_meta_sections(&input);
        assert!(sections[0].table.rows.contains(&vec!["引用档案".into(), "auth-1".into()]));
        assert!(sections[1].table.rows.contains(&vec!["网络档案".into(), "net-1".into()]));
        assert!(sections[2].table.rows.contains(&vec!["环境名称".into(), "env-1".into()]));

        // 无引用：不产出任何分节。
        let counts = &[("url", "postgres://h/db".into())];
        let input = meta_input(&target, counts);
        assert!(connection_meta_sections(&input).is_empty());
    }

    /// 测试用连接目标引用；调用方先持有 owned 值，再把引用传给 [`meta_input`]，
    /// 避免返回值借用函数局部变量（E0515）。
    fn test_target() -> PropertyTargetRef {
        PropertyTargetRef {
            kind: PropertyKind::Connection,
            name: "连接".into(),
            catalog: None,
            schema: None,
            parent: None,
            conn_id: "g_test".into(),
            source: "G".into(),
        }
    }

    /// 测试用连接 ProviderInput（引用 counts 走 meta 分节断言）。
    /// `target` 与 `counts` 均由调用方在同一作用域持有，保证返回引用不悬空。
    fn meta_input<'a>(
        target: &'a PropertyTargetRef,
        counts: &'a [(&'a str, String)],
    ) -> ProviderInput<'a> {
        ProviderInput {
            target,
            conn_label: "测试连接",
            type_id: Some("postgresql"),
            db_type: Some("postgres"),
            stats: ProviderStats::default(),
            counts,
            candidates: &[],
            detail: None,
        }
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

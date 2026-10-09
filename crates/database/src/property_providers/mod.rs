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
    ObjectOverview, OverviewCard, OverviewStatus, PropertyCategory,
    PropertyRow, PropertyTargetRef, PropertyValueKind, ProviderInput, ProviderRegistry,
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

/// 连接级灰置类别（权限 / 事件触发器——先绘制，能力未实现）。
pub(crate) fn connection_categories() -> Vec<PropertyCategory> {
    vec![
        PropertyCategory::disabled(property::PageId::Privileges, "权限"),
        PropertyCategory::disabled(property::PageId::Dependencies, "事件触发器"),
    ]
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

//! SQLite 属性源（对齐 DBeaver `ext.sqlite` 属性：文件库以 名称 / 路径 / 只读 为核心）。
//!
//! 驱动特异部分最小化：属性行 + 类别掩码；列/索引/约束/DDL/数据取数走通用骨架。

use super::{connection_base_overview, connection_base_rows, editable_row, row, stats_cards};
use property::{
    ObjectOverview, OverviewStatus, PropertySection, PropertyKind, PropertyProvider,
    PropertyRow, ProviderInput, ProviderRegistry,
};

/// 注册 SQLite provider。
pub fn register_all(registry: &mut ProviderRegistry) {
    registry.register(Box::new(SqliteConnectionProvider));
    registry.register(Box::new(SqliteTableProvider));
    registry.register(Box::new(SqliteViewProvider));
}

/// SQLite 连接属性源。
pub struct SqliteConnectionProvider;

impl PropertyProvider for SqliteConnectionProvider {
    fn db_type(&self) -> &'static str { "sqlite" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        let mut rows = connection_base_rows(input, "SQLite");
        rows.push(editable_row("Sqlite.read_only", "只读", "false",
            property::PropertyValueKind::Boolean, vec!["true".to_string(), "false".to_string()]));
        rows
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = connection_base_overview(input, "SQLite");
        if let Some(v) = input.count("path") {
            ov.kv.insert(0, ("文件路径".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, input: &ProviderInput) -> Vec<PropertySection> {
        super::connection_categories(&super::DriverCapabilities::sqlite(), input)
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("SQLite 连接属性编辑待扩展".to_string())
    }
}

/// SQLite 表属性源。
pub struct SqliteTableProvider;

impl PropertyProvider for SqliteTableProvider {
    fn db_type(&self) -> &'static str { "sqlite" }
    fn kind(&self) -> PropertyKind { PropertyKind::Table }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("类型", "表"),
            row("限定名", super::qualified_name(input.target)),
            row("归属域", &input.target.source),
            row("列数", &input.stats.column_count.to_string()),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "表".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards: stats_cards(input),
            kv: vec![
                ("作用域".to_string(), input.target.source.clone()),
                ("限定名".to_string(), super::qualified_name(input.target)),
            ],
            note: "SQLite 为文件库：对象统计与表大小随内省懒加载。".to_string(),
            meta: vec![
                ("驱动".to_string(), "SQLite".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        // 表级类别按能力位生成（触发器此前误用 `Dependencies` id，2026-10-24 修正为 `Trigger`）。
        super::table_categories(&super::DriverCapabilities::sqlite())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("SQLite 表属性编辑待扩展".to_string())
    }
}

/// SQLite 视图属性源（视图无索引/约束，分区为 列 / DDL / 数据）。
pub struct SqliteViewProvider;

impl PropertyProvider for SqliteViewProvider {
    fn db_type(&self) -> &'static str { "sqlite" }
    fn kind(&self) -> PropertyKind { PropertyKind::View }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("类型", "视图"),
            row("限定名", super::qualified_name(input.target)),
            row("归属域", &input.target.source),
            row("列数", &input.stats.column_count.to_string()),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "视图".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards: stats_cards(input),
            kv: vec![
                ("作用域".to_string(), input.target.source.clone()),
                ("限定名".to_string(), super::qualified_name(input.target)),
            ],
            note: "SQLite 为文件库：对象统计随内省懒加载。".to_string(),
            meta: vec![
                ("驱动".to_string(), "SQLite".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        super::view_categories(&super::DriverCapabilities::sqlite())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("SQLite 视图属性编辑待扩展".to_string())
    }
}

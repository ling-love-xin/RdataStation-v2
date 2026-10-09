//! SQLite 属性源（对齐 DBeaver `ext.sqlite` 属性：文件库以 名称 / 路径 / 只读 为核心）。
//!
//! 驱动特异部分最小化：属性行 + 类别掩码；列/索引/约束/DDL/数据取数走通用骨架。

use super::{connection_base_overview, editable_row, empty_table, row, stats_cards};
use property::{
    ObjectOverview, OverviewStatus, PageId, PropertyCategory, PropertyKind, PropertyProvider,
    PropertyRow, ProviderInput, ProviderRegistry,
};

/// 注册 SQLite provider。
pub fn register_all(registry: &mut ProviderRegistry) {
    registry.register(Box::new(SqliteConnectionProvider));
    registry.register(Box::new(SqliteTableProvider));
}

/// SQLite 连接属性源。
pub struct SqliteConnectionProvider;

impl PropertyProvider for SqliteConnectionProvider {
    fn driver(&self) -> &'static str { "sqlite" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", input.conn_label),
            row("数据库类型", input.db_type.unwrap_or("SQLite")),
            row("归属域", &input.target.source),
            row("驱动", "SQLite"),
            editable_row("Sqlite.read_only", "只读", "false",
                property::PropertyValueKind::Boolean, vec!["true".to_string(), "false".to_string()]),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = connection_base_overview(input, "SQLite");
        if let Some(v) = input.count("path") {
            ov.kv.insert(0, ("文件路径".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        super::connection_categories()
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("SQLite 连接属性编辑待扩展".to_string())
    }
}

/// SQLite 表属性源。
pub struct SqliteTableProvider;

impl PropertyProvider for SqliteTableProvider {
    fn driver(&self) -> &'static str { "sqlite" }
    fn kind(&self) -> PropertyKind { PropertyKind::Table }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("类型", "表"),
            row("限定名", super::qualified_name(input.target)),
            row("归属域", &input.target.source),
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![
            PropertyCategory { id: PageId::Columns, label: "列".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Indexes, label: "索引".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Ddl, label: "DDL".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Data, label: "数据".into(), enabled: true, table: empty_table() },
            PropertyCategory::disabled(PageId::Privileges, "权限"),
            PropertyCategory::disabled(PageId::Dependencies, "触发器"),
        ]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("SQLite 表属性编辑待扩展".to_string())
    }
}

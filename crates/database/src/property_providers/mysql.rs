//! MySQL 属性源（对齐 DBeaver `ext.mysql`：服务器版 / 字符集 / 校对规则 为核心属性）。
//!
//! 驱动特异部分：属性行 + 类别掩码；列/索引/约束/DDL/数据取数走通用骨架。

use super::{connection_base_overview, connection_base_rows, editable_row, row, stats_cards};
use property::{
    ObjectOverview, OverviewStatus, PropertySection, PropertyKind, PropertyProvider,
    PropertyRow, ProviderInput, ProviderRegistry,
};

/// 注册 MySQL provider。
pub fn register_all(registry: &mut ProviderRegistry) {
    registry.register(Box::new(MysqlConnectionProvider));
    registry.register(Box::new(MysqlTableProvider));
}

/// MySQL 连接属性源。
pub struct MysqlConnectionProvider;

impl PropertyProvider for MysqlConnectionProvider {
    fn db_type(&self) -> &'static str { "mysql" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        let mut rows = connection_base_rows(input, "MySQL");
        rows.push(row("字符集", input.count("charset").unwrap_or("utf8mb4")));
        rows.push(row("校对规则", input.count("collation").unwrap_or("utf8mb4_0900_ai_ci")));
        rows.push(editable_row("Mysql.sql_mode", "SQL 模式", "-",
            property::PropertyValueKind::Text, Vec::new()));
        rows
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = connection_base_overview(input, "MySQL");
        if let Some(v) = input.count("version") {
            ov.kv.insert(0, ("版本".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, input: &ProviderInput) -> Vec<PropertySection> {
        super::connection_categories(&super::DriverCapabilities::mysql(), input)
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("MySQL 连接属性编辑待扩展".to_string())
    }
}

/// MySQL 表属性源。
pub struct MysqlTableProvider;

impl PropertyProvider for MysqlTableProvider {
    fn db_type(&self) -> &'static str { "mysql" }
    fn kind(&self) -> PropertyKind { PropertyKind::Table }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("类型", "表"),
            row("引擎", input.count("engine").unwrap_or("InnoDB")),
            row("字符集", input.count("charset").unwrap_or("utf8mb4")),
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
            note: "行数 / 表大小随内省懒加载。".to_string(),
            meta: vec![
                ("驱动".to_string(), "MySQL".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        super::table_categories(&super::DriverCapabilities::mysql())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("MySQL 表属性编辑待扩展".to_string())
    }
}

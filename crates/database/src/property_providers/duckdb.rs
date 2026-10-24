//! DuckDB 属性源（列式分析引擎：文件库 + 版本为核心属性）。
//!
//! 与 SQLite 同族（文件库），差异：版本 / 文件路径 由调用方 counts 填充。

use super::{connection_base_overview, connection_base_rows, row, stats_cards};
use property::{
    ObjectOverview, OverviewStatus, PropertySection, PropertyKind, PropertyProvider,
    PropertyRow, ProviderInput, ProviderRegistry,
};
/// 注册 DuckDB provider。
pub fn register_all(registry: &mut ProviderRegistry) {
    registry.register(Box::new(DuckDbConnectionProvider));
    registry.register(Box::new(DuckDbTableProvider));
    registry.register(Box::new(DuckDbViewProvider));
}

/// DuckDB 连接属性源。
pub struct DuckDbConnectionProvider;

impl PropertyProvider for DuckDbConnectionProvider {
    fn db_type(&self) -> &'static str { "duckdb" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        connection_base_rows(input, "DuckDB")
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = connection_base_overview(input, "DuckDB");
        if let Some(v) = input.count("version") {
            ov.kv.insert(0, ("版本".to_string(), v.to_string()));
        }
        if let Some(v) = input.count("path") {
            ov.kv.insert(0, ("文件路径".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        super::connection_categories(&super::DriverCapabilities::duckdb())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("DuckDB 连接属性编辑待扩展".to_string())
    }
}

/// DuckDB 表属性源。
pub struct DuckDbTableProvider;

impl PropertyProvider for DuckDbTableProvider {
    fn db_type(&self) -> &'static str { "duckdb" }
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
            note: "DuckDB 为分析引擎：行估计 / 存储统计随内省懒加载。".to_string(),
            meta: vec![
                ("驱动".to_string(), "DuckDB".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        super::table_categories(&super::DriverCapabilities::duckdb())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("DuckDB 表属性编辑待扩展".to_string())
    }
}

/// DuckDB 视图属性源（视图无索引/约束，分区为 列 / DDL / 数据）。
pub struct DuckDbViewProvider;

impl PropertyProvider for DuckDbViewProvider {
    fn db_type(&self) -> &'static str { "duckdb" }
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
            note: "DuckDB 为分析引擎：对象统计随内省懒加载。".to_string(),
            meta: vec![
                ("驱动".to_string(), "DuckDB".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        super::view_categories(&super::DriverCapabilities::duckdb())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("DuckDB 视图属性编辑待扩展".to_string())
    }
}

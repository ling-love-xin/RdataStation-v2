//! Oracle 属性源（**演示"新增驱动不动面板"的样例壳**）。
//!
//! 目的：验证接口化的承诺——新增数据库只需注册 provider，`property` 模型 /
//! `property_panel` 通用骨架 / workbench 渲染层**零改动**。当前只有连接级壳：
//! 属性行 / 洞察 / 灰置类别齐全，真机取数待 Oracle 驱动接入后扩展（只改本文件）。

use super::{connection_base_overview, empty_table, row};
use property::{
    ObjectOverview, PageId, PropertyCategory, PropertyKind, PropertyProvider, PropertyRow,
    ProviderInput, ProviderRegistry,
};

/// 注册 Oracle provider。
pub fn register_all(registry: &mut ProviderRegistry) {
    registry.register(Box::new(OracleConnectionProvider));
}

/// Oracle 连接属性源（壳）。
pub struct OracleConnectionProvider;

impl PropertyProvider for OracleConnectionProvider {
    fn driver(&self) -> &'static str { "oracle" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", input.conn_label),
            row("数据库类型", input.db_type.unwrap_or("Oracle")),
            row("归属域", &input.target.source),
            row("驱动", "Oracle"),
            row("版本", input.count("version").unwrap_or("-")),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = connection_base_overview(input, "Oracle");
        ov.note = "Oracle 真机内省待驱动接入；本壳证明新增数据库无需改动面板代码。".to_string();
        if let Some(v) = input.count("version") {
            ov.kv.insert(0, ("版本".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![
            PropertyCategory { id: PageId::Columns, label: "表".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Ddl, label: "视图".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Source, label: "程序包".into(), enabled: false, table: empty_table() },
            PropertyCategory::disabled(PageId::Privileges, "权限"),
        ]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("Oracle 属性编辑待驱动接入".to_string())
    }
}

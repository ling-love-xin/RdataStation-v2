//! PostgreSQL 属性源（对齐 DBeaver `ext.postgresql` 模型的 `@Property` 清单，源码实证）。
//!
//! 属性行（General）：PostgreDatabase 10 项 / PostgreSchema 5 项 / PostgreTable 7 项（+Base 3 项）；
//! 编辑：列级 RENAME / COMMENT / DEFAULT / NULLABLE（`sql_gen::alter_column_sql`）；
//! 类别树：驱动能力掩码（列/索引/约束/DDL/数据可用；权限/依赖/触发器/规则/策略/Statistics 先绘制置灰）。

use super::{connection_base_overview, connection_base_rows, connection_categories, editable_row, empty_table, qualified_name, row, stats_cards};
use engine::driver::traits::ColumnDetail;
use property::{
    ObjectOverview, OverviewStatus, PageId, PropertyCategory, PropertyKind, PropertyProvider,
    PropertyRow, ProviderInput, ProviderRegistry,
};

/// 注册 PostgreSQL 全部对象类型的 provider。
pub fn register_all(registry: &mut ProviderRegistry) {
    registry.register(Box::new(PgConnectionProvider));
    registry.register(Box::new(PgSchemaProvider));
    registry.register(Box::new(PgTableProvider));
    registry.register(Box::new(PgViewProvider));
    registry.register(Box::new(PgColumnProvider));
    registry.register(Box::new(PgRoutineProvider));
    registry.register(Box::new(PgSequenceProvider));
    registry.register(Box::new(PgTriggerProvider));
}

// ── 连接 ──────────────────────────────────────────────────────────────

/// PG 连接属性源（DBeaver `PostgreDatabase` @Property 清单）。
pub struct PgConnectionProvider;

impl PropertyProvider for PgConnectionProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        let mut rows = connection_base_rows(input, "PostgreSQL");
        // 以下为 DBeaver PostgreDatabase 可编辑配置行；真值来自连接配置（settings 接入前占位 "-"）。
        rows.push(editable_row(
            "Database.description", "描述", "-",
            property::PropertyValueKind::LongText, Vec::new(),
        ));
        rows.push(editable_row(
            "Database.dba", "数据库管理员", "-",
            property::PropertyValueKind::Select, vec!["postgres".to_string()],
        ));
        rows.push(row("模板", "template1"));
        rows.push(row("默认表空间", "pg_default"));
        rows.push(row("默认编码", "UTF8"));
        rows.push(row("排序规则", "C"));
        rows.push(editable_row(
            "Database.connection_limit", "连接限制", "-1",
            property::PropertyValueKind::Text, Vec::new(),
        ));
        rows.push(editable_row(
            "Database.allow_connections", "允许连接", "true",
            property::PropertyValueKind::Boolean, vec!["true".to_string(), "false".to_string()],
        ));
        rows
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = connection_base_overview(input, "PostgreSQL");
        // 连接级统计（schemas / tables / views）由调用方内省填充；未提供则不显示。
        let mut cards = Vec::new();
        if let Some(v) = input.count("schemas") {
            cards.push(property::OverviewCard { label: "Schema".into(), value: v.into(), unit: String::new(), delta: None });
        }
        if let Some(v) = input.count("tables") {
            cards.push(property::OverviewCard { label: "表".into(), value: v.into(), unit: String::new(), delta: None });
        }
        if let Some(v) = input.count("views") {
            cards.push(property::OverviewCard { label: "视图".into(), value: v.into(), unit: String::new(), delta: None });
        }
        ov.cards = cards;
        if let Some(v) = input.count("version") {
            ov.kv.insert(0, ("版本".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        let mut cats = connection_categories();
        cats.insert(0, PropertyCategory { id: PageId::Dependencies, label: "存储".to_string(), enabled: false, table: empty_table() });
        cats
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("连接级属性编辑待 settings 接入".to_string())
    }
}

// ── Schema ───────────────────────────────────────────────────────────

/// PG Schema 属性源（DBeaver `PostgreSchema` @Property 清单）。
pub struct PgSchemaProvider;

impl PropertyProvider for PgSchemaProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Schema }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("命名空间 ID", input.count("oid").unwrap_or("-")),
            editable_row("Schema.owner", "所有者", input.count("owner").unwrap_or("-"),
                property::PropertyValueKind::Select, vec!["postgres".to_string()]),
            editable_row("Schema.description", "描述", input.count("description").unwrap_or("-"),
                property::PropertyValueKind::LongText, Vec::new()),
            row("限定名", qualified_name(input.target)),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut cards = Vec::new();
        for (key, label) in [
            ("tables", "表"), ("views", "视图"), ("matviews", "物化视图"),
            ("routines", "函数"), ("sequences", "序列"),
        ] {
            if let Some(v) = input.count(key) {
                cards.push(property::OverviewCard { label: label.into(), value: v.into(), unit: String::new(), delta: None });
            }
        }
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "Schema".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards,
            kv: vec![
                ("作用域".to_string(), input.target.source.clone()),
                ("限定名".to_string(), qualified_name(input.target)),
            ],
            note: "对象统计随导航树展开懒加载；空类别也保留展示（同 DBeaver）。".to_string(),
            meta: vec![
                ("驱动".to_string(), "PostgreSQL".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![
            PropertyCategory { id: PageId::Columns, label: "表".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Indexes, label: "外部表".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Constraints, label: "视图".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Ddl, label: "物化视图".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Source, label: "索引".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Privileges, label: "权限".into(), enabled: false, table: empty_table() },
            PropertyCategory { id: PageId::Dependencies, label: "依赖".into(), enabled: false, table: empty_table() },
        ]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("Schema 级属性编辑待扩展".to_string())
    }
}

// ── 表 ───────────────────────────────────────────────────────────────

/// PG 表属性源（DBeaver `PostgreTableBase` + `PostgreTable` @Property 清单）。
pub struct PgTableProvider;

impl PropertyProvider for PgTableProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Table }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            editable_row("Table.description", "描述", input.count("description").unwrap_or("-"),
                property::PropertyValueKind::LongText, Vec::new()),
            editable_row("Table.tablespace", "表空间", input.count("tablespace").unwrap_or("pg_default"),
                property::PropertyValueKind::Select, vec!["pg_default".to_string(), "pg_global".to_string()]),
            row("对象 ID", input.count("oid").unwrap_or("-")),
            editable_row("Table.owner", "所有者", input.count("owner").unwrap_or("-"),
                property::PropertyValueKind::Select, vec!["postgres".to_string()]),
            editable_row("Table.row_level_security", "行级安全", input.count("rls").unwrap_or("false"),
                property::PropertyValueKind::Boolean, vec!["true".to_string(), "false".to_string()]),
            row("分区", input.count("partitioned").unwrap_or("否")),
            editable_row("Table.partition_by", "分区依据", input.count("partition_by").unwrap_or("-"),
                property::PropertyValueKind::Text, Vec::new()),
            editable_row("Table.reloptions", "额外选项", "-",
                property::PropertyValueKind::LongText, Vec::new()),
            row("限定名", qualified_name(input.target)),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let mut ov = ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "表".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards: stats_cards(input),
            kv: vec![
                ("作用域".to_string(), input.target.source.clone()),
                ("限定名".to_string(), qualified_name(input.target)),
            ],
            note: "行估计 / 表大小待 `reltuples` 统计接入（懒加载，随属性面板打开）。".to_string(),
            meta: vec![
                ("驱动".to_string(), "PostgreSQL".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        };
        if let Some(v) = input.count("row_estimate") {
            ov.cards.push(property::OverviewCard { label: "行估计".into(), value: v.into(), unit: String::new(), delta: Some("reltuples".into()) });
        }
        ov
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![
            PropertyCategory { id: PageId::Columns, label: "列".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Indexes, label: "索引".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Constraints, label: "约束".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Ddl, label: "DDL".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Data, label: "数据".into(), enabled: true, table: empty_table() },
            PropertyCategory::disabled(PageId::Source, "触发器"),
            PropertyCategory::disabled(PageId::Privileges, "权限"),
            PropertyCategory::disabled(PageId::Dependencies, "依赖"),
            PropertyCategory::disabled(PageId::Routines, "规则"),
            PropertyCategory::disabled(PageId::Partitions, "分区"),
        ]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("表级属性编辑待扩展".to_string())
    }
}

// ── 视图 ─────────────────────────────────────────────────────────────

/// PG 视图属性源（DBeaver `PostgreView` 属性精简版）。
pub struct PgViewProvider;

impl PropertyProvider for PgViewProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::View }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("对象 ID", input.count("oid").unwrap_or("-")),
            editable_row("View.owner", "所有者", input.count("owner").unwrap_or("-"),
                property::PropertyValueKind::Select, vec!["postgres".to_string()]),
            editable_row("View.description", "描述", input.count("description").unwrap_or("-"),
                property::PropertyValueKind::LongText, Vec::new()),
            row("限定名", qualified_name(input.target)),
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
                ("限定名".to_string(), qualified_name(input.target)),
            ],
            note: "视图定义为只读 SELECT；无主键时分页顺序不保证稳定。".to_string(),
            meta: vec![
                ("驱动".to_string(), "PostgreSQL".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![
            PropertyCategory { id: PageId::Columns, label: "列".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Ddl, label: "DDL".into(), enabled: true, table: empty_table() },
            PropertyCategory { id: PageId::Data, label: "数据".into(), enabled: true, table: empty_table() },
            PropertyCategory::disabled(PageId::Privileges, "权限"),
        ]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("视图级属性编辑待扩展".to_string())
    }
}

// ── 列 ───────────────────────────────────────────────────────────────

/// PG 列属性源（DBeaver `PostgreTableColumn` @Property 清单；编辑 = `sql_gen::alter_column_sql`）。
pub struct PgColumnProvider;

impl PropertyProvider for PgColumnProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Column }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        // 列明细由通用骨架经 `ProviderInput::detail` 传入（`engine::driver::traits::ColumnDetail`）。
        let Some(col) = input.detail.and_then(|d| d.downcast_ref::<ColumnDetail>()) else {
            return vec![
                row("名称", &input.target.name),
                row("所属表", input.target.parent.clone().unwrap_or_default()),
                row("提示", "列已不存在（结构可能已变更）"),
            ];
        };
        let mut rows = vec![
            row("名称", col.name.clone()),
            row("所属表", input.target.parent.clone().unwrap_or_default()),
            row("类型", col.data_type.clone()),
            editable_row("Column.nullable", "可空", if col.nullable { "true" } else { "false" },
                property::PropertyValueKind::Boolean, vec!["true".to_string(), "false".to_string()]),
            row("主键", if col.is_primary_key { "是" } else { "否" }),
            row("外键", if col.is_foreign_key { "是" } else { "否" }),
        ];
        if col.ordinal > 0 {
            rows.push(row("序号", col.ordinal.to_string()));
        }
        if let Some(r) = &col.references {
            rows.push(row("引用", format!("{}.{}", r.table, r.column)));
        }
        rows.push(editable_row("Column.default_value", "默认值",
            col.default_value.clone().unwrap_or_else(|| "-".to_string()),
            property::PropertyValueKind::Text, Vec::new()));
        rows.push(editable_row("Column.comment", "注释",
            col.comment.clone().unwrap_or_else(|| "-".to_string()),
            property::PropertyValueKind::LongText, Vec::new()));
        rows
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        let (otype, type_text) = match input.detail.and_then(|d| d.downcast_ref::<ColumnDetail>()) {
            Some(col) => (col.data_type.clone(), "已加载".to_string()),
            None => ("-".to_string(), "未找到".to_string()),
        };
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: format!("列 · {otype}"),
            status: OverviewStatus::Connected,
            status_text: type_text,
            cards: vec![],
            kv: vec![
                ("所属表".to_string(), input.target.parent.clone().unwrap_or_default()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
            note: String::new(),
            meta: vec![
                ("驱动".to_string(), "PostgreSQL".to_string()),
                ("作用域".to_string(), input.target.source.clone()),
            ],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![PropertyCategory::disabled(PageId::Privileges, "权限")]
    }

    fn apply_edit_sql(&self, input: &ProviderInput, row_id: &str, value: &str) -> Result<String, String> {
        // 复用现有列级编辑（RENAME / COMMENT / DEFAULT / NULLABLE）。
        let schema = input.target.schema.clone().unwrap_or_default();
        let table = input.target.parent.clone().unwrap_or_default();
        crate::sql_gen::alter_column_sql(
            "postgres",
            (!schema.is_empty()).then_some(schema.as_str()),
            &table,
            &input.target.name,
            row_id,
            value,
        )
        .map_err(|e| e.to_string())
    }
}

// ── 例程 / 序列 / 触发器 ─────────────────────────────────────────────

/// PG 例程序（函数 / 存储过程）属性源——精简行 + 源码类别。
pub struct PgRoutineProvider;

impl PropertyProvider for PgRoutineProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Routine }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("限定名", qualified_name(input.target)),
            row("类型", "例程"),
            row("归属域", &input.target.source),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "例程".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards: vec![],
            kv: vec![("限定名".to_string(), qualified_name(input.target))],
            note: "源码在子项「源码」页。".to_string(),
            meta: vec![("驱动".to_string(), "PostgreSQL".to_string())],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![PropertyCategory { id: PageId::Source, label: "源码".into(), enabled: true, table: empty_table() }]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("例程序属性编辑待扩展".to_string())
    }
}

/// PG 序列属性源。
pub struct PgSequenceProvider;

impl PropertyProvider for PgSequenceProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Sequence }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("限定名", qualified_name(input.target)),
            row("类型", "序列"),
            row("归属域", &input.target.source),
        ]
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "序列".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards: vec![],
            kv: vec![("限定名".to_string(), qualified_name(input.target))],
            note: String::new(),
            meta: vec![("驱动".to_string(), "PostgreSQL".to_string())],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![PropertyCategory::disabled(PageId::Privileges, "权限")]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("序列属性编辑待扩展".to_string())
    }
}

/// PG 触发器属性源。
pub struct PgTriggerProvider;

impl PropertyProvider for PgTriggerProvider {
    fn driver(&self) -> &'static str { "postgres" }
    fn kind(&self) -> PropertyKind { PropertyKind::Trigger }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        let mut rows = vec![
            row("名称", &input.target.name),
            row("限定名", qualified_name(input.target)),
            row("类型", "触发器"),
            row("归属域", &input.target.source),
        ];
        if let Some(t) = input.target.parent.as_deref().filter(|t| !t.is_empty()) {
            rows.push(row("关联表", t));
        }
        rows
    }

    fn overview(&self, input: &ProviderInput) -> ObjectOverview {
        ObjectOverview {
            title: input.target.name.clone(),
            subtitle: "触发器".to_string(),
            status: OverviewStatus::Connected,
            status_text: "已加载".to_string(),
            cards: vec![],
            kv: vec![("限定名".to_string(), qualified_name(input.target))],
            note: String::new(),
            meta: vec![("驱动".to_string(), "PostgreSQL".to_string())],
        }
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertyCategory> {
        vec![]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("触发器属性编辑待扩展".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use property::{PropertyTargetRef, ProviderInput, ProviderStats};

    fn target(kind: PropertyKind, parent: Option<&str>) -> PropertyTargetRef {
        PropertyTargetRef {
            kind,
            name: "t".to_string(),
            catalog: Some("db".to_string()),
            schema: Some("public".to_string()),
            parent: parent.map(|s| s.to_string()),
            conn_id: "c1".to_string(),
            source: "项目 (P)".to_string(),
        }
    }

    fn input<'a>(
        target: &'a PropertyTargetRef,
        counts: &'a [(&'a str, String)],
        detail: Option<&'a dyn std::any::Any>,
    ) -> ProviderInput<'a> {
        ProviderInput {
            target,
            conn_label: "PG 连接",
            db_type: Some("postgres"),
            stats: ProviderStats { column_count: 3, index_count: 1, constraint_count: 1 },
            counts,
            detail,
        }
    }

    /// 连接 provider：属性行含 DBeaver 可编辑项（描述 / 管理员 / 连接限制 / 允许连接）。
    #[test]
    fn connection_provider_rows_match_dbeaver_shape() {
        let t = target(PropertyKind::Connection, None);
        let rows = PgConnectionProvider.property_defs(&input(&t, &[], None));
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        for want in ["名称", "驱动", "描述", "数据库管理员", "连接限制", "允许连接"] {
            assert!(labels.contains(&want), "缺 {want}: {labels:?}");
        }
        let editable: Vec<&str> = rows.iter().filter(|r| r.editable).map(|r| r.id.as_str()).collect();
        assert!(editable.contains(&"Database.description"));
        assert!(editable.contains(&"Database.allow_connections"));
    }

    /// 列 provider：detail 命中时渲染完整 DBeaver 行；未命中给提示。
    #[test]
    fn column_provider_uses_detail_when_present() {
        let col = ColumnDetail {
            name: "id".to_string(),
            data_type: "INTEGER".to_string(),
            nullable: false,
            is_primary_key: true,
            is_foreign_key: false,
            references: None,
            ordinal: 1,
            default_value: None,
            comment: None,
            extra: Default::default(),
        };
        let t = target(PropertyKind::Column, Some("t"));
        let rows = PgColumnProvider
            .property_defs(&input(&t, &[], Some(&col as &dyn std::any::Any)));
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        for want in ["名称", "类型", "可空", "主键", "默认值", "注释"] {
            assert!(labels.contains(&want), "缺 {want}: {labels:?}");
        }
        assert_eq!(rows[4].value, "是", "主键行");

        let missing = PgColumnProvider.property_defs(&input(&t, &[], None));
        assert!(missing.iter().any(|r| r.label == "提示"));
    }

    /// 表 provider：类别含可用（列/索引/约束/DDL/数据）+ 灰置（权限/依赖/规则/分区）。
    #[test]
    fn table_categories_mix_supported_and_disabled() {
        let t = target(PropertyKind::Table, None);
        let cats = PgTableProvider.categories(&input(&t, &[], None));
        assert!(cats.iter().any(|c| c.id == PageId::Columns && c.enabled));
        assert!(cats.iter().any(|c| c.id == PageId::Data && c.enabled));
        assert!(cats.iter().any(|c| c.id == PageId::Privileges && !c.enabled), "权限灰置");
        assert!(cats.iter().any(|c| c.id == PageId::Partitions && !c.enabled), "分区灰置");
    }

    /// 洞察卡：stats 非零才显示（列/索引/约束）；counts 命中补充（行估计）。
    #[test]
    fn table_overview_cards_from_stats_and_counts() {
        let t = target(PropertyKind::Table, None);
        let counts: &[(&str, String)] = &[("row_estimate", "12345".to_string())];
        let ov = PgTableProvider.overview(&input(&t, counts, None));
        let labels: Vec<&str> = ov.cards.iter().map(|c| c.label.as_str()).collect();
        for want in ["列", "索引", "约束", "行估计"] {
            assert!(labels.contains(&want), "缺 {want}: {labels:?}");
        }
    }

    /// 列编辑 SQL：复用 alter_column_sql（RENAME 等 PG 语句）。
    #[test]
    fn column_edit_sql_generates_alter() {
        let col = ColumnDetail {
            name: "c".to_string(), data_type: "INTEGER".to_string(), nullable: true,
            is_primary_key: false, is_foreign_key: false, references: None,
            ordinal: 1, default_value: None, comment: None, extra: Default::default(),
        };
        let t = target(PropertyKind::Column, Some("t"));
        let input = input(&t, &[], Some(&col as &dyn std::any::Any));
        let sql = PgColumnProvider.apply_edit_sql(&input, "Column.comment", "hello");
        assert!(sql.unwrap().contains("COMMENT ON COLUMN"));
    }
}

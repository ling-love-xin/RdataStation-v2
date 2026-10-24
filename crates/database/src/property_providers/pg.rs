//! PostgreSQL 属性源（对齐 DBeaver `ext.postgresql` 模型的 `@Property` 清单，源码实证）。
//!
//! 属性行（General）：PostgreDatabase 10 项 / PostgreSchema 5 项 / PostgreTable 7 项（+Base 3 项）；
//! 编辑：列级 RENAME / COMMENT / DEFAULT / NULLABLE（`sql_gen::alter_column_sql`）；
//! 类别树：驱动能力掩码（列/索引/约束/DDL/数据可用；权限/依赖/触发器/规则/策略/Statistics 先绘制置灰）。

use super::{connection_base_overview, connection_base_rows, editable_row, empty_table, qualified_name, row, stats_cards};
use engine::driver::traits::ColumnDetail;
use property::{
    ExtraQuery, ObjectOverview, OverviewStatus, PageId, PropertyKind, PropertyProvider,
    PropertyRow, PropertySection, ProviderInput, ProviderRegistry,
};

/// 标识符双引号（编辑 SQL 的角色名 / 表空间名；值来自候选查询，仍按外部输入转义）。
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 字符串字面量（`COMMENT … IS '…'` 的值；单引号双写转义，值按外部输入处理）。
fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 角色候选（`pg_roles` 全量，多行 → `ProviderInput::candidates("roles")`）。
const ROLES_SQL: &str = "SELECT rolname FROM pg_roles ORDER BY 1";
/// 表空间候选（`pg_tablespace` 全量，多行 → `ProviderInput::candidates("tablespaces")`）。
const TABLESPACES_SQL: &str = "SELECT spcname FROM pg_tablespace ORDER BY 1";

/// Select 候选回退：候选查询未命中（连接异常 / 结果为空）时给最小可用集，避免空下拉。
fn roles_fallback() -> Vec<String> {
    vec!["postgres".to_string()]
}
fn tablespaces_fallback() -> Vec<String> {
    vec!["pg_default".to_string()]
}

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
    fn db_type(&self) -> &'static str { "postgresql" }
    fn kind(&self) -> PropertyKind { PropertyKind::Connection }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        let mut rows = connection_base_rows(input, "PostgreSQL");
        // 以下为 DBeaver PostgreDatabase 可编辑配置行；真值来自连接配置。
        // settings 接入前渲染为只读占位（值带「待接入」标注，避免"可点但提交报错"）。
        rows.push(row("数据库管理员", "待接入"));
        rows.push(row("模板", "template1"));
        rows.push(row("默认表空间", "pg_default"));
        rows.push(row("默认编码", "UTF8"));
        rows.push(row("排序规则", "C"));
        rows.push(row("连接限制", "待接入"));
        rows.push(row("允许连接", "待接入"));
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
        if let Some(v) = input.count("server_version") {
            ov.kv.insert(0, ("版本".to_string(), v.to_string()));
        }
        ov
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        // 连接级类别按能力位生成（存储 / 权限 / 事件触发器，均灰置——内容批次未接入）。
        super::connection_categories(&super::DriverCapabilities::pg())
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("连接级属性编辑待 settings 接入".to_string())
    }
}

// ── Schema ───────────────────────────────────────────────────────────

/// PG Schema 属性源（DBeaver `PostgreSchema` @Property 清单）。
pub struct PgSchemaProvider;

impl PropertyProvider for PgSchemaProvider {
    fn db_type(&self) -> &'static str { "postgresql" }
    fn kind(&self) -> PropertyKind { PropertyKind::Schema }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("命名空间 ID", input.count("oid").unwrap_or("-")),
            editable_row("Schema.owner", "所有者", input.count("owner").unwrap_or("-"),
                property::PropertyValueKind::Select,
                input.candidates("roles").map(|v| v.to_vec()).unwrap_or_else(roles_fallback)),
            editable_row("Schema.description", "描述", input.count("description").unwrap_or("-"),
                property::PropertyValueKind::LongText, Vec::new()),
            row("限定名", qualified_name(input.target)),
        ]
    }

    /// Schema 级懒加载：角色候选（`pg_roles` 多行 → Select 候选）。
    fn extra_queries(&self, _input: &ProviderInput) -> Vec<ExtraQuery> {
        vec![ExtraQuery { key: "roles", sql: ROLES_SQL.to_string() }]
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        // Schema 的 表/外部表/视图/物化视图/索引/函数/序列/数据类型/聚合函数 类别
        // 需要真内容（新 PageId + 内省查询），未接入前不声明。
        // 旧实现把「表」错塞 Columns id、「视图」错塞 Constraints id（类别 id 是
        // provider 增删时的稳定业务键，错位会让未来启用时内容进错分支）——已删除。
        // 权限 / 依赖按能力位生成（灰置）。
        super::schema_categories(&super::DriverCapabilities::pg())
    }

    fn apply_edit_sql(&self, input: &ProviderInput, row_id: &str, value: &str) -> Result<String, String> {
        match row_id {
            "Schema.owner" => Ok(format!(
                "ALTER SCHEMA {} OWNER TO {}",
                qualified_name(input.target),
                ident(value)
            )),
            "Schema.description" => Ok(format!(
                "COMMENT ON SCHEMA {} IS {}",
                qualified_name(input.target),
                quote_literal(value)
            )),
            _ => Err(format!("Schema 级属性编辑待扩展（{row_id}）")),
        }
    }
}

// ── 表 ───────────────────────────────────────────────────────────────

/// PG 表属性源（DBeaver `PostgreTableBase` + `PostgreTable` @Property 清单）。
pub struct PgTableProvider;

impl PropertyProvider for PgTableProvider {
    fn db_type(&self) -> &'static str { "postgresql" }
    fn kind(&self) -> PropertyKind { PropertyKind::Table }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            editable_row("Table.description", "描述", input.count("description").unwrap_or("-"),
                property::PropertyValueKind::LongText, Vec::new()),
            editable_row("Table.tablespace", "表空间", input.count("tablespace").unwrap_or("pg_default"),
                property::PropertyValueKind::Select,
                input.candidates("tablespaces").map(|v| v.to_vec()).unwrap_or_else(tablespaces_fallback)),
            row("对象 ID", input.count("oid").unwrap_or("-")),
            editable_row("Table.owner", "所有者", input.count("owner").unwrap_or("-"),
                property::PropertyValueKind::Select,
                input.candidates("roles").map(|v| v.to_vec()).unwrap_or_else(roles_fallback)),
            editable_row("Table.row_level_security", "行级安全", input.count("rls").unwrap_or("false"),
                property::PropertyValueKind::Boolean, vec!["true".to_string(), "false".to_string()]),
            row("分区", input.count("partitioned").unwrap_or("否")),
            // 分区依据 / 额外选项：无直接 ALTER 语法（分区重建 / reloptions 集合变更），
            // 保持只读展示，避免「可点但提交报错」（2026-10-15 修正）。
            row("分区依据", input.count("partition_by").unwrap_or("-")),
            row("额外选项", "-"),
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
        if let Some(v) = input.count("size") {
            ov.cards.push(property::OverviewCard { label: "表大小".into(), value: v.into(), unit: String::new(), delta: Some("pg_total_relation_size".into()) });
        }
        ov
    }

    /// 表级懒加载：行估计 / 表大小（单行 → counts）+ 角色 / 表空间候选（多行 → candidates）。
    fn extra_queries(&self, input: &ProviderInput) -> Vec<ExtraQuery> {
        let q = qualified_name(input.target).replace('\'', "''");
        vec![
            property::ExtraQuery {
                key: "row_estimate",
                sql: format!("SELECT reltuples::text FROM pg_class WHERE oid = '{q}'::regclass"),
            },
            // 行级安全真值（单行 → counts "rls"）：Switch 初始态必须真实，
            // 假值会让用户基于错误的开关状态提交 ALTER。
            property::ExtraQuery {
                key: "rls",
                sql: format!("SELECT relrowsecurity::text FROM pg_class WHERE oid = '{q}'::regclass"),
            },
            property::ExtraQuery {
                key: "size",
                sql: format!("SELECT pg_size_pretty(pg_total_relation_size('{q}'))"),
            },
            ExtraQuery { key: "roles", sql: ROLES_SQL.to_string() },
            ExtraQuery { key: "tablespaces", sql: TABLESPACES_SQL.to_string() },
        ]
    }

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        // 表级类别按能力位生成：可用核心（列/索引/约束/DDL/数据）+ 灰置
        // （触发器/权限/依赖/规则/分区，均未接入）。触发器此前误用 `Source` id
        //（例程序源码页），2026-10-24 能力矩阵收敛时修正为 `Trigger`。
        super::table_categories(&super::DriverCapabilities::pg())
    }

    fn apply_edit_sql(&self, input: &ProviderInput, row_id: &str, value: &str) -> Result<String, String> {
        let q = qualified_name(input.target);
        match row_id {
            "Table.tablespace" => Ok(format!("ALTER TABLE {q} SET TABLESPACE {}", ident(value))),
            "Table.owner" => Ok(format!("ALTER TABLE {q} OWNER TO {}", ident(value))),
            "Table.description" => Ok(format!("COMMENT ON TABLE {q} IS {}", quote_literal(value))),
            "Table.row_level_security" => {
                if value == "true" {
                    Ok(format!("ALTER TABLE {q} ENABLE ROW LEVEL SECURITY"))
                } else {
                    Ok(format!("ALTER TABLE {q} DISABLE ROW LEVEL SECURITY"))
                }
            }
            _ => Err(format!("表级属性编辑待扩展（{row_id}）")),
        }
    }
}

// ── 视图 ─────────────────────────────────────────────────────────────

/// PG 视图属性源（DBeaver `PostgreView` 属性精简版）。
pub struct PgViewProvider;

impl PropertyProvider for PgViewProvider {
    fn db_type(&self) -> &'static str { "postgresql" }
    fn kind(&self) -> PropertyKind { PropertyKind::View }

    fn property_defs(&self, input: &ProviderInput) -> Vec<PropertyRow> {
        vec![
            row("名称", &input.target.name),
            row("对象 ID", input.count("oid").unwrap_or("-")),
            editable_row("View.owner", "所有者", input.count("owner").unwrap_or("-"),
                property::PropertyValueKind::Select,
                input.candidates("roles").map(|v| v.to_vec()).unwrap_or_else(roles_fallback)),
            editable_row("View.description", "描述", input.count("description").unwrap_or("-"),
                property::PropertyValueKind::LongText, Vec::new()),
            row("限定名", qualified_name(input.target)),
        ]
    }

    /// 视图级懒加载：角色候选（`pg_roles` 多行 → Select 候选）。
    fn extra_queries(&self, _input: &ProviderInput) -> Vec<ExtraQuery> {
        vec![ExtraQuery { key: "roles", sql: ROLES_SQL.to_string() }]
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        super::view_categories(&super::DriverCapabilities::pg())
    }

    fn apply_edit_sql(&self, input: &ProviderInput, row_id: &str, value: &str) -> Result<String, String> {
        match row_id {
            "View.owner" => Ok(format!(
                "ALTER VIEW {} OWNER TO {}",
                qualified_name(input.target),
                ident(value)
            )),
            "View.description" => Ok(format!(
                "COMMENT ON VIEW {} IS {}",
                qualified_name(input.target),
                quote_literal(value)
            )),
            _ => Err(format!("视图级属性编辑待扩展（{row_id}）")),
        }
    }
}

// ── 列 ───────────────────────────────────────────────────────────────

/// PG 列属性源（DBeaver `PostgreTableColumn` @Property 清单；编辑 = `sql_gen::alter_column_sql`）。
pub struct PgColumnProvider;

impl PropertyProvider for PgColumnProvider {
    fn db_type(&self) -> &'static str { "postgresql" }
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        // 列对象无可展示子项（权限等未实现类别隐藏，见 mod.rs 展示策略）。
        Vec::new()
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
    fn db_type(&self) -> &'static str { "postgresql" }
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        vec![PropertySection { id: PageId::Source, label: "源码".into(), table: empty_table() }]
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("例程序属性编辑待扩展".to_string())
    }
}

/// PG 序列属性源。
pub struct PgSequenceProvider;

impl PropertyProvider for PgSequenceProvider {
    fn db_type(&self) -> &'static str { "postgresql" }
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
        // 序列对象无可展示子项（同列对象，见 mod.rs 展示策略）。
        Vec::new()
    }

    fn apply_edit_sql(&self, _input: &ProviderInput, _id: &str, _v: &str) -> Result<String, String> {
        Err("序列属性编辑待扩展".to_string())
    }
}

/// PG 触发器属性源。
pub struct PgTriggerProvider;

impl PropertyProvider for PgTriggerProvider {
    fn db_type(&self) -> &'static str { "postgresql" }
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

    fn categories(&self, _input: &ProviderInput) -> Vec<PropertySection> {
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
            type_id: Some("postgresql"),
            db_type: Some("postgres"),
            stats: ProviderStats { column_count: 3, index_count: 1, constraint_count: 1 },
            counts,
            candidates: &[],
            detail,
        }
    }

    /// 连接 provider：DBeaver 可编辑项在 settings 接入前渲染为只读占位（值带「待接入」）；
    /// 基础行接 `ConnectionInfo` 真值（描述 / 认证 / 环境 / 网络 / 连接串，2026-10-24 P0-3）。
    #[test]
    fn connection_provider_rows_match_dbeaver_shape() {
        let t = target(PropertyKind::Connection, None);
        let counts = [
            ("url", "postgres://localhost:5432/mydb".to_string()),
            ("description", "开发库".to_string()),
            ("auth_method", "password".to_string()),
            ("auth_ref", "auth-1".to_string()),
            ("env_ref", "env-1".to_string()),
            ("network_ref", "net-1".to_string()),
        ];
        let rows = PgConnectionProvider.property_defs(&input(&t, &counts, None));
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        for want in [
            "名称", "驱动", "连接串", "描述", "认证", "认证引用", "环境", "网络",
            "数据库管理员", "连接限制", "允许连接",
        ] {
            assert!(labels.contains(&want), "缺 {want}: {labels:?}");
        }
        // 基础行接真值；引用字段显示来源 ID（只读，见 `row` 构造）
        let val = |label: &str| rows.iter().find(|r| r.label == label).map(|r| r.value.as_str());
        assert_eq!(val("连接串"), Some("postgres://localhost:5432/mydb"));
        assert_eq!(val("描述"), Some("开发库"));
        assert_eq!(val("认证"), Some("password"));
        assert_eq!(val("认证引用"), Some("auth-1"));
        assert_eq!(val("环境"), Some("env-1"));
        assert_eq!(val("网络"), Some("net-1"));
        // settings 接入前：配置行只读占位（值标注「待接入」）
        let pending: Vec<&str> = rows
            .iter()
            .filter(|r| r.value == "待接入")
            .map(|r| r.label.as_str())
            .collect();
        for want in ["数据库管理员", "连接限制", "允许连接"] {
            assert!(pending.contains(&want), "缺待接入行 {want}: {pending:?}");
        }
        let editable: Vec<&str> = rows.iter().filter(|r| r.editable).map(|r| r.id.as_str()).collect();
        assert!(!editable.contains(&"Database.description"), "连接描述应只读：{editable:?}");
        assert!(!editable.contains(&"Database.allow_connections"), "允许连接应只读：{editable:?}");
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
            is_identity: false,
            identity_generation: None,
            collation: None,
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

    /// 表 provider：类别含可用核心（列/索引/约束/DDL/数据）；权限/依赖/规则/分区
    /// 等未实现类别**不产出**（隐藏式，2026-10-24 拍板）。
    #[test]
    fn table_categories_hide_unimplemented() {
        let t = target(PropertyKind::Table, None);
        let cats = PgTableProvider.categories(&input(&t, &[], None));
        assert!(cats.iter().any(|c| c.id == PageId::Columns));
        assert!(cats.iter().any(|c| c.id == PageId::Data));
        assert!(!cats.iter().any(|c| c.id == PageId::Privileges), "权限未实现应隐藏");
        assert!(!cats.iter().any(|c| c.id == PageId::Partitions), "分区未实现应隐藏");
        assert!(!cats.iter().any(|c| c.id == PageId::Trigger), "触发器未实现应隐藏");
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
            ordinal: 1, default_value: None, comment: None,
            is_identity: false,
            identity_generation: None,
            collation: None,
            extra: Default::default(),
        };
        let t = target(PropertyKind::Column, Some("t"));
        let input = input(&t, &[], Some(&col as &dyn std::any::Any));
        let sql = PgColumnProvider.apply_edit_sql(&input, "Column.comment", "hello");
        assert!(sql.unwrap().contains("COMMENT ON COLUMN"));
    }

    /// Schema 所有者：候选真值（roles 多行）进 select_options；缺失回退最小集。
    #[test]
    fn schema_owner_select_uses_role_candidates() {
        let t = target(PropertyKind::Schema, None);
        let roles: &[(&str, String)] =
            &[("roles", "postgres|rds_user".into())];
        // candidates 是独立字段，这里模拟骨架收集后的多行候选。
        let candidates: &[(&str, Vec<String>)] =
            &[("roles", vec!["postgres".into(), "rds_user".into()])];
        let with_candidates = ProviderInput {
            target: &t,
            conn_label: "PG 连接",
            type_id: Some("postgresql"),
            db_type: Some("postgres"),
            stats: ProviderStats::default(),
            counts: roles,
            candidates,
            detail: None,
        };
        let rows = PgSchemaProvider.property_defs(&with_candidates);
        let owner = rows.iter().find(|r| r.id == "Schema.owner").expect("owner 行");
        assert_eq!(owner.select_options, vec!["postgres", "rds_user"]);
        assert!(owner.editable);
        // 候选缺失 → 回退最小集（不空下拉）。
        let rows2 = PgSchemaProvider.property_defs(&input(&t, &[], None));
        let owner2 = rows2.iter().find(|r| r.id == "Schema.owner").unwrap();
        assert_eq!(owner2.select_options, vec!["postgres"]);
    }

    /// Schema owner / 描述编辑 SQL：ALTER SCHEMA … OWNER TO / COMMENT ON SCHEMA。
    #[test]
    fn schema_edit_sql_alters_owner_and_comment() {
        let t = target(PropertyKind::Schema, None);
        let input = input(&t, &[], None);
        let sql = PgSchemaProvider
            .apply_edit_sql(&input, "Schema.owner", "rds_admin")
            .expect("owner 编辑应生成 SQL");
        assert_eq!(sql, "ALTER SCHEMA db.public.t OWNER TO \"rds_admin\"");
        let desc = PgSchemaProvider
            .apply_edit_sql(&input, "Schema.description", "分析库")
            .expect("描述编辑应生成 SQL");
        assert_eq!(desc, "COMMENT ON SCHEMA db.public.t IS '分析库'");
    }

    /// 表空间候选：tablespaces 多行进 select_options；owner 同角色候选。
    #[test]
    fn table_select_rows_use_real_candidates() {
        let t = target(PropertyKind::Table, None);
        let candidates: &[(&str, Vec<String>)] = &[
            ("roles", vec!["postgres".into()]),
            ("tablespaces", vec!["pg_default".into(), "fast_ssd".into()]),
        ];
        let input = ProviderInput {
            target: &t,
            conn_label: "PG 连接",
            type_id: Some("postgresql"),
            db_type: Some("postgres"),
            stats: ProviderStats::default(),
            counts: &[("tablespace", "pg_default".into()), ("owner", "postgres".into())],
            candidates,
            detail: None,
        };
        let rows = PgTableProvider.property_defs(&input);
        let ts = rows.iter().find(|r| r.id == "Table.tablespace").unwrap();
        assert_eq!(ts.select_options, vec!["pg_default", "fast_ssd"]);
        let owner = rows.iter().find(|r| r.id == "Table.owner").unwrap();
        assert_eq!(owner.select_options, vec!["postgres"]);
        let sql = PgTableProvider
            .apply_edit_sql(&input, "Table.tablespace", "fast_ssd")
            .expect("表空间编辑应生成 SQL");
        assert_eq!(sql, "ALTER TABLE db.public.t SET TABLESPACE \"fast_ssd\"");
    }

    /// 表描述 / 行级安全编辑 SQL（COMMENT / ENABLE|DISABLE RLS，单引号转义）；
    /// 分区依据 / 额外选项保持只读（无直接 ALTER 语法，避免可点但提交报错）。
    #[test]
    fn table_comment_and_rls_edits_generate_sql() {
        let t = target(PropertyKind::Table, None);
        let input = input(&t, &[], None);
        let desc = PgTableProvider
            .apply_edit_sql(&input, "Table.description", "订单表")
            .expect("描述编辑应生成 SQL");
        assert_eq!(desc, "COMMENT ON TABLE db.public.t IS '订单表'");
        let on = PgTableProvider
            .apply_edit_sql(&input, "Table.row_level_security", "true")
            .expect("RLS 启用应生成 SQL");
        assert_eq!(on, "ALTER TABLE db.public.t ENABLE ROW LEVEL SECURITY");
        let off = PgTableProvider
            .apply_edit_sql(&input, "Table.row_level_security", "false")
            .expect("RLS 禁用应生成 SQL");
        assert_eq!(off, "ALTER TABLE db.public.t DISABLE ROW LEVEL SECURITY");
        // 注释值含单引号：双写转义，不破坏语句。
        let esc = PgTableProvider
            .apply_edit_sql(&input, "Table.description", "it's ok")
            .expect("描述转义应生成 SQL");
        assert_eq!(esc, "COMMENT ON TABLE db.public.t IS 'it''s ok'");

        let rows = PgTableProvider.property_defs(&input);
        // 只读行 id 取显示名（row() 语义）；旧 editable id 不应再出现。
        let part = rows.iter().find(|r| r.label == "分区依据").unwrap();
        let rel = rows.iter().find(|r| r.label == "额外选项").unwrap();
        assert!(!part.editable, "分区依据应只读（无直接 ALTER 语法）");
        assert!(!rel.editable, "额外选项应只读（reloptions 集合变更未接）");
        assert!(rows.iter().all(|r| r.id != "Table.partition_by" && r.id != "Table.reloptions"));
    }

    /// 视图描述编辑 SQL（COMMENT ON VIEW）。
    #[test]
    fn view_description_edit_generates_sql() {
        let t = target(PropertyKind::View, None);
        let input = input(&t, &[], None);
        let sql = PgViewProvider
            .apply_edit_sql(&input, "View.description", "统计视图")
            .expect("视图描述编辑应生成 SQL");
        assert_eq!(sql, "COMMENT ON VIEW db.public.t IS '统计视图'");
    }

    /// Schema 类别：不再复用错位 PageId（表→Columns / 视图→Constraints 等）；
    /// 内容未接入前**不产出任何子项类别**（隐藏式，2026-10-24 拍板）。
    #[test]
    fn schema_categories_have_no_mismatched_page_ids() {
        let t = target(PropertyKind::Schema, None);
        let cats = PgSchemaProvider.categories(&input(&t, &[], None));
        let ids: Vec<PageId> = cats.iter().map(|c| c.id).collect();
        assert!(!ids.contains(&PageId::Columns), "表类别不得复用 Columns id");
        assert!(!ids.contains(&PageId::Constraints), "视图类别不得复用 Constraints id");
        assert!(!ids.contains(&PageId::Indexes));
        assert!(!ids.contains(&PageId::Ddl));
        assert!(!ids.contains(&PageId::Source));
        assert!(cats.is_empty(), "Schema 子项内容未接入前全部隐藏");
    }
}

//! rds-database — 对象属性面板（M4）。
//!
//! DBeaver 式：上部属性网格 + 下部子实体（列 / 索引 / 约束）。
//! 数据来自 `MetadataService`（实时内省）+ `PropertyRef`（导航节点上下文）。
//! 视图形态为编辑区右侧停靠面板，见 `docs/architecture/database/database-navigator-prototype-design.md` §7。

use crate::metadata_service::MetadataService;
use crate::model::{NavSource, PropertyKind, PropertyRef};
use engine::driver::traits::SchemaObjectKind;
use shared::error::CoreError;

/// 属性网格中的一行（label / value）。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyRow {
    pub label: String,
    pub value: String,
}

/// 子实体表格（列 / 索引 / 约束）。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// 子实体分区。
#[derive(Debug, Clone, PartialEq)]
pub struct PropertySection {
    pub label: String,
    pub table: PropertyTable,
}

/// 一个对象的完整属性（供属性面板渲染）。
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectProperties {
    /// 标题（对象名）
    pub title: String,
    /// 对象类型文案（BASE TABLE / VIEW / COLUMN …）
    pub object_type: String,
    /// 来源标签（项目 (P) / 全局 (G) / 共享 (GP)）
    pub source_label: String,
    /// 上部属性网格
    pub properties: Vec<PropertyRow>,
    /// 下部子实体
    pub sections: Vec<PropertySection>,
}

fn row(label: &str, value: impl Into<String>) -> PropertyRow {
    PropertyRow {
        label: label.to_string(),
        value: value.into(),
    }
}

fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "-".to_string())
}

fn qualify(ref_: &PropertyRef) -> String {
    match ref_.kind {
        PropertyKind::Column => {
            let parent = ref_.parent.clone().unwrap_or_default();
            if parent.is_empty() {
                ref_.name.clone()
            } else {
                format!("{parent}.{}", ref_.name)
            }
        }
        _ => crate::sql_gen::qualified_name(
            ref_.catalog.as_deref(),
            ref_.schema.as_deref(),
            &ref_.name,
        ),
    }
}

fn source_label(source: NavSource) -> String {
    match source {
        NavSource::Project => "项目 (P)".to_string(),
        NavSource::Global => "全局 (G)".to_string(),
        NavSource::Shared => "共享 (GP)".to_string(),
    }
}

/// 加载对象属性（实时内省；连接 / Catalog / Schema 无需查询）。
pub async fn load_properties(
    metadata: &MetadataService,
    ref_: &PropertyRef,
    conn_label: &str,
    driver: &str,
    db_type: Option<&str>,
) -> Result<ObjectProperties, CoreError> {
    let source = source_label(ref_.source);
    let mut properties = Vec::new();
    let mut sections = Vec::new();

    let object_type = match ref_.kind {
        PropertyKind::Connection => "连接",
        PropertyKind::Catalog => "Catalog",
        PropertyKind::Schema => "Schema",
        PropertyKind::Table => "BASE TABLE",
        PropertyKind::View => "VIEW",
        PropertyKind::Column => "COLUMN",
        PropertyKind::Routine => "ROUTINE",
        PropertyKind::Sequence => "SEQUENCE",
        PropertyKind::Trigger => "TRIGGER",
    }
    .to_string();

    let title = match ref_.kind {
        PropertyKind::Connection => conn_label.to_string(),
        _ => ref_.name.clone(),
    };

    match ref_.kind {
        PropertyKind::Connection => {
            properties.push(row("名称", conn_label));
            // 数据库类型（`drivers.type_id`）与驱动显示名由调用方（workbench 驱动目录）传入。
            if let Some(t) = db_type {
                if !t.is_empty() {
                    properties.push(row("数据库类型", t));
                }
            }
            properties.push(row("归属域", source.clone()));
            properties.push(row("驱动", driver));
        }
        PropertyKind::Catalog | PropertyKind::Schema => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("类型", object_type.clone()));
            properties.push(row("归属域", source.clone()));
            let qualified = qualify(ref_);
            properties.push(row("限定名", qualified));
        }
        PropertyKind::Table | PropertyKind::View => {
            let catalog = ref_.catalog.clone().unwrap_or_default();
            let schema = ref_.schema.clone().unwrap_or_default();
            let columns = metadata
                .list_columns(&ref_.conn_id, &catalog, &schema, &ref_.name)
                .await?;

            properties.push(row("限定名", qualify(ref_)));
            properties.push(row("类型", object_type.clone()));
            properties.push(row("列数", columns.len().to_string()));
            properties.push(row("归属域", source.clone()));

            sections.push(PropertySection {
                label: format!("列 ({})", columns.len()),
                table: columns_table(&columns),
            });

            if ref_.kind == PropertyKind::Table {
                if let Ok(indexes) = metadata
                    .list_indexes(&ref_.conn_id, &catalog, &schema, &ref_.name)
                    .await
                {
                    if !indexes.is_empty() {
                        sections.push(PropertySection {
                            label: format!("索引 ({})", indexes.len()),
                            table: indexes_table(&indexes),
                        });
                    }
                }
                if let Ok(constraints) = metadata
                    .list_constraints(&ref_.conn_id, &catalog, &schema, &ref_.name)
                    .await
                {
                    if !constraints.is_empty() {
                        sections.push(PropertySection {
                            label: format!("约束 ({})", constraints.len()),
                            table: constraints_table(&constraints),
                        });
                    }
                }
            }
        }
        PropertyKind::Column => {
            let catalog = ref_.catalog.clone().unwrap_or_default();
            let schema = ref_.schema.clone().unwrap_or_default();
            let parent = ref_.parent.clone().unwrap_or_default();
            let columns = metadata
                .list_columns(&ref_.conn_id, &catalog, &schema, &parent)
                .await?;
            if let Some(col) = columns.iter().find(|c| c.name == ref_.name) {
                properties.push(row("名称", col.name.clone()));
                properties.push(row("所属表", qualify(ref_)));
                properties.push(row("类型", col.data_type.clone()));
                properties.push(row("可空", if col.nullable { "是" } else { "否" }));
                properties.push(row("主键", if col.is_primary_key { "是" } else { "否" }));
                properties.push(row("外键", if col.is_foreign_key { "是" } else { "否" }));
                properties.push(row("默认值", opt(&col.default_value)));
                properties.push(row("注释", opt(&col.comment)));
            } else {
                properties.push(row("名称", ref_.name.clone()));
                properties.push(row("所属表", qualify(ref_)));
                properties.push(row("提示", "列已不存在（结构可能已变更）"));
            }
        }
        PropertyKind::Sequence | PropertyKind::Trigger => {
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("限定名", qualify(ref_)));
            properties.push(row("类型", object_type.clone()));
            properties.push(row("归属域", source.clone()));
        }
        PropertyKind::Routine => {
            let catalog = ref_.catalog.clone().unwrap_or_default();
            let schema = ref_.schema.clone().unwrap_or_default();
            properties.push(row("名称", ref_.name.clone()));
            properties.push(row("限定名", qualify(ref_)));
            properties.push(row("类型", object_type.clone()));
            properties.push(row("归属域", source.clone()));

            // `PropertyRef` 不带例程种类（过程 / 函数），而驱动按种类走不同语句：
            // 先按存储过程取，未命中（`None` 或语句报错）再按函数取。
            let mut source_text = metadata
                .get_routine_source(
                    &ref_.conn_id,
                    &catalog,
                    &schema,
                    &ref_.name,
                    SchemaObjectKind::Procedure,
                )
                .await
                .ok()
                .flatten();
            if source_text.is_none() {
                source_text = metadata
                    .get_routine_source(
                        &ref_.conn_id,
                        &catalog,
                        &schema,
                        &ref_.name,
                        SchemaObjectKind::Function,
                    )
                    .await
                    .ok()
                    .flatten();
            }
            match source_text {
                Some(text) => sections.push(PropertySection {
                    label: "源码".to_string(),
                    table: source_table(&text),
                }),
                None => properties.push(row("源码", "（未能获取）")),
            }
        }
    }

    Ok(ObjectProperties {
        title,
        object_type,
        source_label: source,
        properties,
        sections,
    })
}

/// 源码 / DDL 段落：每行一条记录（单列），保留换行。
fn source_table(text: &str) -> PropertyTable {
    let headers = vec!["源码".to_string()];
    let rows = text
        .lines()
        .map(|line| vec![line.to_string()])
        .collect::<Vec<_>>();
    // 空文本（或全空白）时给出占位，避免分区标题下无内容。
    let rows = if rows
        .iter()
        .all(|r| r.first().map(|s| s.trim().is_empty()).unwrap_or(true))
    {
        vec![vec!["（空）".to_string()]]
    } else {
        rows
    };
    PropertyTable { headers, rows }
}

fn columns_table(columns: &[engine::driver::traits::ColumnDetail]) -> PropertyTable {
    let headers = ["#", "名称", "类型", "非空", "主键", "外键", "默认", "注释"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            vec![
                (i + 1).to_string(),
                c.name.clone(),
                c.data_type.clone(),
                if c.nullable { "" } else { "✓" }.to_string(),
                if c.is_primary_key { "PK" } else { "" }.to_string(),
                if c.is_foreign_key { "FK" } else { "" }.to_string(),
                c.default_value.clone().unwrap_or_default(),
                c.comment.clone().unwrap_or_default(),
            ]
        })
        .collect();
    PropertyTable { headers, rows }
}

fn indexes_table(indexes: &[engine::driver::traits::IndexDetail]) -> PropertyTable {
    let headers = ["名称", "类型", "唯一", "主键", "包含列", "注释"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = indexes
        .iter()
        .map(|idx| {
            vec![
                idx.name.clone(),
                idx.index_type.clone().unwrap_or_default(),
                if idx.is_unique { "是" } else { "否" }.to_string(),
                if idx.is_primary { "是" } else { "否" }.to_string(),
                idx.column_names.join(", "),
                idx.comment.clone().unwrap_or_default(),
            ]
        })
        .collect();
    PropertyTable { headers, rows }
}

fn constraints_table(constraints: &[engine::driver::traits::ConstraintDetail]) -> PropertyTable {
    let headers = ["名称", "类型", "列", "引用表", "引用列", "更新", "删除"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = constraints
        .iter()
        .map(|c| {
            vec![
                c.name.clone(),
                c.constraint_type.clone(),
                c.column_names.join(", "),
                c.referenced_table.clone().unwrap_or_default(),
                c.referenced_columns.join(", "),
                c.update_rule.clone().unwrap_or_default(),
                c.delete_rule.clone().unwrap_or_default(),
            ]
        })
        .collect();
    PropertyTable { headers, rows }
}

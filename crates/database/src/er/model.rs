//! ER 输入模型与「元数据 → 模型」桥。
//!
//! 纯函数层：无 I/O、无 GPUI 依赖、不直连库、不碰凭据。输入来自宿主已落地的元数据服务
//! （`MetadataService`：表 / 列 / 约束），与 sidecar 的 `meta.*` 五件套同形状
//! （`ColumnDetail` / `ConstraintDetail` / `NodeInfo`）。输出是只读的 ER 图结构，
//! 供 [`crate::er::layout`] 布局与后续渲染层消费。
//!
//! 桥有两条输出路径：
//! - **约束**：`ConstraintDetail`（`FOREIGN KEY`）→ 一条关系，带约束名 / 级联规则；
//! - **列引用**：`ColumnDetail::references` → 兜底关系（约束缺失、驱动只给列级引用时）。

use std::collections::BTreeSet;

use engine::driver::traits::{ColumnDetail, ConstraintDetail, SchemaObjectKind};

/// 一列在 ER 视图里的只读投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// 是否参与外键。
    ///
    /// 与 [`ErColumn::references`] 的分工同 `ColumnDetail`：本字段答「有没有」，
    /// `references` 答「引到哪」。渲染端判断外键要读本字段，不要读 `references.is_none()`。
    pub is_foreign_key: bool,
    /// 引向的目标端点；`None` = 不引用，或驱动只知「有外键」拿不到目标。
    pub references: Option<ErColumnRef>,
}

/// 外键引用的目标端点（表 / 列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErColumnRef {
    pub table: String,
    pub column: String,
}

/// 实体（表 / 视图）的唯一键，形式为 `schema.name`；无 schema 层时退化为 `name`。
///
/// 只经 [`ErEntity`] 构造，字段由其保持自洽（不在别处直接拼串）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityKey(String);

impl EntityKey {
    /// 由 schema + 短名构造（schema 为空时只取短名）。
    pub fn new(schema: &str, name: &str) -> Self {
        if schema.is_empty() {
            Self(name.to_string())
        } else {
            Self(format!("{schema}.{name}"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 展示短名（最后一段）。
    ///
    /// 带点的短名（带引号标识符）会被截断——第一期接受：驱动端内省口径通常不带点，
    /// 且此处只用于显示，身份判定一律走 [`EntityKey`] 全串。
    pub fn display_name(&self) -> &str {
        self.0.rsplit('.').next().unwrap_or(&self.0)
    }
}

/// 一个实体：表 / 视图 + 其列。
#[derive(Debug, Clone)]
pub struct ErEntity {
    pub schema: String,
    pub name: String,
    pub key: EntityKey,
    pub is_view: bool,
    pub columns: Vec<ErColumn>,
}

impl ErEntity {
    fn new(schema: &str, name: &str, kind: &SchemaObjectKind, columns: Vec<ErColumn>) -> Self {
        Self {
            schema: schema.to_string(),
            name: name.to_string(),
            key: EntityKey::new(schema, name),
            is_view: matches!(kind, SchemaObjectKind::View),
            columns,
        }
    }
}

/// 一条外键关系（子表列 → 父表列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErRelation {
    /// 引用方（子表）。
    pub from: EntityKey,
    pub from_columns: Vec<String>,
    /// 被引用方（父表）。
    pub to: EntityKey,
    pub to_columns: Vec<String>,
    /// 约束名；列级兜底关系为 `None`。
    pub name: Option<String>,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

/// ER 图：实体 + 关系。
#[derive(Debug, Clone, Default)]
pub struct ErDiagram {
    pub entities: Vec<ErEntity>,
    pub relations: Vec<ErRelation>,
}

impl ErDiagram {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// 桥的输入：一张表 / 视图的元数据切片（宿主从 `MetadataService` 取）。
///
/// 刻意不做 `NodeInfo` → 本类型的强绑定转换：导航树与 ER 是两处关注点，
/// 调用方按 schema / name / kind 三要素填即可，避免为复用而耦合两套模型。
#[derive(Debug, Clone)]
pub struct TableInput {
    pub schema: String,
    pub name: String,
    pub kind: SchemaObjectKind,
    pub columns: Vec<ColumnDetail>,
    pub constraints: Vec<ConstraintDetail>,
}

/// 把若干表的元数据切片组装成 ER 图（确定性：输入顺序 → 输出顺序）。
///
/// 规则：
/// - 实体键 = `schema.name`；同名输入只保留首个；
/// - 列按 `ordinal`（1 基）排序；全为 0（驱动没给）时保持输入顺序；
/// - 关系两来源：① `FOREIGN KEY` 约束（带名字 / 级联）；② 列上的 `references`（兜底）。
///   同表内已被约束覆盖的列不再走 ②，避免同一条外键出两条关系；
/// - 端点不在本次集合内的关系被丢弃（跨 schema / 未加载表），保证图自洽。
pub fn build_diagram(inputs: &[TableInput]) -> ErDiagram {
    // 先建实体，登记存在集合（关系只连集合内的端点）
    let mut entities = Vec::with_capacity(inputs.len());
    let mut present: BTreeSet<EntityKey> = BTreeSet::new();
    for input in inputs {
        let key = EntityKey::new(&input.schema, &input.name);
        if !present.insert(key) {
            continue;
        }
        entities.push(ErEntity::new(
            &input.schema,
            &input.name,
            &input.kind,
            columns_of(&input.columns),
        ));
    }

    let mut relations: Vec<ErRelation> = Vec::new();
    // 去重键：(子表, 父表, 子表列组)——多列外键按整组算一条
    let mut seen: BTreeSet<(EntityKey, EntityKey, Vec<String>)> = BTreeSet::new();

    for input in inputs {
        let from = EntityKey::new(&input.schema, &input.name);
        if !present.contains(&from) {
            continue;
        }

        // ① 外键约束（先跑，登记的列在 ② 里跳过）
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for c in &input.constraints {
            if !is_foreign_key_constraint(&c.constraint_type) || c.column_names.is_empty() {
                continue;
            }
            let Some(target_table) = c.referenced_table.as_deref() else {
                continue;
            };
            let to = EntityKey::new(&input.schema, target_table);
            if !present.contains(&to) {
                continue;
            }
            let from_columns = c.column_names.clone();
            if !seen.insert((from.clone(), to.clone(), from_columns.clone())) {
                continue;
            }
            covered.extend(from_columns.iter().cloned());
            relations.push(ErRelation {
                from: from.clone(),
                from_columns,
                to,
                to_columns: c.referenced_columns.clone(),
                name: Some(c.name.clone()),
                on_delete: c.delete_rule.clone(),
                on_update: c.update_rule.clone(),
            });
        }

        // ② 列级引用兜底（驱动只给列上 references、没给约束时）
        for col in &input.columns {
            let Some(r) = &col.references else { continue };
            if covered.contains(&col.name) {
                continue;
            }
            let to = EntityKey::new(&input.schema, &r.table);
            if !present.contains(&to) {
                continue;
            }
            let from_columns = vec![col.name.clone()];
            if !seen.insert((from.clone(), to.clone(), from_columns.clone())) {
                continue;
            }
            relations.push(ErRelation {
                from: from.clone(),
                from_columns,
                to,
                to_columns: vec![r.column.clone()],
                name: None,
                on_delete: None,
                on_update: None,
            });
        }
    }

    ErDiagram { entities, relations }
}

/// `ColumnDetail` → `ErColumn`，按 `ordinal` 排序（全 0 时保持输入顺序）。
fn columns_of(cols: &[ColumnDetail]) -> Vec<ErColumn> {
    let mut ordered: Vec<&ColumnDetail> = cols.iter().collect();
    if ordered.iter().any(|c| c.ordinal > 0) {
        // 稳定排序：ordinal 相同者保持输入顺序
        ordered.sort_by_key(|c| c.ordinal);
    }
    ordered
        .into_iter()
        .map(|c| ErColumn {
            name: c.name.clone(),
            data_type: c.data_type.clone(),
            nullable: c.nullable,
            is_primary_key: c.is_primary_key,
            is_foreign_key: c.is_foreign_key,
            references: c.references.as_ref().map(|r| ErColumnRef {
                table: r.table.clone(),
                column: r.column.clone(),
            }),
        })
        .collect()
}

/// 约束类型是否外键（各家驱动字符串不一：多为 `FOREIGN KEY`，少数简写 `FK`）。
fn is_foreign_key_constraint(constraint_type: &str) -> bool {
    let t = constraint_type.trim().to_ascii_uppercase();
    t.contains("FOREIGN") || t == "FK"
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use engine::driver::traits::{ForeignKeyRef, SchemaObjectKind};

    use super::*;

    fn col(name: &str) -> ColumnDetail {
        ColumnDetail {
            name: name.to_string(),
            data_type: "text".to_string(),
            nullable: true,
            is_primary_key: false,
            is_foreign_key: false,
            references: None,
            ordinal: 0,
            default_value: None,
            comment: None,
            is_identity: false,
            identity_generation: None,
            collation: None,
            extra: HashMap::new(),
        }
    }

    fn fk_constraint(name: &str, cols: &[&str], target: &str, target_cols: &[&str]) -> ConstraintDetail {
        ConstraintDetail {
            name: name.to_string(),
            table_name: String::new(),
            constraint_type: "FOREIGN KEY".to_string(),
            column_names: cols.iter().map(|s| s.to_string()).collect(),
            referenced_table: Some(target.to_string()),
            referenced_columns: target_cols.iter().map(|s| s.to_string()).collect(),
            update_rule: None,
            delete_rule: Some("CASCADE".to_string()),
        }
    }

    fn table(schema: &str, name: &str, columns: Vec<ColumnDetail>, constraints: Vec<ConstraintDetail>) -> TableInput {
        TableInput {
            schema: schema.to_string(),
            name: name.to_string(),
            kind: SchemaObjectKind::Table,
            columns,
            constraints,
        }
    }

    #[test]
    fn builds_entities_and_keeps_input_order() {
        let d = build_diagram(&[
            table("public", "orders", vec![col("id")], vec![]),
            table("public", "users", vec![col("id")], vec![]),
        ]);
        assert_eq!(d.entities.len(), 2);
        assert_eq!(d.entities[0].name, "orders");
        assert_eq!(d.entities[1].name, "users");
        assert_eq!(d.entities[0].key.as_str(), "public.orders");
        assert!(d.relations.is_empty());
    }

    #[test]
    fn columns_sorted_by_ordinal_when_present() {
        let mut a = col("late");
        a.ordinal = 2;
        let mut b = col("first");
        b.ordinal = 1;
        let d = build_diagram(&[table("s", "t", vec![a, b], vec![])]);
        let names: Vec<_> = d.entities[0].columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["first", "late"]);
    }

    #[test]
    fn columns_keep_input_order_when_ordinal_absent() {
        let d = build_diagram(&[table("s", "t", vec![col("beta"), col("alpha")], vec![])]);
        let names: Vec<_> = d.entities[0].columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["beta", "alpha"]);
    }

    #[test]
    fn foreign_key_constraint_becomes_relation() {
        let mut uid = col("user_id");
        uid.ordinal = 2;
        let mut id = col("id");
        id.ordinal = 1;
        let d = build_diagram(&[
            table("public", "users", vec![id.clone()], vec![]),
            table(
                "public",
                "orders",
                vec![id, uid],
                vec![fk_constraint("fk_orders_user", &["user_id"], "users", &["id"])],
            ),
        ]);
        assert_eq!(d.relations.len(), 1);
        let r = &d.relations[0];
        assert_eq!(r.from.as_str(), "public.orders");
        assert_eq!(r.to.as_str(), "public.users");
        assert_eq!(r.from_columns, vec!["user_id"]);
        assert_eq!(r.to_columns, vec!["id"]);
        assert_eq!(r.name.as_deref(), Some("fk_orders_user"));
        assert_eq!(r.on_delete.as_deref(), Some("CASCADE"));
    }

    #[test]
    fn column_references_are_fallback_without_constraint() {
        let mut uid = col("user_id");
        uid.is_foreign_key = true;
        uid.references = Some(ForeignKeyRef {
            table: "users".to_string(),
            column: "id".to_string(),
        });
        let d = build_diagram(&[
            table("public", "users", vec![col("id")], vec![]),
            table("public", "orders", vec![uid], vec![]),
        ]);
        assert_eq!(d.relations.len(), 1);
        assert_eq!(d.relations[0].from_columns, vec!["user_id"]);
        assert_eq!(d.relations[0].name, None);
    }

    #[test]
    fn constraint_wins_over_column_reference() {
        // 同一列既有约束又有列级引用：只出一关系（约束优先，带名）
        let mut uid = col("user_id");
        uid.is_foreign_key = true;
        uid.references = Some(ForeignKeyRef {
            table: "users".to_string(),
            column: "id".to_string(),
        });
        let d = build_diagram(&[
            table("public", "users", vec![col("id")], vec![]),
            table(
                "public",
                "orders",
                vec![uid],
                vec![fk_constraint("fk_orders_user", &["user_id"], "users", &["id"])],
            ),
        ]);
        assert_eq!(d.relations.len(), 1);
        assert_eq!(d.relations[0].name.as_deref(), Some("fk_orders_user"));
    }

    #[test]
    fn composite_constraint_is_one_relation_and_suppresses_column_refs() {
        // 复合外键：约束出一条；参与列上的 references 不再各出一条
        let mut a = col("a");
        a.is_foreign_key = true;
        a.references = Some(ForeignKeyRef { table: "p".to_string(), column: "a".to_string() });
        let mut b = col("b");
        b.is_foreign_key = true;
        b.references = Some(ForeignKeyRef { table: "p".to_string(), column: "b".to_string() });
        let d = build_diagram(&[
            table("s", "p", vec![col("a"), col("b")], vec![]),
            table("s", "c", vec![a, b], vec![fk_constraint("fk_c_p", &["a", "b"], "p", &["a", "b"])]),
        ]);
        assert_eq!(d.relations.len(), 1);
        assert_eq!(d.relations[0].from_columns, vec!["a", "b"]);
    }

    #[test]
    fn dangling_relation_is_dropped() {
        // 引用一张不在本次集合内的表（跨 schema / 未加载）→ 丢关系
        let mut uid = col("user_id");
        uid.is_foreign_key = true;
        uid.references = Some(ForeignKeyRef {
            table: "other_schema_users".to_string(),
            column: "id".to_string(),
        });
        let d = build_diagram(&[table("public", "orders", vec![uid], vec![])]);
        assert!(d.relations.is_empty());
    }

    #[test]
    fn view_kind_is_flagged() {
        let mut input = table("public", "v_orders", vec![col("id")], vec![]);
        input.kind = SchemaObjectKind::View;
        let d = build_diagram(&[input]);
        assert!(d.entities[0].is_view);
    }

    #[test]
    fn duplicate_table_input_kept_once() {
        let d = build_diagram(&[
            table("public", "orders", vec![col("id")], vec![]),
            table("public", "orders", vec![col("id"), col("x")], vec![]),
        ]);
        assert_eq!(d.entities.len(), 1);
        assert_eq!(d.entities[0].columns.len(), 1);
    }

    #[test]
    fn schema_less_key_degrades_to_name() {
        let d = build_diagram(&[table("", "main_tbl", vec![col("id")], vec![])]);
        assert_eq!(d.entities[0].key.as_str(), "main_tbl");
        assert_eq!(d.entities[0].key.display_name(), "main_tbl");
    }
}

//! `meta.*`：驱动自述的元数据面（dev-plan §4.2.2）。
//!
//! 导航树的三个文件夹、`#` 内容档、属性面板都从这里取数据。这一层只做一件事：
//! **把线格式翻成引擎类型**，别的一律不做（不发 RPC、不管会话、不认识 supervisor）。
//!
//! | 方法 | 返回 | 主要消费者 |
//! | --- | --- | --- |
//! | `meta.catalogs` | `[string]` | 导航第一层、`insight` 的存在性校验 |
//! | `meta.schemas` | `[SchemaInfo]` | 导航第二层（`schemas` 能力为假时不出现这一层） |
//! | `meta.objects` | `[ObjectInfo]` | 表 / 视图 / 例程 / 序列 / 触发器五个文件夹 |
//! | `meta.object_detail` | `ObjectDetail` | 列（展开表）、属性面板 |
//! | `meta.routine_source` | `string?` | 例程源码查看 |
//!
//! # 三条口径
//!
//! 1. **`meta.objects` 一次给全，含 `kind`**：五个文件夹共用**一次**内省。对端别按文件夹分家，
//!    否则宿主要跑五遍内省，而它们在内省层面本来就是一条 `information_schema` 查询。
//! 2. **认不出的类别不丢也不摆**：装进 [`MetaObjectKind::Other`]，宿主记一条日志后跳过
//!    （见 [`MetaObject::into_node_info`]）——摆进树会被画成「表」，那是假信息；直接丢掉，
//!    又没人知道驱动报了新东西。
//! 3. **形状不对就是协议错**：缺 `columns`、行不是对象、`kind` 不是字符串 —— 一律
//!    [`DriverError::Protocol`]，并说清是哪个字段。不猜、不补默认值。

use serde_json::Value;

use engine::driver::{ColumnDetail, NodeInfo, SchemaObjectKind};

use super::driver::DriverError;

/// 一个 schema（`meta.schemas` 的元素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaSchema {
    pub name: String,
    pub comment: Option<String>,
}

/// 对象类别（`meta.objects` 里每个对象的 `kind`）。
///
/// 线格式是 snake_case 字符串。**物化视图单独一类**：导航侧把它并到「视图」文件夹，
/// 但原始类别不能丢（属性面板要说实话）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaObjectKind {
    Table,
    View,
    MaterializedView,
    Procedure,
    Function,
    Sequence,
    Trigger,
    /// 导航摆不下的类别（`domain` / `enum` / `type` / `synonym` …）。
    ///
    /// 驱动**应当**只报上面七种；报了别的，宿主留痕但不摆进树。
    Other(String),
}

impl MetaObjectKind {
    pub fn from_wire(kind: &str) -> Self {
        match kind {
            "table" => Self::Table,
            "view" => Self::View,
            // 两种写法都收：「materialized_view」是协议写法，空格那份是对端顺手写的
            "materialized_view" | "materialized view" => Self::MaterializedView,
            "procedure" => Self::Procedure,
            "function" => Self::Function,
            "sequence" => Self::Sequence,
            "trigger" => Self::Trigger,
            other => Self::Other(other.to_string()),
        }
    }

    /// 回线格式的写法（`Other` 原样带回）。
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Table => "table",
            Self::View => "view",
            Self::MaterializedView => "materialized_view",
            Self::Procedure => "procedure",
            Self::Function => "function",
            Self::Sequence => "sequence",
            Self::Trigger => "trigger",
            Self::Other(other) => other.as_str(),
        }
    }

    /// 能摆进「表 / 视图」两处之一吗（物化视图算视图）。
    pub fn is_table_like(&self) -> bool {
        matches!(self, Self::Table | Self::View | Self::MaterializedView)
    }
}

/// 一个对象（`meta.objects` 的元素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaObject {
    pub name: String,
    pub kind: MetaObjectKind,
    pub comment: Option<String>,
    /// 关联父对象（触发器的所属表；其他类别为空）。
    pub parent: Option<String>,
}

impl MetaObject {
    /// 导航树里的类别；摆不平的类别返回 `None`。
    ///
    /// **物化视图归到 [`SchemaObjectKind::View`]**：导航只有五个文件夹，物化视图与视图是同一个
    /// 使用动作（点开看数据）；单开一层要多动导航与定位两处，收益不抵。原始类别在
    /// [`MetaObject::kind`] 里没丢，属性面板按它显示。
    pub fn nav_kind(&self) -> Option<SchemaObjectKind> {
        match self.kind {
            MetaObjectKind::Table => Some(SchemaObjectKind::Table),
            MetaObjectKind::View | MetaObjectKind::MaterializedView => Some(SchemaObjectKind::View),
            MetaObjectKind::Procedure => Some(SchemaObjectKind::Procedure),
            MetaObjectKind::Function => Some(SchemaObjectKind::Function),
            MetaObjectKind::Sequence => Some(SchemaObjectKind::Sequence),
            MetaObjectKind::Trigger => Some(SchemaObjectKind::Trigger),
            MetaObjectKind::Other(_) => None,
        }
    }

    /// 转成引擎的对象项；摆不平的类别返回 `None`（并留一条日志）。
    pub fn into_node_info(self) -> Option<NodeInfo> {
        let Some(kind) = self.nav_kind() else {
            tracing::warn!(
                object = %self.name,
                kind = %self.kind.as_wire(),
                "驱动报了一个导航摆不下的对象类别，已跳过（摆进树会变成假信息）"
            );
            return None;
        };
        let mut node = NodeInfo::new(self.name, kind).with_comment(self.comment);
        if let Some(parent) = self.parent {
            node = node.with_parent(parent);
        }
        Some(node)
    }
}

/// 一列（`meta.object_detail` 的 `columns` 元素）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaColumn {
    pub name: String,
    /// 厂商原始类型名（属性面板与用户可见的那一份）。
    pub type_raw: String,
    /// 归一化类型（`DECIMAL` / `TIMESTAMP` …）；喂 mock / insight / 质量分用（§4.5.1）。
    pub canonical: Option<String>,
    pub nullable: bool,
    pub is_pk: bool,
    pub is_fk: bool,
    pub default_value: Option<String>,
    /// 单元格格式化提示（`decimal(scale=10)`）。
    pub format: Option<String>,
    pub comment: Option<String>,
}

impl MetaColumn {
    /// 转成引擎的列详情。
    ///
    /// `data_type` 放**原始类型名**（用户看得懂，也是既有原生驱动的口径）；
    /// `canonical` / `format` 进 `extra` —— 类型归一化的消费者（schema 分析、mock）
    /// 从 `extra` 取，不让「属性面板显示什么」与「程序怎么判类型」互相迁就。
    pub fn into_column_detail(self) -> ColumnDetail {
        let mut extra = std::collections::HashMap::new();
        if let Some(canonical) = &self.canonical {
            extra.insert("canonical".to_string(), canonical.clone());
        }
        if let Some(format) = &self.format {
            extra.insert("format".to_string(), format.clone());
        }
        ColumnDetail {
            name: self.name,
            data_type: self.type_raw,
            nullable: self.nullable,
            is_primary_key: self.is_pk,
            is_foreign_key: self.is_fk,
            default_value: self.default_value,
            comment: self.comment,
            extra,
            // sidecar 线格式（`meta.object_detail`）不带这两样：列序靠数组位置表达，
            // 外键只知道「有没有」（`is_fk`）—— 要真值得先扩协议，不在这里猜
            references: None,
            ordinal: 0,
        }
    }
}

/// 对象详情（`meta.object_detail`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaObjectDetail {
    /// 对象自己的身份：属性面板靠它决定显示成「表」还是「视图」。
    pub object: MetaObject,
    pub columns: Vec<MetaColumn>,
    /// 索引个数（`None` = 驱动没给；属性面板据此显示「未知」而不是「0 个」）。
    pub indexes: Option<u32>,
    /// 行数估计（不是精确值；驱动自己说得清来源）。
    pub row_count: Option<u64>,
}

// ---------------------------------------------------------------------------
// 解析：线格式 → 上面的类型
// ---------------------------------------------------------------------------

fn protocol(detail: impl Into<String>) -> DriverError {
    DriverError::Protocol {
        detail: detail.into(),
    }
}

/// 取一个必须是数组的字段。缺字段 = 协议错 —— **不静默当空**：
/// 「空」与「对端没说」在导航上是两回事（一个文件夹是空还是没这个能力）。
fn array<'a>(result: &'a Value, field: &str) -> Result<&'a Vec<Value>, DriverError> {
    result
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| protocol(format!("响应里没有 {field} 数组")))
}

fn string_field(obj: &Value, field: &str, context: &str) -> Result<String, DriverError> {
    obj.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| protocol(format!("{context} 的 {field} 不是非空字符串")))
}

fn opt_string(obj: &Value, field: &str) -> Option<String> {
    obj.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn flag(obj: &Value, field: &str, default: bool) -> bool {
    obj.get(field).and_then(Value::as_bool).unwrap_or(default)
}

/// `meta.catalogs` → `[string]`。
pub fn parse_catalogs(result: &Value) -> Result<Vec<String>, DriverError> {
    array(result, "catalogs")?
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .ok_or_else(|| protocol("catalogs 的元素必须是字符串（catalog 名）"))
        })
        .collect()
}

/// `meta.schemas` → `[SchemaInfo]`。
pub fn parse_schemas(result: &Value) -> Result<Vec<MetaSchema>, DriverError> {
    array(result, "schemas")?
        .iter()
        .map(|item| {
            Ok(MetaSchema {
                name: string_field(item, "name", "schemas 的元素")?,
                comment: opt_string(item, "comment"),
            })
        })
        .collect()
}

/// `meta.objects` → `[ObjectInfo]`。
pub fn parse_objects(result: &Value) -> Result<Vec<MetaObject>, DriverError> {
    array(result, "objects")?
        .iter()
        .map(|item| {
            let name = string_field(item, "name", "objects 的元素")?;
            let kind = string_field(item, "kind", "objects 的元素")?;
            Ok(MetaObject {
                name,
                kind: MetaObjectKind::from_wire(&kind),
                comment: opt_string(item, "comment"),
                parent: opt_string(item, "parent"),
            })
        })
        .collect()
}

/// `meta.object_detail` → [`MetaObjectDetail`]。
pub fn parse_object_detail(result: &Value) -> Result<MetaObjectDetail, DriverError> {
    let raw_object = result
        .get("object")
        .filter(|v| v.is_object())
        .ok_or_else(|| protocol("object_detail 响应里没有 object（属性面板要靠它定类别）"))?;
    let object = MetaObject {
        name: string_field(raw_object, "name", "object_detail 的 object")?,
        kind: MetaObjectKind::from_wire(&string_field(
            raw_object,
            "kind",
            "object_detail 的 object",
        )?),
        comment: opt_string(raw_object, "comment"),
        parent: opt_string(raw_object, "parent"),
    };

    let columns = array(result, "columns")?
        .iter()
        .map(|item| {
            let context = "object_detail 的 columns 元素";
            Ok(MetaColumn {
                name: string_field(item, "name", context)?,
                // 列类型拿不到就退回列名不合适，但**报错也不合适**：属性面板少一列类型
                // 比整张详情打不开强。这里用 `unknown`，与 `parse_page` 同一条口径。
                type_raw: opt_string(item, "type_raw").unwrap_or_else(|| "unknown".to_string()),
                canonical: opt_string(item, "canonical"),
                nullable: flag(item, "nullable", true),
                is_pk: flag(item, "is_pk", false),
                is_fk: flag(item, "is_fk", false),
                default_value: opt_string(item, "default"),
                format: opt_string(item, "format"),
                comment: opt_string(item, "comment"),
            })
        })
        .collect::<Result<Vec<_>, DriverError>>()?;

    Ok(MetaObjectDetail {
        object,
        columns,
        indexes: result
            .get("indexes")
            .and_then(Value::as_u64)
            .map(|n| n as u32),
        row_count: result.get("row_count").and_then(Value::as_u64),
    })
}

/// `meta.routine_source` → `string?`（`null` = 驱动查不到这个例程）。
pub fn parse_routine_source(result: &Value) -> Result<Option<String>, DriverError> {
    match result {
        Value::Null => Ok(None),
        Value::String(source) => Ok(Some(source.clone())),
        // 形状只留一种：包一层对象看着无害，但多一种形状就是两份契约
        // （§3.5「契约重复两份」的教训），下一个人会照着他见过的那个写。
        _ => Err(protocol("routine_source 的响应必须是字符串或 null")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalogs_are_strings_and_a_missing_field_is_a_protocol_error() {
        let parsed = parse_catalogs(&json!({ "catalogs": ["main", "demo"] })).unwrap();
        assert_eq!(parsed, vec!["main", "demo"]);
        // 「空」与「对端没说」是两回事：缺字段不静默当空
        assert!(matches!(
            parse_catalogs(&json!({})),
            Err(DriverError::Protocol { .. })
        ));
        assert!(matches!(
            parse_catalogs(&json!({ "catalogs": [1] })),
            Err(DriverError::Protocol { .. })
        ));
    }

    #[test]
    fn schemas_carry_their_comments() {
        let schemas = parse_schemas(&json!({
            "schemas": [
                { "name": "public", "comment": "默认 schema" },
                { "name": "sales" },
            ],
        }))
        .unwrap();
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].comment.as_deref(), Some("默认 schema"));
        assert_eq!(schemas[1].comment, None);
        assert!(matches!(
            parse_schemas(&json!({ "schemas": [{ "comment": "没名字" }] })),
            Err(DriverError::Protocol { .. })
        ));
    }

    #[test]
    fn unknown_kinds_are_kept_on_the_wire_but_not_put_in_the_tree() {
        let objects = parse_objects(&json!({
            "objects": [
                { "name": "orders", "kind": "table", "comment": "订单" },
                { "name": "recent_orders", "kind": "view" },
                { "name": "mv_daily", "kind": "materialized_view" },
                { "name": "trg_audit", "kind": "trigger", "parent": "orders" },
                { "name": "order_state", "kind": "domain" },
            ],
        }))
        .unwrap();

        assert_eq!(objects.len(), 5, "认不出的类别也要解出来（丢了就没人知道驱动报了新东西）");
        assert!(objects.iter().any(|o| o.kind == MetaObjectKind::Other("domain".into())));

        let nodes: Vec<NodeInfo> = objects
            .into_iter()
            .filter_map(MetaObject::into_node_info)
            .collect();
        assert_eq!(nodes.len(), 4, "domain 摆不进五个文件夹，应当被跳过");
        assert_eq!(nodes[0].kind, SchemaObjectKind::Table);
        assert_eq!(nodes[0].comment.as_deref(), Some("订单"));
        assert_eq!(nodes[2].kind, SchemaObjectKind::View, "物化视图归到视图");
        assert_eq!(nodes[3].parent_name.as_deref(), Some("orders"));
    }

    #[test]
    fn kinds_round_trip_on_the_wire() {
        for raw in [
            "table",
            "view",
            "materialized_view",
            "procedure",
            "function",
            "sequence",
            "trigger",
        ] {
            assert_eq!(MetaObjectKind::from_wire(raw).as_wire(), raw);
        }
        assert_eq!(
            MetaObjectKind::from_wire("materialized view").as_wire(),
            "materialized_view"
        );
        assert_eq!(MetaObjectKind::from_wire("synonym").as_wire(), "synonym");
        assert!(MetaObjectKind::from_wire("table").is_table_like());
        assert!(!MetaObjectKind::from_wire("sequence").is_table_like());
    }

    #[test]
    fn object_detail_requires_an_object_but_tolerates_a_bare_column() {
        let detail = parse_object_detail(&json!({
            "object": { "name": "orders", "kind": "table" },
            "columns": [
                { "name": "id", "type_raw": "bigint", "canonical": "BIGINT", "nullable": false, "is_pk": true },
                { "name": "amount", "type_raw": "numeric(38,10)", "canonical": "DECIMAL",
                  "format": "decimal(scale=10)", "comment": "订单金额" },
                { "name": "note" },
            ],
            "indexes": 3,
            "row_count": 12000,
        }))
        .unwrap();

        assert_eq!(detail.object.kind, MetaObjectKind::Table);
        assert_eq!(detail.indexes, Some(3));
        assert_eq!(detail.row_count, Some(12000));
        // 少写 type_raw 的列退化成 unknown：属性面板少一列类型，好过整张详情打不开
        assert_eq!(detail.columns[2].type_raw, "unknown");
        assert!(detail.columns[2].nullable && !detail.columns[2].is_pk, "默认值取宽松那份");

        let column = detail.columns[1].clone().into_column_detail();
        assert_eq!(column.data_type, "numeric(38,10)", "用户看到的是原始类型名");
        assert_eq!(
            column.extra.get("canonical").map(String::as_str),
            Some("DECIMAL")
        );
        assert_eq!(
            column.extra.get("format").map(String::as_str),
            Some("decimal(scale=10)")
        );
        assert_eq!(column.comment.as_deref(), Some("订单金额"));
        assert!(detail.columns[0].clone().into_column_detail().is_primary_key);

        // 没有 object = 协议错：属性面板没法定类别
        assert!(matches!(
            parse_object_detail(&json!({ "columns": [] })),
            Err(DriverError::Protocol { .. })
        ));
        assert!(matches!(
            parse_object_detail(&json!({
                "object": { "name": "orders", "kind": "table" },
            })),
            Err(DriverError::Protocol { .. })
        ));
    }

    #[test]
    fn routine_source_is_a_string_or_null() {
        let source = parse_routine_source(&json!("CREATE FUNCTION order_total()"));
        assert_eq!(source.unwrap().as_deref(), Some("CREATE FUNCTION order_total()"));
        assert_eq!(parse_routine_source(&Value::Null).unwrap(), None);
        // 包一层对象不是协议：形状只留一种
        assert!(matches!(
            parse_routine_source(&json!({ "source": "x" })),
            Err(DriverError::Protocol { .. })
        ));
    }
}

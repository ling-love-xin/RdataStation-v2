use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MetaSchema {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetaObject {
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetaColumn {
    pub name: String,
    pub type_raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pk: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_fk: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetaIndex {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique: Option<bool>,
    #[serde(default)]
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetaObjectDetail {
    pub object: MetaObjectRef,
    pub columns: Vec<MetaColumn>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<MetaIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetaObjectRef {
    pub name: String,
    pub kind: String,
}

impl MetaObjectDetail {
    pub fn new(name: impl Into<String>, kind: impl Into<String>, columns: Vec<MetaColumn>) -> Self {
        Self {
            object: MetaObjectRef {
                name: name.into(),
                kind: kind.into(),
            },
            columns,
            indexes: Vec::new(),
            row_count: None,
        }
    }
}

/// 把数据库自己的对象类型收敛到协议白名单；认不出的返回 None（调用方跳过并记日志）。
#[allow(dead_code)]
pub fn normalize_kind(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_uppercase().replace(' ', "_").as_str() {
        "TABLE" | "U" | "BASE_TABLE" => Some("table"),
        "VIEW" | "V" => Some("view"),
        "MATERIALIZED_VIEW" | "MATERIALIZEDVIEW" => Some("materialized_view"),
        "PROCEDURE" | "P" | "PROC" => Some("procedure"),
        "FUNCTION" | "FN" | "IF" | "TF" | "FUN" => Some("function"),
        "SEQUENCE" | "SO" => Some("sequence"),
        "TRIGGER" | "TR" => Some("trigger"),
        _ => None,
    }
}

//! 表资产的逻辑内容：schema + 数据指纹 = manifest。
//!
//! 设计口径（`docs/architecture/analytics_resource/table-asset-snapshot-design.md`）：
//! - manifest 只描述“这张表是什么”，不绑定物理位置；
//! - 同一份 manifest 可以被多个 snapshot（项目归档、全局发布、fork）共享；
//! - 指纹是**快速内容指纹**，用于判断“同不同”，不冒充加密哈希、不做安全承诺。

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use shared::error::{CommonError, CoreError, StorageError};

/// schema 规范化版本：字段结构变了就升版本，旧 manifest 仍可读。
pub const TABLE_SCHEMA_VERSION: u32 = 1;

/// 一列的表结构描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// DuckDB `default` 的文本形态；没有默认值就是 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// 列顺序，从 0 开始。
    pub ordinal: i32,
}

impl TableColumn {
    pub fn new(
        name: impl Into<String>,
        data_type: impl Into<String>,
        nullable: bool,
        default_value: Option<String>,
        ordinal: i32,
    ) -> Self {
        Self {
            name: name.into(),
            data_type: data_type.into(),
            nullable,
            default_value,
            ordinal,
        }
    }
}

/// 规范化后的表结构。列按 `ordinal` 排序，schema 哈希不依赖调用方传参顺序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSchema {
    pub version: u32,
    pub columns: Vec<TableColumn>,
}

impl TableSchema {
    /// 校验并规范化：列名非空且不重复（DuckDB 标识符大小写不敏感），ordinal 不重复。
    pub fn new(mut columns: Vec<TableColumn>) -> Result<Self, CoreError> {
        if columns.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "表 schema 至少需要一列".to_string(),
            )));
        }
        columns.sort_by(|left, right| {
            left.ordinal
                .cmp(&right.ordinal)
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut names = std::collections::BTreeSet::new();
        let mut ordinals = std::collections::BTreeSet::new();
        for column in &columns {
            let name = column.name.trim();
            if name.is_empty() {
                return Err(CoreError::common(CommonError::General(
                    "表 schema 存在空列名".to_string(),
                )));
            }
            if column.data_type.trim().is_empty() {
                return Err(CoreError::common(CommonError::General(format!(
                    "列「{name}」缺数据类型"
                ))));
            }
            if !names.insert(name.to_lowercase()) {
                return Err(CoreError::common(CommonError::General(format!(
                    "表 schema 列名重复：{name}"
                ))));
            }
            if !ordinals.insert(column.ordinal) {
                return Err(CoreError::common(CommonError::General(format!(
                    "表 schema 列序号重复：{}",
                    column.ordinal
                ))));
            }
        }
        Ok(Self {
            version: TABLE_SCHEMA_VERSION,
            columns,
        })
    }

    /// 规范化 JSON：同一 schema 任意输入顺序都得到同一串。
    pub fn canonical_json(&self) -> Result<String, CoreError> {
        serde_json::to_string(self).map_err(|error| {
            CoreError::storage(StorageError::Serialization {
                format: "table_schema".to_string(),
                reason: error.to_string(),
            })
        })
    }

    /// `sha256("table-schema-v1\n" + canonical_json)`。
    pub fn hash(&self) -> Result<String, CoreError> {
        let canonical = self.canonical_json()?;
        Ok(sha256_hex(&format!("table-schema-v{TABLE_SCHEMA_VERSION}\n{canonical}")))
    }
}

/// 数据侧的快速指纹素材（由 DuckDB 聚合查询算出）。
///
/// `sum_hash` / `xor_hash` 用字符串承接数据库的 64 位无符号值（DuckDB 聚合可能
/// 返回 HUGEINT）；`min/max` 取全表 `hash(col1, col2, ...)` 的最小/最大值，用来
/// 降低“行集合不同但和/异或相同”的碰撞概率。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFingerprint {
    pub row_count: i64,
    pub column_count: i64,
    pub sum_hash: String,
    pub xor_hash: String,
    pub min_hash: Option<String>,
    pub max_hash: Option<String>,
}

impl DataFingerprint {
    pub fn new(
        row_count: i64,
        column_count: i64,
        sum_hash: impl Into<String>,
        xor_hash: impl Into<String>,
        min_hash: Option<String>,
        max_hash: Option<String>,
    ) -> Self {
        Self {
            row_count,
            column_count,
            sum_hash: sum_hash.into(),
            xor_hash: xor_hash.into(),
            min_hash,
            max_hash,
        }
    }

    /// `sha256(canonical_json(rows/cols/sum/xor/min/max))`。
    pub fn fingerprint(&self) -> String {
        #[derive(Serialize)]
        struct Payload<'a> {
            version: u32,
            row_count: i64,
            column_count: i64,
            sum_hash: &'a str,
            xor_hash: &'a str,
            min_hash: &'a Option<String>,
            max_hash: &'a Option<String>,
        }
        let payload = Payload {
            version: 1,
            row_count: self.row_count,
            column_count: self.column_count,
            sum_hash: &self.sum_hash,
            xor_hash: &self.xor_hash,
            min_hash: &self.min_hash,
            max_hash: &self.max_hash,
        };
        let canonical = serde_json::to_string(&payload).unwrap_or_default();
        sha256_hex(&format!("table-data-v1\n{canonical}"))
    }
}

/// 表资产的逻辑内容身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableManifest {
    pub manifest_id: String,
    pub schema_hash: String,
    pub data_fingerprint: String,
    pub row_count: i64,
    pub column_count: i64,
    pub schema_json: String,
}

impl TableManifest {
    /// 由 schema 与数据指纹构造 manifest，并算出稳定的 `manifest_id`。
    pub fn build(schema: &TableSchema, data: &DataFingerprint) -> Result<Self, CoreError> {
        if data.row_count < 0 || data.column_count < 0 {
            return Err(CoreError::common(CommonError::General(
                "数据指纹的行列数不能为负".to_string(),
            )));
        }
        if data.column_count as usize != schema.columns.len() {
            return Err(CoreError::common(CommonError::General(format!(
                "数据指纹列数 {} 与 schema 列数 {} 不一致",
                data.column_count,
                schema.columns.len()
            ))));
        }
        let schema_hash = schema.hash()?;
        let schema_json = schema.canonical_json()?;
        let data_fingerprint = data.fingerprint();
        let manifest_id = manifest_id_of(
            &schema_hash,
            &data_fingerprint,
            data.row_count,
            data.column_count,
        );
        Ok(Self {
            manifest_id,
            schema_hash,
            data_fingerprint,
            row_count: data.row_count,
            column_count: data.column_count,
            schema_json,
        })
    }
}

/// manifest id 只由逻辑内容决定，不含时间/路径/显示名。
fn manifest_id_of(
    schema_hash: &str,
    data_fingerprint: &str,
    row_count: i64,
    column_count: i64,
) -> String {
    sha256_hex(&format!(
        "table-manifest-v1\n{schema_hash}\n{data_fingerprint}\nrows={row_count}\ncols={column_count}"
    ))
}

/// 归档快照的物理表名：`arc_<snapshot_id 前 12 位>`。
pub fn snapshot_table_name(snapshot_id: &str) -> String {
    format!("arc_{}", short_id(snapshot_id, 12))
}

/// checkout 工作表的物理表名：`wrk_<asset_id 前 12 位>_<lease_id 前 8 位>`。
pub fn work_table_name(asset_id: &str, lease_id: &str) -> String {
    format!(
        "wrk_{}_{}",
        short_id(asset_id, 12),
        short_id(lease_id, 8)
    )
}

fn short_id(id: &str, take: usize) -> String {
    let compact: String = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect();
    compact.chars().take(take).collect()
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<TableColumn> {
        vec![
            TableColumn::new("id", "BIGINT", false, None, 0),
            TableColumn::new("Name", "VARCHAR", true, Some("''".to_string()), 1),
        ]
    }

    fn data(rows: i64) -> DataFingerprint {
        DataFingerprint::new(rows, 2, "100", "20", Some("1".to_string()), Some("99".to_string()))
    }

    #[test]
    fn schema_hash_is_order_independent() {
        let mut reversed = columns();
        reversed.reverse();
        let left = TableSchema::new(columns()).expect("schema").hash().expect("hash");
        let right = TableSchema::new(reversed).expect("schema").hash().expect("hash");
        assert_eq!(left, right);
    }

    #[test]
    fn schema_change_changes_hash() {
        let base = TableSchema::new(columns()).expect("schema").hash().expect("hash");
        let mut changed = columns();
        changed[1].nullable = false;
        let other = TableSchema::new(changed).expect("schema").hash().expect("hash");
        assert_ne!(base, other);
    }

    #[test]
    fn manifest_id_depends_on_data_and_schema_only() {
        let schema = TableSchema::new(columns()).expect("schema");
        let first = TableManifest::build(&schema, &data(10)).expect("manifest");
        let same = TableManifest::build(&schema, &data(10)).expect("manifest");
        let other = TableManifest::build(&schema, &data(11)).expect("manifest");
        assert_eq!(first, same);
        assert_ne!(first.manifest_id, other.manifest_id);
    }

    #[test]
    fn manifest_rejects_column_count_mismatch() {
        let schema = TableSchema::new(columns()).expect("schema");
        let bad = DataFingerprint::new(10, 3, "1", "2", None, None);
        assert!(TableManifest::build(&schema, &bad).is_err());
    }

    #[test]
    fn physical_names_are_derived_and_bounded() {
        assert_eq!(
            snapshot_table_name("8f14e45f-ceea-467a-9c1d-2b3e4f5a6b7c"),
            "arc_8f14e45fceea"
        );
        assert_eq!(
            work_table_name("asset-1234567890abcdef", "lease_abcdef123456"),
            "wrk_asset1234567_leaseabc"
        );
    }
}

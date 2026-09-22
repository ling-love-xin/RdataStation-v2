//! Core 内部模型定义
//!
//! 定义 Core 层内部使用的数据模型，包括：
//! - 查询结果结构
//! - 数据值类型
//! - 其他内部数据结构
//!
//! 注意：这些模型会被 api 层重新导出，供前端使用

use arrow::array::*;
use arrow::datatypes::DataType;
use arrow::record_batch::RecordBatch;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;

/// Arrow 批处理类型
pub type ArrowBatch = RecordBatch;

/// 统一的查询结果
#[derive(Debug, Clone, Type, Default)]
pub struct QueryResult {
    pub columns: Vec<String>,
    /// 列类型名称（从 Arrow Schema 提取）
    pub column_types: Vec<String>,
    /// 行数据（Vec<Vec<Value>>）
    pub rows: Vec<Vec<Value>>,
    /// 总行数
    pub total_rows: u32,
    #[specta(skip)]
    pub batches: Vec<ArrowBatch>,
    /// 影响的行数（对于 INSERT/UPDATE/DELETE）
    pub affected_rows: Option<u32>,
    /// 是否是只读查询（SELECT）
    pub is_read_only: Option<bool>,
}

impl QueryResult {
    /// 创建空的查询结果
    pub fn empty() -> Self {
        Self {
            columns: vec![],
            column_types: vec![],
            rows: vec![],
            total_rows: 0,
            batches: vec![],
            affected_rows: None,
            is_read_only: None,
        }
    }

    /// 创建不含 Arrow 数据的简单查询结果（用于 DML/DDL 等场景）
    pub fn simple(
        columns: Vec<String>,
        affected_rows: Option<u32>,
        is_read_only: Option<bool>,
    ) -> Self {
        Self {
            columns,
            column_types: vec![],
            rows: vec![],
            total_rows: 0,
            batches: vec![],
            affected_rows,
            is_read_only,
        }
    }

    /// 从 Arrow 批处理创建查询结果
    pub fn from_batches(columns: Vec<String>, batches: Vec<ArrowBatch>) -> Self {
        let column_types = if let Some(batch) = batches.first() {
            batch
                .schema()
                .fields()
                .iter()
                .map(|f| arrow_type_to_column_type(f.data_type()))
                .collect()
        } else {
            vec![]
        };
        let rows: Vec<Vec<Value>> = batches
            .iter()
            .flat_map(|batch| {
                let num_rows = batch.num_rows();
                (0..num_rows).map(move |row_idx| {
                    (0..batch.num_columns())
                        .map(|col_idx| arrow_value_at(batch.column(col_idx), row_idx))
                        .collect()
                })
            })
            .collect();
        let total_rows = rows.len() as u32;
        Self {
            columns,
            column_types,
            rows,
            total_rows,
            batches,
            // 行结果集不声明「影响行数」：v1 曾把 `total_rows` 当作 `affected_rows`，
            // 使 SELECT 也带影响行数、DML 的真实影响行数反而不可信。
            // 写语句的真实影响行数需由驱动层提供（待办见 docs/architecture/editor/editor-dev-plan.md）。
            affected_rows: None,
            is_read_only: Some(true),
        }
    }

    /// 获取总行数。
    ///
    /// **两条来源合一**：有 Arrow `batches` 时以它为准（驱动产出的形态），否则回退到已物化的
    /// `rows` 字段（跨进程反序列化出来的形态——那里没有 Arrow，见 [`Self::to_rows`] 与
    /// `Serialize` 的说明）。此前只认 `batches`，于是 JSON 往返过的结果「明明有行、报 0 行」。
    pub fn total_rows(&self) -> usize {
        if self.batches.is_empty() {
            self.rows.len()
        } else {
            self.batches.iter().map(|b| b.num_rows()).sum()
        }
    }

    /// 检查是否为空。
    pub fn is_empty(&self) -> bool {
        self.total_rows() == 0
    }

    /// 截断结果到指定行数（原地操作）
    /// 返回实际截断的行数（0 表示未截断）
    pub fn truncate(&mut self, max_rows: usize) -> usize {
        let total = self.total_rows();
        if total <= max_rows {
            return 0;
        }
        if self.batches.is_empty() {
            // 只有 `rows` 字段的形态（JSON 往返）：就地截字段，别报一个没做成的截断数。
            self.rows.truncate(max_rows);
            self.total_rows = self.rows.len() as u32;
            return total - max_rows;
        }
        let mut remaining = max_rows;
        self.batches.retain_mut(|batch| {
            if remaining == 0 {
                return false;
            }
            let batch_rows = batch.num_rows();
            if batch_rows <= remaining {
                remaining -= batch_rows;
                true
            } else {
                let truncated_batch = batch.slice(0, remaining);
                *batch = truncated_batch;
                remaining = 0;
                true
            }
        });
        self.recompute_computed_fields();
        total - max_rows
    }

    /// 按行范围切片（offset, limit）
    /// 保留指定范围的行，丢弃其余
    pub fn slice(&mut self, offset: usize, limit: usize) {
        if self.batches.is_empty() {
            // 只有 `rows` 字段的形态：就地切字段（同 `truncate`）。
            if !self.rows.is_empty() {
                let start = offset.min(self.rows.len());
                let end = (offset + limit).min(self.rows.len());
                self.rows = self.rows[start..end].to_vec();
                self.total_rows = self.rows.len() as u32;
            }
            return;
        }
        let mut new_batches = Vec::new();
        let mut row_offset = 0usize;
        for batch in self.batches.drain(..) {
            let batch_rows = batch.num_rows();
            let batch_end = row_offset + batch_rows;
            if batch_end <= offset {
                row_offset = batch_end;
                continue;
            }
            if row_offset >= offset + limit {
                break;
            }
            let local_start = offset.saturating_sub(row_offset);
            let local_end = ((offset + limit).saturating_sub(row_offset)).min(batch_rows);
            if local_start < local_end {
                let sliced = batch.slice(local_start, local_end - local_start);
                new_batches.push(sliced);
            }
            row_offset = batch_end;
            if row_offset >= offset + limit {
                break;
            }
        }
        self.batches = new_batches;
        self.recompute_computed_fields();
    }

    /// 从 batches 重新计算 rows / total_rows
    fn recompute_computed_fields(&mut self) {
        self.rows = self
            .batches
            .iter()
            .flat_map(|batch| {
                let num_rows = batch.num_rows();
                (0..num_rows).map(move |row_idx| {
                    (0..batch.num_columns())
                        .map(|col_idx| arrow_value_at(batch.column(col_idx), row_idx))
                        .collect()
                })
            })
            .collect();
        self.total_rows = self.rows.len() as u32;
    }

    /// 结果的行数据（`Vec<Vec<Value>>`）。
    ///
    /// 有 `batches` 就从它现算（驱动路径）；没有就回退已物化的 `rows` 字段
    /// （JSON 往返路径）。**这一层是行数据的权威口径**——读结果请走它，别直接读字段。
    pub fn to_rows(&self) -> Vec<Vec<Value>> {
        if self.batches.is_empty() {
            return self.rows.clone();
        }
        let mut rows = Vec::with_capacity(self.total_rows());
        for batch in &self.batches {
            let num_rows = batch.num_rows();
            for row_idx in 0..num_rows {
                let mut row = Vec::with_capacity(self.columns.len());
                for col_idx in 0..batch.num_columns() {
                    row.push(arrow_value_at(batch.column(col_idx), row_idx));
                }
                rows.push(row);
            }
        }
        rows
    }

    /// 从 Arrow Schema 提取列类型名称
    pub fn column_types(&self) -> Vec<String> {
        self.batches
            .first()
            .map(|batch| {
                batch
                    .schema()
                    .fields()
                    .iter()
                    .map(|f| {
                        match f.data_type() {
                            DataType::Int8
                            | DataType::Int16
                            | DataType::Int32
                            | DataType::Int64
                            | DataType::UInt8
                            | DataType::UInt16
                            | DataType::UInt32
                            | DataType::UInt64 => "INTEGER",
                            DataType::Float16 | DataType::Float32 | DataType::Float64 => "FLOAT",
                            DataType::Utf8 | DataType::LargeUtf8 => "TEXT",
                            DataType::Boolean => "BOOLEAN",
                            DataType::Null => "NULL",
                            DataType::Binary
                            | DataType::LargeBinary
                            | DataType::FixedSizeBinary(_) => "BYTES",
                            _ => "TEXT",
                        }
                        .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 将 Arrow 数据类型映射为细粒度列类型字符串
///
/// 保留更多类型精度，便于前端做类型适配（如格式化、排序、图表渲染）
fn arrow_type_to_column_type(data_type: &DataType) -> String {
    match data_type {
        // 整数
        DataType::Int8 => "INT8",
        DataType::Int16 => "INT16",
        DataType::Int32 => "INT32",
        DataType::Int64 => "INT64",
        DataType::UInt8 => "UINT8",
        DataType::UInt16 => "UINT16",
        DataType::UInt32 => "UINT32",
        DataType::UInt64 => "UINT64",
        // 浮点
        DataType::Float16 => "FLOAT16",
        DataType::Float32 => "FLOAT32",
        DataType::Float64 => "FLOAT64",
        // 文本
        DataType::Utf8 => "VARCHAR",
        DataType::LargeUtf8 => "TEXT",
        // 布尔
        DataType::Boolean => "BOOLEAN",
        // 二进制
        DataType::Binary => "BINARY",
        DataType::LargeBinary => "BLOB",
        DataType::FixedSizeBinary(n) => return format!("FIXED_BINARY({})", n),
        // 日期时间
        DataType::Date32 => "DATE",
        DataType::Date64 => "DATETIME",
        DataType::Time32(_) => "TIME",
        DataType::Time64(_) => "TIME",
        DataType::Timestamp(_, _) => "TIMESTAMP",
        DataType::Duration(_) => "DURATION",
        DataType::Interval(_) => "INTERVAL",
        // 精确数值
        DataType::Decimal128(p, s) => return format!("DECIMAL({},{})", p, s),
        DataType::Decimal256(p, s) => return format!("DECIMAL({},{})", p, s),
        // 结构类型
        DataType::Null => "NULL",
        DataType::List(_) => "LIST",
        DataType::Struct(_) => "STRUCT",
        DataType::Map(_, _) => "MAP",
        DataType::Dictionary(_, _) => "DICTIONARY",
        _ => "TEXT",
    }
    .to_string()
}

/// 从 Arrow 数组中提取值
fn arrow_value_at(array: &dyn Array, index: usize) -> Value {
    if array.is_null(index) {
        return Value::Null;
    }
    if let Some(arr) = array.as_any().downcast_ref::<StringArray>() {
        return Value::Text(arr.value(index).to_string());
    }
    // 整数位宽不止 Int64：PG 的 `int4` → Int32、`smallint` → Int16、MySQL 无符号 → UInt64…。
    // 漏掉就会被兜底当成文本（旧实现甚至打印整列 Debug），网格里直接看到垃圾值。
    if let Some(arr) = array.as_any().downcast_ref::<Int64Array>() {
        return Value::Int(arr.value(index));
    }
    if let Some(arr) = array.as_any().downcast_ref::<Int32Array>() {
        return Value::Int(i64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<Int16Array>() {
        return Value::Int(i64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<Int8Array>() {
        return Value::Int(i64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<UInt64Array>() {
        let value = arr.value(index);
        // 超出 i64 的值以文本保留精度（不静默截断为负数）
        return match i64::try_from(value) {
            Ok(n) => Value::Int(n),
            Err(_) => Value::Text(value.to_string()),
        };
    }
    if let Some(arr) = array.as_any().downcast_ref::<UInt32Array>() {
        return Value::Int(i64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<UInt16Array>() {
        return Value::Int(i64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<UInt8Array>() {
        return Value::Int(i64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<Float64Array>() {
        return Value::Float(arr.value(index));
    }
    if let Some(arr) = array.as_any().downcast_ref::<Float32Array>() {
        return Value::Float(f64::from(arr.value(index)));
    }
    if let Some(arr) = array.as_any().downcast_ref::<BooleanArray>() {
        return Value::Bool(arr.value(index));
    }
    if let Some(arr) = array.as_any().downcast_ref::<BinaryArray>() {
        return Value::Bytes(arr.value(index).to_vec());
    }
    // 兜底：交给 Arrow 的单值格式化（Decimal / Date / Timestamp 等能正常显示成文本）。
    // 旧实现是 `format!("{:?}", array)`——把**整列**打印进每个格子（既显示垃圾，又是 O(n²)）。
    Value::Text(
        arrow::util::display::array_value_to_string(array, index)
            .unwrap_or_else(|_| format!("<{}>", array.data_type())),
    )
}

/// 序列化支持
///
/// `rows` / `total_rows` 是**跨进程契约字段**（前端 / 插件的 JSON），而驱动只填 Arrow
/// `batches`，这两个字段在驱动产出的实例里是默认空值（架构 §12 #21）。所以它们在这里
/// **按 `batches` 现算**：直接发字段的话，JSON 里 `rows` 恒为 `[]`、`total_rows` 恒为 0，
/// 读它的消费者拿到的是「有列名、零行」——洞察侧为此踩过一次空表
/// （见 `insight::service::result_columns_and_rows` 的注释与回归）。
///
/// 进程内消费者**不必**经过这里：直接 [`QueryResult::to_rows`] 更省一次物化。
impl Serialize for QueryResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        // 已经物化过（`from_batches` / `truncate` / JSON 往返）的直接发，别白算一遍。
        let (rows, total_rows) = if self.rows.is_empty() && !self.batches.is_empty() {
            (self.to_rows(), self.total_rows() as u32)
        } else {
            (self.rows.clone(), self.total_rows)
        };
        let mut state = serializer.serialize_struct("QueryResult", 6)?;
        state.serialize_field("columns", &self.columns)?;
        state.serialize_field("column_types", &self.column_types)?;
        state.serialize_field("rows", &rows)?;
        state.serialize_field("affected_rows", &self.affected_rows)?;
        state.serialize_field("is_read_only", &self.is_read_only)?;
        state.serialize_field("total_rows", &total_rows)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for QueryResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct QueryResultHelper {
            columns: Vec<String>,
            column_types: Option<Vec<String>>,
            rows: Option<Vec<Vec<Value>>>,
            total_rows: Option<u32>,
            affected_rows: Option<u32>,
            is_read_only: Option<bool>,
        }
        let helper = QueryResultHelper::deserialize(deserializer)?;
        Ok(Self {
            columns: helper.columns,
            column_types: helper.column_types.unwrap_or_default(),
            rows: helper.rows.unwrap_or_default(),
            total_rows: helper.total_rows.unwrap_or(0),
            batches: vec![],
            affected_rows: helper.affected_rows,
            is_read_only: helper.is_read_only,
        })
    }
}

/// 一行数据（列值数组）
pub type Row = Vec<Value>;

/// 与 SQL 类型兼容的值
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(v) => write!(f, "{}", v),
            Value::Int(v) => write!(f, "{}", v),
            Value::Float(v) => write!(f, "{}", v),
            Value::Text(v) => write!(f, "{}", v),
            Value::Bytes(v) => write!(f, "{:?}", v),
        }
    }
}

impl Value {
    /// `Value` → JSON 值（建 DuckDB 临时表 / 跨进程契约用）。
    ///
    /// `Bytes` 走有损 UTF-8：临时表要的是「能算的样本」，二进制列在洞察里只能当文本看
    /// （真二进制统计本就没有意义），比整列变 `NULL` 有用。
    pub fn to_json(self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(v) => serde_json::Value::Bool(v),
            Value::Int(v) => serde_json::Value::Number(v.into()),
            Value::Float(v) => serde_json::Number::from_f64(v)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Text(v) => serde_json::Value::String(v),
            Value::Bytes(v) => serde_json::Value::String(String::from_utf8_lossy(&v).to_string()),
        }
    }

    /// 获取值的类型名称
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Bool(_) => "BOOLEAN",
            Value::Int(_) => "INTEGER",
            Value::Float(_) => "FLOAT",
            Value::Text(_) => "TEXT",
            Value::Bytes(_) => "BYTES",
        }
    }

    /// 检查是否为 NULL
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// 尝试转换为 i64
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Float(v) => Some(*v as i64),
            Value::Text(v) => v.parse().ok(),
            _ => None,
        }
    }

    /// 尝试转换为 f64
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Int(v) => Some(*v as f64),
            Value::Text(v) => v.parse().ok(),
            _ => None,
        }
    }

    /// 尝试转换为字符串
    pub fn as_text(&self) -> Option<String> {
        match self {
            Value::Text(v) => Some(v.clone()),
            Value::Int(v) => Some(v.to_string()),
            Value::Float(v) => Some(v.to_string()),
            Value::Bool(v) => Some(v.to_string()),
            Value::Bytes(v) => Some(String::from_utf8_lossy(v).to_string()),
            Value::Null => None,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Text(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Text(s)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(v as i64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(v as f64)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}

impl<T> From<Option<T>> for Value
where
    T: Into<Value>,
{
    fn from(v: Option<T>) -> Self {
        match v {
            Some(val) => val.into(),
            None => Value::Null,
        }
    }
}

/// Arrow → Value 的映射回归
///
/// 背景（实测）：各驱动只填 `batches`，上层取值一律经 `arrow_value_at`。
/// 这里固定两件事——**驱动实际产出的位宽都要认**（PG 的 `int4` 是 Int32、MySQL 无符号是 UInt64）、
/// 以及**兜底不得是整列 Debug 文本**（旧实现把整列打印进每个单元格，既显示垃圾又是 O(n²)）。
#[cfg(test)]
mod value_mapping_tests {
    use super::*;
    use arrow::datatypes::{Field, Schema};
    use std::sync::Arc;

    fn from_arrays(names: &[&str], arrays: Vec<ArrayRef>) -> QueryResult {
        let fields: Vec<Field> = names
            .iter()
            .zip(arrays.iter())
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect();
        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(schema, arrays).expect("构造 RecordBatch");

        QueryResult::from_batches(
            names.iter().map(|name| (*name).to_string()).collect(),
            vec![batch],
        )
    }

    #[test]
    fn numeric_widths_are_mapped_to_values() {
        let result = from_arrays(
            &["i32", "u64", "f32"],
            vec![
                Arc::new(Int32Array::from(vec![Some(7), None])),
                Arc::new(UInt64Array::from(vec![Some(9), Some(11)])),
                Arc::new(Float32Array::from(vec![Some(1.5), None])),
            ],
        );

        assert_eq!(result.rows[0][0], Value::Int(7));
        assert_eq!(result.rows[1][0], Value::Null);
        assert_eq!(result.rows[0][1], Value::Int(9), "UInt64 必须映射为整数");
        assert_eq!(result.rows[1][1], Value::Int(11));
        assert_eq!(result.rows[0][2], Value::Float(1.5));
        assert_eq!(result.rows[1][2], Value::Null);
    }

    #[test]
    fn unmapped_types_fall_back_to_single_value_text() {
        let result = from_arrays(
            &["d"],
            vec![Arc::new(Date32Array::from(vec![Some(19_000)]))],
        );

        match &result.rows[0][0] {
            Value::Text(text) => {
                assert!(!text.contains("Array"), "兜底不应是数组 Debug 文本：{text}");
                assert!(!text.is_empty(), "兜底应有可读内容");
            }
            other => panic!("Date32 应退化为文本，实际：{other:?}"),
        }
    }

    #[test]
    fn huge_unsigned_values_keep_precision() {
        let result = from_arrays(
            &["u64"],
            vec![Arc::new(UInt64Array::from(vec![Some(u64::MAX)]))],
        );

        assert_eq!(
            result.rows[0][0],
            Value::Text(u64::MAX.to_string()),
            "超出 i64 的无符号值必须保留精度，不得静默截断"
        );
    }

    /// **只填 `batches`** 的实例（native 驱动的形态）序列化时，`rows` / `total_rows`
    /// 必须按 Arrow 现算出来。
    ///
    /// 回归：此前直接发字段 → JSON 里 `rows: []` / `total_rows: 0`，读 JSON 的消费者
    /// 拿到的是「有列名、零行」——结果筛选建临时表、洞察取样本都因此拿过空表。
    #[test]
    fn serializing_batches_only_result_fills_the_json_contract() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "id",
                arrow::datatypes::DataType::Int64,
                false,
            )])),
            vec![Arc::new(Int64Array::from(vec![7, 9]))],
        )
        .expect("构造 RecordBatch");
        let mut result = QueryResult::empty();
        result.columns = vec!["id".to_string()];
        result.batches = vec![batch];
        assert!(
            result.rows.is_empty() && result.total_rows == 0,
            "前提：驱动只填 batches，字段保持默认空值"
        );

        let json = serde_json::to_value(&result).expect("序列化");
        assert_eq!(json["columns"][0], "id");
        assert_eq!(json["total_rows"], 2, "契约里的 total_rows 要现算");
        assert_eq!(json["rows"].as_array().map(Vec::len), Some(2), "行也要现算");
        assert_eq!(json["rows"][1][0], 9, "`Value` 是 untagged：整数就是 JSON 数字");

        // 往返一趟：这份实例的 `batches` 是空的、行在字段里 —— 三个口径（字段 / batches / 方法）
        // 必须给出同一个答案（此前方法只认 `batches`，于是「明明有行、报 0 行」）。
        let back: QueryResult = serde_json::from_value(json).expect("反序列化");
        assert_eq!(back.rows.len(), 2);
        assert_eq!(back.total_rows, 2);
        assert_eq!(back.total_rows(), 2, "方法要认字段这一条来源");
        assert_eq!(back.to_rows().len(), 2, "`to_rows()` 同理");
        assert!(!back.is_empty());
    }
}

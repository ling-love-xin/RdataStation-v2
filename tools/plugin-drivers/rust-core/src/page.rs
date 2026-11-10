use std::collections::{BTreeMap, HashMap};

use arrow::array::{
    ArrayRef, BooleanArray, Date32Array, Float64Array, Int64Array, LargeBinaryArray,
    LargeStringArray, TimestampMicrosecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::ipc::writer::StreamWriter;
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// §4.2 阈值：同时满足行数与字节数才内联。
pub const INLINE_JSON_MAX_ROWS: usize = 200;
pub const INLINE_JSON_MAX_BYTES: usize = 256 * 1024;
pub const ARROW_FRAME_CHUNK: usize = 32 * 1024;

/// 列的逻辑类型。Arrow 只允许协议白名单里的物理类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Canonical {
    Text,
    Integer,
    Float,
    Boolean,
    Binary,
    Timestamp,
    Date,
    Time,
    Json,
}

impl Canonical {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_uppercase().as_str() {
            "TEXT" | "VARCHAR" | "CHAR" | "NVARCHAR" | "NCHAR" | "CLOB" | "LONG" => Some(Self::Text),
            "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "NUMBER" => Some(Self::Integer),
            "FLOAT" | "DOUBLE" | "REAL" | "DECIMAL" | "NUMERIC" => Some(Self::Float),
            "BOOLEAN" | "BOOL" | "BIT" => Some(Self::Boolean),
            "BINARY" | "VARBINARY" | "BLOB" | "RAW" => Some(Self::Binary),
            "TIMESTAMP" | "DATETIME" | "DATETIME2" | "DATETIMEOFFSET" | "TIMESTAMPTZ" => {
                Some(Self::Timestamp)
            }
            "DATE" => Some(Self::Date),
            "TIME" => Some(Self::Time),
            "JSON" | "JSONB" => Some(Self::Json),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Integer => "INTEGER",
            Self::Float => "FLOAT",
            Self::Boolean => "BOOLEAN",
            Self::Binary => "BINARY",
            Self::Timestamp => "TIMESTAMP",
            Self::Date => "DATE",
            Self::Time => "TIME",
            Self::Json => "JSON",
        }
    }

    fn arrow_type(self) -> DataType {
        match self {
            Self::Text | Self::Json | Self::Time => DataType::LargeUtf8,
            Self::Integer => DataType::Int64,
            Self::Float => DataType::Float64,
            Self::Boolean => DataType::Boolean,
            Self::Binary => DataType::LargeBinary,
            Self::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            Self::Date => DataType::Date32,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ColumnDef {
    pub name: String,
    pub type_raw: String,
    pub nullable: bool,
    #[serde(skip)]
    pub canonical: Canonical,
}

impl ColumnDef {
    pub fn new(name: impl Into<String>, type_raw: impl Into<String>, nullable: bool, canonical: Canonical) -> Self {
        Self {
            name: name.into(),
            type_raw: type_raw.into(),
            nullable,
            canonical,
        }
    }

    pub fn to_wire(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "type_raw": self.type_raw,
            "nullable": self.nullable,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Page {
    pub columns: Vec<ColumnDef>,
    pub rows: Vec<Vec<Value>>,
    pub affected_rows: Option<u64>,
    pub has_more: bool,
    pub cursor_id: Option<String>,
    pub notices: Vec<String>,
}

impl Page {
    pub fn empty(columns: Vec<ColumnDef>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
            affected_rows: None,
            has_more: false,
            cursor_id: None,
            notices: Vec::new(),
        }
    }

    pub fn rows(columns: Vec<ColumnDef>, rows: Vec<Vec<Value>>) -> Self {
        Self {
            columns,
            rows,
            affected_rows: None,
            has_more: false,
            cursor_id: None,
            notices: Vec::new(),
        }
    }

    pub fn affected(affected_rows: u64) -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: Some(affected_rows),
            has_more: false,
            cursor_id: None,
            notices: Vec::new(),
        }
    }

    pub fn row_count(&self) -> u64 {
        self.rows.len() as u64
    }

    /// 内联判定：行数 <= 200 且行 JSON 字节和 <= 256 KiB。
    pub fn should_inline(&self) -> bool {
        if self.columns.is_empty() && self.affected_rows.is_some() {
            return true;
        }
        if self.rows.len() > INLINE_JSON_MAX_ROWS {
            return false;
        }
        let mut bytes = 0usize;
        for row in &self.rows {
            bytes += serde_json::to_vec(row).map(|v| v.len()).unwrap_or(usize::MAX);
            if bytes > INLINE_JSON_MAX_BYTES {
                return false;
            }
        }
        true
    }

    /// 内联行必须是对象（列名做键）。重复列名按后者覆盖。
    pub fn inline_rows(&self) -> Vec<Value> {
        let names: Vec<&str> = self.columns.iter().map(|c| c.name.as_str()).collect();
        self.rows
            .iter()
            .map(|row| {
                let mut obj = serde_json::Map::new();
                for (idx, name) in names.iter().enumerate() {
                    obj.insert((*name).to_string(), row.get(idx).cloned().unwrap_or(Value::Null));
                }
                Value::Object(obj)
            })
            .collect()
    }

    pub fn arrow_chunks(&self) -> Result<Vec<Vec<u8>>, String> {
        let mut fields = Vec::with_capacity(self.columns.len());
        for col in &self.columns {
            let mut metadata = HashMap::new();
            metadata.insert("rds.type_raw".to_string(), col.type_raw.clone());
            metadata.insert("rds.canonical".to_string(), col.canonical.as_str().to_string());
            metadata.insert("rds.nullable".to_string(), col.nullable.to_string());
            fields.push(Field::new(&col.name, col.canonical.arrow_type(), col.nullable).with_metadata(metadata));
        }
        let schema = Schema::new(fields);
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(self.columns.len());
        for (idx, col) in self.columns.iter().enumerate() {
            arrays.push(build_array(col.canonical, &self.rows, idx)?);
        }
        let batch = RecordBatch::try_new(std::sync::Arc::new(schema.clone()), arrays)
            .map_err(|e| format!("构造 RecordBatch 失败：{e}"))?;
        let mut buf = Vec::new();
        {
            let mut writer = StreamWriter::try_new(&mut buf, &schema)
                .map_err(|e| format!("Arrow IPC writer 初始化失败：{e}"))?;
            writer.write(&batch).map_err(|e| format!("写 Arrow batch 失败：{e}"))?;
            writer.finish().map_err(|e| format!("收尾 Arrow stream 失败：{e}"))?;
        }
        if buf.is_empty() {
            return Ok(vec![Vec::new()]);
        }
        Ok(buf.chunks(ARROW_FRAME_CHUNK).map(|c| c.to_vec()).collect())
    }
}

fn build_array(canonical: Canonical, rows: &[Vec<Value>], idx: usize) -> Result<ArrayRef, String> {
    let cell = |row: &Vec<Value>| row.get(idx).cloned().unwrap_or(Value::Null);
    let as_text = |v: &Value| -> Option<String> {
        match v {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    };

    let array: ArrayRef = match canonical {
        Canonical::Text | Canonical::Json | Canonical::Time => {
            let values: Vec<Option<String>> = rows.iter().map(|r| as_text(&cell(r))).collect();
            std::sync::Arc::new(LargeStringArray::from(values))
        }
        Canonical::Integer => {
            let values: Vec<Option<i64>> = rows
                .iter()
                .map(|r| match cell(r) {
                    Value::Null => None,
                    Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
                    Value::String(s) => s.parse::<i64>().ok(),
                    Value::Bool(b) => Some(if b { 1 } else { 0 }),
                    _ => None,
                })
                .collect();
            std::sync::Arc::new(Int64Array::from(values))
        }
        Canonical::Float => {
            let values: Vec<Option<f64>> = rows
                .iter()
                .map(|r| match cell(r) {
                    Value::Null => None,
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => s.parse::<f64>().ok(),
                    _ => None,
                })
                .collect();
            std::sync::Arc::new(Float64Array::from(values))
        }
        Canonical::Boolean => {
            let values: Vec<Option<bool>> = rows
                .iter()
                .map(|r| match cell(r) {
                    Value::Null => None,
                    Value::Bool(b) => Some(b),
                    Value::Number(n) => n.as_i64().map(|v| v != 0),
                    Value::String(s) => match s.to_ascii_lowercase().as_str() {
                        "true" | "t" | "1" | "yes" => Some(true),
                        "false" | "f" | "0" | "no" => Some(false),
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            std::sync::Arc::new(BooleanArray::from(values))
        }
        Canonical::Binary => {
            let values: Vec<Option<Vec<u8>>> = rows
                .iter()
                .map(|r| match cell(r) {
                    Value::Null => None,
                    Value::Array(items) => items
                        .iter()
                        .map(|v| v.as_u64().map(|n| n as u8))
                        .collect::<Option<Vec<u8>>>(),
                    Value::String(s) => Some(s.as_bytes().to_vec()),
                    _ => None,
                })
                .collect();
            std::sync::Arc::new(LargeBinaryArray::from(
                values.iter().map(|v| v.as_deref()).collect::<Vec<_>>(),
            ))
        }
        Canonical::Timestamp => {
            let values: Vec<Option<i64>> = rows
                .iter()
                .map(|r| match cell(r) {
                    Value::Null => None,
                    Value::String(s) => parse_timestamp_micros(&s),
                    Value::Number(n) => n.as_i64(),
                    _ => None,
                })
                .collect();
            std::sync::Arc::new(
                TimestampMicrosecondArray::from(values).with_timezone("UTC"),
            )
        }
        Canonical::Date => {
            let values: Vec<Option<i32>> = rows
                .iter()
                .map(|r| match cell(r) {
                    Value::Null => None,
                    Value::String(s) => parse_date_days(&s),
                    Value::Number(n) => n.as_i64().map(|v| v as i32),
                    _ => None,
                })
                .collect();
            std::sync::Arc::new(Date32Array::from(values))
        }
    };
    Ok(array)
}

pub fn parse_timestamp_micros(raw: &str) -> Option<i64> {
    let text = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Some(dt.with_timezone(&Utc).timestamp_micros());
    }
    let normalized = text.trim_end_matches('Z');
    if let Ok(dt) = NaiveDateTime::parse_from_str(normalized, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(dt.and_utc().timestamp_micros());
    }
    for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(normalized, fmt) {
            return Some(dt.and_utc().timestamp_micros());
        }
    }
    // 纯时间：用当天日期换算，只为网格显示稳定。
    if let Ok(time) = NaiveTime::parse_from_str(normalized, "%H:%M:%S%.f") {
        let date = NaiveDate::from_ymd_opt(1970, 1, 1)?;
        return Some(date.and_time(time).and_utc().timestamp_micros());
    }
    None
}

pub fn parse_date_days(raw: &str) -> Option<i32> {
    let text = raw.trim();
    let date_part = text.get(..10).unwrap_or(text);
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    Some((date - NaiveDate::from_ymd_opt(1970, 1, 1)?).num_days() as i32)
}

#[allow(dead_code)]
pub fn format_naive_datetime(dt: NaiveDateTime) -> String {
    format!("{}Z", dt.format("%Y-%m-%dT%H:%M:%S%.6f"))
}

#[allow(dead_code)]
pub fn format_naive_time(time: NaiveTime) -> String {
    let micros = time.nanosecond() / 1000;
    format!("{:02}:{:02}:{:02}.{:06}", time.hour(), time.minute(), time.second(), micros)
}

#[derive(Debug, Clone)]
pub struct ExecuteRequest {
    pub sql: String,
    pub params: Vec<Value>,
    pub max_rows: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub server_version: Option<String>,
    pub default_schema: Option<String>,
    pub capabilities: BTreeMap<String, bool>,
}

impl SessionInfo {
    pub fn capabilities_json(&self) -> Value {
        serde_json::to_value(&self.capabilities).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[cfg(test)]
mod tests {
    use arrow::ipc::reader::StreamReader;
    use serde_json::json;

    use super::*;

    #[test]
    fn arrow_roundtrip_keeps_schema_and_rows() {
        let columns = vec![
            ColumnDef::new("id", "NUMBER", false, Canonical::Integer),
            ColumnDef::new("name", "VARCHAR2", true, Canonical::Text),
            ColumnDef::new("created", "TIMESTAMP", true, Canonical::Timestamp),
            ColumnDef::new("ok", "BOOLEAN", true, Canonical::Boolean),
        ];
        let rows: Vec<Vec<Value>> = (0..3000)
            .map(|i| {
                vec![
                    json!(i),
                    json!(format!("row-{i}")),
                    json!("2026-01-02T03:04:05.123456Z"),
                    json!(i % 2 == 0),
                ]
            })
            .collect();
        let page = Page::rows(columns, rows);
        let chunks = page.arrow_chunks().expect("arrow encode");
        assert!(chunks.len() > 1, "32 KiB 切片必须让跨帧重组发生");
        let mut bytes = Vec::new();
        for chunk in &chunks {
            bytes.extend_from_slice(chunk);
        }
        let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).expect("ipc reader");
        let schema = reader.schema();
        let batch = reader.into_iter().next().expect("batch").expect("batch ok");
        assert_eq!(batch.num_rows(), 3000);
        assert_eq!(schema.field(0).metadata().get("rds.type_raw").map(String::as_str), Some("NUMBER"));
        assert_eq!(schema.field(0).metadata().get("rds.canonical").map(String::as_str), Some("INTEGER"));
        assert_eq!(schema.field(2).data_type(), &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())));
    }

    #[test]
    fn inline_threshold_is_rows_and_bytes() {
        let columns = vec![ColumnDef::new("v", "VARCHAR2", true, Canonical::Text)];
        let small = Page::rows(columns.clone(), (0..200).map(|i| vec![json!(i)]).collect());
        assert!(small.should_inline());
        let big = Page::rows(columns.clone(), (0..201).map(|i| vec![json!(i)]).collect());
        assert!(!big.should_inline());
        let wide = Page::rows(columns, vec![vec![json!("x".repeat(300 * 1024))]]);
        assert!(!wide.should_inline());
    }

    #[test]
    fn empty_page_still_has_columns() {
        let page = Page::empty(vec![ColumnDef::new("id", "NUMBER", false, Canonical::Integer)]);
        assert!(page.should_inline());
        assert_eq!(page.columns.len(), 1);
    }
}

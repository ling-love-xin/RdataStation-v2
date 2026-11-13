use base64::Engine as _;
use rds_driver_sidecar::{Canonical, ColumnDef, DriverError};
use oracle_rs::types::{OracleDate, OracleTimestamp};
use oracle_rs::{ColumnInfo, OracleType, Row, Value};
use serde_json::{json, Number, Value as Json};

/// Oracle 列类型 → 原始类型名（响应 `type_raw` 与 Arrow metadata 共用这一份）。
pub fn type_raw(col: &ColumnInfo) -> String {
    let base = match col.oracle_type {
        OracleType::Varchar => "VARCHAR2",
        OracleType::Char => "CHAR",
        OracleType::Number | OracleType::BinaryInteger => "NUMBER",
        OracleType::BinaryFloat => "BINARY_FLOAT",
        OracleType::BinaryDouble => "BINARY_DOUBLE",
        OracleType::Long => "LONG",
        OracleType::LongRaw => "LONG RAW",
        OracleType::Rowid => "ROWID",
        OracleType::Urowid => "UROWID",
        OracleType::Date => "DATE",
        OracleType::Timestamp => "TIMESTAMP",
        OracleType::TimestampTz => "TIMESTAMP WITH TIME ZONE",
        OracleType::TimestampLtz => "TIMESTAMP WITH LOCAL TIME ZONE",
        OracleType::IntervalYm => "INTERVAL YEAR TO MONTH",
        OracleType::IntervalDs => "INTERVAL DAY TO SECOND",
        OracleType::Raw => "RAW",
        OracleType::Clob => "CLOB",
        OracleType::Blob => "BLOB",
        OracleType::Bfile => "BFILE",
        OracleType::Boolean => "BOOLEAN",
        OracleType::Json => "JSON",
        OracleType::Vector => "VECTOR",
        OracleType::Cursor => "REF CURSOR",
        OracleType::Object => "OBJECT",
    }
    .to_string();

    if matches!(col.oracle_type, OracleType::Number | OracleType::BinaryInteger) {
        if col.precision > 0 {
            if col.scale > 0 {
                return format!("NUMBER({},{})", col.precision, col.scale);
            }
            return format!("NUMBER({})", col.precision);
        }
    }
    if matches!(col.oracle_type, OracleType::Varchar | OracleType::Char | OracleType::Raw) {
        let len = if col.data_size > 0 {
            col.data_size as u32
        } else {
            col.buffer_size
        };
        if len > 0 {
            return format!("{base}({len})");
        }
    }
    base
}

/// Oracle 类型 → 协议归一类型（内联 JSON 与 Arrow 都按这一份走）。
pub fn canonical(col: &ColumnInfo) -> Canonical {
    match col.oracle_type {
        OracleType::Number | OracleType::BinaryInteger => {
            if col.scale > 0 {
                Canonical::Float
            } else {
                Canonical::Integer
            }
        }
        OracleType::BinaryFloat | OracleType::BinaryDouble => Canonical::Float,
        OracleType::Date => Canonical::Date,
        OracleType::Timestamp
        | OracleType::TimestampTz
        | OracleType::TimestampLtz => Canonical::Timestamp,
        OracleType::Raw
        | OracleType::LongRaw
        | OracleType::Blob
        | OracleType::Bfile => Canonical::Binary,
        OracleType::Boolean => Canonical::Boolean,
        OracleType::Json => Canonical::Json,
        OracleType::Varchar
        | OracleType::Char
        | OracleType::Long
        | OracleType::Clob
        | OracleType::Rowid
        | OracleType::Urowid
        | OracleType::Cursor
        | OracleType::Object
        | OracleType::Vector
        | OracleType::IntervalYm
        | OracleType::IntervalDs => Canonical::Text,
    }
}

pub fn column_def(col: &ColumnInfo) -> ColumnDef {
    ColumnDef::new(
        col.name.clone(),
        type_raw(col),
        col.nullable,
        canonical(col),
    )
}

/// 协议 params → oracle-rs 绑定值。对象/数组按 JSON 文本传，避免把结构悄悄改形。
pub fn bind_value(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(v) => Value::Boolean(*v),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::String(n.to_string())
            }
        }
        Json::String(s) => Value::String(s.clone()),
        Json::Array(_) | Json::Object(_) => Value::String(value.to_string()),
    }
}

pub fn convert_row(row: &Row) -> Vec<Json> {
    row.values().iter().map(convert_value).collect()
}

pub fn convert_value(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::String(s) => Json::String(s.clone()),
        Value::Bytes(bytes) => base64(bytes),
        Value::Integer(i) => json!(i),
        Value::Float(f) => {
            if f.is_finite() {
                Number::from_f64(*f).map(Json::Number).unwrap_or_else(|| Json::String(f.to_string()))
            } else {
                Json::String(f.to_string())
            }
        }
        Value::Number(n) => number_json(n.as_str()),
        Value::Date(d) => Json::String(format_date(d)),
        Value::Timestamp(ts) => Json::String(format_timestamp(ts)),
        Value::RowId(_) => Json::String(format!("{value:?}")),
        Value::Boolean(b) => Json::Bool(*b),
        Value::Lob(lob) => {
            if lob.is_null() {
                return Json::Null;
            }
            if let Ok(Some(text)) = lob.as_string() {
                return Json::String(text);
            }
            if let Ok(Some(bytes)) = lob.as_bytes() {
                return base64(&bytes);
            }
            Json::String(format!("{lob:?}"))
        }
        Value::Json(j) => j.clone(),
        Value::Vector(v) => Json::String(format!("{v:?}")),
        Value::Cursor(_) | Value::Collection(_) => Json::String(format!("{value:?}")),
    }
}

fn number_json(raw: &str) -> Json {
    raw.parse::<Number>()
        .map(Json::Number)
        .unwrap_or_else(|_| Json::String(raw.to_string()))
}

fn base64(bytes: &[u8]) -> Json {
    Json::String(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// `DATE` 在 Oracle 里带时分秒，内联统一按 RFC3339（UTC）。
pub fn format_date(date: &OracleDate) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year, date.month, date.day, date.hour, date.minute, date.second
    )
}

/// `TIMESTAMP` 保留微秒；带时区的列按自身偏移输出，不带时区的补 `Z`。
pub fn format_timestamp(ts: &OracleTimestamp) -> String {
    let mut out = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}",
        ts.year, ts.month, ts.day, ts.hour, ts.minute, ts.second, ts.microsecond
    );
    if ts.has_timezone() {
        let sign = if ts.tz_hour_offset < 0 || ts.tz_minute_offset < 0 {
            '-'
        } else {
            '+'
        };
        out.push_str(&format!(
            "{sign}{:02}:{:02}",
            ts.tz_hour_offset.abs(),
            ts.tz_minute_offset.abs()
        ));
    } else {
        out.push('Z');
    }
    out
}

pub fn sql_error(err: impl std::fmt::Display) -> DriverError {
    DriverError::sql(err.to_string())
}

/// 把标识符/字面量拼进 SQL 时的最小转义（元数据名来自数据库本身）。
///
/// 元数据查询已全部改成绑定变量（Oracle 按 SQL 文本硬解析，拼名字等于每个对象
/// 一条新语句），这里暂时没有调用者；留着是为了「确实需要拼标识符」的场景
/// （标识符不能用绑定变量），别把转义规则再发明一遍。
#[allow(dead_code)]
pub fn quote_literal(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_value_keeps_json_shape() {
        assert_eq!(convert_value(&bind_value(&json!(null))), Json::Null);
        assert_eq!(convert_value(&bind_value(&json!(true))), Json::Bool(true));
        assert_eq!(convert_value(&bind_value(&json!(42))), json!(42));
        assert_eq!(
            convert_value(&bind_value(&json!([1, 2]))),
            Json::String("[1,2]".to_string())
        );
        assert_eq!(
            convert_value(&bind_value(&json!({"a": 1}))),
            Json::String("{\"a\":1}".to_string())
        );
    }

    #[test]
    fn quote_literal_escapes_single_quote() {
        assert_eq!(quote_literal("O'Brien"), "'O''Brien'");
    }

    #[test]
    fn date_formats_as_rfc3339() {
        let date = OracleDate {
            year: 2026,
            month: 11,
            day: 10,
            hour: 9,
            minute: 5,
            second: 7,
        };
        assert_eq!(format_date(&date), "2026-11-10T09:05:07Z");
    }

    #[test]
    fn timestamp_keeps_micros_and_offset() {
        let mut ts = OracleTimestamp {
            year: 2026,
            month: 11,
            day: 10,
            hour: 9,
            minute: 5,
            second: 7,
            microsecond: 123456,
            tz_hour_offset: 0,
            tz_minute_offset: 0,
        };
        assert_eq!(format_timestamp(&ts), "2026-11-10T09:05:07.123456Z");
        ts.tz_hour_offset = 8;
        assert_eq!(format_timestamp(&ts), "2026-11-10T09:05:07.123456+08:00");
    }
}

//! SQL Server 值/列类型到宿主 canonical 形状的转换。

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rds_driver_sidecar::{Canonical, ColumnDef};
use serde_json::{Value, json};
use tiberius::{Column, ColumnData, ColumnType};

/// `ColumnType` 不含 Display，也不带长度/精度，只能给出不含尺寸的基类型名。
pub fn type_raw(kind: ColumnType) -> &'static str {
    match kind {
        ColumnType::Null => "NULL",
        ColumnType::Bit | ColumnType::Bitn => "BIT",
        ColumnType::Int1 => "TINYINT",
        ColumnType::Int2 => "SMALLINT",
        ColumnType::Int4 => "INT",
        ColumnType::Int8 => "BIGINT",
        ColumnType::Datetime4 => "SMALLDATETIME",
        ColumnType::Float4 => "REAL",
        ColumnType::Float8 => "FLOAT",
        ColumnType::Money => "MONEY",
        ColumnType::Money4 => "SMALLMONEY",
        ColumnType::Datetime | ColumnType::Datetimen => "DATETIME",
        ColumnType::Guid => "UNIQUEIDENTIFIER",
        ColumnType::Intn => "INT",
        ColumnType::Decimaln | ColumnType::Numericn => "DECIMAL",
        ColumnType::Floatn => "FLOAT",
        ColumnType::Daten => "DATE",
        ColumnType::Timen => "TIME",
        ColumnType::Datetime2 => "DATETIME2",
        ColumnType::DatetimeOffsetn => "DATETIMEOFFSET",
        ColumnType::BigVarBin => "VARBINARY",
        ColumnType::BigVarChar => "VARCHAR",
        ColumnType::BigBinary => "BINARY",
        ColumnType::BigChar => "CHAR",
        ColumnType::NVarchar => "NVARCHAR",
        ColumnType::NChar => "NCHAR",
        ColumnType::Xml => "XML",
        ColumnType::Udt => "UDT",
        ColumnType::Text => "TEXT",
        ColumnType::Image => "IMAGE",
        ColumnType::NText => "NTEXT",
        ColumnType::SSVariant => "SQL_VARIANT",
    }
}

pub fn canonical(kind: ColumnType) -> Canonical {
    match kind {
        ColumnType::Bit | ColumnType::Bitn => Canonical::Boolean,
        ColumnType::Int1
        | ColumnType::Int2
        | ColumnType::Int4
        | ColumnType::Int8
        | ColumnType::Intn => Canonical::Integer,
        ColumnType::Float4
        | ColumnType::Float8
        | ColumnType::Floatn
        | ColumnType::Money
        | ColumnType::Money4
        | ColumnType::Decimaln
        | ColumnType::Numericn => Canonical::Float,
        ColumnType::Daten => Canonical::Date,
        ColumnType::Timen => Canonical::Time,
        ColumnType::Datetime
        | ColumnType::Datetimen
        | ColumnType::Datetime2
        | ColumnType::DatetimeOffsetn
        | ColumnType::Datetime4 => Canonical::Timestamp,
        ColumnType::BigVarBin
        | ColumnType::BigBinary
        | ColumnType::Image
        | ColumnType::Udt => Canonical::Binary,
        ColumnType::Xml => Canonical::Json,
        ColumnType::Null => Canonical::Text,
        _ => Canonical::Text,
    }
}

pub fn column_def(column: &Column) -> ColumnDef {
    ColumnDef::new(
        column.name(),
        type_raw(column.column_type()),
        true,
        canonical(column.column_type()),
    )
}

pub fn convert_row(row: &tiberius::Row) -> Vec<Value> {
    row.cells().map(|(_, data)| convert_data(data)).collect()
}

fn convert_data(data: &ColumnData<'_>) -> Value {
    match data {
        ColumnData::U8(v) => int_json(v.map(i64::from)),
        ColumnData::I16(v) => int_json(v.map(i64::from)),
        ColumnData::I32(v) => int_json(v.map(i64::from)),
        ColumnData::I64(v) => int_json(*v),
        ColumnData::F32(v) => float_json(v.map(f64::from)),
        ColumnData::F64(v) => float_json(*v),
        ColumnData::Bit(v) => v.map_or(Value::Null, |b| Value::Bool(b)),
        ColumnData::String(v) => v
            .as_ref()
            .map_or(Value::Null, |s| Value::String(s.to_string())),
        ColumnData::Guid(v) => v.map_or(Value::Null, |g| Value::String(g.to_string())),
        ColumnData::Binary(v) => v
            .as_ref()
            .map_or(Value::Null, |b| Value::String(BASE64.encode(b.as_ref()))),
        ColumnData::Numeric(v) => {
            v.map_or(Value::Null, |n| Value::String(n.to_string()))
        }
        ColumnData::Xml(v) => v
            .as_ref()
            .map_or(Value::Null, |x| Value::String(x.as_ref().to_string())),
        ColumnData::DateTime(v) => v.map_or(Value::Null, |d| {
            datetime_json(i64::from(d.days()), i64::from(d.seconds_fragments()), 300)
        }),
        ColumnData::SmallDateTime(v) => v.map_or(Value::Null, |d| {
            // TDS 的 smalldatetime 第二段是「自午夜起的分钟数」。
            datetime_json(i64::from(d.days()), i64::from(d.seconds_fragments()) * 60, 1)
        }),
        ColumnData::Date(v) => v.map_or(Value::Null, |d| {
            date_json(d.days() as i64)
        }),
        ColumnData::Time(v) => v.map_or(Value::Null, |t| {
            Value::String(format_time(t.increments(), t.scale()))
        }),
        ColumnData::DateTime2(v) => v.map_or(Value::Null, |dt| {
            Value::String(format_datetime2(dt.date().days() as i64, dt.time().increments(), dt.time().scale(), None))
        }),
        ColumnData::DateTimeOffset(v) => v.map_or(Value::Null, |dto| {
            let dt = dto.datetime2();
            Value::String(format_datetime2(
                dt.date().days() as i64,
                dt.time().increments(),
                dt.time().scale(),
                Some(dto.offset()),
            ))
        }),
    }
}

fn int_json(v: Option<i64>) -> Value {
    v.map_or(Value::Null, |n| json!(n))
}

fn float_json(v: Option<f64>) -> Value {
    match v {
        None => Value::Null,
        Some(f) if f.is_finite() => json!(f),
        Some(f) => Value::String(f.to_string()),
    }
}

fn date_json(days: i64) -> Value {
    match date_from_days(days) {
        Some(date) => Value::String(date.format("%Y-%m-%d").to_string()),
        None => Value::Null,
    }
}

fn datetime_json(days: i64, seconds: i64, per_second: i64) -> Value {
    let Some(date) = date_from_days(days + 693_595) else {
        return Value::Null;
    };
    let total_micros = if per_second == 300 {
        seconds * 1_000_000 / 300
    } else {
        seconds * 1_000_000
    };
    let time = micros_to_time(total_micros);
    match time {
        Some(time) => Value::String(format!("{}", NaiveDateTime::new(date, time).format("%Y-%m-%dT%H:%M:%S%.6fZ"))),
        None => Value::Null,
    }
}

/// `Date::days()` 是自公元 1 年 1 月 1 日起的天数；chrono 的 `days_from_ce` 以 1 年 1 月 1 日为第 1 天。
fn date_from_days(days: i64) -> Option<NaiveDate> {
    NaiveDate::from_num_days_from_ce_opt(days as i32 + 1)
}

fn micros_to_time(micros: i64) -> Option<NaiveTime> {
    if !(0..86_400_000_000).contains(&micros) {
        return None;
    }
    let secs = (micros / 1_000_000) as u32;
    let nano = ((micros % 1_000_000) * 1_000) as u32;
    NaiveTime::from_num_seconds_from_midnight_opt(secs, nano)
}

fn format_time(increments: u64, scale: u8) -> String {
    let micros = if scale >= 6 {
        (increments / 10u64.pow(u32::from(scale - 6))) as i64
    } else {
        increments as i64 * 10i64.pow(6 - u32::from(scale))
    };
    micros_to_time(micros)
        .map(|t| t.format("%H:%M:%S%.6f").to_string())
        .unwrap_or_else(|| "00:00:00".to_string())
}

fn format_datetime2(days: i64, increments: u64, scale: u8, offset_minutes: Option<i16>) -> String {
    let Some(date) = date_from_days(days) else {
        return String::new();
    };
    let micros = if scale >= 6 {
        (increments / 10u64.pow(u32::from(scale - 6))) as i64
    } else {
        increments as i64 * 10i64.pow(6 - u32::from(scale))
    };
    let Some(time) = micros_to_time(micros) else {
        return String::new();
    };
    let naive = NaiveDateTime::new(date, time);
    match offset_minutes {
        Some(offset) => {
            let sign = if offset >= 0 { '+' } else { '-' };
            let abs = offset.abs();
            format!(
                "{}{}{:02}:{:02}",
                naive.format("%Y-%m-%dT%H:%M:%S%.6f"),
                sign,
                abs / 60,
                abs % 60
            )
        }
        None => format!("{}Z", naive.format("%Y-%m-%dT%H:%M:%S%.6f")),
    }
}

/// 绑定参数：JSON 标量直接下推；数组/对象转 JSON 文本，绝不悄悄改形。
pub fn bind_number(value: &serde_json::Number) -> BoundNumber {
    if let Some(n) = value.as_i64() {
        BoundNumber::Int(n)
    } else if let Some(f) = value.as_f64() {
        BoundNumber::Float(f)
    } else {
        BoundNumber::Text(value.to_string())
    }
}

pub enum BoundNumber {
    Int(i64),
    Float(f64),
    Text(String),
}

/// 供 meta 查询复用：日期值转字符串。
#[allow(dead_code)]
pub fn date_offset(base: NaiveDate, days: i64) -> Option<NaiveDate> {
    base.checked_add_signed(Duration::days(days))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiberius::time::{Date, DateTime, SmallDateTime, Time};

    #[test]
    fn sql_date_zero_is_year_one() {
        let value = convert_data(&ColumnData::Date(Some(Date::new(0))));
        assert_eq!(value, json!("0001-01-01"));
    }

    #[test]
    fn datetime_zero_is_1900() {
        let value = convert_data(&ColumnData::DateTime(Some(DateTime::new(0, 0))));
        assert_eq!(value, json!("1900-01-01T00:00:00.000000Z"));
    }

    #[test]
    fn smalldatetime_second_field_is_minutes() {
        let value = convert_data(&ColumnData::SmallDateTime(Some(SmallDateTime::new(
            0, 60,
        ))));
        assert_eq!(value, json!("1900-01-01T01:00:00.000000Z"));
    }

    #[test]
    fn time_keeps_microseconds() {
        let value = convert_data(&ColumnData::Time(Some(Time::new(3723 * 10_000_000, 7))));
        assert_eq!(value, json!("01:02:03.000000"));
    }

    #[test]
    fn datetimeoffset_carries_offset() {
        let dto = tiberius::time::DateTimeOffset::new(
            tiberius::time::DateTime2::new(Date::new(0), Time::new(0, 7)),
            480,
        );
        let value = convert_data(&ColumnData::DateTimeOffset(Some(dto)));
        assert_eq!(value, json!("0001-01-01T00:00:00.000000+08:00"));
    }

    #[test]
    fn numeric_keeps_decimal_text() {
        let value = convert_data(&ColumnData::Numeric(Some(
            tiberius::numeric::Numeric::new_with_scale(12345, 2),
        )));
        assert_eq!(value, json!("123.45"));
    }

    #[test]
    fn type_raw_mapping_covers_common_types() {
        assert_eq!(type_raw(ColumnType::Int4), "INT");
        assert_eq!(type_raw(ColumnType::Datetime2), "DATETIME2");
        assert_eq!(canonical(ColumnType::Datetime2), Canonical::Timestamp);
        assert_eq!(canonical(ColumnType::Daten), Canonical::Date);
    }
}

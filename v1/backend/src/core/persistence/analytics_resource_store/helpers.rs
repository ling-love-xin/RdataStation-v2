use chrono::{DateTime, NaiveDateTime, Utc};

use crate::core::error::CoreError;

pub fn parse_datetime(s: String) -> Result<DateTime<Utc>, CoreError> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| ndt.and_utc())
                .map_err(|_| {
                    crate::core::error::CoreError::common(crate::core::error::CommonError::General(
                        format!("Invalid datetime format: {}", s),
                    ))
                })
        })
}

pub fn parse_datetime_sqlite(s: String) -> Result<DateTime<Utc>, rusqlite::Error> {
    parse_datetime(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            )),
        )
    })
}

//! 将 DuckDB 行转换为 Arrow 批处理
//! TODO(migration): 自 v1 `core/driver/native/duckdb.rs` 抽取，保持原实现；
//! 后续统一由 `shared::arrow` 承载转换能力。
//!
//! **取值覆盖**：数字 / 布尔 / 文本 / 二进制走原生 Arrow 数组，其余（时间戳 / 日期 / 时间 /
//! 十进制 / 区间 / 容器）统一走 [`crate::duckdb::value_text::display_text`] 渲成文本。
//! 千万不要再回到「不认识的都当 NULL」：那会让预览与结果集里这些列看起来是空的。
use std::sync::Arc;

use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use shared::error::{CoreError, DatabaseError};
use shared::models::ArrowBatch;

pub fn duckdb_rows_to_arrow(
    columns: &[String],
    rows: &[Vec<duckdb::types::Value>],
) -> Result<ArrowBatch, CoreError> {
    let num_rows = rows.len();
    let num_cols = columns.len();

    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(num_cols);

    for col_idx in 0..num_cols {
        let mut string_values: Vec<Option<String>> = Vec::with_capacity(num_rows);
        let mut int_values: Vec<Option<i64>> = Vec::with_capacity(num_rows);
        let mut float_values: Vec<Option<f64>> = Vec::with_capacity(num_rows);
        let mut bool_values: Vec<Option<bool>> = Vec::with_capacity(num_rows);
        let mut binary_values: Vec<Option<Vec<u8>>> = Vec::with_capacity(num_rows);

        // 遍历所有行确定最宽类型（0=Null, 1=Bool, 2=Int64, 3=Float64, 4=Blob, 5=Text）
        let mut detected_rank: u8 = 0;

        for row in rows {
            if let Some(value) = row.get(col_idx) {
                match value {
                    duckdb::types::Value::Null => {
                        string_values.push(None);
                        int_values.push(None);
                        float_values.push(None);
                        bool_values.push(None);
                        binary_values.push(None);
                    }
                    duckdb::types::Value::Boolean(b) => {
                        if detected_rank < 1 {
                            detected_rank = 1;
                        }
                        bool_values.push(Some(*b));
                    }
                    duckdb::types::Value::TinyInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::SmallInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::Int(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::BigInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i));
                    }
                    duckdb::types::Value::UTinyInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::USmallInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::UInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::UBigInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::HugeInt(i) => {
                        if detected_rank < 2 {
                            detected_rank = 2;
                        }
                        int_values.push(Some(*i as i64));
                    }
                    duckdb::types::Value::Float(f) => {
                        if detected_rank < 3 {
                            detected_rank = 3;
                        }
                        float_values.push(Some(*f as f64));
                    }
                    duckdb::types::Value::Double(f) => {
                        if detected_rank < 3 {
                            detected_rank = 3;
                        }
                        float_values.push(Some(*f));
                    }
                    duckdb::types::Value::Text(s) => {
                        detected_rank = 5; // Text 为最宽类型
                        string_values.push(Some(s.clone()));
                    }
                    duckdb::types::Value::Blob(b) => {
                        if detected_rank < 4 {
                            detected_rank = 4;
                        }
                        binary_values.push(Some(b.clone()));
                    }
                    duckdb::types::Value::Geometry(b) => {
                        if detected_rank < 4 {
                            detected_rank = 4;
                        }
                        binary_values.push(Some(b.clone()));
                    }
                    // 其余全部走**展示文本**（时间戳 / 日期 / 十进制 / 区间 / 容器 …）：
                    // 以前这里是 `string_values.push(None)`，于是这些列在预览 / 结果集里恒为 NULL，
                    // 而库里其实有值——“生成的数据是空的”是最容易被当成生成器坏掉的观感。
                    other => {
                        if let Some(text) = crate::duckdb::value_text::display_text(other) {
                            detected_rank = 5; // 与 Text 同宽
                            string_values.push(Some(text));
                        } else {
                            string_values.push(None);
                        }
                    }
                }
            }
        }

        let effective_type = match detected_rank {
            1 => DataType::Boolean,
            2 => DataType::Int64,
            3 => DataType::Float64,
            4 => DataType::Binary,
            _ => DataType::Utf8,
        };

        let array: ArrayRef = match effective_type {
            DataType::Boolean => Arc::new(BooleanArray::from(bool_values)),
            DataType::Int64 => Arc::new(Int64Array::from(int_values)),
            DataType::Float64 => Arc::new(Float64Array::from(float_values)),
            DataType::Binary => {
                let refs: Vec<Option<&[u8]>> = binary_values
                    .iter()
                    .map(|opt| opt.as_ref().map(|v| v.as_slice()))
                    .collect();
                Arc::new(BinaryArray::from(refs))
            }
            _ => Arc::new(StringArray::from(string_values)),
        };

        arrays.push(array);
    }

    let fields: Vec<Field> = columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let data_type = arrays[i].data_type().clone();
            Field::new(name, data_type, true)
        })
        .collect();

    let schema = Arc::new(Schema::new(fields));

    RecordBatch::try_new(schema, arrays).map_err(|e| {
        CoreError::database(DatabaseError::Driver {
            db_type: "duckdb".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::types::{TimeUnit, Value};

    /// 单元格取文本（测试助手）：把一批行转成「行列字符串」。
    fn cells(columns: &[&str], rows: Vec<Vec<Value>>) -> Vec<Vec<Option<String>>> {
        let columns: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        let batch = duckdb_rows_to_arrow(&columns, &rows).expect("转换");
        let materialized = shared::models::QueryResult::from_batches(columns, vec![batch]);
        materialized
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| match value {
                        shared::models::Value::Null => None,
                        other => Some(other.to_string()),
                    })
                    .collect()
            })
            .collect()
    }

    /// 时间戳 / 日期 / 十进制不能变 NULL（它们曾经全落进 `_ =>` 分支）。
    #[test]
    fn temporal_and_decimal_values_survive() {
        let rows = vec![
            vec![
                Value::Timestamp(TimeUnit::Second, 1_704_164_645),
                Value::Date32(19724),
                Value::Text("12.34".to_string()),
            ],
            vec![
                Value::Timestamp(TimeUnit::Second, 1_704_164_646),
                Value::Date32(19725),
                Value::Text("13.00".to_string()),
            ],
        ];
        let got = cells(&["ts", "d", "amount"], rows);
        assert_eq!(got[0][0].as_deref(), Some("2024-01-02 03:04:05"));
        assert_eq!(got[0][1].as_deref(), Some("2024-01-02"));
        assert_eq!(got[1][0].as_deref(), Some("2024-01-02 03:04:06"));
        assert_eq!(got[1][1].as_deref(), Some("2024-01-03"));
    }

    /// 容器类型也给文本，而不是空白列。
    #[test]
    fn nested_values_render_as_text() {
        let rows = vec![vec![Value::List(vec![Value::Int(1), Value::Int(2)])]];
        let got = cells(&["nums"], rows);
        assert_eq!(got[0][0].as_deref(), Some("[1, 2]"));
    }

    /// 真正的 NULL 仍然是 NULL（与「不认识这个类型」区分开）。
    #[test]
    fn null_stays_null() {
        let rows = vec![vec![Value::Null], vec![Value::Int(7)]];
        let got = cells(&["n"], rows);
        assert_eq!(got[0][0], None);
        assert_eq!(got[1][0].as_deref(), Some("7"));
    }
}

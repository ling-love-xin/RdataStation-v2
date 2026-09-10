//! 将 DuckDB 行转换为 Arrow 批处理
//! TODO(migration): 自 v1 `core/driver/native/duckdb.rs` 抽取，保持原实现；
//! 后续统一由 `shared::arrow` 承载转换能力。
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
                    _ => {
                        string_values.push(None);
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

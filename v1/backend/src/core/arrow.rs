use arrow::{array::ArrayRef, record_batch::RecordBatch};
use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Arrow 批处理类型
pub type ArrowBatch = RecordBatch;

/// Arrow 批处理流
pub type ArrowBatchStream =
    tokio::sync::mpsc::Receiver<Result<ArrowBatch, crate::core::error::CoreError>>;

/// Arrow 数据格式的统一处理
///
/// 提供 Arrow 格式与其他格式的转换，以及统一的 Arrow 处理接口
pub struct ArrowHandler;

impl ArrowHandler {
    /// 将 Arrow 批处理转换为 JSON
    pub fn batch_to_json(batch: &ArrowBatch) -> serde_json::Value {
        let mut rows = Vec::new();

        for row_idx in 0..batch.num_rows() {
            let mut row = serde_json::Map::new();

            for (col_idx, field) in batch.schema().fields().iter().enumerate() {
                let array = batch.column(col_idx);
                let value = Self::array_value_to_json(array, row_idx);
                row.insert(field.name().to_string(), value);
            }

            rows.push(serde_json::Value::Object(row));
        }

        serde_json::Value::Array(rows)
    }

    /// 将数组值转换为 JSON
    fn array_value_to_json(array: &ArrayRef, index: usize) -> serde_json::Value {
        if array.is_null(index) {
            return serde_json::Value::Null;
        }

        use arrow::datatypes::DataType;

        match array.data_type() {
            DataType::Boolean => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::BooleanArray>() {
                    serde_json::Value::Bool(arr.value(index))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Int8 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::Int8Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index) as i64))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Int16 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::Int16Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index) as i64))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Int32 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::Int32Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index) as i64))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Int64 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::Int64Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index)))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::UInt8 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::UInt8Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index) as u64))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::UInt16 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::UInt16Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index) as u64))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::UInt32 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::UInt32Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index) as u64))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::UInt64 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::UInt64Array>() {
                    serde_json::Value::Number(serde_json::Number::from(arr.value(index)))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Float32 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::Float32Array>() {
                    let val = arr.value(index) as f64;
                    serde_json::Number::from_f64(val)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Float64 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::Float64Array>() {
                    let val = arr.value(index);
                    serde_json::Number::from_f64(val)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Utf8 => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::StringArray>() {
                    serde_json::Value::String(arr.value(index).to_string())
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::LargeUtf8 => {
                if let Some(arr) = array
                    .as_any()
                    .downcast_ref::<arrow::array::LargeStringArray>()
                {
                    serde_json::Value::String(arr.value(index).to_string())
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::Binary => {
                if let Some(arr) = array.as_any().downcast_ref::<arrow::array::BinaryArray>() {
                    let bytes = arr.value(index);
                    serde_json::Value::String(STANDARD.encode(bytes))
                } else {
                    serde_json::Value::Null
                }
            }
            DataType::LargeBinary => {
                if let Some(arr) = array
                    .as_any()
                    .downcast_ref::<arrow::array::LargeBinaryArray>()
                {
                    let bytes = arr.value(index);
                    serde_json::Value::String(STANDARD.encode(bytes))
                } else {
                    serde_json::Value::Null
                }
            }
            _ => {
                // 对于其他类型，返回字符串表示
                serde_json::Value::String(format!("{:?}", array))
            }
        }
    }
}

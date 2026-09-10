use std::pin::Pin;
use std::task::{Context, Poll};

use arrow::array::Array;
use futures::stream::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::core::error::{CoreError, DatabaseError};
use crate::core::models::{ArrowBatch, QueryResult, Value};

use super::TauriAdapterError;

/// 查询结果块
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResultChunk {
    /// 列信息（仅在第一个块中包含）
    pub columns: Option<Vec<String>>,
    /// 当前块的行数据
    pub rows: Vec<Vec<Value>>,
    /// 是否是最后一个块
    pub is_last: bool,
    /// 总记录数（仅在最后一个块中包含）
    pub total_rows: Option<usize>,
}

impl QueryResultChunk {
    /// 创建第一个块
    pub fn first(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        Self {
            columns: Some(columns),
            rows,
            is_last: false,
            total_rows: None,
        }
    }

    /// 创建中间块
    pub fn middle(rows: Vec<Vec<Value>>) -> Self {
        Self {
            columns: None,
            rows,
            is_last: false,
            total_rows: None,
        }
    }

    /// 创建最后一个块
    pub fn last(rows: Vec<Vec<Value>>, total_rows: usize) -> Self {
        Self {
            columns: None,
            rows,
            is_last: true,
            total_rows: Some(total_rows),
        }
    }
}

/// 查询结果流
pub struct QueryResultStream {
    /// 查询结果
    result: Option<QueryResult>,
    /// 当前行索引
    current_index: usize,
    /// 块大小
    chunk_size: usize,
    /// 取消令牌
    cancel_token: CancellationToken,
}

impl QueryResultStream {
    /// 创建新的查询结果流
    pub fn new(
        result: QueryResult,
        chunk_size: Option<usize>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            result: Some(result),
            current_index: 0,
            chunk_size: chunk_size.unwrap_or(1000), // 默认块大小为 1000 行
            cancel_token,
        }
    }

    /// 将查询结果转换为流
    pub fn from_query_result(result: QueryResult, cancel_token: CancellationToken) -> Self {
        Self::new(result, None, cancel_token)
    }

    /// 将查询结果转换为流，指定块大小
    pub fn from_query_result_with_chunk_size(
        result: QueryResult,
        chunk_size: usize,
        cancel_token: CancellationToken,
    ) -> Self {
        Self::new(result, Some(chunk_size), cancel_token)
    }
}

impl Stream for QueryResultStream {
    type Item = Result<QueryResultChunk, TauriAdapterError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 检查是否已取消
        if self.cancel_token.is_cancelled() {
            return Poll::Ready(Some(Err(TauriAdapterError::CoreError(
                CoreError::database(DatabaseError::query(
                    "stream",
                    "Query cancelled".to_string(),
                )),
            ))));
        }

        // 保存当前索引和块大小
        let current_index = self.current_index;
        let chunk_size = self.chunk_size;

        // 获取当前结果的可变引用
        let result = match &mut self.result {
            Some(result) => result,
            None => return Poll::Ready(None),
        };

        // 检查是否已处理完所有行
        if current_index >= result.total_rows() {
            // 标记结果已处理完毕
            self.result.take();
            return Poll::Ready(None);
        }

        // 计算当前块的结束索引
        let end_index = std::cmp::min(current_index + chunk_size, result.total_rows());

        // 获取当前块的行
        let rows = extract_rows_from_batches(&result.batches, current_index, end_index);

        // 创建块
        let chunk = if current_index == 0 {
            // 第一个块，包含列信息
            QueryResultChunk::first(result.columns.clone(), rows)
        } else if end_index >= result.total_rows() {
            // 最后一个块，标记为最后一个并包含总行数
            QueryResultChunk::last(rows, result.total_rows())
        } else {
            // 中间块
            QueryResultChunk::middle(rows)
        };

        // 更新当前索引
        self.current_index = end_index;

        // 返回块
        Poll::Ready(Some(Ok(chunk)))
    }
}

/// 流适配器，用于 Tauri 命令
pub struct StreamAdapter;

impl StreamAdapter {
    /// 将查询结果转换为可用于 Tauri 的流
    pub fn adapt_query_result(
        result: QueryResult,
        cancel_token: CancellationToken,
    ) -> impl Stream<Item = Result<QueryResultChunk, TauriAdapterError>> {
        QueryResultStream::from_query_result(result, cancel_token)
    }

    /// 将查询结果转换为可用于 Tauri 的流，指定块大小
    pub fn adapt_query_result_with_chunk_size(
        result: QueryResult,
        chunk_size: usize,
        cancel_token: CancellationToken,
    ) -> impl Stream<Item = Result<QueryResultChunk, TauriAdapterError>> {
        QueryResultStream::from_query_result_with_chunk_size(result, chunk_size, cancel_token)
    }
}

/// 从 Arrow 批处理中提取行数据
fn extract_rows_from_batches(batches: &[ArrowBatch], start: usize, end: usize) -> Vec<Vec<Value>> {
    let mut rows = Vec::new();
    let mut current_row = 0;

    for batch in batches {
        let num_rows = batch.num_rows();

        for row_idx in 0..num_rows {
            if current_row >= start && current_row < end {
                let mut row_values = Vec::new();
                for col_idx in 0..batch.num_columns() {
                    let column = batch.column(col_idx);
                    let value = arrow_array_to_value(column, row_idx);
                    row_values.push(value);
                }
                rows.push(row_values);
            }
            current_row += 1;
        }

        if current_row >= end {
            break;
        }
    }

    rows
}

/// 将 Arrow 数组中的值转换为 Value
fn arrow_array_to_value(array: &dyn Array, index: usize) -> Value {
    use arrow::array::*;

    if array.is_null(index) {
        return Value::Null;
    }

    if let Some(arr) = array.as_any().downcast_ref::<StringArray>() {
        return Value::Text(arr.value(index).to_string());
    }
    if let Some(arr) = array.as_any().downcast_ref::<Int64Array>() {
        return Value::Int(arr.value(index));
    }
    if let Some(arr) = array.as_any().downcast_ref::<Float64Array>() {
        return Value::Float(arr.value(index));
    }
    if let Some(arr) = array.as_any().downcast_ref::<BooleanArray>() {
        return Value::Bool(arr.value(index));
    }
    if let Some(arr) = array.as_any().downcast_ref::<BinaryArray>() {
        return Value::Bytes(arr.value(index).to_vec());
    }

    Value::Text(format!("{:?}", array))
}

/// 流处理工具
pub mod stream_utils {
    use super::*;

    /// 创建取消令牌
    pub fn create_cancel_token() -> CancellationToken {
        CancellationToken::new()
    }

    /// 取消流处理
    pub fn cancel_stream(token: CancellationToken) {
        token.cancel();
    }

    /// 处理流结果，将结果收集到内存中
    pub async fn collect_stream_results<S, T>(stream: S) -> Result<Vec<T>, TauriAdapterError>
    where
        S: Stream<Item = Result<T, TauriAdapterError>>,
    {
        let mut results = Vec::new();

        futures::pin_mut!(stream);

        while let Some(result) = stream.next().await {
            results.push(result?);
        }

        Ok(results)
    }

    /// 处理流结果，应用回调函数
    pub async fn process_stream_results<S, T, F>(
        stream: S,
        mut callback: F,
    ) -> Result<(), TauriAdapterError>
    where
        S: Stream<Item = Result<T, TauriAdapterError>>,
        F: FnMut(T) -> Result<(), TauriAdapterError>,
    {
        futures::pin_mut!(stream);

        while let Some(result) = stream.next().await {
            callback(result?)?;
        }

        Ok(())
    }
}

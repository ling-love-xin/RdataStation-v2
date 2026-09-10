/**
 * 流式执行引擎
 *
 * 负责：
 * - 流式查询结果拼接
 * - 多结果集合并
 * - 后处理（过滤、排序、聚合）
 */
use crate::core::dbi::context::QueryContext;
use crate::core::dbi::engine::ExecutionEngine;
use crate::core::error::CommonError;
use crate::core::error::CoreError;
use crate::core::models::{ArrowBatch, QueryResult, Value};
use arrow::array::BooleanArray as ArrowBooleanArray;
use arrow::array::{Float64Array, Int64Array, StringArray};
use arrow::compute::kernels::filter::filter_record_batch;
use arrow::compute::{
    concat_batches, lexsort_to_indices, take_record_batch, SortColumn, SortOptions,
};
use std::sync::Arc;

/// 流式执行引擎
pub struct StreamEngine;

impl StreamEngine {
    /// 创建新的流式引擎
    pub fn new() -> Self {
        Self
    }
}

impl Default for StreamEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamEngine {
    /// 合并多个查询结果
    ///
    /// # 参数
    /// - `results`: 查询结果列表
    ///
    /// # 返回
    /// 合并后的查询结果
    pub fn merge_results(&self, results: Vec<QueryResult>) -> Result<QueryResult, CoreError> {
        if results.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "No results to merge".to_string(),
            )));
        }

        if results.len() == 1 {
            return results.into_iter().next().ok_or_else(|| {
                CoreError::common(CommonError::General(
                    "Expected exactly one result".to_string(),
                ))
            });
        }

        let first = &results[0];
        let columns = first.columns.clone();

        for result in results.iter().skip(1) {
            if result.columns != columns {
                return Err(CoreError::common(CommonError::General(
                    "Cannot merge results with different column schemas".to_string(),
                )));
            }
        }

        let mut merged_batches: Vec<ArrowBatch> = Vec::new();

        for result in results {
            merged_batches.extend(result.batches);
        }

        let final_batches = if merged_batches.len() > 1 {
            let schema = merged_batches[0].schema();
            vec![concat_batches(&schema, &merged_batches).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to merge batches: {}",
                    e
                )))
            })?]
        } else {
            merged_batches
        };

        Ok(QueryResult {
            columns,
            batches: final_batches,
            ..Default::default()
        })
    }

    /// 对查询结果进行过滤
    ///
    /// # 参数
    /// - `result`: 查询结果
    /// - `column_index`: 过滤列索引
    /// - `filter_value`: 过滤值
    ///
    /// # 返回
    /// 过滤后的查询结果
    pub fn filter_result(
        &self,
        result: QueryResult,
        column_index: usize,
        filter_value: &Value,
    ) -> Result<QueryResult, CoreError> {
        if column_index >= result.columns.len() {
            return Err(CoreError::common(CommonError::General(format!(
                "Column index {} out of range",
                column_index
            ))));
        }

        let mut filtered_batches = Vec::new();

        for batch in result.batches {
            let column = batch.column(column_index);
            let filter_array = match filter_value {
                Value::Int(v) => {
                    if let Some(arr) = column.as_any().downcast_ref::<Int64Array>() {
                        Arc::new(ArrowBooleanArray::from_iter(
                            arr.iter().map(|opt| opt.map(|val| val == *v)),
                        ))
                    } else {
                        Arc::new(ArrowBooleanArray::from(vec![false; batch.num_rows()]))
                    }
                }
                Value::Float(v) => {
                    if let Some(arr) = column.as_any().downcast_ref::<Float64Array>() {
                        Arc::new(ArrowBooleanArray::from_iter(
                            arr.iter().map(|opt| opt.map(|val| val == *v)),
                        ))
                    } else {
                        Arc::new(ArrowBooleanArray::from(vec![false; batch.num_rows()]))
                    }
                }
                Value::Text(v) => {
                    if let Some(arr) = column.as_any().downcast_ref::<StringArray>() {
                        Arc::new(ArrowBooleanArray::from_iter(
                            arr.iter().map(|opt| opt.map(|val| val == v)),
                        ))
                    } else {
                        Arc::new(ArrowBooleanArray::from(vec![false; batch.num_rows()]))
                    }
                }
                _ => Arc::new(ArrowBooleanArray::from(vec![false; batch.num_rows()])),
            };

            let filtered = filter_record_batch(&batch, &filter_array).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to filter batch: {}",
                    e
                )))
            })?;
            filtered_batches.push(filtered);
        }

        Ok(QueryResult {
            columns: result.columns,
            batches: filtered_batches,
            ..Default::default()
        })
    }

    /// 对查询结果进行排序
    ///
    /// # 参数
    /// - `result`: 查询结果
    /// - `column_index`: 排序列索引
    /// - `ascending`: 是否升序
    ///
    /// # 返回
    /// 排序后的查询结果
    pub fn sort_result(
        &self,
        result: QueryResult,
        column_index: usize,
        ascending: bool,
    ) -> Result<QueryResult, CoreError> {
        if column_index >= result.columns.len() {
            return Err(CoreError::common(CommonError::General(format!(
                "Column index {} out of range",
                column_index
            ))));
        }

        if result.batches.is_empty() {
            return Ok(result);
        }

        let schema = result.batches[0].schema();
        let merged = concat_batches(&schema, &result.batches).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to merge batches for sorting: {}",
                e
            )))
        })?;

        let sort_column = merged.column(column_index);
        let sort_columns = vec![SortColumn {
            values: sort_column.clone(),
            options: Some(SortOptions {
                descending: !ascending,
                nulls_first: true,
            }),
        }];

        let sort_indices = lexsort_to_indices(&sort_columns, None).map_err(|e| {
            CoreError::common(CommonError::General(format!("Failed to sort: {}", e)))
        })?;

        let sorted = take_record_batch(&merged, &sort_indices).map_err(|e| {
            CoreError::common(CommonError::General(format!("Failed to apply sort: {}", e)))
        })?;

        Ok(QueryResult {
            columns: result.columns,
            batches: vec![sorted],
            ..Default::default()
        })
    }

    /// 对查询结果进行限制
    ///
    /// # 参数
    /// - `result`: 查询结果
    /// - `limit`: 限制行数
    ///
    /// # 返回
    /// 限制后的查询结果
    pub fn limit_result(&self, result: QueryResult, limit: usize) -> QueryResult {
        if result.batches.is_empty() {
            return result;
        }

        let mut limited_batches = Vec::new();
        let mut remaining = limit;

        for batch in result.batches {
            if remaining == 0 {
                break;
            }

            let batch_rows = batch.num_rows();
            if batch_rows <= remaining {
                limited_batches.push(batch);
                remaining -= batch_rows;
            } else {
                let indices: Vec<i64> = (0..remaining as i64).collect();
                let indices_array = arrow::array::Int64Array::from(indices);
                if let Ok(limited) = take_record_batch(&batch, &indices_array) {
                    limited_batches.push(limited);
                }
                remaining = 0;
            }
        }

        let _total_limited = limited_batches.iter().map(|b| b.num_rows()).sum::<usize>();

        QueryResult {
            columns: result.columns,
            batches: limited_batches,
            ..Default::default()
        }
    }
}

/// 比较两个 Value 值
#[allow(dead_code)]
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        (Value::Null, _) => std::cmp::Ordering::Less,
        (_, Value::Null) => std::cmp::Ordering::Greater,
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Text(a), Value::Text(b)) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

#[async_trait::async_trait]
impl ExecutionEngine for StreamEngine {
    async fn execute(&self, sql: &str, _context: &QueryContext) -> Result<QueryResult, CoreError> {
        // 流式引擎通过解析特殊注释指令来执行操作
        // 例如: SELECT * FROM table -- stream:limit:100 -- stream:sort:0:asc

        // 解析流式指令
        let mut limit: Option<usize> = None;
        let mut sort_column: Option<(usize, bool)> = None;
        let mut filter: Option<(usize, Value)> = None;

        if let Some(pos) = sql.find("-- stream:") {
            let directives = &sql[pos + 9..];
            for directive in directives.split_whitespace() {
                if directive.starts_with("limit:") {
                    if let Some(limit_str) = directive.strip_prefix("limit:") {
                        if let Ok(l) = limit_str.parse::<usize>() {
                            limit = Some(l);
                        }
                    }
                } else if let Some(stripped) = directive.strip_prefix("sort:") {
                    let parts: Vec<&str> = stripped.split(':').collect();
                    if parts.len() >= 2 {
                        if let (Ok(col), asc) = (parts[0].parse::<usize>(), parts[1] == "asc") {
                            sort_column = Some((col, asc));
                        }
                    }
                } else if let Some(stripped) = directive.strip_prefix("filter:") {
                    let parts: Vec<&str> = stripped.split(':').collect();
                    if parts.len() >= 2 {
                        if let Ok(col) = parts[0].parse::<usize>() {
                            let value = if let Ok(i) = parts[1].parse::<i64>() {
                                Value::Int(i)
                            } else if let Ok(f) = parts[1].parse::<f64>() {
                                Value::Float(f)
                            } else {
                                Value::Text(parts[1].to_string())
                            };
                            filter = Some((col, value));
                        }
                    }
                }
            }
        }

        // 注意：这里需要实际执行基础查询
        // 由于流式引擎是后处理引擎，需要依赖其他引擎执行基础查询
        // 这里返回一个占位结果，实际使用时会通过 merge_results 等方法处理
        let base_sql = if let Some(pos) = sql.find("-- stream:") {
            sql[..pos].trim()
        } else {
            sql
        };

        // 如果上下文提供了基础查询结果，则应用后处理
        // 否则返回错误，提示需要使用 merge_results 等方法
        if base_sql.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "StreamEngine requires a base query or merged results".to_string(),
            )));
        }

        // 这里应该调用其他引擎执行基础查询
        // 但由于架构限制，这里返回一个空结果
        // 实际使用时，应该通过 QueryRouter 或其他机制获取基础查询结果
        let _ = (limit, sort_column, filter);
        Ok(QueryResult::empty())
    }

    fn name(&self) -> &str {
        "stream"
    }

    fn supports(&self, sql: &str) -> bool {
        sql.contains("-- stream:") || sql.contains("-- merge:") || sql.contains("-- post:")
    }
}

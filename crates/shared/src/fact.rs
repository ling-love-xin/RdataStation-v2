//! 结果集事实层（L0）：复用 Arrow batches 的类型化事实档。
//!
//! 分层口径（见 `docs/analysis/结果集数据流-事实层显示层分离方案.md` §3 / §4）：
//! - L0 事实层 [`FactFrame`]：持有 Arrow batches 引用 + schema + 列名，是类型保真的唯一来源；
//! - L1 显示层：现状 `QueryData.rows: Vec<Vec<String>>`，由事实层单向派生，永不回写事实层；
//! - 事实档只在会话内存存活，不落盘、不进跨进程 JSON 契约（序列化契约不含 batches）。
//!
//! 起步档事实值复用 [`crate::models::Value`] 六变体（Null / Bool / Int / Float / Text / Bytes）；
//! 日期 / Decimal / Timestamp 等类型族扩展属 E 阶段 engine 跨线 backlog，本模块不替它承诺保真。

use arrow::datatypes::{DataType, SchemaRef};
use arrow::record_batch::RecordBatch;
use thiserror::Error;

use crate::models::{arrow_value_at, QueryResult, Value};

/// 事实值：起步档复用显示层同源的六变体，避免再造一套类型化行结构。
pub type FactValue = Value;

/// 事实列类型族（窄于 Arrow `DataType`），供排序 / 统计 / 分组选择实现分支。
///
/// 未覆盖的 Arrow 类型统一落 [`FactKind::Other`]，**不静默降级**成 Utf8；
/// 下游遇到 `Other` 走文本兜底路径并在界面标注无类型档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactKind {
    Int,
    Float,
    Bool,
    Binary,
    Utf8,
    Other,
}

impl FactKind {
    /// 从 Arrow schema 数据类型推导类型族；匹配面与 `models.rs` 的类型映射对齐。
    pub fn from_data_type(data_type: &DataType) -> Self {
        match data_type {
            DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64 => Self::Int,
            DataType::Float16 | DataType::Float32 | DataType::Float64 => Self::Float,
            DataType::Boolean => Self::Bool,
            DataType::Utf8 | DataType::LargeUtf8 => Self::Utf8,
            DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_) => Self::Binary,
            _ => Self::Other,
        }
    }
}

/// 事实档构造 / 访问失败。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FactError {
    /// 事实档不接受空 batches（DML / DDL / 加速档 / 联邦档 / JSON 往返形态走 None 降级档）。
    #[error("事实档不接受空 batches")]
    EmptyBatches,
    /// 追加批次与基准 schema 形状不一致（列数 / 列名 / 数据类型任一不符）。
    #[error("追加批次形状不一致：期望 {expected} 列，实际 {actual} 列")]
    ShapeMismatch { expected: usize, actual: usize },
    /// 列索引越界。
    #[error("列索引越界：{col}（共 {total} 列）")]
    ColumnIndexOutOfRange { col: usize, total: usize },
    /// 行索引越界（按全局行号，跨批累加）。
    #[error("行索引越界：{row}（共 {total} 行）")]
    RowIndexOutOfRange { row: usize, total: usize },
}

/// 事实档：Arrow batches 的会话内存持有者。
///
/// **不 derive `Clone`**：batches 深拷贝代价高，槽位侧用 `Arc<FactFrame>` 共享
/// （`result_panel` 的 `FactState::InMemory`）；需要副本时先想清楚是否真要物化两份。
///
/// 【B2】`temp_table` 挂本地 DuckDB 常驻表名：进分析 / 带类型导出触发物化后回挂，
/// 重复分析复用同一张表、不重建；`None` = 尚未物化。表档物化后 `release_batches`
/// 释放 Rust 堆上的 batches，只保留 schema / 列名 / 行数 / 表名供元数据与类型族访问。
#[derive(Debug)]
pub struct FactFrame {
    columns: Vec<String>,
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    /// 常驻表名（`tmp_q_` 前缀；`None` = 尚未物化）。
    temp_table: Option<String>,
    /// 行数缓存：表档释放 batches 后仍要报出真实行数，不能按空批累加回 0。
    row_count: usize,
}

impl FactFrame {
    /// 从 `QueryResult` 构造事实档；batches 为空（无类型化回执）返回 `None`，由调用方走降级档。
    pub fn from_query_result(result: &QueryResult) -> Option<Self> {
        if result.batches.is_empty() {
            return None;
        }
        Self::new(result.columns.clone(), result.batches.clone()).ok()
    }

    /// 显式构造：空 batches 或批次间形状不一致返回 [`FactError`]。
    pub fn new(columns: Vec<String>, batches: Vec<RecordBatch>) -> Result<Self, FactError> {
        if batches.is_empty() {
            return Err(FactError::EmptyBatches);
        }
        let schema = batches[0].schema();
        for batch in &batches[1..] {
            ensure_same_schema(&schema, batch.schema())?;
        }
        let row_count = batches.iter().map(|batch| batch.num_rows()).sum();
        Ok(Self {
            columns,
            schema,
            batches,
            temp_table: None,
            row_count,
        })
    }

    /// 列名（与 batches schema 列一一对应）。
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// 基准 schema（首批批次 schema；追加批次须与它同形状）。
    pub fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    /// 底层 batches 只读引用；渲染 / 派生路径通过访问器取值，不直接拆批。
    /// 表档释放后返回空切片（表档不回读 Rust 堆，方案 §4.1 表档边界）。
    pub fn batches(&self) -> &[RecordBatch] {
        &self.batches
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// 全局行数（构造 / 追加时缓存；表档释放 batches 后仍返回真实行数）。
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// 常驻表名（`None` = 尚未物化；已物化 = 重复分析复用这张表，不重建）。
    pub fn temp_table(&self) -> Option<&str> {
        self.temp_table.as_deref()
    }

    /// 回挂常驻表名（分析 / 带类型导出触发物化后由主线程回填路径调用）。
    pub fn set_temp_table(&mut self, table: String) {
        self.temp_table = Some(table);
    }

    /// 是否已经释放 batches（表档状态；`true` 时不允许再走 Rust 堆取值路径）。
    pub fn is_released(&self) -> bool {
        self.batches.is_empty()
    }

    /// 表档物化后释放 Rust 堆上的 batches：schema / 列名 / 行数 / 表名保留，
    /// 供元数据页、类型族判断与状态栏占用展示；数据本体留在本地 DuckDB 常驻表里。
    ///
    /// 表档上的排序 / 统计 / 分组一律 SQL 下推，不把表读回 Rust 堆（方案 §4.1 表档边界）。
    pub fn release_batches(&mut self) {
        self.batches.clear();
    }

    /// 列类型族；列索引越界返回 [`FactError::ColumnIndexOutOfRange`]。
    pub fn column_kind(&self, col: usize) -> Result<FactKind, FactError> {
        self.check_col(col)?;
        Ok(FactKind::from_data_type(
            self.schema.field(col).data_type(),
        ))
    }

    /// 按全局行号 + 列号取值；行索引跨批定位（批内偏移累加）。
    ///
    /// 表档（batches 已释放）不回读 Rust 堆：这里按行越界报错，调用方应走 SQL 下推路径。
    pub fn value_at(&self, row: usize, col: usize) -> Result<FactValue, FactError> {
        self.check_col(col)?;
        if self.batches.is_empty() {
            return Err(FactError::RowIndexOutOfRange {
                row,
                total: self.row_count,
            });
        }
        let mut remaining = row;
        for batch in &self.batches {
            let batch_rows = batch.num_rows();
            if remaining < batch_rows {
                return Ok(arrow_value_at(batch.column(col).as_ref(), remaining));
            }
            remaining -= batch_rows;
        }
        Err(FactError::RowIndexOutOfRange {
            row,
            total: self.row_count,
        })
    }

    /// 派生显示矩阵的唯一入口：值按 `Display` 渲染，`NULL` → `"NULL"`，
    /// 口径对齐现状 `editor_exec.rs` 的 `cell_text = Display`；渲染层不另写一套取值规则。
    ///
    /// 表档（batches 已释放）不回读：返回空矩阵，调用方应保持显示矩阵口径。
    pub fn rows_as_text(&self) -> Vec<Vec<String>> {
        if self.batches.is_empty() {
            return Vec::new();
        }
        let total = self.row_count;
        let cols = self.column_count();
        let mut rows = Vec::with_capacity(total);
        for row in 0..total {
            let mut line = Vec::with_capacity(cols);
            for col in 0..cols {
                let value = self.value_at(row, col).unwrap_or(Value::Null);
                line.push(value.to_string());
            }
            rows.push(line);
        }
        rows
    }

    /// 追加新段 batches；形状与基准 schema 不一致时**整批拒绝**，现有内容不被部分污染。
    pub fn append_batches(&mut self, new: Vec<RecordBatch>) -> Result<(), FactError> {
        if new.is_empty() {
            return Err(FactError::EmptyBatches);
        }
        for batch in &new {
            ensure_same_schema(&self.schema, batch.schema())?;
        }
        self.row_count += new.iter().map(|batch| batch.num_rows()).sum::<usize>();
        self.batches.extend(new);
        Ok(())
    }

    /// 列式占用估算（字节）：逐列 buffers 长度 + null 位图长度；
    /// 不含 Schema / Arc 元信息（量级小，水位估算可忽略）。
    ///
    /// 表档 batches 已释放：Rust 堆占用报 0，表内占用由 DuckDB 侧统计（方案 §4.1）。
    /// 双份估算（事实档 + 显示矩阵）由分流层按 `batches × 5` 系数折算，本方法只报事实档列式字节。
    pub fn estimated_bytes(&self) -> usize {
        let mut total = 0usize;
        for batch in &self.batches {
            for array in batch.columns() {
                let data = array.to_data();
                total += data.buffers().iter().map(|buffer| buffer.len()).sum::<usize>();
                if let Some(nulls) = data.nulls() {
                    total += nulls.buffer().len();
                }
            }
        }
        total
    }

    fn check_col(&self, col: usize) -> Result<(), FactError> {
        if col >= self.column_count() {
            Err(FactError::ColumnIndexOutOfRange {
                col,
                total: self.column_count(),
            })
        } else {
            Ok(())
        }
    }

    /// 【B5】内存档列式列统计：Int / Float 族从原生数值聚合 min / max / sum / avg；
    /// Bool 族只计非空与 NULL（无数值聚合）；Utf8 / Binary / Other 族返回 `None`，
    /// 由调用方回退显示矩阵的文本口径。
    ///
    /// 表档（batches 已释放）一律 `None`：表档本地统计走 SQL 下推，不回读 Rust 堆
    /// （方案 §4.1 表档边界）。
    pub fn column_stats(&self, col: usize) -> Option<FactColumnStats> {
        self.check_col(col).ok()?;
        if self.batches.is_empty() {
            return None;
        }
        let kind = self.column_kind(col).ok()?;
        if matches!(kind, FactKind::Utf8 | FactKind::Binary | FactKind::Other) {
            return None;
        }
        let mut stats = FactColumnStats::default();
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut sum = 0.0;
        let mut numeric = 0usize;
        for row in 0..self.row_count {
            let Ok(value) = self.value_at(row, col) else {
                continue;
            };
            if matches!(value, Value::Null) {
                stats.nulls += 1;
                continue;
            }
            stats.count += 1;
            if matches!(kind, FactKind::Int | FactKind::Float) {
                if let Some(number) = value.as_float() {
                    min = min.min(number);
                    max = max.max(number);
                    sum += number;
                    numeric += 1;
                }
            }
        }
        if numeric > 0 {
            stats.min = Some(min);
            stats.max = Some(max);
            stats.sum = Some(sum);
            stats.avg = Some(sum / numeric as f64);
        }
        Some(stats)
    }

    /// 【B5】内存档某格原生数值（排序用）：Int / Float 族返回 `f64`；Bool / 文本 /
    /// 二进制 / 其他族与 NULL 返回 `None`，调用方回退显示矩阵比较口径。
    ///
    /// 表档（batches 已释放）返回 `None`，不回读 Rust 堆。
    pub fn column_number(&self, row: usize, col: usize) -> Option<f64> {
        self.check_col(col).ok()?;
        if self.batches.is_empty() {
            return None;
        }
        let kind = self.column_kind(col).ok()?;
        if !matches!(kind, FactKind::Int | FactKind::Float) {
            return None;
        }
        self.value_at(row, col).ok()?.as_float()
    }

    /// 【D1】带类型档导出（Parquet / XLSX）的事实档可用模式：host 回执标注与
    /// workbench 执行分流共用同一口径，避免两处判定漂移（方案 §7-D）。
    ///
    /// 判定顺序与 B4 分析分支对齐：已挂常驻表名即复用（无论 batches 是否已释放），
    /// 其次才按内存档 batches 直灌；其余归显示矩阵猜型降级档。
    pub fn export_mode(&self) -> FactExportMode {
        if self.temp_table.is_some() {
            FactExportMode::ExistingTable
        } else if !self.is_released() && !self.batches.is_empty() {
            FactExportMode::Batches
        } else {
            FactExportMode::Matrix
        }
    }
}

/// 【B5】事实档列式列统计（与网格层 `ColumnStats` 字段对齐，便于直接映射；
/// shared 不反向依赖 result_panel，故在此另立一份窄结构）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FactColumnStats {
    /// 非 NULL 的行数
    pub count: usize,
    /// NULL 的行数
    pub nulls: usize,
    /// 数值样本最小值（无数值样本 = `None`）
    pub min: Option<f64>,
    /// 数值样本最大值
    pub max: Option<f64>,
    /// 数值样本之和
    pub sum: Option<f64>,
    /// 数值样本均值
    pub avg: Option<f64>,
}

/// 形状校验：列数、列名、数据类型逐项一致；nullable 不参与（显示 / 分析路径不区分可空性）。
fn ensure_same_schema(expected: &SchemaRef, actual: SchemaRef) -> Result<(), FactError> {
    let expected_cols = expected.fields().len();
    let actual_cols = actual.fields().len();
    if expected_cols != actual_cols {
        return Err(FactError::ShapeMismatch {
            expected: expected_cols,
            actual: actual_cols,
        });
    }
    for i in 0..expected_cols {
        let e = expected.field(i);
        let a = actual.field(i);
        if e.name() != a.name() || e.data_type() != a.data_type() {
            return Err(FactError::ShapeMismatch {
                expected: expected_cols,
                actual: actual_cols,
            });
        }
    }
    Ok(())
}

/// 事实档载体偏好（决策层；持久化枚举在 settings crate，装配层映射过来，避免本模块
/// 反向依赖 settings）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoragePref {
    /// 自动：按双份估算水位分流（高水位上界 / 低水位滞回线）。
    Auto,
    /// 强制内存档：即使估算超水位也驻 Rust 堆，不走表档。
    Memory,
    /// 强制表档：即使估算在水位内也走常驻表（B 批接通；A 批无表档构造路径）。
    Table,
}

/// 当前已在槽位的事实档载体（分流滞回判断用）。
///
/// 只传当前载体标签，不引用帧本体——避免 shared 反向依赖 result_panel 的 `FactState`；
/// 三态口径与 `result_panel::FactState` 对齐（None / InMemory / OnDuckDb）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTier {
    None,
    InMemory,
    OnDuckDb,
}

/// 分流决策输出的目标载体意向（纯决策，不构造帧；`OnDuckDb` 构造在 B 批接通）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactTier {
    None,
    InMemory,
    OnDuckDb,
}

/// 【D1】带类型档导出（Parquet / XLSX）的事实档分流模式（方案 §7-D）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactExportMode {
    /// 内存档：batches 经 Arrow Appender 直灌新建临时表 → `COPY` → 临时表即用即收。
    Batches,
    /// 表档：复用已有常驻表 `COPY`，不新建表、不回收常驻表（归事实档生命周期管）。
    ExistingTable,
    /// 无事实档 / 已释放且无表名 / 空批：显示矩阵 internal 猜型降级档，整列落 `VARCHAR`，
    /// 回执须显式标注口径（数字列被收窄成文本）。
    Matrix,
}

impl FactExportMode {
    /// 是否为显示矩阵猜型降级档（回执标注用；保真两档返回 `false`）。
    pub fn is_degraded(self) -> bool {
        matches!(self, Self::Matrix)
    }
}

/// 【D1】`Option<&FactFrame>` 口径的导出分流：`None` 与不可用事实档统一归降级档。
pub fn fact_export_mode(fact: Option<&FactFrame>) -> FactExportMode {
    match fact {
        Some(frame) => frame.export_mode(),
        None => FactExportMode::Matrix,
    }
}

/// Auto 分流上界（双份估算字节；方案 §8.3 实测标定）。
///
/// 双份估算超此值 → 表档；对照 text-3000 双份 1.98 MiB，可容纳约 1.1 万行文本列，
/// 数值列余量更大（6 行实测表见方案 §8.1，探针日志 `.rds/tmp/watermark-probe.log`）。
pub const MEMORY_HIGH_WATERMARK: usize = 8 * 1024 * 1024;

/// Auto 分流滞回线：已在内存档的结果集估算低于此值才允许回落 `None`；
/// 3–8 MiB 区间保持当前档不变，防水位边界抖动来回迁移载体。
pub const MEMORY_LOW_WATERMARK: usize = 3 * 1024 * 1024;

/// 双份估算系数：物化前只有 batches，matrix 粗估 = batches × 4（数值最坏 4.13×，取整
/// 保守；文本 1.53× 被覆盖），加上 batches 本体 × 1 → dual ≈ batches × 5；
/// B 阶段按列类型族细化系数。
pub const BATCHES_DUAL_FACTOR: usize = 5;

/// 物化前只有 batches 字节：折算双份（事实档 + 显示矩阵）估算字节，饱和防溢出。
pub fn dual_estimate(batches_bytes: usize) -> usize {
    batches_bytes.saturating_mul(BATCHES_DUAL_FACTOR)
}

/// Auto 分流纯函数：给定双份估算字节、偏好档、当前载体，返回目标载体意向。
///
/// 规则（方案 §4.1 / §8.3）：
/// - 强制 [`StoragePref::Memory`] → [`FactTier::InMemory`]（不看水位）；强制
///   [`StoragePref::Table`] → [`FactTier::OnDuckDb`]（不看水位）；
/// - `Auto` + 当前 `InMemory`：估算 < 低水位 → `None` 回落；低–高区间保持 `InMemory`
///   （滞回防抖动）；超 高水位 → `OnDuckDb`；
/// - `Auto` + 当前 `None`（新结果集 / 已降级）：估算 ≤ 高水位 → `InMemory`；超 高水位 →
///   `OnDuckDb`；
/// - `Auto` + 当前 `OnDuckDb`：保持 `OnDuckDb`（表档不回读 Rust 堆；回落内存档属 B 批细化）。
///
/// A 批落地口径：`OnDuckDb` 意向尚未接通表档构造，调用方按 `None` 降级处理（方案跳 4
/// 风险段：A 批 Auto 超水位时不构造事实档，表档转换 B 批接通）。
pub fn storage_choice(dual_bytes: usize, pref: StoragePref, current: CurrentTier) -> FactTier {
    match pref {
        StoragePref::Memory => FactTier::InMemory,
        StoragePref::Table => FactTier::OnDuckDb,
        StoragePref::Auto => match current {
            CurrentTier::InMemory => {
                if dual_bytes < MEMORY_LOW_WATERMARK {
                    FactTier::None
                } else if dual_bytes > MEMORY_HIGH_WATERMARK {
                    FactTier::OnDuckDb
                } else {
                    FactTier::InMemory
                }
            }
            CurrentTier::None => {
                if dual_bytes > MEMORY_HIGH_WATERMARK {
                    FactTier::OnDuckDb
                } else {
                    FactTier::InMemory
                }
            }
            CurrentTier::OnDuckDb => FactTier::OnDuckDb,
        },
    }
}

#[cfg(test)]
mod fact_tests {
    use super::*;
    use arrow::array::*;
    use arrow::datatypes::{Field, Schema};
    use std::sync::Arc;

    /// 由同构数组构造事实档（测试辅助；列名与数组顺序一一对应）。
    fn fact_from_arrays(names: &[&str], arrays: Vec<ArrayRef>) -> FactFrame {
        let fields: Vec<Field> = names
            .iter()
            .zip(arrays.iter())
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect();
        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(schema, arrays).expect("构造 RecordBatch");
        FactFrame::new(names.iter().map(|n| n.to_string()).collect(), vec![batch])
            .expect("构造事实档")
    }

    #[test]
    fn fact_frame_holds_columns_and_rows() {
        let fact = fact_from_arrays(
            &["id", "name"],
            vec![
                Arc::new(Int64Array::from(vec![Some(1), Some(2)])),
                Arc::new(StringArray::from(vec![Some("a"), Some("b")])),
            ],
        );
        assert_eq!(fact.columns(), &["id".to_string(), "name".to_string()]);
        assert_eq!(fact.column_count(), 2);
        assert_eq!(fact.row_count(), 2);
    }

    #[test]
    fn from_query_result_returns_none_without_batches() {
        let empty = QueryResult::empty();
        assert!(FactFrame::from_query_result(&empty).is_none());

        let simple = QueryResult::simple(vec!["id".to_string()], Some(3), Some(false));
        assert!(FactFrame::from_query_result(&simple).is_none());
    }

    #[test]
    fn column_kind_maps_type_families() {
        let fact = fact_from_arrays(
            &["i", "f", "b", "t", "bin", "date"],
            vec![
                Arc::new(Int64Array::from(vec![Some(1)])),
                Arc::new(Float64Array::from(vec![Some(1.5)])),
                Arc::new(BooleanArray::from(vec![Some(true)])),
                Arc::new(StringArray::from(vec![Some("x")])),
                Arc::new(BinaryArray::from(vec![Some(&[1u8, 2u8][..])])),
                Arc::new(arrow::array::Date32Array::from(vec![Some(0)])),
            ],
        );
        assert_eq!(fact.column_kind(0).unwrap(), FactKind::Int);
        assert_eq!(fact.column_kind(1).unwrap(), FactKind::Float);
        assert_eq!(fact.column_kind(2).unwrap(), FactKind::Bool);
        assert_eq!(fact.column_kind(3).unwrap(), FactKind::Utf8);
        assert_eq!(fact.column_kind(4).unwrap(), FactKind::Binary);
        // Date32 起步档未纳入类型族，落 Other 而不是静默当 Utf8
        assert_eq!(fact.column_kind(5).unwrap(), FactKind::Other);
        assert!(matches!(
            fact.column_kind(6),
            Err(FactError::ColumnIndexOutOfRange { .. })
        ));
    }

    #[test]
    fn value_at_reads_across_batches_and_reports_out_of_range() {
        let first = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![10i64, 20]))],
        )
        .expect("首批");
        let second = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![30i64, 40]))],
        )
        .expect("次批");
        let mut fact = FactFrame::new(vec!["id".to_string()], vec![first]).expect("首批构造");
        fact.append_batches(vec![second]).expect("同形状追加");

        assert_eq!(fact.row_count(), 4);
        assert_eq!(fact.value_at(0, 0).unwrap(), Value::Int(10));
        assert_eq!(fact.value_at(2, 0).unwrap(), Value::Int(30), "跨批定位到第二批首行");
        assert_eq!(fact.value_at(3, 0).unwrap(), Value::Int(40));
        assert!(matches!(
            fact.value_at(4, 0),
            Err(FactError::RowIndexOutOfRange { row: 4, total: 4 })
        ));
    }

    #[test]
    fn rows_as_text_matches_display_rendering() {
        let fact = fact_from_arrays(
            &["n", "v"],
            vec![
                Arc::new(Int64Array::from(vec![Some(42), None])),
                Arc::new(StringArray::from(vec![Some("hi"), None])),
            ],
        );
        let text = fact.rows_as_text();
        assert_eq!(text[0], vec!["42".to_string(), "hi".to_string()]);
        // NULL 走 Display 字面量，与现状 cell_text 口径一致
        assert_eq!(text[1], vec!["NULL".to_string(), "NULL".to_string()]);
    }

    #[test]
    fn append_batches_accepts_same_shape_and_rejects_mismatch() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![1i64]))],
        )
        .expect("基准批");
        let mut fact = FactFrame::new(vec!["id".to_string()], vec![batch]).expect("构造");

        let same = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![2i64]))],
        )
        .expect("同形状批");
        fact.append_batches(vec![same]).expect("同形状追加成功");
        assert_eq!(fact.row_count(), 2);

        // 列数不一致：整批拒绝，现有行数不变
        let wider = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("id", DataType::Int64, false),
                Field::new("name", DataType::Utf8, false),
            ])),
            vec![
                Arc::new(Int64Array::from(vec![3i64])),
                Arc::new(StringArray::from(vec!["x"])),
            ],
        )
        .expect("异形状批");
        let err = fact.append_batches(vec![wider]).unwrap_err();
        assert!(matches!(err, FactError::ShapeMismatch { expected: 1, actual: 2 }));
        assert_eq!(fact.row_count(), 2, "拒绝后现有内容不被污染");

        // 列名 / 类型不一致同样拒绝
        let renamed = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("code", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![4i64]))],
        )
        .expect("改名批");
        assert!(matches!(
            fact.append_batches(vec![renamed]),
            Err(FactError::ShapeMismatch { .. })
        ));

        assert!(matches!(
            fact.append_batches(vec![]),
            Err(FactError::EmptyBatches)
        ));
    }

    #[test]
    fn estimated_bytes_is_nonempty_and_grows_with_batches() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![1i64, 2, 3]))],
        )
        .expect("基准批");
        let mut fact = FactFrame::new(vec!["id".to_string()], vec![batch]).expect("构造");
        let first = fact.estimated_bytes();
        assert!(first > 0, "列式 buffers + null 位图非空");

        let more = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![Arc::new(Int64Array::from(vec![4i64, 5, 6]))],
        )
        .expect("追加批");
        fact.append_batches(vec![more]).expect("追加");
        assert!(fact.estimated_bytes() > first, "追加后列式占用随批数增长");
    }

    /// 【B2】常驻表名回挂：物化后挂表名，重复分析据此复用、不重建。
    #[test]
    fn temp_table_name_is_recorded_on_materialize() {
        let mut fact = fact_from_arrays(&["id"], vec![Arc::new(Int64Array::from(vec![1i64]))]);
        assert_eq!(fact.temp_table(), None, "未物化 = 无表名");
        fact.set_temp_table("tmp_q_result_42".to_string());
        assert_eq!(fact.temp_table(), Some("tmp_q_result_42"));
    }

    /// 【B2】表档释放 batches：行数 / schema / 表名保留，Rust 堆占用归零；
    /// 释放后取值路径按"表档不回读"报错，显示矩阵派生为空。
    #[test]
    fn release_batches_keeps_row_count_and_drops_rust_heap() {
        let mut fact = fact_from_arrays(
            &["id", "name"],
            vec![
                Arc::new(Int64Array::from(vec![Some(1), Some(2)])),
                Arc::new(StringArray::from(vec![Some("a"), Some("b")])),
            ],
        );
        fact.set_temp_table("tmp_q_result_7".to_string());
        let before = fact.estimated_bytes();
        assert!(before > 0);
        assert_eq!(fact.row_count(), 2);

        fact.release_batches();
        assert!(fact.is_released());
        assert_eq!(fact.row_count(), 2, "行数缓存保留，不随 batches 清空回 0");
        assert_eq!(fact.estimated_bytes(), 0, "表档 Rust 堆占用报 0");
        assert_eq!(fact.temp_table(), Some("tmp_q_result_7"));
        assert_eq!(fact.column_count(), 2, "列名 / schema 保留供元数据与类型族访问");
        assert!(fact.rows_as_text().is_empty(), "表档不回读显示矩阵");
        assert!(matches!(
            fact.value_at(0, 0),
            Err(FactError::RowIndexOutOfRange { .. })
        ));
    }

    /// 水位口径探针（A0 实测迁移入正式测试）：量化三类典型结果集的列式字节 vs 显示矩阵字节，
    /// 为 8 / 3 MiB 水位标定留回归依据；数值只进测试日志，不参与运行时分流。
    #[test]
    fn watermark_probe_prints_footprint_table() {
        /// 列式字节：逐列 buffers 长度 + null 位图长度；本仓结果集无嵌套列，不递归 child_data。
        fn batch_bytes(batch: &RecordBatch) -> usize {
            let mut total = 0usize;
            for col in batch.columns() {
                let data = col.to_data();
                total += data.buffers().iter().map(|b| b.len()).sum::<usize>();
                if let Some(nulls) = data.nulls() {
                    total += nulls.buffer().len();
                }
            }
            total
        }

        /// 显示矩阵字节：字符串内容 + Rust 结构开销（String 24B / 行 Vec 24B / 外层 Vec 24B）；
        /// 列头字符串量级小，忽略不计。口径对齐现状 `QueryData.rows: Vec<Vec<String>>`。
        fn matrix_bytes(rows: &[Vec<String>]) -> usize {
            let mut content = 0usize;
            let mut cells = 0usize;
            for row in rows {
                for cell in row {
                    content += cell.len();
                    cells += 1;
                }
            }
            let string_meta = cells * std::mem::size_of::<String>();
            let row_meta = rows.len() * std::mem::size_of::<Vec<String>>();
            let outer_meta = std::mem::size_of::<Vec<Vec<String>>>();
            content + string_meta + row_meta + outer_meta
        }

        fn numeric(rows: usize) -> FactFrame {
            let ids: Vec<Option<i64>> = (0..rows as i64).map(Some).collect();
            let vals: Vec<Option<i64>> = (0..rows as i64).map(|i| Some(i * 13)).collect();
            let floats: Vec<Option<f64>> = (0..rows).map(|i| Some(i as f64 / 100.0)).collect();
            let arrays: Vec<ArrayRef> = vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(Int64Array::from(vals.clone())),
                Arc::new(Int64Array::from(vals)),
                Arc::new(Int64Array::from((0..rows as i64).map(Some).collect::<Vec<_>>())),
                Arc::new(Float64Array::from(floats)),
            ];
            fact_from_arrays(&["a", "b", "c", "d", "e"], arrays)
        }

        fn text(rows: usize) -> FactFrame {
            let mut arrays: Vec<ArrayRef> = Vec::with_capacity(6);
            for col in 0..6 {
                let cells: Vec<Option<String>> =
                    (0..rows).map(|row| Some(format!("名称{row}-字段{col}示例文本内容备注"))).collect();
                arrays.push(Arc::new(StringArray::from(cells)));
            }
            fact_from_arrays(&["f1", "f2", "f3", "f4", "f5", "f6"], arrays)
        }

        fn mixed(rows: usize) -> FactFrame {
            let arrays: Vec<ArrayRef> = vec![
                Arc::new(Int64Array::from((0..rows as i64).map(Some).collect::<Vec<_>>())),
                Arc::new(Int64Array::from((0..rows as i64).map(|i| Some(i * 7)).collect::<Vec<_>>())),
                Arc::new(Float64Array::from((0..rows).map(|i| Some(i as f64 * 0.5)).collect::<Vec<_>>())),
                Arc::new(Float64Array::from((0..rows).map(|i| Some(i as f64)).collect::<Vec<_>>())),
                Arc::new(StringArray::from(
                    (0..rows).map(|row| Some(format!("项{row}"))).collect::<Vec<_>>(),
                )),
                Arc::new(StringArray::from(
                    (0..rows).map(|row| Some(format!("备注{row}"))).collect::<Vec<_>>(),
                )),
                Arc::new(BooleanArray::from((0..rows).map(|i| Some(i % 2 == 0)).collect::<Vec<_>>())),
            ];
            fact_from_arrays(&["id", "code", "rate", "price", "title", "note", "flag"], arrays)
        }

        for rows in [1000usize, 3000] {
            for (label, fact) in [
                (format!("numeric-{rows}"), numeric(rows)),
                (format!("text-{rows}"), text(rows)),
                (format!("mixed-{rows}"), mixed(rows)),
            ] {
                let batches: usize = fact.batches().iter().map(batch_bytes).sum();
                let matrix = matrix_bytes(&fact.rows_as_text());
                let dual = batches + matrix;
                let ratio = if batches > 0 {
                    matrix as f64 / batches as f64
                } else {
                    f64::NAN
                };
                let cols = fact.column_count().max(1);
                let rows_n = fact.row_count().max(1);
                eprintln!(
                    "[probe] {label} rows={} cols={} batches={}B matrix={}B dual={}B matrix/batches={:.2}x avg_cell_matrix={:.1}B",
                    fact.row_count(),
                    cols,
                    batches,
                    matrix,
                    dual,
                    ratio,
                    matrix as f64 / (rows_n * cols) as f64,
                );
                assert!(
                    fact.row_count() > 0 && batches > 0,
                    "探针前提：行与列式数据都非空"
                );
            }
        }
    }

    /// 【A4】双份估算 = batches × 5，饱和防溢出。
    #[test]
    fn dual_estimate_multiplies_batches_by_five_and_saturates() {
        assert_eq!(dual_estimate(1_000), 5_000);
        assert_eq!(dual_estimate(usize::MAX), usize::MAX, "饱和不溢出");
    }

    /// 【A4】强制档不看水位：Memory 落内存档，Table 落表档意向。
    #[test]
    fn forced_pref_ignores_watermark() {
        let huge = MEMORY_HIGH_WATERMARK + 1;
        assert_eq!(
            storage_choice(huge, StoragePref::Memory, CurrentTier::None),
            FactTier::InMemory
        );
        assert_eq!(
            storage_choice(0, StoragePref::Table, CurrentTier::None),
            FactTier::OnDuckDb
        );
    }

    /// 【A4】Auto + 新结果集（当前 None）：高水位内落内存档，超上界落表档意向。
    #[test]
    fn auto_new_set_chooses_by_high_watermark() {
        assert_eq!(
            storage_choice(MEMORY_HIGH_WATERMARK, StoragePref::Auto, CurrentTier::None),
            FactTier::InMemory,
            "等于高水位保持内存档（滞回上界含端点）"
        );
        assert_eq!(
            storage_choice(
                MEMORY_HIGH_WATERMARK + 1,
                StoragePref::Auto,
                CurrentTier::None
            ),
            FactTier::OnDuckDb
        );
    }

    /// 【A4】Auto + 已在内存档：低水位下回落 None；滞回区间保持内存档；超上界转表档。
    #[test]
    fn auto_in_memory_follows_hysteresis_band() {
        assert_eq!(
            storage_choice(
                MEMORY_LOW_WATERMARK - 1,
                StoragePref::Auto,
                CurrentTier::InMemory
            ),
            FactTier::None,
            "低于低水位回落"
        );
        assert_eq!(
            storage_choice(
                MEMORY_LOW_WATERMARK,
                StoragePref::Auto,
                CurrentTier::InMemory
            ),
            FactTier::InMemory,
            "等于低水位保持内存档（滞回下界含端点）"
        );
        assert_eq!(
            storage_choice(
                (MEMORY_LOW_WATERMARK + MEMORY_HIGH_WATERMARK) / 2,
                StoragePref::Auto,
                CurrentTier::InMemory
            ),
            FactTier::InMemory,
            "低–高区间保持当前档"
        );
        assert_eq!(
            storage_choice(
                MEMORY_HIGH_WATERMARK + 1,
                StoragePref::Auto,
                CurrentTier::InMemory
            ),
            FactTier::OnDuckDb
        );
    }

    /// 【A4】Auto + 已在表档：保持表档，不回落 Rust 堆（B 批细化回落条件）。
    #[test]
    fn auto_on_duckdb_stays_on_duckdb() {
        assert_eq!(
            storage_choice(0, StoragePref::Auto, CurrentTier::OnDuckDb),
            FactTier::OnDuckDb
        );
    }

    /// 【B5】Int 列列式统计：原生数值聚合 min / max / sum / avg，NULL 单列计数。
    #[test]
    fn column_stats_aggregates_native_int_values() {
        let fact = fact_from_arrays(
            &["score"],
            vec![Arc::new(Int64Array::from(vec![Some(3), None, Some(7)]))],
        );
        let stats = fact.column_stats(0).expect("Int 列给列式统计");
        assert_eq!(stats.count, 2);
        assert_eq!(stats.nulls, 1);
        assert_eq!(stats.min, Some(3.0));
        assert_eq!(stats.max, Some(7.0));
        assert_eq!(stats.sum, Some(10.0));
        assert_eq!(stats.avg, Some(5.0));
    }

    /// 【B5】文本列返回 `None`：调用方回退显示矩阵文本口径，不在事实档猜数值。
    #[test]
    fn column_stats_returns_none_for_text_column() {
        let fact = fact_from_arrays(
            &["name"],
            vec![Arc::new(StringArray::from(vec![Some("a"), Some("b")]))],
        );
        assert!(fact.column_stats(0).is_none());
        assert!(fact.column_number(0, 0).is_none());
    }

    /// 【B5】Bool 列只计非空 / NULL，不填数值聚合字段。
    #[test]
    fn column_stats_bool_counts_nulls_without_numeric() {
        let fact = fact_from_arrays(
            &["flag"],
            vec![Arc::new(BooleanArray::from(vec![Some(true), None, Some(false)]))],
        );
        let stats = fact.column_stats(0).expect("Bool 列给计数统计");
        assert_eq!(stats.count, 2);
        assert_eq!(stats.nulls, 1);
        assert_eq!(stats.min, None);
        assert_eq!(stats.max, None);
        assert_eq!(stats.sum, None);
        assert_eq!(stats.avg, None);
    }

    /// 【B5】`column_number` 读原生数值；文本族与越界行 / 列返回 `None`。
    #[test]
    fn column_number_reads_native_numeric() {
        let fact = fact_from_arrays(
            &["id", "name"],
            vec![
                Arc::new(Int64Array::from(vec![Some(3), Some(7)])),
                Arc::new(StringArray::from(vec![Some("a"), Some("b")])),
            ],
        );
        assert_eq!(fact.column_number(0, 0), Some(3.0));
        assert_eq!(fact.column_number(1, 0), Some(7.0));
        assert!(fact.column_number(0, 1).is_none(), "文本列无数值");
        assert!(fact.column_number(9, 0).is_none(), "越界行无数值");
        assert!(fact.column_number(0, 9).is_none(), "越界列无数值");
    }

    /// 【B5】表档释放 batches 后列式统计 / 数值取数一律 `None`：本地交互回退矩阵口径。
    #[test]
    fn column_stats_none_after_releasing_batches() {
        let mut fact = fact_from_arrays(
            &["id"],
            vec![Arc::new(Int64Array::from(vec![Some(3), Some(7)]))],
        );
        fact.release_batches();
        assert!(fact.column_stats(0).is_none());
        assert!(fact.column_number(0, 0).is_none());
    }

    /// 【D1】导出分流三态：内存档 = Batches；释放挂表名 = ExistingTable；无事实档 = Matrix。
    #[test]
    fn export_mode_distinguishes_batches_existing_and_matrix() {
        let memory = fact_from_arrays(&["id"], vec![Arc::new(Int64Array::from(vec![1, 2]))]);
        assert_eq!(memory.export_mode(), FactExportMode::Batches);
        assert!(!memory.export_mode().is_degraded());

        let mut table = fact_from_arrays(&["id"], vec![Arc::new(Int64Array::from(vec![1, 2]))]);
        table.set_temp_table("tmp_q_9".to_string());
        table.release_batches();
        assert_eq!(table.export_mode(), FactExportMode::ExistingTable);
        assert!(!table.export_mode().is_degraded());

        // 已释放但没挂表名（异常兜底）：回退降级档
        let mut orphan = fact_from_arrays(&["id"], vec![Arc::new(Int64Array::from(vec![1, 2]))]);
        orphan.release_batches();
        assert_eq!(orphan.export_mode(), FactExportMode::Matrix);
        assert!(orphan.export_mode().is_degraded());

        assert_eq!(fact_export_mode(None), FactExportMode::Matrix);
        assert_eq!(fact_export_mode(Some(&memory)), FactExportMode::Batches);
    }
}

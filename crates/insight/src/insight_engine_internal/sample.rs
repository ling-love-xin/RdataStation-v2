//! 取样与迷你统计：取样档迷你图、表样本、EDA 包

use std::collections::HashMap;

use shared::error::{CommonError, CoreError};

use super::*;
use crate::model::types::{DistributionBin, TableProfile, TextFrequency};
use crate::model::{ColumnKind, ColumnMiniStats, ColumnStatBrief};

/// 取样档迷你统计（C7：A 方案——迷你统计并入取样链路）。
///
/// 对**评估全表**（确认窗口生成的临时表）做查询级过滤现算（Q2：行 LIMIT +
/// 列投影，不建子表、不重取源），逐列产出轻量分布供列卡折叠态迷你图：
/// 数值 = 8 柱等宽直方图、文本/时间 = Top 3 值频次、布尔 = 真值占比。
///
/// 列按 [`ColumnKind`] 投影并 `CAST` 成统一类型（DOUBLE / BOOLEAN / VARCHAR），
/// 避免 DECIMAL、日期时间等类型在 Rust 侧转换的坑；空值由 `Option` 表达。
/// 列无有效样本 / 类型未识别返回 `None`（视图不渲染迷你图，不编假值）。
pub fn compute_sample_mini_stats_on(
    conn: &duckdb::Connection,
    temp_table: &str,
    columns: &[(String, ColumnKind)],
) -> Result<Vec<(String, Option<ColumnMiniStats>)>, CoreError> {
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    let mut select_parts = Vec::with_capacity(columns.len());
    for (name, kind) in columns {
        // 列名经 quote_ident 转义（与 column summary 同口径）
        let quoted = quote_ident(name);
        let cast = match kind {
            ColumnKind::Numeric => "AS DOUBLE",
            ColumnKind::Boolean => "AS BOOLEAN",
            ColumnKind::Text | ColumnKind::DateTime | ColumnKind::Unknown => "AS VARCHAR",
        };
        select_parts.push(format!("CAST({quoted} {cast})"));
    }
    let sql = format!(
        "SELECT {} FROM {} LIMIT {}",
        select_parts.join(", "),
        quote_ident(temp_table),
        SAMPLE_MINI_LIMIT
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample mini stats prepare failed: {}",
            e
        )))
    })?;
    let mut rows = stmt.query([]).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample mini stats query failed: {}",
            e
        )))
    })?;

    let mut numerics: Vec<Vec<f64>> = columns.iter().map(|_| Vec::new()).collect();
    let mut texts: Vec<Vec<String>> = columns.iter().map(|_| Vec::new()).collect();
    let mut bools: Vec<(u32, u32)> = columns.iter().map(|_| (0, 0)).collect();
    while let Some(row) = rows.next().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample mini stats row failed: {}",
            e
        )))
    })? {
        for (ix, (_, kind)) in columns.iter().enumerate() {
            match kind {
                ColumnKind::Numeric => {
                    if let Some(v) = row.get::<_, Option<f64>>(ix)? {
                        numerics[ix].push(v);
                    }
                }
                ColumnKind::Boolean => {
                    if let Some(v) = row.get::<_, Option<bool>>(ix)? {
                        if v {
                            bools[ix].0 += 1;
                        } else {
                            bools[ix].1 += 1;
                        }
                    }
                }
                ColumnKind::Text | ColumnKind::DateTime | ColumnKind::Unknown => {
                    if let Some(v) = row.get::<_, Option<String>>(ix)? {
                        texts[ix].push(v);
                    }
                }
            }
        }
    }

    let mut out = Vec::with_capacity(columns.len());
    for (ix, (name, kind)) in columns.iter().enumerate() {
        let mini = match kind {
            ColumnKind::Numeric => {
                let vals = &numerics[ix];
                if vals.is_empty() {
                    None
                } else {
                    Some(ColumnMiniStats::Numeric(sample_histogram_bins(vals)))
                }
            }
            ColumnKind::Boolean => {
                let (t, f) = bools[ix];
                let total = t + f;
                if total == 0 {
                    None
                } else {
                    Some(ColumnMiniStats::Boolean {
                        true_ratio: t as f64 / total as f64,
                    })
                }
            }
            ColumnKind::Text | ColumnKind::DateTime => {
                let vals = &texts[ix];
                if vals.is_empty() {
                    None
                } else {
                    Some(ColumnMiniStats::Frequencies(sample_top_frequencies(
                        vals, 3,
                    )))
                }
            }
            // 类型未识别：不渲染分布（与评估侧口径一致——Unknown 无类型专属统计）
            ColumnKind::Unknown => None,
        };
        out.push((name.clone(), mini));
    }
    Ok(out)
}

/// 等宽 16 柱直方图（取样档迷你图）：min/max 均分 16 档；全同值 / 不可比回退单柱。
///
/// 迷你图渲染只读 `count`（`label` 不进 UI），口径与评估直方图同族（等宽分箱）。
fn sample_histogram_bins(values: &[f64]) -> Vec<DistributionBin> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &v in values {
        min = min.min(v);
        max = max.max(v);
    }
    let n = values.len() as f64;
    if !min.is_finite() || !max.is_finite() || (max - min).abs() < f64::EPSILON {
        return vec![DistributionBin {
            label: format!("{min:.2}"),
            count: values.len() as u32,
            ratio: 1.0,
        }];
    }
    // 16 柱（2026-11-06 YData 密度对齐：240px 折叠行宽下 16 柱 ≈ 15px/柱，可读）
    const BINS: usize = 16;
    let width = (max - min) / BINS as f64;
    let mut counts = [0u32; BINS];
    for &v in values {
        let mut ix = ((v - min) / width).floor() as usize;
        if ix >= BINS {
            ix = BINS - 1;
        }
        counts[ix] += 1;
    }
    (0..BINS)
        .map(|ix| {
            let lo = min + width * ix as f64;
            let hi = if ix == BINS - 1 { max } else { lo + width };
            DistributionBin {
                label: format!("{lo:.2}~{hi:.2}"),
                count: counts[ix],
                ratio: counts[ix] as f64 / n,
            }
        })
        .collect()
}

/// Top N 值频次（文本 / 时间列迷你图）：按频次降序，同频按字典序（稳定输出）。
fn sample_top_frequencies(values: &[String], n: usize) -> Vec<TextFrequency> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for v in values {
        *counts.entry(v.as_str()).or_insert(0) += 1;
    }
    let total = values.len().max(1) as f64;
    let mut top: Vec<_> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    top.into_iter()
        .take(n)
        .map(|(v, c)| TextFrequency {
            value: v.to_string(),
            count: c,
            ratio: c as f64 / total,
        })
        .collect()
}

/// 带并发控制与全局连接的取样档迷你统计入口（调用方拿不到连接时用）。
pub fn compute_sample_mini_stats(
    temp_table: &str,
    columns: &[(String, ColumnKind)],
) -> Result<Vec<(String, Option<ColumnMiniStats>)>, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    compute_sample_mini_stats_on(&conn, temp_table, columns)
}

/// 临时表样本（EDA 样本区）：`SELECT * LIMIT 5` 的轻查询，文本化前 5 行。
///
/// 与画像分开（画像不依赖样本，样本失败不阻塞画像）：只给探索用的一瞥。
/// 值经 `ValueRef` 手动文本化（`duckdb::types::Value` 没有 `Display`）；
/// 空值显示为 `NULL` 字样，未覆盖类型兜底用 Debug 表示（不编假值）。
pub fn get_temp_table_sample(temp_table: &str) -> Result<crate::model::TableSample, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    get_temp_table_sample_on(&conn, temp_table)
}

/// 表探查的**一次成型产物**：一次并发令牌 + 一次连接锁内完成四个轻查询。
///
/// 为什么要有它：表探查（DESCRIBE / 列摘要 / 重复行 / 样本）是**一个逻辑操作**，
/// 旧序列各自抢一次令牌与锁——快速失败窗口 ×4（并发配额下「看一眼表」反而比
/// 「评估一列」更容易撞 D12 上限），四段之间还可能被别的操作插队。失败口径与
/// 旧序列一致：DESCRIBE 硬失败；摘要 / 重复行 / 样本各自「失败为 `None`，不阻塞画像」。
pub fn get_table_eda_bundle(temp_table: &str) -> Result<TableEdaBundle, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    let profile = super::profile::get_temp_table_profile_on(&conn, temp_table)?;
    let column_names: Vec<String> = profile
        .columns
        .iter()
        .map(|c| c.column_name.clone())
        .collect();
    let column_summary = super::profile::compute_table_column_summary_on(&conn, temp_table, &column_names).ok();
    let dup_rows = super::profile::compute_dup_rows_on(&conn, temp_table).ok();
    let sample = get_temp_table_sample_on(&conn, temp_table).ok();
    Ok(TableEdaBundle {
        profile,
        column_summary,
        dup_rows,
        sample,
    })
}

/// [`get_table_eda_bundle`] 的产物：列清单硬保证，三段附表统计可缺席（不编假值）。
pub struct TableEdaBundle {
    pub profile: TableProfile,
    /// 全列空值率 / 唯一率摘要（失败 / 未算为 `None`）
    pub column_summary: Option<Vec<ColumnStatBrief>>,
    /// 整行重复冗余行数（失败为 `None`）
    pub dup_rows: Option<u64>,
    /// 前 5 行样本（失败为 `None`）
    pub sample: Option<crate::model::TableSample>,
}

/// 在**调用方已持有的连接**上取样本（同 `*_on` 系列的口径）。
pub fn get_temp_table_sample_on(
    conn: &duckdb::Connection,
    temp_table: &str,
) -> Result<crate::model::TableSample, CoreError> {
    use crate::model::TableSample;
    let sql = format!("SELECT * FROM {} LIMIT 5", quote_ident(temp_table));
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample prepare failed: {}",
            e
        )))
    })?;
    let mut rows = Vec::new();
    let mut q = stmt.query([]).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample query failed: {}",
            e
        )))
    })?;
    // 列名在查询执行后才可读（duckdb crate 的 `column_names()` 要求已 execute，
    // 否则断言 "The statement was not executed yet"）；`q` 内部持有执行后的
    // `&Statement`，走 `as_ref()` 读 schema，避免与 `stmt` 的可变借用冲突
    let columns: Vec<String> = q
        .as_ref()
        .map(|s| s.column_names().iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    while let Some(row) = q.next().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB sample row failed: {}",
            e
        )))
    })? {
        let mut cells = Vec::with_capacity(columns.len());
        for ix in 0..columns.len() {
            let value = row.get_ref(ix).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "DuckDB sample cell failed: {}",
                    e
                )))
            })?;
            cells.push(sample_cell_text(&value));
        }
        rows.push(cells);
    }
    Ok(TableSample { columns, rows })
}

/// 样本单元格文本化（`NULL` 显示为字样，未覆盖类型兜底 Debug——不编假值）。
///
/// DuckDB 的 `ValueRef` 按逻辑类型细分整型变体（Tiny/Small/Int/Big/Huge 与各无符号），
/// 归并成字符串即可；时间 / Interval / Decimal 等走 Debug 兜底（仍有可读输出）。
fn sample_cell_text(value: &duckdb::types::ValueRef) -> String {
    use duckdb::types::ValueRef;
    match value {
        ValueRef::Null => "NULL".into(),
        ValueRef::Boolean(b) => b.to_string(),
        ValueRef::TinyInt(i) => i.to_string(),
        ValueRef::SmallInt(i) => i.to_string(),
        ValueRef::Int(i) => i.to_string(),
        ValueRef::BigInt(i) => i.to_string(),
        ValueRef::HugeInt(i) => i.to_string(),
        ValueRef::UTinyInt(i) => i.to_string(),
        ValueRef::USmallInt(i) => i.to_string(),
        ValueRef::UInt(i) => i.to_string(),
        ValueRef::UBigInt(i) => i.to_string(),
        ValueRef::UHugeInt(i) => i.to_string(),
        ValueRef::Float(f) => f.to_string(),
        ValueRef::Double(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => format!("<blob {} 字节>", b.len()),
        other => format!("{other:?}"),
    }
}

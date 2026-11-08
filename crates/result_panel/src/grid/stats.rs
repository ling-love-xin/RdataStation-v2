//! 列统计域（`grid/` 第六块）：`ColumnStats` 口径（矩阵 / 原生列式两路）+ 分组计数
//! + 统计文本行。2026-10-29 拆分时自 [`crate::grid`] 顶层迁入。

use ::shared::FactColumnStats;
/// 【P2-3】一列的统计结果（数值字段只在列里有可解析的数值样本时才填）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColumnStats {
    /// 非 NULL 的行数
    pub count: usize,
    /// `NULL` 的行数（与渲染同一判定）
    pub nulls: usize,
    /// 数值样本的最小值
    pub min: Option<f64>,
    /// 数值样本的最大值
    pub max: Option<f64>,
    /// 数值样本之和
    pub sum: Option<f64>,
    /// 数值样本均值
    pub avg: Option<f64>,
}

/// 【P2-3】整列统计：遍历该列一次，`NULL`（与渲染同款判定）计空值，
/// 其余值能按 f64 解析的参与数值统计；文本 / 日期 / BLOB 只计非空数。
/// 纯函数（输入只有 rows），方便单测。
pub fn column_stats(rows: &[Vec<String>], column: usize) -> ColumnStats {
    let values: Vec<String> = rows
        .iter()
        .map(|row| row.get(column).cloned().unwrap_or_default())
        .collect();
    column_stats_from_values(&values)
}

/// 【P2-3】列级统计内核：输入已经是「一列取值」，不必再套二维矩阵。
/// delegate 的列级访问器（`column_stats_for`）与矩阵版 `column_stats` 共用这份口径。
pub fn column_stats_from_values(values: &[String]) -> ColumnStats {
    let mut stats = ColumnStats::default();
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut numeric = 0usize;
    for cell in values {
        if cell == "NULL" {
            stats.nulls += 1;
            continue;
        }
        stats.count += 1;
        if let Ok(value) = cell.parse::<f64>() {
            min = min.min(value);
            max = max.max(value);
            sum += value;
            numeric += 1;
        }
    }
    stats.min = (numeric > 0).then_some(min);
    stats.max = (numeric > 0).then_some(max);
    stats.sum = (numeric > 0).then_some(sum);
    stats.avg = (numeric > 0).then_some(sum / numeric as f64);
    stats
}

/// 【B5】事实档列式统计 → 网格展示统计（字段一一对应；shared 不反向依赖本 crate，
/// 故在此做一次窄映射，不把 `FactColumnStats` 泄漏到渲染层）。
pub fn stats_from_fact(stats: FactColumnStats) -> ColumnStats {
    ColumnStats {
        count: stats.count,
        nulls: stats.nulls,
        min: stats.min,
        max: stats.max,
        sum: stats.sum,
        avg: stats.avg,
    }
}

/// 【分组】列级计数内核：输入已经是「一列取值」；保留首现顺序，`NULL` 独立成组。
pub fn group_by_values(values: &[String]) -> Vec<(String, usize)> {
    let mut groups: Vec<(String, usize)> = Vec::new();
    for value in values {
        if let Some((_, count)) = groups.iter_mut().find(|(v, _)| *v == *value) {
            *count += 1;
        } else {
            groups.push((value.clone(), 1));
        }
    }
    groups
}

/// 【B5】数值分组键归一化：有限整数写成整数字面量（`2.0` → `"2"`），避免与 `2` 被拆成两组；
/// 非整数 / 超大整数用标准浮点文本。`NaN` / `Infinity` 不进入本路径（事实帧数值族不含它们）。
pub(crate) fn number_group_key(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.0e18 {
        (value as i64).to_string()
    } else {
        value.to_string()
    }
}

/// 【P2-3】统计的展示行（一行一个键值；数值有则给，没有就不摆那一行）
pub fn stats_lines(stats: &ColumnStats) -> Vec<String> {
    let mut lines = vec![
        format!("值 {} 个", stats.count),
        format!("NULL {} 个", stats.nulls),
    ];
    let mut push_num = |label: &str, value: f64| {
        lines.push(format!("{label} {}", fmt_stat_num(value)));
    };
    if let Some(min) = stats.min {
        push_num("最小", min);
    }
    if let Some(max) = stats.max {
        push_num("最大", max);
    }
    if let Some(avg) = stats.avg {
        push_num("均值", avg);
    }
    if let Some(sum) = stats.sum {
        push_num("合计", sum);
    }
    lines
}

/// 统计数字的人读格式：固定 6 位小数截尾，去尾零（`1.500000` → `1.5`）
pub(crate) fn fmt_stat_num(value: f64) -> String {
    let mut text = format!("{value:.6}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// 【R2 计算页】千分位格式化（对齐 DBeaver Calc 的 `DOUBLE_FORMAT` =
/// `###,###,###,###,###,##0.###`，见 `AggregateColumnsPanel.java:65-66`）：
/// 整数部分每 3 位插一个逗号，小数部分沿用 [`fmt_stat_num`] 的去尾零口径。
/// 纯字符串处理，超大值不溢出（不做浮点截断）。
pub fn fmt_grouped(value: f64) -> String {
    if !value.is_finite() {
        // NaN / ±Inf：DBeaver 走 `Object.toString()`，本仓沿用原样文本
        return fmt_stat_num(value);
    }
    let text = fmt_stat_num(value);
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest.to_string()),
        None => ("", text),
    };
    let (int_part, frac) = match body.split_once('.') {
        Some((int, frac)) => (int.to_string(), Some(frac.to_string())),
        None => (body, None),
    };
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (ix, ch) in int_part.chars().enumerate() {
        if ix > 0 && (int_part.chars().count() - ix) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    match frac {
        Some(frac) => format!("{sign}{grouped}.{frac}"),
        None => format!("{sign}{grouped}"),
    }
}

/// 【R2 计算页】字符串口径的极值：排除 `NULL` 后按**字典序**取最小 / 最大，
/// 供文本列在字符串口径下给出「最小值 / 最大值」（对齐 DBeaver
/// `aggregateAsStrings` 让 MIN/MAX 走字符串比较的语义）。
/// 纯函数（输入只有一列取值），与 [`column_stats_from_values`] 同一输入形态。
pub fn text_minmax(values: &[String]) -> (Option<String>, Option<String>) {
    let mut min: Option<&String> = None;
    let mut max: Option<&String> = None;
    for cell in values {
        if cell == "NULL" {
            continue;
        }
        if min.map_or(true, |current| cell < current) {
            min = Some(cell);
        }
        if max.map_or(true, |current| cell > current) {
            max = Some(cell);
        }
    }
    (min.cloned(), max.cloned())
}

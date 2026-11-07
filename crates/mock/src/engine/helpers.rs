// ==================== 辅助函数 ====================

use super::*;

// ==================== 辅助函数 ====================

/// 标识符规范化（列名 / 表名共用）：非字母数字与下划线 → `_`，两侧去 `_`。
///
/// 生成期临时表的列名由本函数得出，**建表 / 追加写回时也必须复用同一算法**：
/// 临时表列名与目标表列名不一致时，`INSERT` 的列清单会直接报「列不存在」。
/// 列名规范化后为空（如全为符号）由调用方报错；表名则退到 `auto_table_*`（见 `sanitize_table_name`）。
pub fn sanitize_identifier(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// 生成器参数在**采样期会不会直接 panic**（空区间 / 权重和非正 / 除零）。
///
/// 为什么必须在**生成前**拦住：这些参数原样进 `rand` 的采样——
/// - `RandomInt` 反向区间 → `Uniform::new_inclusive` 失败（`expect` panic）；
/// - `RandomFloat` / `RandomDecimal` / `Words` / `Sentence` / `Sentences` / `Password` 的 `min..max`
///   是**半开区间**，`min >= max` 即空区间（`random_range` panic）；
/// - `Date` / `DateTime` / `DateTimeBetween` 走 fake 的 `(0..分钟差)`，区间不足一分钟也是空区间；
/// - `ValueSet` 向空集合取随机索引、`Sequence` 做 `row_index % len`（除零）、
///   `Weighted` 抽 `0.0..total`（总权重非正）。
///
/// 后果不是「这一次任务失败」：panic 发生在**持有内存库连接锁**期间，锁会被毒化，
/// 该进程之后每一次生成都报 `poisoned lock`，用户只能重启应用（实测）。
/// 唯一列的**取值域够不够**：摸不到目标行数就**生成前**报错，不让它跑到半途才失败。
///
/// 背景：唯一性靠「抽到一个没出现过的值为止，最多重试 100 次」实现。值域小于行数时这条路
/// 注定走不通，用户看到的是跑了一会儿（甚至几十万行之后）才弹「无法生成不重复的值」，
/// 而真正的原因在一开始就摆在那里——不符合本模块一贯的「生成前把关」原则
/// （见 [`generator_param_problem`]）。
///
/// 只判定**能算出有限值域**的生成器，其余取值域连续或极大，不在这里误伤
/// （万一真撞上，保留运行时重试那条兜底路径）。
pub(crate) fn unique_capacity_problem(col: &ColumnDef, row_count: u32) -> Option<String> {
    let capacity: usize = match &col.generator {
        GeneratorConfig::ValueSet { values } => values.len(),
        GeneratorConfig::Sequence { values, .. } => values.len(),
        GeneratorConfig::Weighted { choices } => choices.len(),
        GeneratorConfig::Constant { .. } => 1,
        // ratio 为 0 或满时恒取一侧，实际只有一个取值
        GeneratorConfig::Boolean { ratio } => {
            if *ratio == 0 || *ratio >= 100 {
                1
            } else {
                2
            }
        }
        GeneratorConfig::Digit => 10,
        GeneratorConfig::RandomInt { min, max } => {
            (i64::from(*max) - i64::from(*min) + 1).max(0) as usize
        }
        // 其余（含自增、浮点 / 小数、随机的各种文本）取值域足够大或连续
        _ => return None,
    };

    // 空值不参与唯一约束（SQL 里多个 NULL 合法），按期望的非空行数算需求。
    // nullable_ratio 接近 1 时需求趋近于 0——全空列的唯一性天然成立，不能误报。
    let needed = if col.nullable_ratio >= 1.0 {
        0usize
    } else {
        (f64::from(row_count) * (1.0 - col.nullable_ratio)).ceil() as usize
    };

    if capacity >= needed {
        return None;
    }

    Some(format!(
        "列 '{}' 勾了唯一，但它的生成器至多产出 {capacity} 个不同取值，不足 {needed} 行——\
         请放宽生成器的取值域，或降低行数 / 取消唯一",
        col.name
    ))
}

/// 「参数能不能采样」的**唯一闸门**，错误信息要能直接指导用户改哪个字段。
pub(crate) fn generator_param_problem(generator: &GeneratorConfig) -> Option<String> {
    match generator {
        // ===== 集合类：空集合 / 全零权重（生成期除零或空区间）=====
        GeneratorConfig::ValueSet { values } if values.is_empty() => {
            Some("的「取值集合」为空：请在列编辑对话框里填取值（一行一个）".to_string())
        }
        GeneratorConfig::Sequence { values, .. } if values.is_empty() => {
            Some("的「序列取值」集合为空：请在列编辑对话框里填取值（一行一个）".to_string())
        }
        GeneratorConfig::Weighted { choices } => {
            if choices.is_empty() {
                return Some(
                    "的「加权选项」为空：请在列编辑对话框里填「值, 权重」（至少一个权重大于 0）"
                        .to_string(),
                );
            }
            if choices
                .iter()
                .any(|(_, weight)| !weight.is_finite() || *weight < 0.0)
            {
                return Some(
                    "的「加权选项」权重必须是不小于 0 的有限数（负数 / NaN 会让总权重变成非正，抽不到值）"
                        .to_string(),
                );
            }
            if !choices.iter().any(|(_, weight)| *weight > 0.0) {
                return Some("的「加权选项」权重全为 0：请至少让一个权重大于 0".to_string());
            }
            None
        }

        // ===== 随机区间类：空区间 =====
        GeneratorConfig::RandomInt { min, max } if min > max => Some(format!(
            "的随机整数区间是空的（min {min} > max {max}）：把 min 改小或 max 改大"
        )),
        GeneratorConfig::RandomFloat { min, max, .. }
            if !min.is_finite() || !max.is_finite() || min >= max =>
        {
            Some(format!(
                "的随机小数区间不是有效区间（min {min} / max {max}）：需要两者都是有限数且 min < max"
            ))
        }
        GeneratorConfig::RandomDecimal { min, max, .. }
            if !min.is_finite() || !max.is_finite() || min >= max =>
        {
            Some(format!(
                "的随机小数区间不是有效区间（min {min} / max {max}）：需要两者都是有限数且 min < max"
            ))
        }
        GeneratorConfig::Words { min, max }
        | GeneratorConfig::Sentence { min, max }
        | GeneratorConfig::Sentences { min, max }
        | GeneratorConfig::Password { min, max }
            if min >= max =>
        {
            Some(format!(
                "的取值区间是空的（min {min} ≥ max {max}）：需要 min < max（区间是半开区间）"
            ))
        }

        // ===== 分布族：参数越界会让抽样退化成常量或 NaN =====
        GeneratorConfig::Poisson { lambda } | GeneratorConfig::Exponential { lambda }
            if !lambda.is_finite() || *lambda <= 0.0 =>
        {
            Some(format!(
                "的「强度 λ」需为大于 0 的有限数（当前 {lambda}）：它决定事件到达速率，越小事件越稀"
            ))
        }
        GeneratorConfig::Pareto { scale_value, .. }
            if !scale_value.is_finite() || *scale_value <= 0.0 =>
        {
            Some(format!(
                "的「尺度（最小值）」需为大于 0 的有限数（当前 {scale_value}）：帕累托分布的取值不会低于它"
            ))
        }
        GeneratorConfig::Pareto { alpha, .. } if !alpha.is_finite() || *alpha <= 0.0 => {
            Some(format!(
                "的「形状参数 α」需为大于 0 的有限数（当前 {alpha}）：α 越小尾巴越重，为 0 时算不出取值"
            ))
        }
        GeneratorConfig::Beta { alpha, beta }
            if !alpha.is_finite() || !beta.is_finite() || *alpha <= 0.0 || *beta <= 0.0 =>
        {
            Some(format!(
                "的「形状参数 α / β」需为大于 0 的有限数（当前 α {alpha} / β {beta}）：Beta 分布取值落在 0~1 之间"
            ))
        }
        GeneratorConfig::Binomial { trials, .. } if *trials == 0 => {
            Some("的「试验次数 n」需大于 0：n 为 0 时整列都是同一个值".to_string())
        }
        GeneratorConfig::Binomial { probability, .. }
            if !probability.is_finite() || *probability < 0.0 || *probability > 1.0 =>
        {
            Some(format!(
                "的「概率 p」需落在 0~1 之间（当前 {probability}）：p 是单次试验成功的概率"
            ))
        }
        GeneratorConfig::TimeSeries {
            start,
            trend,
            amplitude,
            noise,
            ..
        } if !start.is_finite()
            || !trend.is_finite()
            || !amplitude.is_finite()
            || !noise.is_finite() =>
        {
            Some(format!(
                "的「起始值 / 趋势 / 周期振幅 / 噪声强度」需均为有限数（当前 {start} / {trend} / {amplitude} / {noise}）：NaN 或无穷会让整列取值失去意义"
            ))
        }

        // ===== 日期时间类：区间（fake 按「分钟差」取偏移，差 ≤ 0 就是空区间）→ 工作时段 → 工作日历 =====
        // 三个开关各自独立：未勾选的那一项不校验（留着旧值不该拦住生成），一次只报一个问题。
        GeneratorConfig::DateTime {
            min,
            max,
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        } => datetime_range_problem(min, max)
            .or_else(|| work_hours_problem(*work_hours_only, work_hour_start, work_hour_end))
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),
        GeneratorConfig::DateTimeBefore {
            before,
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        } => datetime_range_problem(crate::generators::DEFAULT_DATETIME_MIN, before)
            .or_else(|| work_hours_problem(*work_hours_only, work_hour_start, work_hour_end))
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),
        GeneratorConfig::DateTimeAfter {
            after,
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        } => datetime_range_problem(after, crate::generators::DEFAULT_DATETIME_MAX)
            .or_else(|| work_hours_problem(*work_hours_only, work_hour_start, work_hour_end))
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),
        GeneratorConfig::Date {
            min,
            max,
            workdays_only,
            work_week,
            skip_dates,
            work_dates,
        } => date_range_problem(min, max)
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),
        GeneratorConfig::DateTimeBetween {
            start: min,
            end: max,
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        } => datetime_range_problem(min, max)
            .or_else(|| work_hours_problem(*work_hours_only, work_hour_start, work_hour_end))
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),

        // ===== 顺序日期的工作日历：步长须是整天，其余同上一组 =====
        GeneratorConfig::SequentialDate {
            workdays_only,
            step_seconds,
            work_week,
            skip_dates,
            work_dates,
            ..
        } if *workdays_only => work_days_step_problem(*step_seconds)
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),
        GeneratorConfig::SequentialDateWithGaps {
            workdays_only,
            step_seconds,
            work_week,
            skip_dates,
            work_dates,
            ..
        } if *workdays_only => work_days_step_problem(*step_seconds)
            .or_else(|| work_calendar_problem(*workdays_only, work_week, skip_dates, work_dates)),

        _ => None,
    }
}

/// 工作日历里两个日期列表各自最多多少条。
///
/// 不是存储限制而是**生成期预算**：「按工作日推进」要对每个值数一遍区间内的工作日，
/// 列表越长逐行判定越久（一年节假日 ≈ 20 条，366 条已是一年逐日列满的量级）。
const MAX_CALENDAR_DATES: usize = 366;

/// 工作时段窗口的合法性：两个时刻要能解析，且不能是零长窗口。
///
/// 起 > 止 是**合法**的（跨零点 / 夜班，如 `22:00~06:00`），所以只拦「相等」；
/// 未勾选「仅工作时段」时不校验（字段里留着旧值不该拦住生成）。
pub(crate) fn work_hours_problem(work_hours_only: bool, start: &str, end: &str) -> Option<String> {
    if !work_hours_only {
        return None;
    }
    let Some(start_minutes) = parse_hour_minutes(start) else {
        return Some(format!(
            "的「工作时段起（HH:MM）」里的「{start}」不是时刻：需要 24 小时制的 `HH:MM`，如 09:00"
        ));
    };
    let Some(end_minutes) = parse_hour_minutes(end) else {
        return Some(format!(
            "的「工作时段止（HH:MM）」里的「{end}」不是时刻：需要 24 小时制的 `HH:MM`，如 18:00"
        ));
    };
    if start_minutes >= 1440 {
        return Some(format!(
            "的「工作时段起（HH:MM）」需早于 24:00（当前「{start}」）：24:00 只能当结束时刻"
        ));
    }
    if start_minutes == end_minutes {
        return Some(format!(
            "的工作时段是零长窗口（起止都是「{start}」）：起 > 止 会按跨零点（夜班）理解，起 = 止 则无值可放"
        ));
    }
    None
}

/// 工作日历（工作周掩码 + 跳过日期 + 上班日期）的合法性。
///
/// 七个日期生成器共用同一套规则；非法值必须拦在生成前——掩码非法会让日历静默退到周一~周五，
/// 日期串写错会让那一天静默不生效，两者用户都看不出来。
pub(crate) fn work_calendar_problem(
    workdays_only: bool,
    work_week: &str,
    skip_dates: &[String],
    work_dates: &[String],
) -> Option<String> {
    if !workdays_only {
        return None;
    }
    let bytes = work_week.as_bytes();
    if bytes.len() != 7 || !bytes.iter().all(|b| *b == b'0' || *b == b'1') {
        return Some(format!(
            "的「工作周（周一~周日，1 上班）」需是 7 位 0/1（当前「{work_week}」）：如 1111100 = 周一~周五上班"
        ));
    }
    if !bytes.iter().any(|b| *b == b'1') {
        return Some(
            "的「工作周（周一~周日，1 上班）」全为 0：至少要让一天上班，否则整列无值可放"
                .to_string(),
        );
    }
    for (label, list) in [
        ("跳过日期（节假日）", skip_dates),
        ("上班日期（调休）", work_dates),
    ] {
        if list.len() > MAX_CALENDAR_DATES {
            return Some(format!(
                "的「{label}」最多 {MAX_CALENDAR_DATES} 条（当前 {} 条）：一年 20 条上下就够",
                list.len()
            ));
        }
        // 日期必须是**规范 10 位**：`2026-1-1` 也能被 chrono 解析，但与生成侧按字节比较的
        // 日历列表对不上，会静默失效——所以要求「解析回来与输入逐字相同」
        if let Some(bad) = list.iter().find(|d| {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map(|parsed| parsed.format("%Y-%m-%d").to_string() != **d)
                .unwrap_or(true)
        }) {
            return Some(format!(
                "的「{label}」里「{bad}」不是日期：需要 10 位的 `YYYY-MM-DD`（一行一个）"
            ));
        }
    }
    None
}

/// 「按工作日推进」要求步长是**整整天的正数**：半天 / 负数在「工作日」这个刻度上没有意义。
pub(crate) fn work_days_step_problem(step_seconds: i32) -> Option<String> {
    if step_seconds < 86_400 || step_seconds % 86_400 != 0 {
        return Some(format!(
            "的「步长（秒）」勾选「仅工作日」时需是不小于 86400 的整天数（当前 {step_seconds} 秒）：如 86400 = 每个工作日一行"
        ));
    }
    None
}

/// `DateTime` / `DateTimeBetween` 的区间问题：解析得出的两界差值必须**至少一分钟**。
///
/// 解析不出来的串不在这里拦：生成器自己会回退到默认窗口（并记 `tracing::warn!`）。
pub(crate) fn datetime_range_problem(min: &str, max: &str) -> Option<String> {
    let parse = |text: &str| {
        chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|d| d.to_utc())
    };
    let (Some(start), Some(end)) = (parse(min), parse(max)) else {
        return None;
    };
    ((end - start).num_minutes() <= 0)
        .then(|| format!("的时间区间不足一分钟（{min} ~ {max}）：需要 max 比 min 晚一分钟以上"))
}

/// `Date` 的区间问题：两界都能解析时，`max` 不能早于 `min`（同一天可以）。
pub(crate) fn date_range_problem(min: &str, max: &str) -> Option<String> {
    let parse = |text: &str| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok();
    let (Some(start), Some(end)) = (parse(min), parse(max)) else {
        return None;
    };
    (end < start).then(|| format!("的日期区间反向（{min} ~ {max}）：把 max 改到 min 之后"))
}

pub(crate) fn sanitize_table_name(name: &str) -> String {
    let safe = sanitize_identifier(name).to_lowercase();
    if safe.is_empty() {
        format!(
            "auto_table_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    } else {
        safe
    }
}

/// DuckDB 值 → SQL 字面量（导出 `.sql` 用）。
///
/// 覆盖范围必须 ⊇ mock 能产出的列类型（`ColumnDataType::to_duckdb_type` 的取值集合）：
/// 时间戳 / 日期 / 十进制 / 大整数 / 二进制一个都不能漏——漏掉的会被**静默**写成 `NULL`，
/// 导出的脚本就悄悄丢了这些列的数据（v1 遗留：这里原本只列了 9 个变体，DateTime / DECIMAL 全变 NULL）。
pub(crate) fn value_to_sql_literal(val: &duckdb::types::Value) -> String {
    use duckdb::types::Value;
    match val {
        Value::Null => "NULL".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(i) => i.to_string(),
        Value::SmallInt(i) => i.to_string(),
        Value::Int(i) => i.to_string(),
        Value::BigInt(i) => i.to_string(),
        Value::HugeInt(i) => i.to_string(),
        Value::UHugeInt(i) => i.to_string(),
        Value::UTinyInt(i) => i.to_string(),
        Value::USmallInt(i) => i.to_string(),
        Value::UInt(i) => i.to_string(),
        Value::UBigInt(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        // 十进制按**数字字面量**写（`Decimal` 的 Display 带小数点，不加引号）
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => quote_sql_text(s),
        Value::Enum(s) => quote_sql_text(s),
        // 时间类加类型前缀：目标库不会把它当字符串列
        Value::Timestamp(unit, raw) => format!(
            "TIMESTAMP '{}'",
            engine::duckdb::value_text::timestamp_text(*unit, *raw)
        ),
        Value::Date32(days) => {
            format!("DATE '{}'", engine::duckdb::value_text::date_text(*days))
        }
        Value::Time64(unit, raw) => format!(
            "TIME '{}'",
            engine::duckdb::value_text::time_text(*unit, *raw)
        ),
        Value::Interval {
            months,
            days,
            nanos,
        } => format!(
            "INTERVAL '{}'",
            engine::duckdb::value_text::interval_text(*months, *days, *nanos)
        ),
        Value::Blob(bytes) | Value::Geometry(bytes) => {
            format!("'{}'::BLOB", engine::duckdb::value_text::blob_hex(bytes))
        }
        // mock 产不出的容器 / 联合类型：不再静默——真出现了要看得见
        other => {
            tracing::warn!(
                "Mock: 导出 SQL 时遇到未覆盖的 DuckDB 值类型（{:?}），已写 NULL",
                std::mem::discriminant(other)
            );
            "NULL".to_string()
        }
    }
}

pub(crate) fn quote_sql_text(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}


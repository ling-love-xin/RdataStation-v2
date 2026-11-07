// ==================== 日期时间辅助函数 ====================

use super::*;

pub(crate) fn parse_date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .unwrap_or_else(|| {
            tracing::warn!("Mock: invalid date '{}', falling back to 2020-01-01", s);
            chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap_or_default()
        })
}

/// `DateTimeBefore` / `DateTimeAfter` 的另半边默认窗口（与旧的写死值一致）。
pub(crate) const DEFAULT_DATETIME_MIN: &str = "2020-01-01T00:00:00Z";
pub(crate) const DEFAULT_DATETIME_MAX: &str = "2030-12-31T23:59:59Z";

/// 工作时段窗口（当日分钟数）。
///
/// `start > end` 表示**跨零点**（夜班，如 `22:00~06:00`）；长度为零时退到默认窗口。
#[derive(Debug, Clone, Copy)]
pub(crate) struct WorkHours {
    start: u32,
    end: u32,
}

impl WorkHours {
    pub(crate) fn parse(start_text: &str, end_text: &str) -> Self {
        // // 默认值与目录脚本的 `STRING_DEFAULTS`（09:00 / 18:00）一致；这里再兵一次是为了
        // 护栏被绕开时（旧记录 / 手改 JSON）仍然有一个可用窗口，而不是 panic 或零长窗口。
        let start = parse_hour_minutes(start_text).unwrap_or(9 * 60);
        let end = parse_hour_minutes(end_text).unwrap_or(18 * 60);
        if start > 1440 || end > 1440 || start == end {
            tracing::warn!(
                "Mock: 工作时段「{}~{}」不是一个可用窗口，回退到 09:00~18:00",
                start_text,
                end_text
            );
            return Self {
                start: 9 * 60,
                end: 18 * 60,
            };
        }
        Self { start, end }
    }

    /// 在窗口里均匀取一个日内时刻（跨零点时两段按长度加权）。
    pub(crate) fn sample(&self, rng: &mut StdRng) -> chrono::NaiveTime {
        let minutes = if self.end > self.start {
            (self.start..self.end).fake_with_rng::<u32, _>(rng)
        } else {
            let before_midnight = 1440 - self.start;
            let total = before_midnight + self.end;
            if total == 0 {
                0
            } else if (0..total).fake_with_rng::<u32, _>(rng) < before_midnight {
                (self.start..1440).fake_with_rng::<u32, _>(rng)
            } else {
                (0..self.end).fake_with_rng::<u32, _>(rng)
            }
        };
        chrono::NaiveTime::from_num_seconds_from_midnight_opt(minutes * 60, 0).unwrap_or_default()
    }
}

/// `HH:MM`（或单个 `HH`）→ 当日分钟数；`24:00` 是合法上界。
pub(crate) fn parse_hour_minutes(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    let (hour_text, minute_text) = match trimmed.split_once(':') {
        Some((h, m)) => (h.trim(), m.trim()),
        None => (trimmed, "0"),
    };
    let hour: u32 = hour_text.parse().ok()?;
    let minute: u32 = minute_text.parse().ok()?;
    (hour <= 24 && minute < 60).then(|| hour * 60 + minute)
}

/// 日期时间类四个变体共用的日历参数（列级，7 个字段一包）。
pub(crate) struct CalendarArgs<'a> {
    workdays_only: bool,
    work_hours_only: bool,
    work_hour_start: &'a str,
    work_hour_end: &'a str,
    work_week: &'a str,
    skip_dates: &'a [String],
    work_dates: &'a [String],
}

impl<'a> CalendarArgs<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        workdays_only: bool,
        work_hours_only: bool,
        work_hour_start: &'a str,
        work_hour_end: &'a str,
        work_week: &'a str,
        skip_dates: &'a [String],
        work_dates: &'a [String],
    ) -> Self {
        Self {
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        }
    }

    pub(crate) fn calendar(&self) -> Option<WorkCalendar<'a>> {
        self.workdays_only
            .then(|| WorkCalendar::new(self.work_week, self.skip_dates, self.work_dates))
    }

    pub(crate) fn hours(&self) -> Option<WorkHours> {
        self.work_hours_only
            .then(|| WorkHours::parse(self.work_hour_start, self.work_hour_end))
    }
}

/// 区间随机时刻 + 列级日历参数（四个日期时间变体共用）。
pub(crate) fn datetime_with_calendar(
    min: &str,
    max: &str,
    args: CalendarArgs<'_>,
    rng: &mut StdRng,
) -> String {
    let calendar = args.calendar();
    datetime_between_with_calendar(min, max, calendar.as_ref(), args.hours(), rng)
}

/// 解析起始时刻：`YYYY-MM-DD HH:MM:SS`，只给日期时按当天 00:00:00；
/// 解析不了就回退到 epoch（与旧行为一致，`warn` 带上生成器名便于定位列）。
pub(crate) fn parse_start_datetime(label: &str, text: &str) -> chrono::NaiveDateTime {
    let trimmed = text.trim();
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return dt;
    }
    chrono::NaiveDateTime::parse_from_str(&format!("{} 00:00:00", trimmed), "%Y-%m-%d %H:%M:%S")
        .inspect_err(|e| {
            tracing::warn!(
                "{label}: invalid start date '{}', falling back to epoch: {}",
                text,
                e
            )
        })
        .unwrap_or_default()
}

/// 按**工作日数**推进：第 `row_index` 行取从 `start` 起第 `row_index × 每天几个工作日` 个工作日，
/// 日内时刻保持起始时刻（护栏保证步长是整天）。
pub(crate) fn advance_work_days(
    calendar: &WorkCalendar<'_>,
    start: chrono::NaiveDateTime,
    steps: i64,
    step_seconds: i32,
) -> chrono::NaiveDateTime {
    let per_step = (step_seconds as i64 / 86_400).max(1);
    let work_days = steps * per_step;
    match calendar.nth_work_day(start.date(), work_days) {
        Some(date) => chrono::NaiveDateTime::new(date, start.time()),
        // 日历算不下去（理论上不会）：退回自然日推进，至少不中断生成
        None => start + chrono::Duration::days(work_days),
    }
}

/// 区间随机时刻 + 可选的工作日历约束（列级参数，见 `WorkCalendar` / `WorkHours`）。
///
/// 落点规则：
/// 1. 先在 `[min, max]` 里均匀取一个时刻；
/// 2. 「仅工作日」：落在休息日就重抽（最多 30 次），仍不行就顺延到下一个工作日；
/// 3. 「仅工作时段」：把日内时刻改到给定窗口（默认 09:00~18:00，可跨零点）；
/// 4. 最后夹回 `[min, max]`——区间端点优先于日历约束（窗口比日历窄时至少不越界）。
pub(crate) fn datetime_between_with_calendar(
    min: &str,
    max: &str,
    calendar: Option<&WorkCalendar<'_>>,
    hours: Option<WorkHours>,
    rng: &mut StdRng,
) -> String {
    use chrono::{DateTime, Utc};
    use fake::faker::chrono::en::DateTimeBetween;

    let default_start = DateTime::parse_from_rfc3339(DEFAULT_DATETIME_MIN)
        .map(|d| d.to_utc())
        .unwrap_or_default();
    let default_end = DateTime::parse_from_rfc3339(DEFAULT_DATETIME_MAX)
        .map(|d| d.to_utc())
        .unwrap_or_default();
    let s = DateTime::parse_from_rfc3339(min)
        .map(|d| d.to_utc())
        .unwrap_or(default_start);
    let e = DateTime::parse_from_rfc3339(max)
        .map(|d| d.to_utc())
        .unwrap_or(default_end);
    let lower = s.naive_utc();
    let upper = e.naive_utc();

    let mut picked = DateTimeBetween(s, e)
        .fake_with_rng::<DateTime<Utc>, _>(rng)
        .naive_utc();
    if let Some(calendar) = calendar {
        for _ in 0..30 {
            if calendar.is_work_day(picked.date()) {
                break;
            }
            picked = DateTimeBetween(s, e)
                .fake_with_rng::<DateTime<Utc>, _>(rng)
                .naive_utc();
        }
        if !calendar.is_work_day(picked.date()) {
            picked =
                chrono::NaiveDateTime::new(calendar.snap_forward(picked.date()), picked.time());
        }
    }
    if let Some(hours) = hours {
        picked = chrono::NaiveDateTime::new(picked.date(), hours.sample(rng));
    }
    // 夹回区间：顺延与工作时段都可能把时刻推出窗口，端点优先
    // （不用 `clamp`：区间上下界反了它会 panic，而这里是热路径，不能把会话带走）
    picked = if picked < lower {
        lower
    } else if picked > upper {
        upper
    } else {
        picked
    };
    picked.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 从 ISO 串（`YYYY-MM-DD`）取出日期；列表项与区间比较都走字节，不做完整解析。
pub(crate) fn key_to_date(bytes: &[u8]) -> Option<chrono::NaiveDate> {
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let digit = |i: usize| -> Option<u32> {
        bytes[i]
            .is_ascii_digit()
            .then(|| u32::from(bytes[i] - b'0'))
    };
    let year = digit(0)? * 1000 + digit(1)? * 100 + digit(2)? * 10 + digit(3)?;
    let month = digit(5)? * 10 + digit(6)?;
    let day = digit(8)? * 10 + digit(9)?;
    chrono::NaiveDate::from_ymd_opt(year as i32, month, day)
}

/// 日期的 ISO 键（`YYYY-MM-DD` 的字节形式）：与列表项按字节比较，免解析、免分配。
pub(crate) fn iso_key(date: chrono::NaiveDate) -> [u8; 10] {
    use chrono::Datelike;
    let (year, month, day) = (date.year(), date.month(), date.day());
    let mut key = [b'0'; 10];
    for (i, digit) in [year / 1000, (year / 100) % 10, (year / 10) % 10, year % 10]
        .iter()
        .enumerate()
    {
        key[i] = b'0' + (*digit as u8);
    }
    key[4] = b'-';
    key[5] = b'0' + (month / 10) as u8;
    key[6] = b'0' + (month % 10) as u8;
    key[7] = b'-';
    key[8] = b'0' + (day / 10) as u8;
    key[9] = b'0' + (day % 10) as u8;
    key
}

/// **列级工作日历**（参数就在列自己的生成器上，不与别的列共享）。
///
/// 判定顺序：**上班日期（调休）优先 → 跳过日期（节假日）→ 工作周掩码**。
/// 列表项按 ISO 串的字节序比较（`YYYY-MM-DD` 的字典序就是时间序），不建 `NaiveDate`。
pub(crate) struct WorkCalendar<'a> {
    /// 周一~周日，`true` = 上班
    week: [bool; 7],
    skip_dates: &'a [String],
    work_dates: &'a [String],
}

impl<'a> WorkCalendar<'a> {
    /// 从生成器字段构造。掩码非法时退到周一~周五（生成前护栏会拦住非法值，这里是兜底）。
    pub(crate) fn new(work_week: &str, skip_dates: &'a [String], work_dates: &'a [String]) -> Self {
        let bytes = work_week.as_bytes();
        let mut week = [true, true, true, true, true, false, false];
        if bytes.len() == 7 && bytes.iter().all(|b| *b == b'0' || *b == b'1') {
            for (i, b) in bytes.iter().enumerate() {
                week[i] = *b == b'1';
            }
        }
        Self {
            week,
            skip_dates,
            work_dates,
        }
    }

    /// 两个列表都空且掩码是整周形状时，判定退化成纯算术（常见情况的快路径）。
    pub(crate) fn lists_empty(&self) -> bool {
        self.skip_dates.is_empty() && self.work_dates.is_empty()
    }

    pub(crate) fn list_has(list: &[String], key: &[u8]) -> bool {
        list.iter().any(|item| item.as_bytes() == key)
    }

    pub(crate) fn is_work_day(&self, date: chrono::NaiveDate) -> bool {
        use chrono::Datelike;
        let key = iso_key(date);
        if Self::list_has(self.work_dates, &key) {
            return true;
        }
        if Self::list_has(self.skip_dates, &key) {
            return false;
        }
        self.week[date.weekday().num_days_from_monday() as usize]
    }

    /// 顺延到 `date` 及其之后的第一个工作日；扫到一年还没有就放弃（返回原值）。
    pub(crate) fn snap_forward(&self, date: chrono::NaiveDate) -> chrono::NaiveDate {
        let mut probe = date;
        for _ in 0..366 {
            if self.is_work_day(probe) {
                return probe;
            }
            match probe.succ_opt() {
                Some(next) => probe = next,
                None => break,
            }
        }
        tracing::warn!("Mock: 工作日历里没有可用工作日，回退到自然日推进");
        date
    }

    /// `[start, end)` 里的工作日数：掩码段用算术（整周 + 余数），列表段用线性扫。
    pub(crate) fn work_days_between(&self, start: chrono::NaiveDate, end: chrono::NaiveDate) -> i64 {
        use chrono::Datelike;
        let total = (end - start).num_days();
        if total <= 0 {
            return 0;
        }
        let per_week = self.week.iter().filter(|w| **w).count() as i64;
        let start_wd = start.weekday().num_days_from_monday() as usize;
        let mut rest = (total / 7) * (7 - per_week);
        for i in 0..(total % 7) as usize {
            if !self.week[(start_wd + i) % 7] {
                rest += 1;
            }
        }
        if !self.lists_empty() {
            let (start_key, end_key) = (iso_key(start), iso_key(end));
            let in_range =
                |bytes: &[u8]| bytes.len() == 10 && bytes >= &start_key[..] && bytes < &end_key[..];
            // 掩码说上班、但被列进跳过日期 → 变成休息日（白名单优先，不重复算）
            for entry in self.skip_dates {
                let bytes = entry.as_bytes();
                if !in_range(bytes) || Self::list_has(self.work_dates, bytes) {
                    continue;
                }
                if let Some(date) = key_to_date(bytes) {
                    if self.week[date.weekday().num_days_from_monday() as usize] {
                        rest += 1;
                    }
                }
            }
            // 掩码说休息、但被列进上班日期（调休）→ 变成工作日
            for entry in self.work_dates {
                let bytes = entry.as_bytes();
                if !in_range(bytes) {
                    continue;
                }
                if let Some(date) = key_to_date(bytes) {
                    if !self.week[date.weekday().num_days_from_monday() as usize] {
                        rest -= 1;
                    }
                }
            }
        }
        total - rest
    }

    /// 从 `start` 起往后第 `k` 个工作日（`k = 0` 就是 `start` 或它的下一个工作日）。
    ///
    /// 先按“每周若干天”估一个自然日跨度，再用实际工作日数纠正（每天至多差 1，收敛很快）。
    pub(crate) fn nth_work_day(&self, start: chrono::NaiveDate, k: i64) -> Option<chrono::NaiveDate> {
        let mut span = k + 2 * (k / 5 + 1);
        for _ in 0..64 {
            let candidate = start.checked_add_signed(chrono::Duration::days(span))?;
            let counted = self.work_days_between(start, candidate);
            if counted == k {
                return Some(self.snap_forward(candidate));
            }
            span += k - counted;
            if span < 0 {
                span = 0;
            }
        }
        tracing::warn!("Mock: 工作日推进未收敛，回退到自然日推进");
        None
    }
}

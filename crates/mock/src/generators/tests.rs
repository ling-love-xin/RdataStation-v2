// ==================== 测试 ====================

use super::*;
use fake::rand::SeedableRng;
use fake::rand::rngs::StdRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    #[test]
    fn test_auto_increment() {
        let generator = GeneratorConfig::AutoIncrement { start: 1, step: 1 };
        assert_eq!(generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn), "1");
        assert_eq!(generate_cell(&generator, &mut rng(), 5, &Locale::ZhCn), "6");
        assert_eq!(
            generate_cell(&generator, &mut rng(), 9, &Locale::ZhCn),
            "10"
        );
    }

    #[test]
    fn test_random_int_range() {
        let generator = GeneratorConfig::RandomInt { min: 1, max: 10 };
        for _ in 0..50 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            let n: i64 = val.parse().unwrap();
            assert!(n >= 1 && n <= 10, "value {} out of range", n);
        }
    }

    #[test]
    fn test_constant_value() {
        let generator = GeneratorConfig::Constant {
            value: "hello".to_string(),
        };
        for i in 0..10 {
            assert_eq!(
                generate_cell(&generator, &mut rng(), i, &Locale::ZhCn),
                "hello"
            );
        }
    }

    #[test]
    fn test_boolean_ratio() {
        let generator = GeneratorConfig::Boolean { ratio: 0 };
        for _ in 0..20 {
            assert_eq!(
                generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn),
                "false"
            );
        }

        let generator = GeneratorConfig::Boolean { ratio: 100 };
        for _ in 0..20 {
            assert_eq!(
                generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn),
                "true"
            );
        }
    }

    #[test]
    fn test_digit_single_character() {
        let generator = GeneratorConfig::Digit;
        for _ in 0..30 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            assert_eq!(val.len(), 1, "digit should be single char, got: {}", val);
            assert!(
                val.parse::<u8>().is_ok(),
                "digit should be numeric, got: {}",
                val
            );
        }
    }

    #[test]
    fn test_words_range() {
        let generator = GeneratorConfig::Words { min: 2, max: 5 };
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            let word_count = val.split_whitespace().count();
            assert!(
                word_count >= 2 && word_count <= 5,
                "expected 2-5 words, got {}: '{}'",
                word_count,
                val
            );
        }
    }

    #[test]
    fn test_sentence_non_empty() {
        let generator = GeneratorConfig::Sentence { min: 3, max: 10 };
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            assert!(!val.is_empty(), "sentence should not be empty");
        }
    }

    #[test]
    fn test_uuid_format() {
        let generator = GeneratorConfig::UuidV4;
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            assert_eq!(val.len(), 36, "UUID should be 36 chars, got: {}", val);
            assert_eq!(val.chars().nth(8), Some('-'), "missing dash at pos 8");
            assert_eq!(val.chars().nth(13), Some('-'), "missing dash at pos 13");
        }
    }

    #[test]
    fn test_email_format() {
        let generator = GeneratorConfig::Email;
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            assert!(val.contains('@'), "email should contain @: {}", val);
            assert!(val.contains('.'), "email should contain .: {}", val);
        }
    }

    #[test]
    fn test_ipv4_format() {
        let generator = GeneratorConfig::IPv4;
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            let parts: Vec<&str> = val.split('.').collect();
            assert_eq!(parts.len(), 4, "IPv4 should have 4 parts: {}", val);
            for p in parts {
                let _n: u8 = p.parse().unwrap();
                // u8 类型天然保证值在 0..=255 范围内
            }
        }
    }

    #[test]
    fn test_random_float_precision() {
        let generator = GeneratorConfig::RandomFloat {
            min: 0.0,
            max: 1.0,
            precision: 2,
        };
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            let parts: Vec<&str> = val.split('.').collect();
            if parts.len() == 2 {
                assert!(
                    parts[1].len() <= 2,
                    "precision should be <= 2, got: {}",
                    val
                );
            }
        }
    }

    #[test]
    fn test_datetime_format() {
        let generator = GeneratorConfig::date_time("2020-01-01", "2025-12-31");
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            assert!(
                val.contains('T') || val.contains(' '),
                "datetime should contain separator: {}",
                val
            );
        }
    }

    #[test]
    fn test_phone_number_format() {
        let generator = GeneratorConfig::PhoneNumber;
        for _ in 0..10 {
            let val = generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn);
            assert!(!val.is_empty(), "phone number should not be empty");
        }
    }

    // ===== 分布族与时序：统计性质冒烟（宽容差，只求量级正确）=====

    /// 取一列样本值，供分布类断言复用。
    fn sample_column(generator: &GeneratorConfig, count: usize) -> Vec<f64> {
        let mut rng = rng();
        (0..count)
            .map(|i| {
                generate_cell(generator, &mut rng, i, &Locale::ZhCn)
                    .parse::<f64>()
                    .expect("生成值应能解析为数字")
            })
            .collect()
    }

    fn mean(values: &[f64]) -> f64 {
        values.iter().sum::<f64>() / values.len() as f64
    }

    #[test]
    fn test_poisson_is_non_negative_integer_near_lambda() {
        let samples = sample_column(&GeneratorConfig::Poisson { lambda: 5.0 }, 5_000);
        assert!(
            samples.iter().all(|v| *v >= 0.0 && v.fract() == 0.0),
            "泊松取值应为非负整数"
        );
        let avg = mean(&samples);
        assert!(
            (3.0..8.0).contains(&avg),
            "泊松 λ=5 的样本均值应接近 5，实际 {avg}"
        );
    }

    /// λ 大于 30 走正态近似（避免 O(λ) 次循环），这里只校验量级仍然对得上。
    #[test]
    fn test_poisson_large_lambda_uses_normal_approximation() {
        let samples = sample_column(&GeneratorConfig::Poisson { lambda: 200.0 }, 2_000);
        let avg = mean(&samples);
        assert!(
            (180.0..220.0).contains(&avg),
            "泊松 λ=200 的样本均值应接近 200，实际 {avg}"
        );
    }

    #[test]
    fn test_exponential_mean_near_inverse_lambda() {
        let samples = sample_column(&GeneratorConfig::Exponential { lambda: 2.0 }, 5_000);
        assert!(samples.iter().all(|v| *v >= 0.0), "指数分布取值不应为负");
        let avg = mean(&samples);
        assert!(
            (0.35..0.65).contains(&avg),
            "指数 λ=2 的样本均值应接近 0.5，实际 {avg}"
        );
    }

    #[test]
    fn test_pareto_never_below_scale() {
        let samples = sample_column(
            &GeneratorConfig::Pareto {
                scale_value: 10.0,
                alpha: 1.5,
            },
            1_000,
        );
        assert!(
            samples.iter().all(|v| *v >= 10.0),
            "帕累托取值不应低于最小取值（实测最小 {:?}）",
            samples.iter().cloned().fold(f64::INFINITY, f64::min)
        );
    }

    #[test]
    fn test_beta_within_unit_interval() {
        let samples = sample_column(
            &GeneratorConfig::Beta {
                alpha: 2.0,
                beta: 5.0,
            },
            1_000,
        );
        assert!(
            samples.iter().all(|v| (0.0..=1.0).contains(v)),
            "Beta 取值应落在 0~1 之间"
        );
        // Beta(2,5) 的期望是 α/(α+β) ≈ 0.286
        let avg = mean(&samples);
        assert!(
            (0.20..0.38).contains(&avg),
            "Beta(2,5) 的样本均值应接近 0.286，实际 {avg}"
        );
    }

    #[test]
    fn test_binomial_count_within_trials_near_np() {
        let samples = sample_column(
            &GeneratorConfig::Binomial {
                trials: 10,
                probability: 0.5,
            },
            2_000,
        );
        assert!(
            samples
                .iter()
                .all(|v| (0.0..=10.0).contains(v) && v.fract() == 0.0),
            "二项取值应为 0~n 之间的整数"
        );
        let avg = mean(&samples);
        assert!(
            (4.5..5.5).contains(&avg),
            "二项 n=10 p=0.5 的样本均值应接近 5，实际 {avg}"
        );
    }

    /// 时序：按行序推进，相隔一个周期的两行只差趋势（周期项互相抵消）。
    #[test]
    fn test_time_series_period_cancels_over_full_cycle() {
        let generator = GeneratorConfig::TimeSeries {
            start: 50.0,
            trend: 1.5,
            period: 12,
            amplitude: 7.0,
            noise: 0.0,
        };
        let mut seq_rng = rng();
        let value_at = |rng: &mut StdRng, i: usize| -> f64 {
            generate_cell(&generator, rng, i, &Locale::ZhCn)
                .parse()
                .expect("时序取值应能解析为数字")
        };
        for i in 0..12 {
            let diff = value_at(&mut seq_rng, i + 12) - value_at(&mut seq_rng, i);
            let expected = 1.5 * 12.0;
            assert!(
                (diff - expected).abs() < 1e-9,
                "相隔一个周期应只差趋势×周期={expected}，实际 {diff}"
            );
        }
        // 同 seed 重放整列一致
        let mut first = rng();
        let mut second = rng();
        let column_a: Vec<f64> = (0..24).map(|i| value_at(&mut first, i)).collect();
        let column_b: Vec<f64> = (0..24).map(|i| value_at(&mut second, i)).collect();
        assert_eq!(column_a, column_b, "同一 seed 应生成完全相同的时序列");
    }

    /// `period` 为 0 表示不叠加周期项：此时取值只含起始值 + 趋势。
    #[test]
    fn test_time_series_without_period_keeps_trend_only() {
        let generator = GeneratorConfig::TimeSeries {
            start: 10.0,
            trend: 3.0,
            period: 0,
            amplitude: 9.0,
            noise: 0.0,
        };
        let samples = sample_column(&generator, 6);
        for (i, value) in samples.iter().enumerate() {
            let expected = 10.0 + 3.0 * i as f64;
            assert!(
                (value - expected).abs() < 1e-9,
                "第 {i} 行应为 {expected}，实际 {value}"
            );
        }
    }

    /// 均值参数别被写反：正态的均值在 `mean`，标准差在 `std_dev`。
    #[test]
    fn test_normal_mean_and_std_dev_are_reasonable() {
        let samples = sample_column(
            &GeneratorConfig::Normal {
                mean: 100.0,
                std_dev: 15.0,
            },
            5_000,
        );
        let avg = mean(&samples);
        assert!(
            (99.0..101.0).contains(&avg),
            "正态均值应接近 100，实际 {avg}"
        );
        assert!(
            samples.iter().any(|v| *v < 100.0) && samples.iter().any(|v| *v > 100.0),
            "正态分布应同时产生均值两侧的取值"
        );
    }

    // ===== 工作日历（列级参数）：工作周掩码 + 跳过日期 + 上班日期 =====

    /// 造一个顺序日期生成器（默认工作周 = 周一~周五）。
    fn sequential(
        start: &str,
        step_seconds: i32,
        workdays_only: bool,
        skip_dates: &[&str],
        work_dates: &[&str],
    ) -> GeneratorConfig {
        GeneratorConfig::SequentialDate {
            start: start.to_string(),
            step_seconds,
            workdays_only,
            work_week: "1111100".to_string(),
            skip_dates: skip_dates.iter().map(|d| (*d).to_string()).collect(),
            work_dates: work_dates.iter().map(|d| (*d).to_string()).collect(),
        }
    }

    /// 取某列连续几行的值（顺序日期是确定性的，同 seed 不影响结果）。
    fn sequential_column(generator: &GeneratorConfig, rows: usize) -> Vec<String> {
        let mut rng = rng();
        (0..rows)
            .map(|i| generate_cell(generator, &mut rng, i, &Locale::ZhCn))
            .collect()
    }

    /// 2024-01-01 是周一：按工作日推进时周末两天不占行，自动跳到周一。
    #[test]
    fn test_sequential_date_skips_weekends() {
        let generator = sequential("2024-01-01 09:00:00", 86_400, true, &[], &[]);
        let values = sequential_column(&generator, 8);
        assert_eq!(
            values,
            vec![
                "2024-01-01 09:00:00", // 周一
                "2024-01-02 09:00:00",
                "2024-01-03 09:00:00",
                "2024-01-04 09:00:00",
                "2024-01-05 09:00:00", // 周五
                "2024-01-08 09:00:00", // 跳过 1/6、1/7 周末
                "2024-01-09 09:00:00",
                "2024-01-10 09:00:00",
            ]
        );
    }

    /// 关掉开关就回到旧行为：周末照出（步长固定）。
    #[test]
    fn test_sequential_date_without_calendar_keeps_fixed_steps() {
        let generator = sequential("2024-01-05 00:00:00", 86_400, false, &[], &[]);
        let values = sequential_column(&generator, 3);
        assert_eq!(
            values,
            vec![
                "2024-01-05 00:00:00",
                "2024-01-06 00:00:00", // 周六照出
                "2024-01-07 00:00:00",
            ]
        );
    }

    /// 跨年 + 黑名单（节假日）+ 白名单（调休上班）。
    #[test]
    fn test_sequential_date_calendar_lists_and_year_boundary() {
        // 不加列表：2023-12-29（周五）之后直接跳元旦（1/1 周一）
        let plain = sequential("2023-12-29 00:00:00", 86_400, true, &[], &[]);
        assert_eq!(
            sequential_column(&plain, 4),
            vec![
                "2023-12-29 00:00:00",
                "2024-01-01 00:00:00",
                "2024-01-02 00:00:00",
                "2024-01-03 00:00:00",
            ]
        );
        // 跳过 1/1（元旦）→ 下一行应该是 1/2
        let skipped = sequential("2023-12-29 00:00:00", 86_400, true, &["2024-01-01"], &[]);
        assert_eq!(
            sequential_column(&skipped, 3),
            vec![
                "2023-12-29 00:00:00",
                "2024-01-02 00:00:00",
                "2024-01-03 00:00:00",
            ]
        );
        // 调休：12/30（周六）上班 → 它要出现在序列里
        let extra = sequential("2023-12-29 00:00:00", 86_400, true, &[], &["2023-12-30"]);
        assert_eq!(
            sequential_column(&extra, 4),
            vec![
                "2023-12-29 00:00:00",
                "2023-12-30 00:00:00",
                "2024-01-01 00:00:00",
                "2024-01-02 00:00:00",
            ]
        );
    }

    /// 步长可以是多个工作日；起始日落在休息日时从下一个工作日起步。
    #[test]
    fn test_sequential_date_supports_multi_day_steps_and_rest_start() {
        let every_two = sequential("2024-01-01 00:00:00", 172_800, true, &[], &[]);
        assert_eq!(
            sequential_column(&every_two, 3),
            vec![
                "2024-01-01 00:00:00",
                "2024-01-03 00:00:00",
                "2024-01-05 00:00:00",
            ]
        );
        // 起始是周六（2024-01-06）→ 第 0 行就是下一个工作日周一
        let from_saturday = sequential("2024-01-06 08:30:00", 86_400, true, &[], &[]);
        let values = sequential_column(&from_saturday, 2);
        assert_eq!(values[0], "2024-01-08 08:30:00");
        assert_eq!(values[1], "2024-01-09 08:30:00");
    }

    /// `date_time_between` + 仅工作日：采样只落在工作日上，且不越出给定区间。
    #[test]
    fn test_datetime_between_workdays_only_lands_on_work_days() {
        use chrono::Datelike;
        let generator = GeneratorConfig::DateTimeBetween {
            start: "2024-01-01T00:00:00Z".to_string(),
            end: "2024-03-31T23:59:59Z".to_string(),
            workdays_only: true,
            work_hours_only: false,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: vec!["2024-01-02".to_string()],
            work_dates: vec!["2024-01-06".to_string()],
        };
        let mut rng = rng();
        let mut saturday_hits = 0;
        // 1000 次：周六（调休）在 65 个可用工作日里占 1 个，命中一次的把握足够
        for i in 0..1000 {
            let value = generate_cell(&generator, &mut rng, i, &Locale::ZhCn);
            let dt = chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S")
                .expect("应是标准时刻文本");
            assert!(
                dt >= chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    && dt
                        <= chrono::NaiveDate::from_ymd_opt(2024, 3, 31)
                            .unwrap()
                            .and_hms_opt(23, 59, 59)
                            .unwrap(),
                "取值不能越出给定区间：{value}"
            );
            let date = dt.date();
            assert_ne!(
                date,
                chrono::NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                "跳过日期不应出现：{value}"
            );
            let weekday = date.weekday();
            if date == chrono::NaiveDate::from_ymd_opt(2024, 1, 6).unwrap() {
                saturday_hits += 1; // 调休的周六：允许（而且是唯一允许的周六）
                continue;
            }
            assert!(
                !matches!(weekday, chrono::Weekday::Sat | chrono::Weekday::Sun),
                "周末不应出现：{value}"
            );
        }
        assert!(
            saturday_hits > 0,
            "调休的周六也应被采到（1000 次采样一次都没中说明抽样有偏）"
        );
    }

    /// `date_time_between` + 仅工作时段：日内时刻落在 09:00~18:00。
    #[test]
    fn test_datetime_between_work_hours_only() {
        let generator = GeneratorConfig::DateTimeBetween {
            start: "2024-01-01T00:00:00Z".to_string(),
            end: "2024-12-31T23:59:59Z".to_string(),
            workdays_only: false,
            work_hours_only: true,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        };
        let mut rng = rng();
        for i in 0..100 {
            let value = generate_cell(&generator, &mut rng, i, &Locale::ZhCn);
            let time = value
                .split(' ')
                .nth(1)
                .expect("应是「日期 时刻」两段")
                .to_string();
            assert!(
                ("09:00:00".."18:00:00").contains(&time.as_str()),
                "工作时段外不该出现：{value}"
            );
        }
    }

    /// 工作时段窗口可以自定义：时刻落在给定两段之间。
    #[test]
    fn test_datetime_between_custom_work_hours_window() {
        let generator = GeneratorConfig::DateTimeBetween {
            start: "2024-01-01T00:00:00Z".to_string(),
            end: "2024-12-31T23:59:59Z".to_string(),
            workdays_only: false,
            work_hours_only: true,
            work_hour_start: "10:30".to_string(),
            work_hour_end: "12:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        };
        let mut rng = rng();
        for i in 0..200 {
            let value = generate_cell(&generator, &mut rng, i, &Locale::ZhCn);
            let time = value.split(' ').nth(1).expect("两段").to_string();
            assert!(
                ("10:30:00".."12:00:00").contains(&time.as_str()),
                "应落在自定义窗口内：{value}"
            );
        }
    }

    /// 起 > 止 按跨零点（夜班）理解：`22:00~06:00` 的两段都要被采到。
    #[test]
    fn test_datetime_between_overnight_work_hours_window() {
        let generator = GeneratorConfig::DateTimeBetween {
            start: "2024-01-01T00:00:00Z".to_string(),
            end: "2024-12-31T23:59:59Z".to_string(),
            workdays_only: false,
            work_hours_only: true,
            work_hour_start: "22:00".to_string(),
            work_hour_end: "06:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        };
        let mut rng = rng();
        let (mut before_midnight, mut after_midnight) = (0, 0);
        for i in 0..400 {
            let value = generate_cell(&generator, &mut rng, i, &Locale::ZhCn);
            let time = value.split(' ').nth(1).expect("两段").to_string();
            if ("22:00:00".."24:00:00").contains(&time.as_str()) {
                before_midnight += 1;
            } else if ("00:00:00".."06:00:00").contains(&time.as_str()) {
                after_midnight += 1;
            } else {
                panic!("夜班窗口外的时刻：{value}");
            }
        }
        assert!(
            before_midnight > 0 && after_midnight > 0,
            "跨零点的两段都该被采到：{before_midnight} / {after_midnight}"
        );
    }

    /// 日期列（只到天）+ 仅工作日：只出工作日、跳过列表生效、不越出区间。
    #[test]
    fn test_date_calendar_lands_on_work_days() {
        use chrono::{Datelike, NaiveDate, Weekday};
        let generator = GeneratorConfig::Date {
            min: "2024-01-01".to_string(),
            max: "2024-03-31".to_string(),
            workdays_only: true,
            work_week: "1111100".to_string(),
            skip_dates: vec!["2024-01-02".to_string(), "2024-02-12".to_string()],
            work_dates: Vec::new(),
        };
        let (min_date, max_date) = (
            NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 3, 31).unwrap(),
        );
        let mut rng = rng();
        for i in 0..300 {
            let value = generate_cell(&generator, &mut rng, i, &Locale::ZhCn);
            let date = NaiveDate::parse_from_str(&value, "%Y-%m-%d").expect("应是日期文本");
            assert!(date >= min_date && date <= max_date, "越出区间：{value}");
            assert!(
                !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
                "周末不该出现：{value}"
            );
            assert_ne!(
                date,
                NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                "跳过日期不该出现：{value}"
            );
        }
    }

    /// `date_time`与`datetime_before` 也接了同一套日历（同一实现路径，只验开关关不关）。
    #[test]
    fn test_other_datetime_generators_accept_the_calendar() {
        use chrono::{Datelike, NaiveDateTime, Weekday};
        let generators = [
            GeneratorConfig::DateTime {
                min: "2024-01-01T00:00:00Z".to_string(),
                max: "2024-01-31T23:59:59Z".to_string(),
                workdays_only: true,
                work_hours_only: false,
                work_hour_start: "09:00".to_string(),
                work_hour_end: "18:00".to_string(),
                work_week: "1111100".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            },
            GeneratorConfig::DateTimeBefore {
                before: "2024-01-31T23:59:59Z".to_string(),
                workdays_only: true,
                work_hours_only: true,
                work_hour_start: "09:00".to_string(),
                work_hour_end: "18:00".to_string(),
                work_week: "1111100".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            },
            GeneratorConfig::DateTimeAfter {
                after: "2024-01-01T00:00:00Z".to_string(),
                workdays_only: true,
                work_hours_only: false,
                work_hour_start: "09:00".to_string(),
                work_hour_end: "18:00".to_string(),
                work_week: "1111100".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            },
        ];
        let mut rng = rng();
        for generator in &generators {
            for i in 0..50 {
                let value = generate_cell(generator, &mut rng, i, &Locale::ZhCn);
                let dt = NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S")
                    .expect("应是标准时刻文本");
                assert!(
                    !matches!(dt.date().weekday(), Weekday::Sat | Weekday::Sun),
                    "周末不该出现：{value}"
                );
            }
        }
    }
}

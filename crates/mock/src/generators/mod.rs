use fake::RngExt;
// 辅助函数子模块
mod numeric;
mod datetime;
mod text;

// 导入辅助函数
use numeric::*;
use datetime::*;
use text::*;

// 对外 re-export
pub(crate) use datetime::{parse_hour_minutes, DEFAULT_DATETIME_MIN, DEFAULT_DATETIME_MAX};

use fake::rand::rngs::StdRng;
use fake::{Fake, Faker};

use crate::models::{ColumnDataType, GeneratorConfig, Locale};

/// 生成器 → 产出数据类（技术视角分组与数据类徽标共用）。
///
/// 与 [`generate_cell`] 同构：**穷尽 match、无 `_ =>` 兜底**，新增 `GeneratorConfig` 变体会
/// 编译失败强制补齐此处归类。分组口径与 `generate_cell` 的输出形态一一对应：
/// - 数值：整数 / 浮点 / 定点小数按变体区分；`Duration` 输出秒数（整数）；
/// - 时间：`Time` 输出 `%H:%M:%S` 字符串（无独立 Time 列类型 → Varchar），日期系按粒度分
///   `Date` / `DateTime`；
/// - 文本：多行文本（句子组 / 段落 / Markdown）→ `Text`，其余单值串 → `Varchar`；
/// - 约束类（ValueSet / Sequence / Weighted）输出字符串 → `Varchar`。
pub(crate) fn data_type_for_generator(config: &GeneratorConfig) -> ColumnDataType {
    match config {
        // 整数类
        GeneratorConfig::AutoIncrement { .. }
        | GeneratorConfig::RandomInt { .. }
        | GeneratorConfig::Poisson { .. }
        | GeneratorConfig::Binomial { .. }
        | GeneratorConfig::Duration => ColumnDataType::Integer,
        // 浮点类
        GeneratorConfig::RandomFloat { .. }
        | GeneratorConfig::Normal { .. }
        | GeneratorConfig::LogNormal { .. }
        | GeneratorConfig::Exponential { .. }
        | GeneratorConfig::Pareto { .. }
        | GeneratorConfig::Beta { .. }
        | GeneratorConfig::RandomWalk { .. }
        | GeneratorConfig::TimeSeries { .. }
        | GeneratorConfig::Latitude
        | GeneratorConfig::Longitude => ColumnDataType::Double,
        // 定点小数（默认精度标度 = 常见金额位）
        GeneratorConfig::RandomDecimal { .. } => ColumnDataType::Decimal {
            precision: 18,
            scale: 2,
        },
        GeneratorConfig::Boolean { .. } => ColumnDataType::Boolean,
        GeneratorConfig::Date { .. }
        | GeneratorConfig::SequentialDate { .. }
        | GeneratorConfig::SequentialDateWithGaps { .. } => ColumnDataType::Date,
        GeneratorConfig::DateTime { .. }
        | GeneratorConfig::DateTimeBefore { .. }
        | GeneratorConfig::DateTimeAfter { .. }
        | GeneratorConfig::DateTimeBetween { .. } => ColumnDataType::DateTime,
        GeneratorConfig::UuidV1
        | GeneratorConfig::UuidV3
        | GeneratorConfig::UuidV4
        | GeneratorConfig::UuidV5 => ColumnDataType::Uuid,
        // 多行文本
        GeneratorConfig::Sentences { .. }
        | GeneratorConfig::Paragraph { .. }
        | GeneratorConfig::Paragraphs { .. }
        | GeneratorConfig::MarkdownItalicWord { .. }
        | GeneratorConfig::MarkdownBoldWord { .. }
        | GeneratorConfig::MarkdownLink { .. }
        | GeneratorConfig::MarkdownBulletPoints { .. }
        | GeneratorConfig::MarkdownListItems { .. }
        | GeneratorConfig::MarkdownBlockQuoteSingle { .. }
        | GeneratorConfig::MarkdownBlockQuoteMulti { .. }
        | GeneratorConfig::MarkdownCode { .. } => ColumnDataType::Text,
        // 其余全部为单值字符串
        GeneratorConfig::Digit
        | GeneratorConfig::NumberWithFormat { .. }
        | GeneratorConfig::Constant { .. }
        | GeneratorConfig::Word
        | GeneratorConfig::Words { .. }
        | GeneratorConfig::Sentence { .. }
        | GeneratorConfig::Regex { .. }
        | GeneratorConfig::Template { .. }
        | GeneratorConfig::Name
        | GeneratorConfig::NameWithTitle
        | GeneratorConfig::FirstName
        | GeneratorConfig::LastName
        | GeneratorConfig::Title
        | GeneratorConfig::Suffix
        | GeneratorConfig::Email
        | GeneratorConfig::SafeEmail
        | GeneratorConfig::FreeEmailProvider
        | GeneratorConfig::DomainSuffix
        | GeneratorConfig::FreeEmail
        | GeneratorConfig::PhoneNumber
        | GeneratorConfig::CellNumber
        | GeneratorConfig::Username
        | GeneratorConfig::Password { .. }
        | GeneratorConfig::Country
        | GeneratorConfig::CountryCode
        | GeneratorConfig::CountryName
        | GeneratorConfig::City
        | GeneratorConfig::CityPrefix
        | GeneratorConfig::CitySuffix
        | GeneratorConfig::StateName
        | GeneratorConfig::StateAbbr
        | GeneratorConfig::StreetName
        | GeneratorConfig::StreetSuffix
        | GeneratorConfig::ZipCode
        | GeneratorConfig::PostCode
        | GeneratorConfig::BuildingNumber
        | GeneratorConfig::SecondaryAddress
        | GeneratorConfig::SecondaryAddressType
        | GeneratorConfig::Geohash { .. }
        | GeneratorConfig::TimeZone
        | GeneratorConfig::IpAddress
        | GeneratorConfig::IPv4
        | GeneratorConfig::IPv6
        | GeneratorConfig::IP
        | GeneratorConfig::MacAddress
        | GeneratorConfig::Time
        | GeneratorConfig::CompanyName
        | GeneratorConfig::CompanySuffix
        | GeneratorConfig::JobTitle
        | GeneratorConfig::Profession
        | GeneratorConfig::Industry
        | GeneratorConfig::Seniority
        | GeneratorConfig::Field
        | GeneratorConfig::Position
        | GeneratorConfig::Buzzword
        | GeneratorConfig::BuzzwordMiddle
        | GeneratorConfig::BuzzwordTail
        | GeneratorConfig::CatchPhrase
        | GeneratorConfig::BsVerb
        | GeneratorConfig::BsAdj
        | GeneratorConfig::BsNoun
        | GeneratorConfig::Bs
        | GeneratorConfig::CurrencyCode
        | GeneratorConfig::CurrencyName
        | GeneratorConfig::CurrencySymbol
        | GeneratorConfig::Bic
        | GeneratorConfig::Isin
        | GeneratorConfig::CreditCardNumber
        | GeneratorConfig::Url
        | GeneratorConfig::UserAgent
        | GeneratorConfig::MimeType
        | GeneratorConfig::Semver
        | GeneratorConfig::SemverStable
        | GeneratorConfig::SemverUnstable
        | GeneratorConfig::FilePath
        | GeneratorConfig::FileName
        | GeneratorConfig::FileExtension
        | GeneratorConfig::DirPath
        | GeneratorConfig::ImageUrl { .. }
        | GeneratorConfig::ImageUrlWithSeed { .. }
        | GeneratorConfig::ImageUrlGrayscale { .. }
        | GeneratorConfig::ImageUrlBlur { .. }
        | GeneratorConfig::ImageUrlCustom { .. }
        | GeneratorConfig::HexColor
        | GeneratorConfig::RgbColor
        | GeneratorConfig::RgbaColor
        | GeneratorConfig::HslColor
        | GeneratorConfig::HslaColor
        | GeneratorConfig::Color
        | GeneratorConfig::FerroidULID
        | GeneratorConfig::FerroidTwitterId
        | GeneratorConfig::FerroidInstagramId
        | GeneratorConfig::FerroidMastodonId
        | GeneratorConfig::FerroidDiscordId
        | GeneratorConfig::Isbn
        | GeneratorConfig::Isbn10
        | GeneratorConfig::Isbn13
        | GeneratorConfig::RfcStatusCode
        | GeneratorConfig::ValidStatusCode
        | GeneratorConfig::LicencePlate
        | GeneratorConfig::HealthInsuranceCode
        | GeneratorConfig::ValueSet { .. }
        | GeneratorConfig::Sequence { .. }
        | GeneratorConfig::Weighted { .. } => ColumnDataType::Varchar { length: None },
    }
}

/// A2：按语言偏好（[`Locale`]）把 faker 派发到 fake crate 的对应 locale 模块。
///
/// fake 的 `def_fakers!` 给每个 locale 生成**同名函数**（如 `name::zh_cn::Name()` 与
/// `name::en::Name()`），所以这里是编译期可枚举的 match，不是运行时查表。
/// 数据未覆盖的 (faker, locale) 由 fake 的 `Data` trait 默认实现**静默回落英文**——
/// 本模块不重复它（UI 层已有「随语言偏好」标记，提示用户不保证所有生成器都有母语数据）。
macro_rules! locale_faker {
    ($locale:expr, $module:ident, $faker:ident, $rng:expr) => {
        match $locale {
            Locale::ZhCn => fake::faker::$module::zh_cn::$faker().fake_with_rng::<String, _>($rng),
            Locale::En => fake::faker::$module::en::$faker().fake_with_rng::<String, _>($rng),
            Locale::JaJp => fake::faker::$module::ja_jp::$faker().fake_with_rng::<String, _>($rng),
            Locale::ZhTw => fake::faker::$module::zh_tw::$faker().fake_with_rng::<String, _>($rng),
            Locale::FrFr => fake::faker::$module::fr_fr::$faker().fake_with_rng::<String, _>($rng),
            Locale::DeDe => fake::faker::$module::de_de::$faker().fake_with_rng::<String, _>($rng),
            Locale::ItIt => fake::faker::$module::it_it::$faker().fake_with_rng::<String, _>($rng),
            Locale::PtBr => fake::faker::$module::pt_br::$faker().fake_with_rng::<String, _>($rng),
            Locale::PtPt => fake::faker::$module::pt_pt::$faker().fake_with_rng::<String, _>($rng),
            Locale::NlNl => fake::faker::$module::nl_nl::$faker().fake_with_rng::<String, _>($rng),
            Locale::ArSa => fake::faker::$module::ar_sa::$faker().fake_with_rng::<String, _>($rng),
            Locale::TrTr => fake::faker::$module::tr_tr::$faker().fake_with_rng::<String, _>($rng),
            Locale::FaIr => fake::faker::$module::fa_ir::$faker().fake_with_rng::<String, _>($rng),
        }
    };
}

pub(super) fn generate_cell(
    generator: &GeneratorConfig,
    rng: &mut StdRng,
    row_index: usize,
    locale: &Locale,
) -> String {
    match generator {
        // ========== 数值类 ==========
        GeneratorConfig::AutoIncrement { start, step } => {
            (*start as i64 + (row_index as i64) * *step as i64).to_string()
        }
        GeneratorConfig::RandomInt { min, max } => (*min as i64..=*max as i64)
            .fake_with_rng::<i64, _>(rng)
            .to_string(),
        GeneratorConfig::RandomFloat {
            min,
            max,
            precision,
        } => {
            let val: f64 = (*min..*max).fake_with_rng(rng);
            format!("{:.prec$}", val, prec = *precision as usize)
        }
        GeneratorConfig::RandomDecimal { min, max, scale } => {
            let val: f64 = (*min..*max).fake_with_rng(rng);
            format!("{:.scl$}", val, scl = *scale as usize)
        }
        GeneratorConfig::Digit => {
            use fake::faker::number::en::Digit;
            Digit().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::NumberWithFormat { fmt } => {
            use fake::faker::number::en::NumberWithFormat;
            NumberWithFormat(fmt).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Normal { mean, std_dev } => {
            (mean + std_dev * standard_normal(rng)).to_string()
        }
        GeneratorConfig::LogNormal { median, dispersion } => {
            let value = (median.ln() + dispersion * standard_normal(rng)).exp();
            value.to_string()
        }
        GeneratorConfig::RandomWalk {
            start,
            step,
            volatility,
        } => {
            let base = *start + (row_index as f64) * *step;
            // 游走的方差随步数线性增长（布朗运动）：第 i 行的噪声尺度是 √i·波动率
            let noise_scale = (row_index as f64).sqrt() * *volatility;
            (base + noise_scale * standard_normal(rng)).to_string()
        }
        GeneratorConfig::Boolean { ratio } => {
            use fake::faker::boolean::en::Boolean;
            Boolean(*ratio).fake_with_rng::<bool, _>(rng).to_string()
        }
        GeneratorConfig::Poisson { lambda } => {
            // 计数类分布（到达数 / 事件数）：λ 小走 Knuth 逐次相乘（精确），
            // λ 大改用正态近似（N(λ, √λ) 取整）——否则单值就是 O(λ) 次循环，
            // 十万行 × λ=1000 会卡住生成。
            let lambda = lambda.max(1e-9);
            let count = if lambda < 30.0 {
                let limit = (-lambda).exp();
                let mut k: u64 = 0;
                let mut product = 1.0_f64;
                loop {
                    product *= rng.random::<f64>();
                    if product <= limit || k >= 100_000 {
                        break k;
                    }
                    k += 1;
                }
            } else {
                (lambda + lambda.sqrt() * standard_normal(rng))
                    .round()
                    .max(0.0) as u64
            };
            count.to_string()
        }
        GeneratorConfig::Exponential { lambda } => {
            // 等待时间 / 间隔：逆变换法（1 - U 避免 ln(0)）
            let lambda = lambda.max(1e-9);
            let u = 1.0 - rng.random::<f64>();
            (-u.ln() / lambda).to_string()
        }
        GeneratorConfig::Pareto { scale_value, alpha } => {
            // 长尾（幂律）：逆变换 —— x_m / U^(1/α)，恒 ≥ x_m
            let alpha = alpha.max(1e-9);
            let u = 1.0 - rng.random::<f64>();
            (scale_value * u.powf(-1.0 / alpha)).to_string()
        }
        GeneratorConfig::Beta { alpha, beta } => {
            // 比例 / 比率类（0..1）：两个 Gamma 之比
            let x = gamma_sample(alpha.max(1e-9), rng);
            let y = gamma_sample(beta.max(1e-9), rng);
            let value = if x + y > 0.0 { x / (x + y) } else { 0.5 };
            value.to_string()
        }
        GeneratorConfig::Binomial {
            trials,
            probability,
        } => {
            // 成功次数：n 小逐次抽（精确），n 大用正态近似（np 与 np(1-p)）
            let p = probability.clamp(0.0, 1.0);
            let successes = if *trials <= 64 {
                (0..*trials).filter(|_| rng.random::<f64>() < p).count() as u64
            } else {
                let n = f64::from(*trials);
                let sigma = (n * p * (1.0 - p)).sqrt();
                (n * p + sigma * standard_normal(rng)).round().clamp(0.0, n) as u64
            };
            successes.to_string()
        }
        GeneratorConfig::TimeSeries {
            start,
            trend,
            period,
            amplitude,
            noise,
        } => {
            // 时序数值：按行序推进 —— 趋势（每行增量）+ 周期（行数为周期）+ 噪声。
            // 与 `sequential_date` 按同一行序展开，两列并排就是一条时间序列。
            let i = row_index as f64;
            let seasonal = if *period == 0 {
                0.0
            } else {
                let phase = 2.0 * std::f64::consts::PI * i / f64::from(*period);
                amplitude * phase.sin()
            };
            (start + trend * i + seasonal + noise * standard_normal(rng)).to_string()
        }

        // ========== 文本类 ==========
        GeneratorConfig::Constant { value } => value.clone(),
        GeneratorConfig::Words { min, max } => {
            use fake::faker::lorem::en::Words;
            let words: Vec<String> = Words(*min as usize..*max as usize).fake_with_rng(rng);
            words.join(" ")
        }
        GeneratorConfig::Sentence { min, max } => {
            use fake::faker::lorem::en::Sentence;
            Sentence(*min as usize..*max as usize).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Sentences { min, max } => {
            use fake::faker::lorem::en::Sentences;
            let sentences: Vec<String> = Sentences(*min as usize..*max as usize).fake_with_rng(rng);
            sentences.join(" ")
        }
        GeneratorConfig::Paragraph { count } => {
            use fake::faker::lorem::en::Paragraph;
            Paragraph(*count as usize..*count as usize + 1).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Paragraphs { count } => {
            use fake::faker::lorem::en::Paragraphs;
            Paragraphs(*count as usize..(*count as usize + 1))
                .fake_with_rng::<Vec<String>, _>(rng)
                .join("\n\n")
        }
        GeneratorConfig::Word => {
            use fake::faker::lorem::en::Word;
            Word().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Regex { pattern } => generate_from_regex(pattern, rng),
        GeneratorConfig::Template { template } => generate_from_template(template, rng),

        // ========== 个人信息 ==========
        // A2：姓名 / 电话 / 邮箱 / 城市 / 公司名等按语言偏好派发（`locale_faker!`）
        GeneratorConfig::Name => locale_faker!(locale, name, Name, rng),
        GeneratorConfig::NameWithTitle => {
            use fake::faker::name::en::NameWithTitle;
            NameWithTitle().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FirstName => locale_faker!(locale, name, FirstName, rng),
        GeneratorConfig::LastName => locale_faker!(locale, name, LastName, rng),
        GeneratorConfig::Title => {
            use fake::faker::name::en::Title;
            Title().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Suffix => {
            use fake::faker::name::en::Suffix;
            Suffix().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Email => {
            use fake::faker::internet::en::FreeEmail;
            FreeEmail().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::SafeEmail => {
            use fake::faker::internet::en::SafeEmail;
            SafeEmail().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FreeEmailProvider => {
            use fake::faker::internet::en::FreeEmailProvider;
            FreeEmailProvider().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::DomainSuffix => {
            use fake::faker::internet::en::DomainSuffix;
            DomainSuffix().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FreeEmail => {
            use fake::faker::internet::en::FreeEmail;
            FreeEmail().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::PhoneNumber => locale_faker!(locale, phone_number, PhoneNumber, rng),
        GeneratorConfig::CellNumber => locale_faker!(locale, phone_number, CellNumber, rng),
        GeneratorConfig::Username => {
            use fake::faker::internet::en::Username;
            Username().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Password { min, max } => {
            use fake::faker::internet::en::Password;
            Password(*min as usize..*max as usize).fake_with_rng::<String, _>(rng)
        }

        // ========== 地址类 ==========
        GeneratorConfig::Country => {
            use fake::faker::address::en::CountryCode;
            CountryCode().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CountryCode => {
            use fake::faker::address::en::CountryCode;
            CountryCode().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CountryName => {
            use fake::faker::address::en::CountryName;
            CountryName().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::City => locale_faker!(locale, address, CityName, rng),
        GeneratorConfig::CityPrefix => {
            use fake::faker::address::en::CityPrefix;
            CityPrefix().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CitySuffix => {
            use fake::faker::address::en::CitySuffix;
            CitySuffix().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::StateName => {
            use fake::faker::address::en::StateName;
            StateName().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::StateAbbr => {
            use fake::faker::address::en::StateAbbr;
            StateAbbr().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::StreetName => locale_faker!(locale, address, StreetName, rng),
        GeneratorConfig::StreetSuffix => {
            use fake::faker::address::en::StreetSuffix;
            StreetSuffix().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::ZipCode => {
            use fake::faker::address::en::ZipCode;
            ZipCode().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::PostCode => {
            use fake::faker::address::en::PostCode;
            PostCode().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::BuildingNumber => {
            use fake::faker::address::en::BuildingNumber;
            BuildingNumber().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::SecondaryAddress => {
            use fake::faker::address::en::SecondaryAddress;
            SecondaryAddress().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::SecondaryAddressType => {
            use fake::faker::address::en::SecondaryAddressType;
            SecondaryAddressType().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Latitude => {
            use fake::faker::address::en::Latitude;
            Latitude().fake_with_rng::<f64, _>(rng).to_string()
        }
        GeneratorConfig::Longitude => {
            use fake::faker::address::en::Longitude;
            Longitude().fake_with_rng::<f64, _>(rng).to_string()
        }
        GeneratorConfig::Geohash { precision } => {
            use fake::faker::address::en::Geohash;
            Geohash(*precision).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::TimeZone => {
            use fake::faker::address::en::TimeZone;
            TimeZone().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::IpAddress => {
            use fake::faker::internet::en::IP;
            IP().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::IPv4 => {
            use fake::faker::internet::en::IPv4;
            IPv4().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::IPv6 => {
            use fake::faker::internet::en::IPv6;
            IPv6().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::IP => {
            use fake::faker::internet::en::IP;
            IP().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::MacAddress => {
            use fake::faker::internet::en::MACAddress;
            MACAddress().fake_with_rng::<String, _>(rng)
        }

        // ========== 日期时间 ==========
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
        } => datetime_with_calendar(
            min,
            max,
            CalendarArgs::new(
                *workdays_only,
                *work_hours_only,
                work_hour_start,
                work_hour_end,
                work_week,
                skip_dates,
                work_dates,
            ),
            rng,
        ),
        GeneratorConfig::DateTimeBefore {
            before,
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        } => datetime_with_calendar(
            DEFAULT_DATETIME_MIN,
            before,
            CalendarArgs::new(
                *workdays_only,
                *work_hours_only,
                work_hour_start,
                work_hour_end,
                work_week,
                skip_dates,
                work_dates,
            ),
            rng,
        ),
        GeneratorConfig::DateTimeAfter {
            after,
            workdays_only,
            work_hours_only,
            work_hour_start,
            work_hour_end,
            work_week,
            skip_dates,
            work_dates,
        } => datetime_with_calendar(
            after,
            DEFAULT_DATETIME_MAX,
            CalendarArgs::new(
                *workdays_only,
                *work_hours_only,
                work_hour_start,
                work_hour_end,
                work_week,
                skip_dates,
                work_dates,
            ),
            rng,
        ),
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
        } => datetime_with_calendar(
            min,
            max,
            CalendarArgs::new(
                *workdays_only,
                *work_hours_only,
                work_hour_start,
                work_hour_end,
                work_week,
                skip_dates,
                work_dates,
            ),
            rng,
        ),
        GeneratorConfig::Date {
            min,
            max,
            workdays_only,
            work_week,
            skip_dates,
            work_dates,
        } => {
            use fake::faker::chrono::en::{Date, DateTimeBetween};
            if *workdays_only {
                // 日历开启时用日期时间的采样器（重抽 + 顺延 + 夹回区间），只取日期部分
                let lower = format!("{}T00:00:00Z", min.trim());
                let upper = format!("{}T23:59:59Z", max.trim());
                let value = datetime_between_with_calendar(
                    &lower,
                    &upper,
                    Some(&WorkCalendar::new(work_week, skip_dates, work_dates)),
                    None,
                    rng,
                );
                return value.split(' ').next().unwrap_or_default().to_string();
            }
            let s = parse_date(min);
            let e = parse_date(max);
            if let (Some(start), Some(end)) = (
                s.and_hms_opt(0, 0, 0).map(|d| d.and_utc()),
                e.and_hms_opt(23, 59, 59).map(|d| d.and_utc()),
            ) {
                DateTimeBetween(start, end)
                    .fake_with_rng::<chrono::DateTime<chrono::Utc>, _>(rng)
                    .format("%Y-%m-%d")
                    .to_string()
            } else {
                Date()
                    .fake_with_rng::<chrono::NaiveDate, _>(rng)
                    .format("%Y-%m-%d")
                    .to_string()
            }
        }
        GeneratorConfig::Time => {
            use fake::faker::chrono::en::Time;
            Time()
                .fake_with_rng::<chrono::NaiveTime, _>(rng)
                .format("%H:%M:%S")
                .to_string()
        }
        GeneratorConfig::Duration => {
            use fake::faker::chrono::en::Duration;
            let d: chrono::Duration = Duration().fake_with_rng(rng);
            format!("{}", d.num_seconds())
        }
        GeneratorConfig::SequentialDate {
            start,
            step_seconds,
            workdays_only,
            work_week,
            skip_dates,
            work_dates,
        } => {
            let dt = parse_start_datetime("SequentialDate", start);
            let new_dt = if *workdays_only {
                let calendar = WorkCalendar::new(work_week, skip_dates, work_dates);
                advance_work_days(&calendar, dt, row_index as i64, *step_seconds)
            } else {
                dt + chrono::Duration::seconds(*step_seconds as i64 * row_index as i64)
            };
            new_dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        GeneratorConfig::SequentialDateWithGaps {
            start,
            step_seconds,
            miss_probability,
            workdays_only,
            work_week,
            skip_dates,
            work_dates,
        } => {
            let roll: f64 = rng.random();
            if roll < *miss_probability {
                return String::new();
            }
            let dt = parse_start_datetime("SequentialDateWithGaps", start);
            let total_steps = (row_index as f64 * (1.0 - *miss_probability)).max(0.0) as i64;
            let new_dt = if *workdays_only {
                let calendar = WorkCalendar::new(work_week, skip_dates, work_dates);
                advance_work_days(&calendar, dt, total_steps, *step_seconds)
            } else {
                dt + chrono::Duration::seconds(*step_seconds as i64 * total_steps)
            };
            new_dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }

        // ========== 商业类 ==========
        GeneratorConfig::CompanyName => locale_faker!(locale, company, CompanyName, rng),
        GeneratorConfig::CompanySuffix => {
            use fake::faker::company::en::CompanySuffix;
            CompanySuffix().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::JobTitle => {
            let titles = vec![
                "高级工程师",
                "产品经理",
                "技术总监",
                "项目经理",
                "架构师",
                "数据分析师",
                "运营经理",
                "市场总监",
                "财务经理",
                "人力资源总监",
                "后端工程师",
                "前端工程师",
                "测试工程师",
                "运维工程师",
                "设计师",
                "实习生",
            ];
            let idx = (0..titles.len()).fake_with_rng::<usize, _>(rng);
            titles[idx].to_string()
        }
        GeneratorConfig::Profession => {
            use fake::faker::company::en::Profession;
            Profession().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Industry => {
            use fake::faker::company::en::Industry;
            Industry().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Seniority => {
            use fake::faker::job::en::Seniority;
            Seniority().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Field => {
            use fake::faker::job::en::Field;
            Field().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Position => {
            use fake::faker::job::en::Position;
            Position().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Buzzword => {
            use fake::faker::company::en::Buzzword;
            Buzzword().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::BuzzwordMiddle => {
            use fake::faker::company::en::BuzzwordMiddle;
            BuzzwordMiddle().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::BuzzwordTail => {
            use fake::faker::company::en::BuzzwordTail;
            BuzzwordTail().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CatchPhrase => {
            use fake::faker::company::en::CatchPhrase;
            CatchPhrase().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::BsVerb => {
            use fake::faker::company::en::BsVerb;
            BsVerb().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::BsAdj => {
            use fake::faker::company::en::BsAdj;
            BsAdj().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::BsNoun => {
            use fake::faker::company::en::BsNoun;
            BsNoun().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Bs => {
            use fake::faker::company::en::Bs;
            Bs().fake_with_rng::<String, _>(rng)
        }

        // ========== 金融类 ==========
        GeneratorConfig::CurrencyCode => {
            use fake::faker::currency::en::CurrencyCode;
            CurrencyCode().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CurrencyName => {
            use fake::faker::currency::en::CurrencyName;
            CurrencyName().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CurrencySymbol => {
            use fake::faker::currency::en::CurrencySymbol;
            CurrencySymbol().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Bic => {
            use fake::faker::finance::en::Bic;
            Bic().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Isin => {
            use fake::faker::finance::en::Isin;
            Isin().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CreditCardNumber => {
            use fake::faker::creditcard::en::CreditCardNumber;
            CreditCardNumber().fake_with_rng::<String, _>(rng)
        }

        // ========== UUID ==========
        GeneratorConfig::UuidV1 => fake::uuid::UUIDv1.fake_with_rng::<String, _>(rng),
        GeneratorConfig::UuidV3 => fake::uuid::UUIDv3.fake_with_rng::<String, _>(rng),
        GeneratorConfig::UuidV4 => fake::uuid::UUIDv4.fake_with_rng::<String, _>(rng),
        GeneratorConfig::UuidV5 => fake::uuid::UUIDv5.fake_with_rng::<String, _>(rng),

        // ========== 网络/技术 ==========
        GeneratorConfig::Url => {
            let tlds = ["com", "org", "net", "io", "dev", "app"];
            let tld = tlds[(0..tlds.len()).fake_with_rng::<usize, _>(rng)];
            let host: String = (0..(5..12).fake_with_rng::<usize, _>(rng))
                .map(|_| ((97..123).fake_with_rng::<u8, _>(rng)) as char)
                .collect();
            let path: String = (0..(0..3).fake_with_rng::<usize, _>(rng))
                .map(|_| {
                    let seg: String = (0..(3..8).fake_with_rng::<usize, _>(rng))
                        .map(|_| ((97..123).fake_with_rng::<u8, _>(rng)) as char)
                        .collect();
                    format!("/{}", seg)
                })
                .collect();
            format!("https://{}.{}{}", host, tld, path)
        }
        GeneratorConfig::UserAgent => {
            use fake::faker::internet::en::UserAgent;
            UserAgent().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::MimeType => {
            use fake::faker::filesystem::en::MimeType;
            MimeType().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Semver => {
            use fake::faker::filesystem::en::Semver;
            Semver().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::SemverStable => {
            use fake::faker::filesystem::en::SemverStable;
            SemverStable().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::SemverUnstable => {
            use fake::faker::filesystem::en::SemverUnstable;
            SemverUnstable().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FilePath => {
            use fake::faker::filesystem::en::FilePath;
            FilePath().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FileName => {
            use fake::faker::filesystem::en::FileName;
            FileName().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FileExtension => {
            use fake::faker::filesystem::en::FileExtension;
            FileExtension().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::DirPath => {
            use fake::faker::filesystem::en::DirPath;
            DirPath().fake_with_rng::<String, _>(rng)
        }

        // ========== Picsum 图片 ==========
        GeneratorConfig::ImageUrl { width, height } => {
            use fake::faker::impls::picsum::ImageOptions;
            use fake::faker::picsum::en::ImageCustom;
            let opts = ImageOptions {
                width: Some((*width).min(u16::MAX as u32) as u16),
                height: Some((*height).min(u16::MAX as u32) as u16),
                grayscale: false,
                blur: None,
                seed: None,
            };
            ImageCustom(opts).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::ImageUrlWithSeed {
            width,
            height,
            seed,
        } => {
            use fake::faker::impls::picsum::ImageOptions;
            use fake::faker::picsum::en::ImageCustom;
            let opts = ImageOptions {
                width: Some((*width).min(u16::MAX as u32) as u16),
                height: Some((*height).min(u16::MAX as u32) as u16),
                grayscale: false,
                blur: None,
                seed: Some(seed.to_string()),
            };
            ImageCustom(opts).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::ImageUrlGrayscale { width, height } => {
            use fake::faker::impls::picsum::ImageOptions;
            use fake::faker::picsum::en::ImageCustom;
            let opts = ImageOptions {
                width: Some((*width).min(u16::MAX as u32) as u16),
                height: Some((*height).min(u16::MAX as u32) as u16),
                grayscale: true,
                blur: None,
                seed: None,
            };
            ImageCustom(opts).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::ImageUrlBlur {
            width,
            height,
            blur_amount,
        } => {
            use fake::faker::impls::picsum::ImageOptions;
            use fake::faker::picsum::en::ImageCustom;
            let opts = ImageOptions {
                width: Some((*width).min(u16::MAX as u32) as u16),
                height: Some((*height).min(u16::MAX as u32) as u16),
                grayscale: false,
                blur: Some(*blur_amount),
                seed: None,
            };
            ImageCustom(opts).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::ImageUrlCustom {
            width,
            height,
            grayscale,
            blur_amount,
            seed,
        } => {
            use fake::faker::picsum::en::ImageCustom;
            let opts = fake::faker::impls::picsum::ImageOptions {
                width: Some((*width).min(u16::MAX as u32) as u16),
                height: Some((*height).min(u16::MAX as u32) as u16),
                grayscale: *grayscale,
                blur: *blur_amount,
                seed: seed.map(|s| s.to_string()),
            };
            ImageCustom(opts).fake_with_rng::<String, _>(rng)
        }

        // ========== 颜色类 ==========
        GeneratorConfig::HexColor => {
            use fake::faker::color::en::HexColor;
            HexColor().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::RgbColor => {
            use fake::faker::color::en::RgbColor;
            RgbColor().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::RgbaColor => {
            use fake::faker::color::en::RgbaColor;
            RgbaColor().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::HslColor => {
            use fake::faker::color::en::HslColor;
            HslColor().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::HslaColor => {
            use fake::faker::color::en::HslaColor;
            HslaColor().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Color => {
            use fake::faker::color::en::Color;
            Color().fake_with_rng::<String, _>(rng)
        }

        // ========== Ferroid ID ==========
        GeneratorConfig::FerroidULID => fake::ferroid::FerroidULID.fake_with_rng::<String, _>(rng),
        GeneratorConfig::FerroidTwitterId => {
            fake::ferroid::FerroidTwitterId.fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FerroidInstagramId => {
            fake::ferroid::FerroidInstagramId.fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FerroidMastodonId => {
            fake::ferroid::FerroidMastodonId.fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FerroidDiscordId => {
            fake::ferroid::FerroidDiscordId.fake_with_rng::<String, _>(rng)
        }

        // ========== 编码标准 ==========
        GeneratorConfig::Isbn => {
            use fake::faker::barcode::en::Isbn;
            Isbn().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Isbn10 => {
            use fake::faker::barcode::en::Isbn10;
            Isbn10().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::Isbn13 => {
            use fake::faker::barcode::en::Isbn13;
            Isbn13().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::RfcStatusCode => {
            use fake::faker::http::en::RfcStatusCode;
            RfcStatusCode().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::ValidStatusCode => {
            use fake::faker::http::en::ValidStatusCode;
            ValidStatusCode().fake_with_rng::<String, _>(rng)
        }

        // ========== 汽车/行政 ==========
        GeneratorConfig::LicencePlate => {
            let letters: String = (0..3)
                .map(|_| ((65..91).fake_with_rng::<u8, _>(rng)) as char)
                .collect();
            let digits: String = (0..4)
                .map(|_| ((48..58).fake_with_rng::<u8, _>(rng)) as char)
                .collect();
            format!("{}-{}", letters, digits)
        }
        GeneratorConfig::HealthInsuranceCode => {
            format!(
                "{}{}{}",
                (0..100).fake_with_rng::<u32, _>(rng),
                (0..100).fake_with_rng::<u32, _>(rng),
                (0..10000).fake_with_rng::<u32, _>(rng),
            )
        }

        // ========== Markdown（需要 Range<usize> 参数，返回 Vec<String> 或 String）==========
        GeneratorConfig::MarkdownItalicWord => {
            use fake::faker::markdown::en::ItalicWord;
            ItalicWord().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::MarkdownBoldWord => {
            use fake::faker::markdown::en::BoldWord;
            BoldWord().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::MarkdownLink => {
            use fake::faker::markdown::en::Link;
            Link().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::MarkdownBulletPoints => {
            use fake::faker::markdown::en::BulletPoints;
            let items: Vec<String> = BulletPoints(3..6).fake_with_rng(rng);
            items.join("\n")
        }
        GeneratorConfig::MarkdownListItems => {
            use fake::faker::markdown::en::ListItems;
            let items: Vec<String> = ListItems(3..6).fake_with_rng(rng);
            items.join("\n")
        }
        GeneratorConfig::MarkdownBlockQuoteSingle => {
            use fake::faker::markdown::en::BlockQuoteSingleLine;
            BlockQuoteSingleLine(1..3).fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::MarkdownBlockQuoteMulti => {
            use fake::faker::markdown::en::BlockQuoteMultiLine;
            let lines: Vec<String> = BlockQuoteMultiLine(2..5).fake_with_rng(rng);
            lines.join("\n")
        }
        GeneratorConfig::MarkdownCode => {
            use fake::faker::markdown::en::Code;
            Code(1..3).fake_with_rng::<String, _>(rng)
        }

        // ========== 约束类 ==========
        GeneratorConfig::ValueSet { values } => {
            let idx = (0..values.len()).fake_with_rng::<usize, _>(rng);
            values[idx].clone()
        }
        GeneratorConfig::Sequence { values, cycle } => {
            let idx = if *cycle {
                row_index % values.len()
            } else if row_index < values.len() {
                row_index
            } else {
                values.len() - 1
            };
            values[idx].clone()
        }
        GeneratorConfig::Weighted { choices } => {
            let total: f64 = choices.iter().map(|(_, w)| w).sum();
            let rand_val: f64 = (0.0..total).fake_with_rng(rng);
            let mut cumulative = 0.0;
            for (val, weight) in choices {
                cumulative += weight;
                if rand_val < cumulative {
                    return val.clone();
                }
            }
            choices.last().map(|(v, _)| v.clone()).unwrap_or_default()
        }
    }
}

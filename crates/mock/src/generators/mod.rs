use fake::RngExt;
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

/// 标准正态随机数（均值 0、标准差 1）：Box-Muller 变换。
///
/// 只依赖 `rand` 提供的均匀分布，避免为几个分布额外引入 `rand_distr` 依赖。
fn standard_normal(rng: &mut StdRng) -> f64 {
    // 取 1-U 而不是 U，避开 ln(0) 得到 -inf
    let u1 = 1.0 - rng.random::<f64>();
    let u2 = rng.random::<f64>();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Gamma 分布抽样（Marsaglia-Tsang 挤压法），要求 `shape > 0`。
///
/// Beta 分布由两个 Gamma 之比得到，所以必须支持 `shape < 1`：
/// 此时用提升变换 `x^(1/shape) · Γ(shape + 1)` 绕开主分支对 `shape ≥ 1` 的要求。
fn gamma_sample(shape: f64, rng: &mut StdRng) -> f64 {
    if shape < 1.0 {
        let u = 1.0 - rng.random::<f64>();
        let g = gamma_sample(shape + 1.0, rng);
        return g * u.powf(1.0 / shape);
    }
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = standard_normal(rng);
        let v = 1.0 + c * x;
        if v <= 0.0 {
            continue;
        }
        let v = v * v * v;
        let u: f64 = rng.random::<f64>();
        // 挤压判据：先试廉价的对数判据，落空再走完整判据
        if u < 1.0 - 0.0331 * x.powi(4) || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
            return d * v;
        }
    }
}

fn parse_date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .unwrap_or_else(|| {
            tracing::warn!("Mock: invalid date '{}', falling back to 2020-01-01", s);
            chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap_or_default()
        })
}

/// `DateTimeBefore` / `DateTimeAfter` 的另半边默认窗口（与旧的写死值一致）。
pub(super) const DEFAULT_DATETIME_MIN: &str = "2020-01-01T00:00:00Z";
pub(super) const DEFAULT_DATETIME_MAX: &str = "2030-12-31T23:59:59Z";

/// 工作时段窗口（当日分钟数）。
///
/// `start > end` 表示**跨零点**（夜班，如 `22:00~06:00`）；长度为零时退到默认窗口。
#[derive(Debug, Clone, Copy)]
struct WorkHours {
    start: u32,
    end: u32,
}

impl WorkHours {
    fn parse(start_text: &str, end_text: &str) -> Self {
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
    fn sample(&self, rng: &mut StdRng) -> chrono::NaiveTime {
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
pub(super) fn parse_hour_minutes(text: &str) -> Option<u32> {
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
struct CalendarArgs<'a> {
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
    fn new(
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

    fn calendar(&self) -> Option<WorkCalendar<'a>> {
        self.workdays_only
            .then(|| WorkCalendar::new(self.work_week, self.skip_dates, self.work_dates))
    }

    fn hours(&self) -> Option<WorkHours> {
        self.work_hours_only
            .then(|| WorkHours::parse(self.work_hour_start, self.work_hour_end))
    }
}

/// 区间随机时刻 + 列级日历参数（四个日期时间变体共用）。
fn datetime_with_calendar(
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
fn parse_start_datetime(label: &str, text: &str) -> chrono::NaiveDateTime {
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
fn advance_work_days(
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
fn datetime_between_with_calendar(
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
fn key_to_date(bytes: &[u8]) -> Option<chrono::NaiveDate> {
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
fn iso_key(date: chrono::NaiveDate) -> [u8; 10] {
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
struct WorkCalendar<'a> {
    /// 周一~周日，`true` = 上班
    week: [bool; 7],
    skip_dates: &'a [String],
    work_dates: &'a [String],
}

impl<'a> WorkCalendar<'a> {
    /// 从生成器字段构造。掩码非法时退到周一~周五（生成前护栏会拦住非法值，这里是兜底）。
    fn new(work_week: &str, skip_dates: &'a [String], work_dates: &'a [String]) -> Self {
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
    fn lists_empty(&self) -> bool {
        self.skip_dates.is_empty() && self.work_dates.is_empty()
    }

    fn list_has(list: &[String], key: &[u8]) -> bool {
        list.iter().any(|item| item.as_bytes() == key)
    }

    fn is_work_day(&self, date: chrono::NaiveDate) -> bool {
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
    fn snap_forward(&self, date: chrono::NaiveDate) -> chrono::NaiveDate {
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
    fn work_days_between(&self, start: chrono::NaiveDate, end: chrono::NaiveDate) -> i64 {
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
    fn nth_work_day(&self, start: chrono::NaiveDate, k: i64) -> Option<chrono::NaiveDate> {
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

/// 从正则表达式生成随机字符串（支持常见模式）
fn generate_from_regex(pattern: &str, rng: &mut StdRng) -> String {
    let mut result = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let len = chars.len();
    while i < len {
        match chars[i] {
            '\\' if i + 1 < len => {
                i += 1;
                match chars[i] {
                    'd' => result.push((b'0' + (0..10).fake_with_rng::<u8, _>(rng)) as char),
                    'w' => {
                        let pool =
                            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
                        let idx = (0..pool.len()).fake_with_rng::<usize, _>(rng);
                        result.push(pool[idx] as char);
                    }
                    's' => result.push(' '),
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    c => result.push(c),
                }
            }
            '[' => {
                let mut class_chars = Vec::new();
                i += 1;
                let mut negated = false;
                if i < len && chars[i] == '^' {
                    negated = true;
                    i += 1;
                }
                while i < len && chars[i] != ']' {
                    if i + 2 < len && chars[i + 1] == '-' {
                        let start = chars[i] as u32;
                        let end = chars[i + 2] as u32;
                        if end >= start {
                            for c in (start..=end).take(256) {
                                class_chars.push(char::from_u32(c).unwrap_or(' '));
                            }
                        }
                        i += 3;
                    } else {
                        class_chars.push(chars[i]);
                        i += 1;
                    }
                }
                if i < len {
                    i += 1;
                }
                if negated {
                    let full: Vec<char> = (32..127).filter_map(char::from_u32).collect();
                    class_chars = full
                        .into_iter()
                        .filter(|c| !class_chars.contains(c))
                        .collect();
                }
                if !class_chars.is_empty() {
                    let idx = (0..class_chars.len()).fake_with_rng::<usize, _>(rng);
                    result.push(class_chars[idx]);
                }
            }
            '{' => {
                i += 1;
                let mut num_str = String::new();
                while i < len && chars[i] != '}' && chars[i] != ',' {
                    num_str.push(chars[i]);
                    i += 1;
                }
                let min: usize = num_str.parse().unwrap_or(1);
                let mut max = min;
                if i < len && chars[i] == ',' {
                    i += 1;
                    num_str.clear();
                    while i < len && chars[i] != '}' {
                        num_str.push(chars[i]);
                        i += 1;
                    }
                    max = num_str.parse().unwrap_or(min);
                }
                if i < len {
                    i += 1;
                }
                let count = if max > min {
                    (min..=max).fake_with_rng::<usize, _>(rng)
                } else {
                    min
                };
                if let Some(last) = result.pop() {
                    for _ in 0..count {
                        result.push(last);
                    }
                }
            }
            '+' | '*' | '?' | '.' => {}
            '(' | ')' | '^' | '$' => {}
            c => result.push(c),
        }
        i += 1;
    }
    if result.is_empty() {
        Faker.fake_with_rng::<String, _>(rng)
    } else {
        result
    }
}

type TemplateGenFn = fn(&mut StdRng) -> String;

/// 模板字符串替换：{{name}} → 生成值
fn generate_from_template(template: &str, rng: &mut StdRng) -> String {
    let mut result = template.to_string();

    let replacements: &[(&str, TemplateGenFn)] = &[
        ("name", |r| {
            use fake::faker::name::zh_cn::Name;
            Name().fake_with_rng::<String, _>(r)
        }),
        ("first_name", |r| {
            use fake::faker::name::zh_cn::FirstName;
            FirstName().fake_with_rng::<String, _>(r)
        }),
        ("last_name", |r| {
            use fake::faker::name::zh_cn::LastName;
            LastName().fake_with_rng::<String, _>(r)
        }),
        ("email", |r| {
            use fake::faker::internet::en::SafeEmail;
            SafeEmail().fake_with_rng::<String, _>(r)
        }),
        ("uuid", |r| fake::uuid::UUIDv4.fake_with_rng::<String, _>(r)),
        ("word", |r| {
            use fake::faker::lorem::en::Word;
            Word().fake_with_rng::<String, _>(r)
        }),
        ("sentence", |r| {
            use fake::faker::lorem::en::Sentence;
            Sentence(3..8).fake_with_rng::<String, _>(r)
        }),
        ("phone", |r| {
            use fake::faker::phone_number::zh_cn::PhoneNumber;
            PhoneNumber().fake_with_rng::<String, _>(r)
        }),
        ("date", |r| {
            use fake::faker::chrono::en::Date;
            Date()
                .fake_with_rng::<chrono::NaiveDate, _>(r)
                .format("%Y-%m-%d")
                .to_string()
        }),
        ("datetime", |r| {
            fake::faker::chrono::en::DateTime()
                .fake_with_rng::<chrono::DateTime<chrono::Utc>, _>(r)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        }),
    ];

    for (key, gen_fn) in replacements {
        let placeholder = format!("{{{{{}}}}}", key);
        if result.contains(&placeholder) {
            let value = gen_fn(rng);
            result = result.replace(&placeholder, &value);
        }
    }

    // 处理 {{int:MIN-MAX}} 格式（手动解析，避免额外依赖）
    let mut int_result = String::new();
    let template_bytes = result.as_bytes();
    let mut pos = 0;
    let prefix = b"{{int:";
    while pos < template_bytes.len() {
        if pos + prefix.len() <= template_bytes.len()
            && &template_bytes[pos..pos + prefix.len()] == prefix
        {
            let start_idx = pos + prefix.len();
            let mut end_pos = start_idx;
            while end_pos < template_bytes.len()
                && template_bytes[end_pos] != b'-'
                && template_bytes[end_pos] != b'}'
            {
                end_pos += 1;
            }
            let min_str = std::str::from_utf8(&template_bytes[start_idx..end_pos]).unwrap_or("0");
            if end_pos < template_bytes.len() && template_bytes[end_pos] == b'-' {
                end_pos += 1;
                let val_start = end_pos;
                while end_pos < template_bytes.len() && template_bytes[end_pos] != b'}' {
                    end_pos += 1;
                }
                let max_str =
                    std::str::from_utf8(&template_bytes[val_start..end_pos]).unwrap_or("100");
                if end_pos < template_bytes.len() && template_bytes[end_pos] == b'}' {
                    end_pos += 1;
                }
                let min: i64 = min_str.parse().unwrap_or(0);
                let max: i64 = max_str.parse().unwrap_or(100);
                let val: i64 = if max >= min {
                    (min..=max).fake_with_rng::<i64, _>(rng)
                } else {
                    min
                };
                int_result.push_str(&val.to_string());
            } else {
                int_result.push_str(&result[pos..end_pos]);
            }
            pos = end_pos;
        } else {
            int_result.push(result.as_bytes()[pos] as char);
            pos += 1;
        }
    }
    result = int_result;

    result
}

#[cfg(test)]
mod tests;


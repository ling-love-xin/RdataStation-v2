use fake::rand::rngs::StdRng;
use fake::RngExt;
use fake::{Fake, Faker};

use crate::models::{GeneratorConfig, Locale};

pub(super) fn generate_cell(
    generator: &GeneratorConfig,
    rng: &mut StdRng,
    row_index: usize,
    _locale: &Locale,
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
        GeneratorConfig::Name => {
            use fake::faker::name::zh_cn::Name;
            Name().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::NameWithTitle => {
            use fake::faker::name::en::NameWithTitle;
            NameWithTitle().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::FirstName => {
            use fake::faker::name::zh_cn::FirstName;
            FirstName().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::LastName => {
            use fake::faker::name::zh_cn::LastName;
            LastName().fake_with_rng::<String, _>(rng)
        }
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
        GeneratorConfig::PhoneNumber => {
            use fake::faker::phone_number::zh_cn::PhoneNumber;
            PhoneNumber().fake_with_rng::<String, _>(rng)
        }
        GeneratorConfig::CellNumber => {
            use fake::faker::phone_number::zh_cn::CellNumber;
            CellNumber().fake_with_rng::<String, _>(rng)
        }
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
        GeneratorConfig::City => {
            use fake::faker::address::zh_cn::CityName;
            CityName().fake_with_rng::<String, _>(rng)
        }
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
        GeneratorConfig::StreetName => {
            use fake::faker::address::zh_cn::StreetName;
            StreetName().fake_with_rng::<String, _>(rng)
        }
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
        GeneratorConfig::DateTime { min, max } => datetime_between(min, max, rng),
        GeneratorConfig::DateTimeBetween {
            start: min,
            end: max,
            workdays_only,
            work_hours_only,
            work_week,
            skip_dates,
            work_dates,
        } => {
            let calendar =
                workdays_only.then(|| WorkCalendar::new(work_week, skip_dates, work_dates));
            datetime_between_with_calendar(min, max, calendar.as_ref(), *work_hours_only, rng)
        }
        GeneratorConfig::DateTimeBefore { before } => {
            let min = "2020-01-01T00:00:00Z";
            datetime_between(min, before, rng)
        }
        GeneratorConfig::DateTimeAfter { after } => {
            let max = "2030-12-31T23:59:59Z";
            datetime_between(after, max, rng)
        }
        GeneratorConfig::Date { min, max } => {
            use fake::faker::chrono::en::{Date, DateTimeBetween};
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
        GeneratorConfig::CompanyName => {
            use fake::faker::company::zh_cn::CompanyName;
            CompanyName().fake_with_rng::<String, _>(rng)
        }
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
        GeneratorConfig::ForeignKey { values } => {
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

fn datetime_between(min: &str, max: &str, rng: &mut StdRng) -> String {
    use chrono::{DateTime, Utc};
    use fake::faker::chrono::en::DateTimeBetween;

    let default_start = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
        .map(|d| d.to_utc())
        .unwrap_or_default();
    let default_end = DateTime::parse_from_rfc3339("2030-12-31T23:59:59Z")
        .map(|d| d.to_utc())
        .unwrap_or_default();

    let s = DateTime::parse_from_rfc3339(min)
        .map(|d| d.to_utc())
        .unwrap_or(default_start);
    let e = DateTime::parse_from_rfc3339(max)
        .map(|d| d.to_utc())
        .unwrap_or(default_end);

    DateTimeBetween(s, e)
        .fake_with_rng::<DateTime<Utc>, _>(rng)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// 工作时段（仅工作时段开关的固定窗口，与参数标签写法一致）。
const WORK_HOUR_START: u32 = 9;
const WORK_HOUR_END: u32 = 18;

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

/// 区间随机时刻 + 可选的工作日历约束（列级参数，见 `WorkCalendar`）。
///
/// 落点规则：
/// 1. 先在 `[min, max]` 里均匀取一个时刻；
/// 2. 「仅工作日」：落在休息日就重抽（最多 30 次），仍不行就顺延到下一个工作日；
/// 3. 「仅工作时段」：把日内时刻改到 09:00~18:00；
/// 4. 最后夹回 `[min, max]`——区间端点优先于日历约束（窗口比日历窄时至少不越界）。
fn datetime_between_with_calendar(
    min: &str,
    max: &str,
    calendar: Option<&WorkCalendar<'_>>,
    work_hours_only: bool,
    rng: &mut StdRng,
) -> String {
    use chrono::{DateTime, Utc};
    use fake::faker::chrono::en::DateTimeBetween;

    let default_start = DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
        .map(|d| d.to_utc())
        .unwrap_or_default();
    let default_end = DateTime::parse_from_rfc3339("2030-12-31T23:59:59Z")
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
    if work_hours_only {
        let minutes = (WORK_HOUR_START * 60..WORK_HOUR_END * 60).fake_with_rng::<u32, _>(rng);
        if let Some(time) = chrono::NaiveTime::from_num_seconds_from_midnight_opt(minutes * 60, 0) {
            picked = chrono::NaiveDateTime::new(picked.date(), time);
        }
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

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use fake::rand::rngs::StdRng;
    use fake::rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(42)
    }

    #[test]
    fn test_auto_increment() {
        let generator = GeneratorConfig::AutoIncrement { start: 1, step: 1 };
        assert_eq!(generate_cell(&generator, &mut rng(), 0, &Locale::ZhCn), "1");
        assert_eq!(generate_cell(&generator, &mut rng(), 5, &Locale::ZhCn), "6");
        assert_eq!(generate_cell(&generator, &mut rng(), 9, &Locale::ZhCn), "10");
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
            assert!(val.parse::<u8>().is_ok(), "digit should be numeric, got: {}", val);
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
        let generator = GeneratorConfig::DateTime {
            min: "2020-01-01".to_string(),
            max: "2025-12-31".to_string(),
        };
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
}

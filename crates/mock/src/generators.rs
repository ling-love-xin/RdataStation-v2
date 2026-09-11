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
            let u1: f64 = rng.random();
            let u1 = u1.max(1e-10);
            let u2: f64 = rng.random();
            let z = (-2.0_f64 * u1.ln()).sqrt() * (2.0_f64 * std::f64::consts::PI * u2).cos();
            (mean + std_dev * z).to_string()
        }
        GeneratorConfig::LogNormal { median, dispersion } => {
            let u1: f64 = rng.random();
            let u1 = u1.max(1e-10);
            let u2: f64 = rng.random();
            let z = (-2.0_f64 * u1.ln()).sqrt() * (2.0_f64 * std::f64::consts::PI * u2).cos();
            (median.ln() + dispersion * z).exp().to_string()
        }
        GeneratorConfig::RandomWalk {
            start,
            step,
            volatility,
        } => {
            let base = *start + (row_index as f64) * *step;
            let noise_scale = (row_index as f64).sqrt().max(0.0) * *volatility;
            let u1: f64 = rng.random();
            let u1 = u1.max(1e-10);
            let u2: f64 = rng.random();
            let noise = (-2.0_f64 * u1.ln()).sqrt()
                * (2.0_f64 * std::f64::consts::PI * u2).cos()
                * noise_scale;
            (base + noise).to_string()
        }
        GeneratorConfig::Boolean { ratio } => {
            use fake::faker::boolean::en::Boolean;
            Boolean(*ratio).fake_with_rng::<bool, _>(rng).to_string()
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
        GeneratorConfig::DateTime { min, max }
        | GeneratorConfig::DateTimeBetween {
            start: min,
            end: max,
        } => datetime_between(min, max, rng),
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
        } => {
            let dt = chrono::NaiveDateTime::parse_from_str(start, "%Y-%m-%d %H:%M:%S")
                .unwrap_or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(
                        &format!("{} 00:00:00", start),
                        "%Y-%m-%d %H:%M:%S",
                    )
                    .inspect_err(|e| {
                        tracing::warn!(
                            "SequentialDate: invalid start date '{}', falling back to epoch: {}",
                            start,
                            e
                        )
                    })
                    .unwrap_or_default()
                });
            let new_dt = dt + chrono::Duration::seconds(*step_seconds as i64 * row_index as i64);
            new_dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        GeneratorConfig::SequentialDateWithGaps {
            start,
            step_seconds,
            miss_probability,
        } => {
            let roll: f64 = rng.random();
            if roll < *miss_probability {
                return String::new();
            }
            let dt = chrono::NaiveDateTime::parse_from_str(start, "%Y-%m-%d %H:%M:%S")
                .unwrap_or_else(|_| {
                    chrono::NaiveDateTime::parse_from_str(
                        &format!("{} 00:00:00", start),
                        "%Y-%m-%d %H:%M:%S",
                    )
                    .inspect_err(|e| {
                        tracing::warn!(
                            "SequentialDateWithGaps: invalid start date '{}', falling back to epoch: {}",
                            start, e
                        )
                    })
                    .unwrap_or_default()
                });
            let total_steps = (row_index as f64 * (1.0 - *miss_probability)).max(0.0) as i64;
            let new_dt = dt + chrono::Duration::seconds(*step_seconds as i64 * total_steps);
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
}

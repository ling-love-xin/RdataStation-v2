use crate::error::{MockError, MockResult};
use crate::models::{ColumnDataType, ColumnMappingResponse, GeneratorConfig};

#[derive(Debug, Clone)]
pub struct ColumnMappingRule {
    pub patterns: &'static [&'static str],
    pub generator: fn() -> GeneratorConfig,
    pub confidence: &'static str,
    pub sample_value: &'static str,
}

/// 源库列类型字符串 → mock 列数据类型（宽松匹配，未知类型回退 `Text`）。
///
/// 这是**唯一**的类型字符串入口，服务于两类调用方：
/// - 源库元数据（`INFORMATION_SCHEMA` / 导航树）给出的原始类型串，含长度精度，
///   如 `VARCHAR(64)` / `DECIMAL(12,2)` / `TIMESTAMP`；
/// - 内部规范名（模板使用的全小写名：`integer` / `decimal` / `datetime` …）。
///
/// 两类输入都按大写前缀/全等匹配，因此同一张表不会因为「谁调用的」而得到不同列类型。
/// 未知类型回退 `Text`（DDL 渲染同为 `VARCHAR`，见 `ColumnDataType::to_duckdb_type`）。
///
/// 返回 `Result` 的原因是 `DECIMAL(p,s)` 的两类边界（见 [`parse_decimal`]）：
/// 标度大于精度（`s > p`）是无解的坏类型，直接报错比「建表时被 DuckDB 拒绝」更好懂；
/// 其余路径全部 `Ok`（未知 / 带修饰的类型仍走回退，不因签名变化收紧旧行为）。
pub fn parse_data_type(data_type: &str) -> MockResult<ColumnDataType> {
    let up = data_type.trim().to_uppercase();
    // 先剁掉参数部分，再取首词：
    // `VARCHAR(64)` → `VARCHAR`；`INT(11) UNSIGNED` → `INT`；`DOUBLE PRECISION` → `DOUBLE`；
    // `TIMESTAMP WITH TIME ZONE` → `TIMESTAMP`（这样带修饰的类型不会落到末行回退）
    let word = up
        .split('(')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string();
    if word.starts_with("VARCHAR") || word.starts_with("CHAR") {
        Ok(ColumnDataType::Varchar { length: None })
    } else if word == "TINYINT"
        || word == "SMALLINT"
        || word == "MEDIUMINT"
        || word == "INT"
        || word == "INTEGER"
        || word == "INT2"
        || word == "INT4"
    {
        Ok(ColumnDataType::Integer)
    } else if word == "BIGINT" || word == "INT8" {
        Ok(ColumnDataType::BigInt)
    } else if word.starts_with("DECIMAL")
        || word.starts_with("NUMERIC")
        || word.starts_with("NUMBER")
    {
        parse_decimal(&up)
    } else if word == "BOOLEAN" || word == "BOOL" {
        Ok(ColumnDataType::Boolean)
    } else if word == "DOUBLE" || word == "FLOAT8" {
        Ok(ColumnDataType::Double)
    } else if word == "FLOAT" || word == "FLOAT4" || word == "REAL" {
        Ok(ColumnDataType::Float)
    } else if word == "DATE" {
        Ok(ColumnDataType::Date)
    } else if word.starts_with("TIMESTAMP") || word.starts_with("DATETIME") {
        Ok(ColumnDataType::Timestamp)
    } else if word == "UUID" {
        Ok(ColumnDataType::Uuid)
    } else if word == "BLOB" || word == "BYTEA" || word.starts_with("BINARY") {
        Ok(ColumnDataType::Blob)
    } else {
        // `TEXT` / `CLOB` / `STRING` 与真正认不出的类型一律归到 `Text`
        // （DDL 同为 `VARCHAR`；两者在 mock 里不需要区分）
        Ok(ColumnDataType::Text)
    }
}

/// `DECIMAL(p,s)`（含 `NUMERIC(p,s)` / `NUMBER(p,s)`）的**完整**解析。
///
/// - 无参数 / 参数残缺（如 `DECIMAL(10)`）→ 默认 `DECIMAL(18,2)`（旧行为，不收紧）；
/// - `s > p` → 报错：标度超过精度在 DuckDB 里直接拒建表，这里把错误提前到「导入 / 读回」时；
/// - `p == 0` → 报错：零精度不是合法的十进制列；
/// - `p > 38` → **显式降级 `DOUBLE`** 并记警告：DuckDB `DECIMAL` 精度上限是 38，
///   写 `DECIMAL(65,30)` 会被拒，与其让列悄悄落成 VARCHAR，不如明确降级成双精度浮点。
fn parse_decimal(raw: &str) -> MockResult<ColumnDataType> {
    let Some(open) = raw.find('(') else {
        return Ok(ColumnDataType::Decimal {
            precision: 18,
            scale: 2,
        });
    };
    let Some(close) = raw.rfind(')') else {
        return Ok(ColumnDataType::Decimal {
            precision: 18,
            scale: 2,
        });
    };
    let parts: Vec<&str> = raw[open + 1..close].split(',').map(str::trim).collect();
    // 只认「两参」形式（与计划口径一致）：参数残缺 / 多参 / 非数字都回到默认 18,2
    if parts.len() != 2 {
        return Ok(ColumnDataType::Decimal {
            precision: 18,
            scale: 2,
        });
    }
    let (Ok(precision), Ok(scale)) = (parts[0].parse::<u8>(), parts[1].parse::<u8>()) else {
        return Ok(ColumnDataType::Decimal {
            precision: 18,
            scale: 2,
        });
    };
    if scale > precision || precision == 0 {
        return Err(MockError::InvalidColumn(format!(
            "DECIMAL({precision},{scale}) 的标度大于精度（或精度为 0）：标度不能超过精度，如 DECIMAL(10,2)"
        )));
    }
    if precision > 38 {
        tracing::warn!(
            "Mock: DECIMAL({precision},{scale}) 的精度超出 DuckDB 上限（38），已降级为 DOUBLE"
        );
        return Ok(ColumnDataType::Double);
    }
    Ok(ColumnDataType::Decimal {
        precision,
        scale,
    })
}

pub struct ColumnMapper;

impl ColumnMapper {
    pub fn infer(column_name: &str, data_type: &ColumnDataType) -> ColumnMappingResponse {
        let name_lower = column_name.to_lowercase();
        let rule = Self::find_rule(&name_lower, data_type);
        let generator = (rule.generator)();

        ColumnMappingResponse {
            column_name: column_name.to_string(),
            generator: generator.clone(),
            confidence: rule.confidence.to_string(),
            sample_value: rule.sample_value.to_string(),
        }
    }

    fn find_rule(name_lower: &str, data_type: &ColumnDataType) -> ColumnMappingRule {
        // 精确匹配表
        let exact_rules = Self::exact_rules();
        for rule in &exact_rules {
            for pattern in rule.patterns {
                if name_lower == *pattern {
                    return rule.clone();
                }
            }
        }

        // 后缀/前缀匹配
        for rule in &exact_rules {
            for pattern in rule.patterns {
                if name_lower.ends_with(pattern) {
                    return rule.clone();
                }
                if name_lower.starts_with(pattern) {
                    return rule.clone();
                }
            }
        }

        // 子串匹配（模糊）
        let fuzzy_rules = Self::fuzzy_rules();
        for rule in &fuzzy_rules {
            for pattern in rule.patterns {
                if name_lower.contains(pattern) {
                    return rule.clone();
                }
            }
        }

        // 兜底：根据数据类型推断
        Self::fallback_by_type(data_type)
    }

    fn exact_rules() -> Vec<ColumnMappingRule> {
        vec![
            ColumnMappingRule {
                patterns: &["id", "_id", "_key"],
                generator: || GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                confidence: "high",
                sample_value: "1, 2, 3...",
            },
            ColumnMappingRule {
                patterns: &["uuid", "guid"],
                generator: || GeneratorConfig::UuidV4,
                confidence: "high",
                sample_value: "550e8400-e29b-41d4-a716-446655440000",
            },
            ColumnMappingRule {
                patterns: &["uuid_v1"],
                generator: || GeneratorConfig::UuidV1,
                confidence: "high",
                sample_value: "d3cfd3e0-...",
            },
            ColumnMappingRule {
                patterns: &["ulid", "sortable_id"],
                generator: || GeneratorConfig::FerroidULID,
                confidence: "high",
                sample_value: "01ARZ3NDEK...",
            },
            ColumnMappingRule {
                patterns: &["twitter_id", "x_id"],
                generator: || GeneratorConfig::FerroidTwitterId,
                confidence: "high",
                sample_value: "182736453728192",
            },
            ColumnMappingRule {
                patterns: &["discord_id"],
                generator: || GeneratorConfig::FerroidDiscordId,
                confidence: "high",
                sample_value: "1234567890123456",
            },
            ColumnMappingRule {
                patterns: &["email", "_email"],
                generator: || GeneratorConfig::SafeEmail,
                confidence: "high",
                sample_value: "zhangsan@example.com",
            },
            ColumnMappingRule {
                patterns: &["free_email", "public_email"],
                generator: || GeneratorConfig::FreeEmail,
                confidence: "high",
                sample_value: "zhangsan@gmail.com",
            },
            ColumnMappingRule {
                patterns: &["email_provider", "mail_provider"],
                generator: || GeneratorConfig::FreeEmailProvider,
                confidence: "high",
                sample_value: "gmail.com",
            },
            ColumnMappingRule {
                patterns: &["domain", "domain_suffix", "tld"],
                generator: || GeneratorConfig::DomainSuffix,
                confidence: "high",
                sample_value: "com",
            },
            ColumnMappingRule {
                patterns: &["full_name", "fullname"],
                generator: || GeneratorConfig::NameWithTitle,
                confidence: "high",
                sample_value: "Dr. 张三",
            },
            ColumnMappingRule {
                patterns: &["title", "honorific"],
                generator: || GeneratorConfig::Title,
                confidence: "high",
                sample_value: "Mr.",
            },
            ColumnMappingRule {
                patterns: &["name", "_name"],
                generator: || GeneratorConfig::Name,
                confidence: "high",
                sample_value: "张三",
            },
            ColumnMappingRule {
                patterns: &["username", "login"],
                generator: || GeneratorConfig::Username,
                confidence: "high",
                sample_value: "john_doe",
            },
            ColumnMappingRule {
                patterns: &["password", "pwd", "pass", "secret"],
                generator: || GeneratorConfig::Password { min: 8, max: 20 },
                confidence: "high",
                sample_value: "********",
            },
            ColumnMappingRule {
                patterns: &["cell", "cellphone", "cell_number"],
                generator: || GeneratorConfig::CellNumber,
                confidence: "high",
                sample_value: "13912345678",
            },
            ColumnMappingRule {
                patterns: &["phone", "mobile", "tel", "phone_number", "telephone"],
                generator: || GeneratorConfig::PhoneNumber,
                confidence: "high",
                sample_value: "138-xxxx-xxxx",
            },
            ColumnMappingRule {
                patterns: &["street", "street_name"],
                generator: || GeneratorConfig::StreetName,
                confidence: "high",
                sample_value: "长安街",
            },
            ColumnMappingRule {
                patterns: &["address", "addr"],
                generator: || GeneratorConfig::StreetName,
                confidence: "medium",
                sample_value: "北京市朝阳区xx路",
            },
            ColumnMappingRule {
                patterns: &["secondary_address", "addr2", "address2", "secondary_addr"],
                generator: || GeneratorConfig::SecondaryAddress,
                confidence: "high",
                sample_value: "Apt. 123",
            },
            ColumnMappingRule {
                patterns: &["address_type", "addr_type", "secondary_type"],
                generator: || GeneratorConfig::SecondaryAddressType,
                confidence: "high",
                sample_value: "Suite",
            },
            ColumnMappingRule {
                patterns: &["city"],
                generator: || GeneratorConfig::City,
                confidence: "high",
                sample_value: "北京市",
            },
            ColumnMappingRule {
                patterns: &["country_code"],
                generator: || GeneratorConfig::CountryCode,
                confidence: "high",
                sample_value: "CN",
            },
            ColumnMappingRule {
                patterns: &["country", "nation"],
                generator: || GeneratorConfig::CountryName,
                confidence: "high",
                sample_value: "中华人民共和国",
            },
            ColumnMappingRule {
                patterns: &["province", "state", "state_name"],
                generator: || GeneratorConfig::StateName,
                confidence: "high",
                sample_value: "广东省",
            },
            ColumnMappingRule {
                patterns: &["postcode", "post_code", "postal_code"],
                generator: || GeneratorConfig::PostCode,
                confidence: "high",
                sample_value: "100000",
            },
            ColumnMappingRule {
                patterns: &["zipcode", "zip", "zip_code"],
                generator: || GeneratorConfig::ZipCode,
                confidence: "high",
                sample_value: "100000",
            },
            ColumnMappingRule {
                patterns: &["latitude", "lat"],
                generator: || GeneratorConfig::Latitude,
                confidence: "high",
                sample_value: "39.9042",
            },
            ColumnMappingRule {
                patterns: &["longitude", "lng", "lon"],
                generator: || GeneratorConfig::Longitude,
                confidence: "high",
                sample_value: "116.4074",
            },
            ColumnMappingRule {
                patterns: &["geohash"],
                generator: || GeneratorConfig::Geohash { precision: 6 },
                confidence: "high",
                sample_value: "wx4g0b",
            },
            ColumnMappingRule {
                patterns: &["timezone", "time_zone"],
                generator: || GeneratorConfig::TimeZone,
                confidence: "high",
                sample_value: "Asia/Shanghai",
            },
            ColumnMappingRule {
                patterns: &["mac", "mac_address"],
                generator: || GeneratorConfig::MacAddress,
                confidence: "high",
                sample_value: "00:1A:2B:3C:4D:5E",
            },
            ColumnMappingRule {
                patterns: &["url", "link", "href", "website"],
                generator: || GeneratorConfig::Url,
                confidence: "high",
                sample_value: "https://www.example.com",
            },
            ColumnMappingRule {
                patterns: &["ip", "ip_address"],
                generator: || GeneratorConfig::IpAddress,
                confidence: "high",
                sample_value: "192.168.1.1",
            },
            ColumnMappingRule {
                patterns: &["ipv4", "ip_v4"],
                generator: || GeneratorConfig::IPv4,
                confidence: "high",
                sample_value: "192.168.1.1",
            },
            ColumnMappingRule {
                patterns: &["ipv6", "ip_v6"],
                generator: || GeneratorConfig::IPv6,
                confidence: "high",
                sample_value: "::1",
            },
            ColumnMappingRule {
                patterns: &["user_agent", "ua"],
                generator: || GeneratorConfig::UserAgent,
                confidence: "high",
                sample_value: "Mozilla/5.0...",
            },
            ColumnMappingRule {
                patterns: &["mime", "mime_type"],
                generator: || GeneratorConfig::MimeType,
                confidence: "high",
                sample_value: "application/json",
            },
            ColumnMappingRule {
                patterns: &["semver", "version"],
                generator: || GeneratorConfig::Semver,
                confidence: "high",
                sample_value: "1.2.3",
            },
            ColumnMappingRule {
                patterns: &["file_path", "path"],
                generator: || GeneratorConfig::FilePath,
                confidence: "high",
                sample_value: "/usr/local/bin/app",
            },
            ColumnMappingRule {
                patterns: &["file_name"],
                generator: || GeneratorConfig::FileName,
                confidence: "high",
                sample_value: "document.pdf",
            },
            ColumnMappingRule {
                patterns: &["file_ext", "extension"],
                generator: || GeneratorConfig::FileExtension,
                confidence: "high",
                sample_value: ".pdf",
            },
            ColumnMappingRule {
                patterns: &[
                    "image_url",
                    "img_url",
                    "pic_url",
                    "avatar_url",
                    "photo_url",
                    "thumbnail_url",
                ],
                generator: || GeneratorConfig::ImageUrl {
                    width: 400,
                    height: 300,
                },
                confidence: "high",
                sample_value: "https://picsum.photos/400/300",
            },
            ColumnMappingRule {
                patterns: &["avatar", "photo", "image", "img", "pic", "thumbnail"],
                generator: || GeneratorConfig::ImageUrl {
                    width: 200,
                    height: 200,
                },
                confidence: "medium",
                sample_value: "https://picsum.photos/200/200",
            },
            ColumnMappingRule {
                patterns: &["word", "single_word"],
                generator: || GeneratorConfig::Word,
                confidence: "high",
                sample_value: "example",
            },
            ColumnMappingRule {
                patterns: &["hex_color", "hex"],
                generator: || GeneratorConfig::HexColor,
                confidence: "high",
                sample_value: "#ff5733",
            },
            ColumnMappingRule {
                patterns: &["rgb", "rgb_color"],
                generator: || GeneratorConfig::RgbColor,
                confidence: "high",
                sample_value: "rgb(255, 87, 51)",
            },
            ColumnMappingRule {
                patterns: &["company", "corp", "organization"],
                generator: || GeneratorConfig::CompanyName,
                confidence: "high",
                sample_value: "XX科技有限公司",
            },
            ColumnMappingRule {
                patterns: &["job", "job_title"],
                generator: || GeneratorConfig::JobTitle,
                confidence: "high",
                sample_value: "高级工程师",
            },
            ColumnMappingRule {
                patterns: &["profession"],
                generator: || GeneratorConfig::Profession,
                confidence: "high",
                sample_value: "软件工程师",
            },
            ColumnMappingRule {
                patterns: &["industry"],
                generator: || GeneratorConfig::Industry,
                confidence: "high",
                sample_value: "信息技术",
            },
            ColumnMappingRule {
                patterns: &["bic", "swift"],
                generator: || GeneratorConfig::Bic,
                confidence: "high",
                sample_value: "BKCHCNBJ",
            },
            ColumnMappingRule {
                patterns: &["isin"],
                generator: || GeneratorConfig::Isin,
                confidence: "high",
                sample_value: "US0378331005",
            },
            ColumnMappingRule {
                patterns: &["isbn"],
                generator: || GeneratorConfig::Isbn,
                confidence: "high",
                sample_value: "978-3-16-148410-0",
            },
            ColumnMappingRule {
                patterns: &["credit_card", "card_no", "card_number"],
                generator: || GeneratorConfig::CreditCardNumber,
                confidence: "high",
                sample_value: "4539-xxxx-xxxx-xxxx",
            },
            ColumnMappingRule {
                patterns: &["amount", "price", "fee", "cost", "total"],
                generator: || GeneratorConfig::RandomDecimal {
                    min: 0.01,
                    max: 99999.99,
                    scale: 2,
                },
                confidence: "high",
                sample_value: "1234.56",
            },
            ColumnMappingRule {
                patterns: &["quantity", "qty", "count", "num"],
                generator: || GeneratorConfig::RandomInt { min: 1, max: 1000 },
                confidence: "high",
                sample_value: "42",
            },
            ColumnMappingRule {
                patterns: &["is_active", "enabled", "flag", "is_"],
                generator: || GeneratorConfig::Boolean { ratio: 50 },
                confidence: "high",
                sample_value: "true",
            },
            ColumnMappingRule {
                patterns: &["ratio", "percentage"],
                generator: || GeneratorConfig::RandomFloat {
                    min: 0.0,
                    max: 100.0,
                    precision: 2,
                },
                confidence: "high",
                sample_value: "67.89",
            },
            ColumnMappingRule {
                patterns: &["created_at", "create_time", "create_date"],
                generator: || {
                    GeneratorConfig::date_time("2024-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                },
                confidence: "high",
                sample_value: "2024-06-15 08:30:00",
            },
            ColumnMappingRule {
                patterns: &["updated_at", "update_time", "modified_at"],
                generator: || {
                    GeneratorConfig::date_time("2024-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                },
                confidence: "high",
                sample_value: "2024-12-01 14:45:00",
            },
            ColumnMappingRule {
                patterns: &["birth_date", "dob", "birthday"],
                generator: || GeneratorConfig::date("1950-01-01", "2005-12-31"),
                confidence: "high",
                sample_value: "1990-05-20",
            },
            ColumnMappingRule {
                patterns: &["first_name"],
                generator: || GeneratorConfig::FirstName,
                confidence: "high",
                sample_value: "张",
            },
            ColumnMappingRule {
                patterns: &["last_name"],
                generator: || GeneratorConfig::LastName,
                confidence: "high",
                sample_value: "三",
            },
            ColumnMappingRule {
                patterns: &["age"],
                generator: || GeneratorConfig::RandomInt { min: 18, max: 80 },
                confidence: "high",
                sample_value: "35",
            },
            ColumnMappingRule {
                patterns: &["gender", "sex"],
                generator: || GeneratorConfig::ValueSet {
                    values: vec!["男".to_string(), "女".to_string()],
                },
                confidence: "high",
                sample_value: "男",
            },
            ColumnMappingRule {
                patterns: &["plate", "license_plate"],
                generator: || GeneratorConfig::LicencePlate,
                confidence: "high",
                sample_value: "京A12345",
            },
            ColumnMappingRule {
                patterns: &["insurance", "health_id", "health_insurance"],
                generator: || GeneratorConfig::HealthInsuranceCode,
                confidence: "high",
                sample_value: "123456789012",
            },
            ColumnMappingRule {
                patterns: &["http_status", "status_code"],
                generator: || GeneratorConfig::ValidStatusCode,
                confidence: "high",
                sample_value: "200",
            },
            ColumnMappingRule {
                patterns: &["color"],
                generator: || GeneratorConfig::HexColor,
                confidence: "high",
                sample_value: "#ff5733",
            },
        ]
    }

    fn fuzzy_rules() -> Vec<ColumnMappingRule> {
        vec![
            ColumnMappingRule {
                patterns: &["status", "state"],
                generator: || GeneratorConfig::ValueSet {
                    values: vec![
                        "active".to_string(),
                        "pending".to_string(),
                        "cancelled".to_string(),
                    ],
                },
                confidence: "medium",
                sample_value: "active",
            },
            ColumnMappingRule {
                patterns: &["type", "category", "kind"],
                generator: || GeneratorConfig::Words { min: 1, max: 2 },
                confidence: "medium",
                sample_value: "annual",
            },
            ColumnMappingRule {
                patterns: &["description", "desc", "body", "content", "text"],
                generator: || GeneratorConfig::Paragraph { count: 1 },
                confidence: "medium",
                sample_value: "Lorem ipsum dolor sit amet...",
            },
            ColumnMappingRule {
                patterns: &["note", "remark", "memo", "comment"],
                generator: || GeneratorConfig::Words { min: 3, max: 8 },
                confidence: "medium",
                sample_value: "这是一条备注信息",
            },
            ColumnMappingRule {
                patterns: &["markdown", "md"],
                generator: || GeneratorConfig::MarkdownBlockQuoteMulti,
                confidence: "medium",
                sample_value: "> quote...",
            },
            ColumnMappingRule {
                patterns: &["buzzword_middle"],
                generator: || GeneratorConfig::BuzzwordMiddle,
                confidence: "high",
                sample_value: "dynamic",
            },
            ColumnMappingRule {
                patterns: &["buzzword_tail"],
                generator: || GeneratorConfig::BuzzwordTail,
                confidence: "high",
                sample_value: "solutions",
            },
            ColumnMappingRule {
                patterns: &["bs_verb"],
                generator: || GeneratorConfig::BsVerb,
                confidence: "high",
                sample_value: "implement",
            },
            ColumnMappingRule {
                patterns: &["bs_adj"],
                generator: || GeneratorConfig::BsAdj,
                confidence: "high",
                sample_value: "enterprise",
            },
            ColumnMappingRule {
                patterns: &["bs_noun"],
                generator: || GeneratorConfig::BsNoun,
                confidence: "high",
                sample_value: "framework",
            },
            ColumnMappingRule {
                patterns: &["bs", "buzzword_phrase", "bs_phrase"],
                generator: || GeneratorConfig::Bs,
                confidence: "high",
                sample_value: "innovate next-generation platforms",
            },
            ColumnMappingRule {
                patterns: &["info", "data"],
                generator: || GeneratorConfig::Words { min: 3, max: 5 },
                confidence: "low",
                sample_value: "随机文本",
            },
        ]
    }

    fn fallback_by_type(data_type: &ColumnDataType) -> ColumnMappingRule {
        match data_type {
            ColumnDataType::Integer | ColumnDataType::BigInt => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::RandomInt { min: 0, max: 10000 },
                confidence: "low",
                sample_value: "42",
            },
            ColumnDataType::Float | ColumnDataType::Double => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::RandomFloat {
                    min: 0.0,
                    max: 10000.0,
                    precision: 2,
                },
                confidence: "low",
                sample_value: "123.45",
            },
            ColumnDataType::Decimal { .. } => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::RandomDecimal {
                    min: 0.0,
                    max: 10000.0,
                    scale: 2,
                },
                confidence: "low",
                sample_value: "1234.56",
            },
            ColumnDataType::Boolean => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::Boolean { ratio: 50 },
                confidence: "low",
                sample_value: "true",
            },
            ColumnDataType::Date => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::date("2020-01-01", "2025-12-31"),
                confidence: "low",
                sample_value: "2024-01-15",
            },
            ColumnDataType::DateTime | ColumnDataType::Timestamp => ColumnMappingRule {
                patterns: &[],
                generator: || {
                    GeneratorConfig::date_time("2020-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                },
                confidence: "low",
                sample_value: "2024-01-15 08:30:00",
            },
            ColumnDataType::Uuid => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::UuidV4,
                confidence: "low",
                sample_value: "550e8400-...",
            },
            // 未知列名的文本列：`Sentence(1..3)`——**上界必须大于下界**：
            // `Sentence(min..max)` 走 `rand` 的 `random_range`，空区间会直接 panic（`1..1` 就是空区间），
            // 而 panic 发生在持有内存库连接锁期间，会把整个进程的 mock 生成打坏。
            ColumnDataType::Varchar { .. } | ColumnDataType::Text => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::Sentence { min: 1, max: 3 },
                confidence: "low",
                sample_value: "自然语言句子",
            },
            ColumnDataType::Blob => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::HexColor,
                confidence: "low",
                sample_value: "#1A2B3C",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_exact_match_id() {
        let resp = ColumnMapper::infer("user_id", &ColumnDataType::BigInt);
        assert_eq!(resp.confidence, "high");
        assert!(matches!(
            resp.generator,
            GeneratorConfig::AutoIncrement { .. }
        ));
    }

    #[test]
    fn test_infer_exact_match_email() {
        let resp = ColumnMapper::infer("email", &ColumnDataType::Varchar { length: None });
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::SafeEmail));
    }

    #[test]
    fn test_infer_suffix_match_name() {
        let resp = ColumnMapper::infer("customer_name", &ColumnDataType::Varchar { length: None });
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::Name));
    }

    #[test]
    fn test_infer_prefix_match_is_active() {
        let resp = ColumnMapper::infer("is_deleted", &ColumnDataType::Boolean);
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::Boolean { .. }));
    }

    #[test]
    fn test_infer_fuzzy_match_description() {
        let resp = ColumnMapper::infer("product_desc", &ColumnDataType::Varchar { length: None });
        assert_eq!(resp.confidence, "medium");
        assert!(matches!(resp.generator, GeneratorConfig::Paragraph { .. }));
    }

    #[test]
    fn test_infer_fallback_integer() {
        let resp = ColumnMapper::infer("xyz_field", &ColumnDataType::Integer);
        assert_eq!(resp.confidence, "low");
        assert!(matches!(resp.generator, GeneratorConfig::RandomInt { .. }));
    }

    #[test]
    fn test_infer_fallback_float() {
        let resp = ColumnMapper::infer("unknown", &ColumnDataType::Float);
        assert_eq!(resp.confidence, "low");
        assert!(matches!(
            resp.generator,
            GeneratorConfig::RandomFloat { .. }
        ));
    }

    #[test]
    fn test_infer_fallback_boolean() {
        let resp = ColumnMapper::infer("flag_field", &ColumnDataType::Boolean);
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::Boolean { .. }));
    }

    #[test]
    fn test_infer_fallback_uuid() {
        let resp = ColumnMapper::infer("some_uuid_column", &ColumnDataType::Uuid);
        assert_eq!(resp.confidence, "low");
        assert!(matches!(resp.generator, GeneratorConfig::UuidV4));
    }

    #[test]
    fn test_infer_fallback_date() {
        let resp = ColumnMapper::infer("some_date", &ColumnDataType::Date);
        assert_eq!(resp.confidence, "low");
        assert!(matches!(resp.generator, GeneratorConfig::Date { .. }));
    }

    #[test]
    fn test_infer_fallback_datetime() {
        let resp = ColumnMapper::infer("some_time", &ColumnDataType::DateTime);
        assert_eq!(resp.confidence, "low");
        assert!(matches!(resp.generator, GeneratorConfig::DateTime { .. }));
    }

    #[test]
    fn test_infer_amount_field() {
        let resp = ColumnMapper::infer(
            "total_amount",
            &ColumnDataType::Decimal {
                precision: 12,
                scale: 2,
            },
        );
        assert_eq!(resp.confidence, "high");
        assert!(matches!(
            resp.generator,
            GeneratorConfig::RandomDecimal { .. }
        ));
    }

    #[test]
    fn test_infer_count_field() {
        let resp = ColumnMapper::infer("item_count", &ColumnDataType::Integer);
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::RandomInt { .. }));
    }

    #[test]
    fn test_infer_created_at() {
        let resp = ColumnMapper::infer("created_at", &ColumnDataType::DateTime);
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::DateTime { .. }));
    }

    #[test]
    fn test_infer_updated_at() {
        let resp = ColumnMapper::infer("update_time", &ColumnDataType::Timestamp);
        assert_eq!(resp.confidence, "high");
        assert!(matches!(resp.generator, GeneratorConfig::DateTime { .. }));
    }

    #[test]
    fn test_infer_timestamp_with_no_match() {
        let resp = ColumnMapper::infer("ts", &ColumnDataType::Timestamp);
        assert_eq!(resp.confidence, "low");
        assert!(matches!(resp.generator, GeneratorConfig::DateTime { .. }));
    }

    // ==================== 类型字符串解析 ====================

    #[test]
    fn test_parse_data_type_integer_family() {
        assert!(matches!(
            parse_data_type("INTEGER").unwrap(),
            ColumnDataType::Integer
        ));
        assert!(matches!(
            parse_data_type("int").unwrap(),
            ColumnDataType::Integer
        ));
        assert!(matches!(
            parse_data_type("TINYINT").unwrap(),
            ColumnDataType::Integer
        ));
        assert!(matches!(
            parse_data_type("SMALLINT").unwrap(),
            ColumnDataType::Integer
        ));
        assert!(matches!(
            parse_data_type("BIGINT").unwrap(),
            ColumnDataType::BigInt
        ));
    }

    #[test]
    fn test_parse_data_type_parameterized_types() {
        // 源库元数据带长度/精度：仍应识别到真实类型，而不是回退到 VARCHAR。
        assert!(matches!(
            parse_data_type("VARCHAR(64)").unwrap(),
            ColumnDataType::Varchar { .. }
        ));
        // DECIMAL 精度标度**完整解析**（不再写死 18,2）
        assert_eq!(
            parse_data_type("DECIMAL(12,2)").unwrap(),
            ColumnDataType::Decimal {
                precision: 12,
                scale: 2
            }
        );
        assert!(matches!(
            parse_data_type("NUMERIC").unwrap(),
            ColumnDataType::Decimal { .. }
        ));
    }

    /// 带修饰的类型串（各家驱动给的形状不一）必须都认出来——
    /// 落回 `Text` 会让金额 / 时间列静默降级为文本（下游 `SUM` / 时间运算直接用不了）。
    #[test]
    fn test_parse_data_type_recognizes_decorated_types() {
        for (raw, expected) in [
            ("NUMERIC(10,3)", "decimal"),
            ("NUMBER(10,2)", "decimal"),
            ("DECIMAL128(38,0)", "decimal"),
            ("DOUBLE PRECISION", "double"),
            ("FLOAT8", "double"),
            ("FLOAT4", "float"),
            ("TIMESTAMP WITH TIME ZONE", "timestamp"),
            ("TIMESTAMP WITHOUT TIME ZONE", "timestamp"),
            ("TIMESTAMPTZ", "timestamp"),
            ("CHARACTER VARYING(32)", "varchar"),
            ("INT(11) UNSIGNED", "integer"),
            ("INT8", "bigint"),
            ("MEDIUMINT", "integer"),
            ("BYTEA", "blob"),
        ] {
            let actual = match parse_data_type(raw).unwrap() {
                ColumnDataType::Decimal { .. } => "decimal",
                ColumnDataType::Double => "double",
                ColumnDataType::Float => "float",
                ColumnDataType::Timestamp => "timestamp",
                ColumnDataType::Varchar { .. } => "varchar",
                ColumnDataType::Integer => "integer",
                ColumnDataType::BigInt => "bigint",
                ColumnDataType::Blob => "blob",
                ColumnDataType::Text => "text",
                other => panic!("{raw} 落到了意外类型：{other:?}"),
            };
            assert_eq!(actual, expected, "{raw}");
        }
    }

    /// S2：`DECIMAL(p,s)` 四种边界——常规透传 / 上限内透传 / 超上限降级 / 非法组合报错。
    #[test]
    fn test_parse_data_type_decimal_precision_scale() {
        // 常规：精度标度原样透传
        assert_eq!(
            parse_data_type("DECIMAL(10,4)").unwrap(),
            ColumnDataType::Decimal {
                precision: 10,
                scale: 4
            }
        );
        // 上限（38）内：原样透传
        assert_eq!(
            parse_data_type("DECIMAL(38,10)").unwrap(),
            ColumnDataType::Decimal {
                precision: 38,
                scale: 10
            }
        );
        // 无参数：仍是旧默认（不收紧既有行为）
        assert_eq!(
            parse_data_type("DECIMAL").unwrap(),
            ColumnDataType::Decimal {
                precision: 18,
                scale: 2
            }
        );
        // 单参形式：不满足两参，按旧默认
        assert_eq!(
            parse_data_type("DECIMAL(10)").unwrap(),
            ColumnDataType::Decimal {
                precision: 18,
                scale: 2
            }
        );
    }

    /// S2：精度超过 DuckDB 上限（38）→ 显式降级 `DOUBLE`（写 65,30 会被库拒，降级比落 VARCHAR 强）。
    #[test]
    fn test_parse_data_type_decimal_over_precision_degrades_to_double() {
        assert_eq!(parse_data_type("DECIMAL(65,30)").unwrap(), ColumnDataType::Double);
        assert_eq!(parse_data_type("NUMERIC(100,10)").unwrap(), ColumnDataType::Double);
    }

    /// S2：标度大于精度（或精度为 0）是坏类型，报错而不是悄悄写死默认。
    #[test]
    fn test_parse_data_type_decimal_illegal_combos_error() {
        let err = parse_data_type("DECIMAL(4,10)")
            .expect_err("标度大于精度必须报错")
            .to_string();
        assert!(err.contains("DECIMAL(4,10)"), "{err}");
        assert!(err.contains("标度"), "{err}");

        let err_zero = parse_data_type("DECIMAL(0,2)")
            .expect_err("精度为 0 必须报错")
            .to_string();
        assert!(err_zero.contains("DECIMAL(0,2)"), "{err_zero}");
    }

    #[test]
    fn test_parse_data_type_numeric_and_boolean() {
        assert!(matches!(
            parse_data_type("DOUBLE").unwrap(),
            ColumnDataType::Double
        ));
        assert!(matches!(
            parse_data_type("FLOAT").unwrap(),
            ColumnDataType::Float
        ));
        assert!(matches!(
            parse_data_type("REAL").unwrap(),
            ColumnDataType::Float
        ));
        assert!(matches!(
            parse_data_type("BOOLEAN").unwrap(),
            ColumnDataType::Boolean
        ));
        assert!(matches!(
            parse_data_type("bool").unwrap(),
            ColumnDataType::Boolean
        ));
    }

    #[test]
    fn test_parse_data_type_time_and_misc() {
        assert!(matches!(
            parse_data_type("DATE").unwrap(),
            ColumnDataType::Date
        ));
        assert!(matches!(
            parse_data_type("TIMESTAMP").unwrap(),
            ColumnDataType::Timestamp
        ));
        assert!(matches!(
            parse_data_type("DATETIME").unwrap(),
            ColumnDataType::Timestamp
        ));
        assert!(matches!(
            parse_data_type("UUID").unwrap(),
            ColumnDataType::Uuid
        ));
        assert!(matches!(
            parse_data_type("BLOB").unwrap(),
            ColumnDataType::Blob
        ));
        assert!(matches!(
            parse_data_type("  text  ").unwrap(),
            ColumnDataType::Text
        ));
    }

    #[test]
    fn test_parse_data_type_unknown_falls_back_to_text() {
        assert!(matches!(
            parse_data_type("WEIRD").unwrap(),
            ColumnDataType::Text
        ));
        assert!(matches!(
            parse_data_type("").unwrap(),
            ColumnDataType::Text
        ));
    }

    /// 内部规范名（模板使用）必须与源库类型串走同一套判定。
    #[test]
    fn test_parse_data_type_canonical_names() {
        assert!(matches!(
            parse_data_type("integer").unwrap(),
            ColumnDataType::Integer
        ));
        assert!(matches!(
            parse_data_type("bigint").unwrap(),
            ColumnDataType::BigInt
        ));
        assert!(matches!(
            parse_data_type("float").unwrap(),
            ColumnDataType::Float
        ));
        assert!(matches!(
            parse_data_type("double").unwrap(),
            ColumnDataType::Double
        ));
        assert!(matches!(
            parse_data_type("decimal").unwrap(),
            ColumnDataType::Decimal { .. }
        ));
        assert!(matches!(
            parse_data_type("boolean").unwrap(),
            ColumnDataType::Boolean
        ));
        assert!(matches!(
            parse_data_type("varchar").unwrap(),
            ColumnDataType::Varchar { .. }
        ));
        assert!(matches!(
            parse_data_type("text").unwrap(),
            ColumnDataType::Text
        ));
        assert!(matches!(
            parse_data_type("date").unwrap(),
            ColumnDataType::Date
        ));
        assert!(matches!(
            parse_data_type("datetime").unwrap(),
            ColumnDataType::Timestamp
        ));
        assert!(matches!(
            parse_data_type("timestamp").unwrap(),
            ColumnDataType::Timestamp
        ));
        assert!(matches!(
            parse_data_type("uuid").unwrap(),
            ColumnDataType::Uuid
        ));
        assert!(matches!(
            parse_data_type("blob").unwrap(),
            ColumnDataType::Blob
        ));
    }
}

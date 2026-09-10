use crate::mock::models::{ColumnDataType, ColumnMappingResponse, GeneratorConfig};

#[derive(Debug, Clone)]
pub struct ColumnMappingRule {
    pub patterns: &'static [&'static str],
    pub generator: fn() -> GeneratorConfig,
    pub confidence: &'static str,
    pub sample_value: &'static str,
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
                generator: || GeneratorConfig::DateTime {
                    min: "2024-01-01T00:00:00Z".to_string(),
                    max: "2025-12-31T23:59:59Z".to_string(),
                },
                confidence: "high",
                sample_value: "2024-06-15 08:30:00",
            },
            ColumnMappingRule {
                patterns: &["updated_at", "update_time", "modified_at"],
                generator: || GeneratorConfig::DateTime {
                    min: "2024-01-01T00:00:00Z".to_string(),
                    max: "2025-12-31T23:59:59Z".to_string(),
                },
                confidence: "high",
                sample_value: "2024-12-01 14:45:00",
            },
            ColumnMappingRule {
                patterns: &["birth_date", "dob", "birthday"],
                generator: || GeneratorConfig::Date {
                    min: "1950-01-01".to_string(),
                    max: "2005-12-31".to_string(),
                },
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
                generator: || GeneratorConfig::ForeignKey {
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
                generator: || GeneratorConfig::ForeignKey {
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
                generator: || GeneratorConfig::Date {
                    min: "2020-01-01".to_string(),
                    max: "2025-12-31".to_string(),
                },
                confidence: "low",
                sample_value: "2024-01-15",
            },
            ColumnDataType::DateTime | ColumnDataType::Timestamp => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::DateTime {
                    min: "2020-01-01T00:00:00Z".to_string(),
                    max: "2025-12-31T23:59:59Z".to_string(),
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
            ColumnDataType::Varchar { .. } | ColumnDataType::Text => ColumnMappingRule {
                patterns: &[],
                generator: || GeneratorConfig::Sentence { min: 1, max: 1 },
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
}

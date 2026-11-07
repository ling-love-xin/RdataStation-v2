//! 生成器目录（M7）——由 `models.rs` 的 `GeneratorConfig` 穷尽派生。
//!
//! 本文件由 `tools/gen_mock_generator_catalog.py` 生成，**不要手改**：
//! 新增 / 删除 `GeneratorConfig` 变体后重跑脚本，`spec_of` 的穷尽 match 会强制补齐。
//! 注意：`is_method_spec` / `methods_in_type`（方法级划分，2026-10-31 手加）是
//! 脚本不生成的独立函数，重跑脚本后需手动保留。
//!
//! 用途：面板展示（中文标签）、生成器选择器（分类浏览 + 搜索）、参数编辑表单。

use crate::models::{ColumnDataType, GeneratorConfig};
use serde::{Deserialize, Serialize};
use specta::Type;

/// 生成器分类（面板分组用；顺序即展示顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum GeneratorCategory {
    Numeric,
    Text,
    Markdown,
    Person,
    Address,
    DateTime,
    Business,
    Finance,
    Tech,
    Image,
    Color,
    FerroidId,
    Code,
    Vehicle,
    Constraint,
}

impl GeneratorCategory {
    /// 全部分类（展示顺序）
    pub const ALL: [GeneratorCategory; 15] = [
        GeneratorCategory::Numeric,
        GeneratorCategory::Text,
        GeneratorCategory::Markdown,
        GeneratorCategory::Person,
        GeneratorCategory::Address,
        GeneratorCategory::DateTime,
        GeneratorCategory::Business,
        GeneratorCategory::Finance,
        GeneratorCategory::Tech,
        GeneratorCategory::Image,
        GeneratorCategory::Color,
        GeneratorCategory::FerroidId,
        GeneratorCategory::Code,
        GeneratorCategory::Vehicle,
        GeneratorCategory::Constraint,
    ];

    /// 中文标签
    pub fn label(self) -> &'static str {
        match self {
            GeneratorCategory::Numeric => "数值",
            GeneratorCategory::Text => "文本",
            GeneratorCategory::Markdown => "Markdown",
            GeneratorCategory::Person => "个人信息",
            GeneratorCategory::Address => "地址与网络标识",
            GeneratorCategory::DateTime => "日期时间",
            GeneratorCategory::Business => "商业",
            GeneratorCategory::Finance => "金融",
            GeneratorCategory::Tech => "网络与技术",
            GeneratorCategory::Image => "图片",
            GeneratorCategory::Color => "颜色",
            GeneratorCategory::FerroidId => "Ferroid ID",
            GeneratorCategory::Code => "标准编码",
            GeneratorCategory::Vehicle => "汽车与行政",
            GeneratorCategory::Constraint => "约束",
        }
    }
}

/// 参数类型（决定参数编辑器渲染何种输入控件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// 整数（可带上下限）
    Int,
    /// 浮点数
    Float,
    /// 文本
    Text,
    /// 布尔
    Bool,
    /// 复杂参数（字符串列表 / 加权选项）——面板用多行文本编辑（一行一项）
    Complex,
}

/// 一个可编辑参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamField {
    /// 字段名（与 `GeneratorConfig` 变体字段同名；JSON 补丁按此键写回）
    pub key: &'static str,
    /// 中文标签
    pub label: &'static str,
    /// 参数类型
    pub kind: ParamKind,
}

/// 生成器规格。
///
/// `PartialEq` 是 `SearchableVec` 下拉项（`SearchableListItem::Value: PartialEq`）要求的；
/// 本文件由脚本生成，脚本重跑会丢手加 derive（届时补回即可）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorSpec {
    /// 稳定标识（snake_case，与 v1 前端 `GeneratorType` 对齐）
    pub name: &'static str,
    /// 中文标签
    pub label: &'static str,
    /// 分类
    pub category: GeneratorCategory,
    /// 可编辑参数（顺序即表单顺序）
    pub params: &'static [ParamField],
}


// 规格常量子模块
mod specs;


// ==================== 查询入口 ====================

/// 当前配置对应的规格（穷尽 match：新增变体将编译失败，强制补规格）。
pub fn spec_of(config: &GeneratorConfig) -> &'static GeneratorSpec {
    match config {
        GeneratorConfig::AutoIncrement { .. } => &specs::SPEC_AUTO_INCREMENT,
        GeneratorConfig::RandomInt { .. } => &specs::SPEC_RANDOM_INT,
        GeneratorConfig::RandomFloat { .. } => &specs::SPEC_RANDOM_FLOAT,
        GeneratorConfig::RandomDecimal { .. } => &specs::SPEC_RANDOM_DECIMAL,
        GeneratorConfig::Digit => &specs::SPEC_DIGIT,
        GeneratorConfig::NumberWithFormat { .. } => &specs::SPEC_NUMBER_WITH_FORMAT,
        GeneratorConfig::Normal { .. } => &specs::SPEC_NORMAL,
        GeneratorConfig::LogNormal { .. } => &specs::SPEC_LOG_NORMAL,
        GeneratorConfig::RandomWalk { .. } => &specs::SPEC_RANDOM_WALK,
        GeneratorConfig::Boolean { .. } => &specs::SPEC_BOOLEAN,
        GeneratorConfig::Poisson { .. } => &specs::SPEC_POISSON,
        GeneratorConfig::Exponential { .. } => &specs::SPEC_EXPONENTIAL,
        GeneratorConfig::Pareto { .. } => &specs::SPEC_PARETO,
        GeneratorConfig::Beta { .. } => &specs::SPEC_BETA,
        GeneratorConfig::Binomial { .. } => &specs::SPEC_BINOMIAL,
        GeneratorConfig::TimeSeries { .. } => &specs::SPEC_TIME_SERIES,
        GeneratorConfig::Constant { .. } => &specs::SPEC_CONSTANT,
        GeneratorConfig::Words { .. } => &specs::SPEC_WORDS,
        GeneratorConfig::Sentence { .. } => &specs::SPEC_SENTENCE,
        GeneratorConfig::Sentences { .. } => &specs::SPEC_SENTENCES,
        GeneratorConfig::Paragraph { .. } => &specs::SPEC_PARAGRAPH,
        GeneratorConfig::Paragraphs { .. } => &specs::SPEC_PARAGRAPHS,
        GeneratorConfig::Word => &specs::SPEC_WORD,
        GeneratorConfig::Regex { .. } => &specs::SPEC_REGEX,
        GeneratorConfig::Template { .. } => &specs::SPEC_TEMPLATE,
        GeneratorConfig::MarkdownItalicWord => &specs::SPEC_MARKDOWN_ITALIC_WORD,
        GeneratorConfig::MarkdownBoldWord => &specs::SPEC_MARKDOWN_BOLD_WORD,
        GeneratorConfig::MarkdownLink => &specs::SPEC_MARKDOWN_LINK,
        GeneratorConfig::MarkdownBulletPoints => &specs::SPEC_MARKDOWN_BULLET_POINTS,
        GeneratorConfig::MarkdownListItems => &specs::SPEC_MARKDOWN_LIST_ITEMS,
        GeneratorConfig::MarkdownBlockQuoteSingle => &specs::SPEC_MARKDOWN_BLOCK_QUOTE_SINGLE,
        GeneratorConfig::MarkdownBlockQuoteMulti => &specs::SPEC_MARKDOWN_BLOCK_QUOTE_MULTI,
        GeneratorConfig::MarkdownCode => &specs::SPEC_MARKDOWN_CODE,
        GeneratorConfig::Name => &specs::SPEC_NAME,
        GeneratorConfig::NameWithTitle => &specs::SPEC_NAME_WITH_TITLE,
        GeneratorConfig::FirstName => &specs::SPEC_FIRST_NAME,
        GeneratorConfig::LastName => &specs::SPEC_LAST_NAME,
        GeneratorConfig::Title => &specs::SPEC_TITLE,
        GeneratorConfig::Suffix => &specs::SPEC_SUFFIX,
        GeneratorConfig::Email => &specs::SPEC_EMAIL,
        GeneratorConfig::SafeEmail => &specs::SPEC_SAFE_EMAIL,
        GeneratorConfig::FreeEmailProvider => &specs::SPEC_FREE_EMAIL_PROVIDER,
        GeneratorConfig::DomainSuffix => &specs::SPEC_DOMAIN_SUFFIX,
        GeneratorConfig::FreeEmail => &specs::SPEC_FREE_EMAIL,
        GeneratorConfig::PhoneNumber => &specs::SPEC_PHONE_NUMBER,
        GeneratorConfig::CellNumber => &specs::SPEC_CELL_NUMBER,
        GeneratorConfig::Username => &specs::SPEC_USERNAME,
        GeneratorConfig::Password { .. } => &specs::SPEC_PASSWORD,
        GeneratorConfig::Country => &specs::SPEC_COUNTRY,
        GeneratorConfig::CountryCode => &specs::SPEC_COUNTRY_CODE,
        GeneratorConfig::CountryName => &specs::SPEC_COUNTRY_NAME,
        GeneratorConfig::City => &specs::SPEC_CITY,
        GeneratorConfig::CityPrefix => &specs::SPEC_CITY_PREFIX,
        GeneratorConfig::CitySuffix => &specs::SPEC_CITY_SUFFIX,
        GeneratorConfig::StateName => &specs::SPEC_STATE_NAME,
        GeneratorConfig::StateAbbr => &specs::SPEC_STATE_ABBR,
        GeneratorConfig::StreetName => &specs::SPEC_STREET_NAME,
        GeneratorConfig::StreetSuffix => &specs::SPEC_STREET_SUFFIX,
        GeneratorConfig::ZipCode => &specs::SPEC_ZIP_CODE,
        GeneratorConfig::PostCode => &specs::SPEC_POST_CODE,
        GeneratorConfig::BuildingNumber => &specs::SPEC_BUILDING_NUMBER,
        GeneratorConfig::SecondaryAddress => &specs::SPEC_SECONDARY_ADDRESS,
        GeneratorConfig::SecondaryAddressType => &specs::SPEC_SECONDARY_ADDRESS_TYPE,
        GeneratorConfig::Latitude => &specs::SPEC_LATITUDE,
        GeneratorConfig::Longitude => &specs::SPEC_LONGITUDE,
        GeneratorConfig::Geohash { .. } => &specs::SPEC_GEOHASH,
        GeneratorConfig::TimeZone => &specs::SPEC_TIME_ZONE,
        GeneratorConfig::IpAddress => &specs::SPEC_IP_ADDRESS,
        GeneratorConfig::IPv4 => &specs::SPEC_I_PV4,
        GeneratorConfig::IPv6 => &specs::SPEC_I_PV6,
        GeneratorConfig::IP => &specs::SPEC_I_P,
        GeneratorConfig::MacAddress => &specs::SPEC_MAC_ADDRESS,
        GeneratorConfig::DateTime { .. } => &specs::SPEC_DATE_TIME,
        GeneratorConfig::DateTimeBefore { .. } => &specs::SPEC_DATE_TIME_BEFORE,
        GeneratorConfig::DateTimeAfter { .. } => &specs::SPEC_DATE_TIME_AFTER,
        GeneratorConfig::DateTimeBetween { .. } => &specs::SPEC_DATE_TIME_BETWEEN,
        GeneratorConfig::Date { .. } => &specs::SPEC_DATE,
        GeneratorConfig::Time => &specs::SPEC_TIME,
        GeneratorConfig::Duration => &specs::SPEC_DURATION,
        GeneratorConfig::SequentialDate { .. } => &specs::SPEC_SEQUENTIAL_DATE,
        GeneratorConfig::SequentialDateWithGaps { .. } => &specs::SPEC_SEQUENTIAL_DATE_WITH_GAPS,
        GeneratorConfig::CompanyName => &specs::SPEC_COMPANY_NAME,
        GeneratorConfig::CompanySuffix => &specs::SPEC_COMPANY_SUFFIX,
        GeneratorConfig::JobTitle => &specs::SPEC_JOB_TITLE,
        GeneratorConfig::Profession => &specs::SPEC_PROFESSION,
        GeneratorConfig::Industry => &specs::SPEC_INDUSTRY,
        GeneratorConfig::Seniority => &specs::SPEC_SENIORITY,
        GeneratorConfig::Field => &specs::SPEC_FIELD,
        GeneratorConfig::Position => &specs::SPEC_POSITION,
        GeneratorConfig::Buzzword => &specs::SPEC_BUZZWORD,
        GeneratorConfig::BuzzwordMiddle => &specs::SPEC_BUZZWORD_MIDDLE,
        GeneratorConfig::BuzzwordTail => &specs::SPEC_BUZZWORD_TAIL,
        GeneratorConfig::CatchPhrase => &specs::SPEC_CATCH_PHRASE,
        GeneratorConfig::BsVerb => &specs::SPEC_BS_VERB,
        GeneratorConfig::BsAdj => &specs::SPEC_BS_ADJ,
        GeneratorConfig::BsNoun => &specs::SPEC_BS_NOUN,
        GeneratorConfig::Bs => &specs::SPEC_BS,
        GeneratorConfig::CurrencyCode => &specs::SPEC_CURRENCY_CODE,
        GeneratorConfig::CurrencyName => &specs::SPEC_CURRENCY_NAME,
        GeneratorConfig::CurrencySymbol => &specs::SPEC_CURRENCY_SYMBOL,
        GeneratorConfig::Bic => &specs::SPEC_BIC,
        GeneratorConfig::Isin => &specs::SPEC_ISIN,
        GeneratorConfig::CreditCardNumber => &specs::SPEC_CREDIT_CARD_NUMBER,
        GeneratorConfig::UuidV1 => &specs::SPEC_UUID_V1,
        GeneratorConfig::UuidV3 => &specs::SPEC_UUID_V3,
        GeneratorConfig::UuidV4 => &specs::SPEC_UUID_V4,
        GeneratorConfig::UuidV5 => &specs::SPEC_UUID_V5,
        GeneratorConfig::Url => &specs::SPEC_URL,
        GeneratorConfig::UserAgent => &specs::SPEC_USER_AGENT,
        GeneratorConfig::MimeType => &specs::SPEC_MIME_TYPE,
        GeneratorConfig::Semver => &specs::SPEC_SEMVER,
        GeneratorConfig::SemverStable => &specs::SPEC_SEMVER_STABLE,
        GeneratorConfig::SemverUnstable => &specs::SPEC_SEMVER_UNSTABLE,
        GeneratorConfig::FilePath => &specs::SPEC_FILE_PATH,
        GeneratorConfig::FileName => &specs::SPEC_FILE_NAME,
        GeneratorConfig::FileExtension => &specs::SPEC_FILE_EXTENSION,
        GeneratorConfig::DirPath => &specs::SPEC_DIR_PATH,
        GeneratorConfig::ImageUrl { .. } => &specs::SPEC_IMAGE_URL,
        GeneratorConfig::ImageUrlWithSeed { .. } => &specs::SPEC_IMAGE_URL_WITH_SEED,
        GeneratorConfig::ImageUrlGrayscale { .. } => &specs::SPEC_IMAGE_URL_GRAYSCALE,
        GeneratorConfig::ImageUrlBlur { .. } => &specs::SPEC_IMAGE_URL_BLUR,
        GeneratorConfig::ImageUrlCustom { .. } => &specs::SPEC_IMAGE_URL_CUSTOM,
        GeneratorConfig::HexColor => &specs::SPEC_HEX_COLOR,
        GeneratorConfig::RgbColor => &specs::SPEC_RGB_COLOR,
        GeneratorConfig::RgbaColor => &specs::SPEC_RGBA_COLOR,
        GeneratorConfig::HslColor => &specs::SPEC_HSL_COLOR,
        GeneratorConfig::HslaColor => &specs::SPEC_HSLA_COLOR,
        GeneratorConfig::Color => &specs::SPEC_COLOR,
        GeneratorConfig::FerroidULID => &specs::SPEC_FERROID_U_L_I_D,
        GeneratorConfig::FerroidTwitterId => &specs::SPEC_FERROID_TWITTER_ID,
        GeneratorConfig::FerroidInstagramId => &specs::SPEC_FERROID_INSTAGRAM_ID,
        GeneratorConfig::FerroidMastodonId => &specs::SPEC_FERROID_MASTODON_ID,
        GeneratorConfig::FerroidDiscordId => &specs::SPEC_FERROID_DISCORD_ID,
        GeneratorConfig::Isbn => &specs::SPEC_ISBN,
        GeneratorConfig::Isbn10 => &specs::SPEC_ISBN10,
        GeneratorConfig::Isbn13 => &specs::SPEC_ISBN13,
        GeneratorConfig::RfcStatusCode => &specs::SPEC_RFC_STATUS_CODE,
        GeneratorConfig::ValidStatusCode => &specs::SPEC_VALID_STATUS_CODE,
        GeneratorConfig::LicencePlate => &specs::SPEC_LICENCE_PLATE,
        GeneratorConfig::HealthInsuranceCode => &specs::SPEC_HEALTH_INSURANCE_CODE,
        GeneratorConfig::ValueSet { .. } => &specs::SPEC_FOREIGN_KEY,
        GeneratorConfig::Sequence { .. } => &specs::SPEC_SEQUENCE,
        GeneratorConfig::Weighted { .. } => &specs::SPEC_WEIGHTED,
    }
}

/// 全部生成器规格（选择器浏览用；顺序 = 分类顺序 + 变体声明顺序）。
pub fn all_specs() -> &'static [&'static GeneratorSpec] {
    ALL_SPECS
}

/// 规格表（static：避免每次调用构造临时数组）。
static ALL_SPECS: &[&GeneratorSpec] = &[
    &specs::SPEC_AUTO_INCREMENT,
    &specs::SPEC_RANDOM_INT,
    &specs::SPEC_RANDOM_FLOAT,
    &specs::SPEC_RANDOM_DECIMAL,
    &specs::SPEC_DIGIT,
    &specs::SPEC_NUMBER_WITH_FORMAT,
    &specs::SPEC_NORMAL,
    &specs::SPEC_LOG_NORMAL,
    &specs::SPEC_RANDOM_WALK,
    &specs::SPEC_BOOLEAN,
    &specs::SPEC_POISSON,
    &specs::SPEC_EXPONENTIAL,
    &specs::SPEC_PARETO,
    &specs::SPEC_BETA,
    &specs::SPEC_BINOMIAL,
    &specs::SPEC_TIME_SERIES,
    &specs::SPEC_CONSTANT,
    &specs::SPEC_WORDS,
    &specs::SPEC_SENTENCE,
    &specs::SPEC_SENTENCES,
    &specs::SPEC_PARAGRAPH,
    &specs::SPEC_PARAGRAPHS,
    &specs::SPEC_WORD,
    &specs::SPEC_REGEX,
    &specs::SPEC_TEMPLATE,
    &specs::SPEC_MARKDOWN_ITALIC_WORD,
    &specs::SPEC_MARKDOWN_BOLD_WORD,
    &specs::SPEC_MARKDOWN_LINK,
    &specs::SPEC_MARKDOWN_BULLET_POINTS,
    &specs::SPEC_MARKDOWN_LIST_ITEMS,
    &specs::SPEC_MARKDOWN_BLOCK_QUOTE_SINGLE,
    &specs::SPEC_MARKDOWN_BLOCK_QUOTE_MULTI,
    &specs::SPEC_MARKDOWN_CODE,
    &specs::SPEC_NAME,
    &specs::SPEC_NAME_WITH_TITLE,
    &specs::SPEC_FIRST_NAME,
    &specs::SPEC_LAST_NAME,
    &specs::SPEC_TITLE,
    &specs::SPEC_SUFFIX,
    &specs::SPEC_EMAIL,
    &specs::SPEC_SAFE_EMAIL,
    &specs::SPEC_FREE_EMAIL_PROVIDER,
    &specs::SPEC_DOMAIN_SUFFIX,
    &specs::SPEC_FREE_EMAIL,
    &specs::SPEC_PHONE_NUMBER,
    &specs::SPEC_CELL_NUMBER,
    &specs::SPEC_USERNAME,
    &specs::SPEC_PASSWORD,
    &specs::SPEC_COUNTRY,
    &specs::SPEC_COUNTRY_CODE,
    &specs::SPEC_COUNTRY_NAME,
    &specs::SPEC_CITY,
    &specs::SPEC_CITY_PREFIX,
    &specs::SPEC_CITY_SUFFIX,
    &specs::SPEC_STATE_NAME,
    &specs::SPEC_STATE_ABBR,
    &specs::SPEC_STREET_NAME,
    &specs::SPEC_STREET_SUFFIX,
    &specs::SPEC_ZIP_CODE,
    &specs::SPEC_POST_CODE,
    &specs::SPEC_BUILDING_NUMBER,
    &specs::SPEC_SECONDARY_ADDRESS,
    &specs::SPEC_SECONDARY_ADDRESS_TYPE,
    &specs::SPEC_LATITUDE,
    &specs::SPEC_LONGITUDE,
    &specs::SPEC_GEOHASH,
    &specs::SPEC_TIME_ZONE,
    &specs::SPEC_IP_ADDRESS,
    &specs::SPEC_I_PV4,
    &specs::SPEC_I_PV6,
    &specs::SPEC_I_P,
    &specs::SPEC_MAC_ADDRESS,
    &specs::SPEC_DATE_TIME,
    &specs::SPEC_DATE_TIME_BEFORE,
    &specs::SPEC_DATE_TIME_AFTER,
    &specs::SPEC_DATE_TIME_BETWEEN,
    &specs::SPEC_DATE,
    &specs::SPEC_TIME,
    &specs::SPEC_DURATION,
    &specs::SPEC_SEQUENTIAL_DATE,
    &specs::SPEC_SEQUENTIAL_DATE_WITH_GAPS,
    &specs::SPEC_COMPANY_NAME,
    &specs::SPEC_COMPANY_SUFFIX,
    &specs::SPEC_JOB_TITLE,
    &specs::SPEC_PROFESSION,
    &specs::SPEC_INDUSTRY,
    &specs::SPEC_SENIORITY,
    &specs::SPEC_FIELD,
    &specs::SPEC_POSITION,
    &specs::SPEC_BUZZWORD,
    &specs::SPEC_BUZZWORD_MIDDLE,
    &specs::SPEC_BUZZWORD_TAIL,
    &specs::SPEC_CATCH_PHRASE,
    &specs::SPEC_BS_VERB,
    &specs::SPEC_BS_ADJ,
    &specs::SPEC_BS_NOUN,
    &specs::SPEC_BS,
    &specs::SPEC_CURRENCY_CODE,
    &specs::SPEC_CURRENCY_NAME,
    &specs::SPEC_CURRENCY_SYMBOL,
    &specs::SPEC_BIC,
    &specs::SPEC_ISIN,
    &specs::SPEC_CREDIT_CARD_NUMBER,
    &specs::SPEC_UUID_V1,
    &specs::SPEC_UUID_V3,
    &specs::SPEC_UUID_V4,
    &specs::SPEC_UUID_V5,
    &specs::SPEC_URL,
    &specs::SPEC_USER_AGENT,
    &specs::SPEC_MIME_TYPE,
    &specs::SPEC_SEMVER,
    &specs::SPEC_SEMVER_STABLE,
    &specs::SPEC_SEMVER_UNSTABLE,
    &specs::SPEC_FILE_PATH,
    &specs::SPEC_FILE_NAME,
    &specs::SPEC_FILE_EXTENSION,
    &specs::SPEC_DIR_PATH,
    &specs::SPEC_IMAGE_URL,
    &specs::SPEC_IMAGE_URL_WITH_SEED,
    &specs::SPEC_IMAGE_URL_GRAYSCALE,
    &specs::SPEC_IMAGE_URL_BLUR,
    &specs::SPEC_IMAGE_URL_CUSTOM,
    &specs::SPEC_HEX_COLOR,
    &specs::SPEC_RGB_COLOR,
    &specs::SPEC_RGBA_COLOR,
    &specs::SPEC_HSL_COLOR,
    &specs::SPEC_HSLA_COLOR,
    &specs::SPEC_COLOR,
    &specs::SPEC_FERROID_U_L_I_D,
    &specs::SPEC_FERROID_TWITTER_ID,
    &specs::SPEC_FERROID_INSTAGRAM_ID,
    &specs::SPEC_FERROID_MASTODON_ID,
    &specs::SPEC_FERROID_DISCORD_ID,
    &specs::SPEC_ISBN,
    &specs::SPEC_ISBN10,
    &specs::SPEC_ISBN13,
    &specs::SPEC_RFC_STATUS_CODE,
    &specs::SPEC_VALID_STATUS_CODE,
    &specs::SPEC_LICENCE_PLATE,
    &specs::SPEC_HEALTH_INSURANCE_CODE,
    &specs::SPEC_FOREIGN_KEY,
    &specs::SPEC_SEQUENCE,
    &specs::SPEC_WEIGHTED,
];

/// 某分类下的规格。
pub fn specs_in(category: GeneratorCategory) -> Vec<&'static GeneratorSpec> {
    all_specs()
        .iter()
        .copied()
        .filter(|s| s.category == category)
        .collect()
}

/// 方法级判定（技术视角「数据类型」Tab 只展示方法级生成器）。
///
/// 方法级 = 纯数据生成算法：数值 / 文本 / Markdown / 日期时间 / 约束 五类分类的全部
/// 生成器，外加「网络与技术」中的 UUID 系列（原型第四版：INTEGER 组展开随机整数 / 自增 /
/// 正态 / 泊松 / 随机漫步，VARCHAR 组展开随机字符串 / 模板 / 正则 / 单词等——都是方法）。
/// 其余分类（个人信息 / 地址与网络标识 / 商业 / 金融 / 网络实体 / 图片 / 颜色 / Ferroid ID /
/// 标准编码 / 汽车与行政）是**业务生成器**，只在业务视角可见——消除「varchar 下展开的
/// 却是业务组」的观感（用户定稿：数据类型下展开的都是方法）。
pub fn is_method_spec(spec: &GeneratorSpec) -> bool {
    match spec.category {
        GeneratorCategory::Numeric
        | GeneratorCategory::Text
        | GeneratorCategory::Markdown
        | GeneratorCategory::DateTime
        | GeneratorCategory::Constraint => true,
        GeneratorCategory::Tech => {
            matches!(spec.name, "uuid_v1" | "uuid_v3" | "uuid_v4" | "uuid_v5")
        }
        _ => false,
    }
}

/// 方法级规格（供「基础生成器」候选等按类型筛方法的场景复用）。
pub fn methods_in_type(data_type: &ColumnDataType) -> Vec<&'static GeneratorSpec> {
    all_specs()
        .iter()
        .copied()
        .filter(|spec| is_method_spec(spec))
        .filter(|spec| {
            crate::generators::data_type_for_generator(
                &default_of(spec.name).expect("方法级规格必有默认配置"),
            ) == *data_type
        })
        .collect()
}

/// 按标识查规格。
pub fn spec_by_name(name: &str) -> Option<&'static GeneratorSpec> {
    all_specs().iter().copied().find(|s| s.name == name)
}

/// 由规格标识构造一个可用的默认配置（选择器切换生成器时使用）。
///
/// 参数取「可读默认值」（如区间 1~10、日期 2020-01-01~2025-12-31），
/// 用户可在参数区继续调整；复杂参数（列表 / 加权选项）默认留空，
/// 需在列编辑对话框里填写（留空会在生成前被拦住，不会 panic）。
pub fn default_of(name: &str) -> Option<GeneratorConfig> {
    let config = match name {
        "auto_increment" => GeneratorConfig::AutoIncrement { start: 1, step: 1 },
        "random_int" => GeneratorConfig::RandomInt { min: 1, max: 10 },
        "random_float" => GeneratorConfig::RandomFloat {
            min: 0.0,
            max: 10000.0,
            precision: 2,
        },
        "random_decimal" => GeneratorConfig::RandomDecimal {
            min: 0.0,
            max: 10000.0,
            scale: 2,
        },
        "digit" => GeneratorConfig::Digit,
        "number_with_format" => GeneratorConfig::NumberWithFormat {
            fmt: "000".to_string(),
        },
        "normal" => GeneratorConfig::Normal {
            mean: 0.0,
            std_dev: 1.0,
        },
        "log_normal" => GeneratorConfig::LogNormal {
            median: 1.0,
            dispersion: 0.5,
        },
        "random_walk" => GeneratorConfig::RandomWalk {
            start: 0.0,
            step: 1.0,
            volatility: 0.1,
        },
        "boolean" => GeneratorConfig::Boolean { ratio: 50 },
        "poisson" => GeneratorConfig::Poisson { lambda: 1.0 },
        "exponential" => GeneratorConfig::Exponential { lambda: 1.0 },
        "pareto" => GeneratorConfig::Pareto {
            scale_value: 1.0,
            alpha: 1.5,
        },
        "beta" => GeneratorConfig::Beta {
            alpha: 1.5,
            beta: 1.0,
        },
        "binomial" => GeneratorConfig::Binomial {
            trials: 10,
            probability: 0.5,
        },
        "time_series" => GeneratorConfig::TimeSeries {
            start: 0.0,
            trend: 0.0,
            period: 24,
            amplitude: 1.0,
            noise: 0.1,
        },
        "constant" => GeneratorConfig::Constant {
            value: "".to_string(),
        },
        "words" => GeneratorConfig::Words { min: 1, max: 10 },
        "sentence" => GeneratorConfig::Sentence { min: 1, max: 10 },
        "sentences" => GeneratorConfig::Sentences { min: 1, max: 10 },
        "paragraph" => GeneratorConfig::Paragraph { count: 1 },
        "paragraphs" => GeneratorConfig::Paragraphs { count: 1 },
        "word" => GeneratorConfig::Word,
        "regex" => GeneratorConfig::Regex {
            pattern: "\\d{3}-\\d{4}".to_string(),
        },
        "template" => GeneratorConfig::Template {
            template: "{value}".to_string(),
        },
        "markdown_italic_word" => GeneratorConfig::MarkdownItalicWord,
        "markdown_bold_word" => GeneratorConfig::MarkdownBoldWord,
        "markdown_link" => GeneratorConfig::MarkdownLink,
        "markdown_bullet_points" => GeneratorConfig::MarkdownBulletPoints,
        "markdown_list_items" => GeneratorConfig::MarkdownListItems,
        "markdown_block_quote_single" => GeneratorConfig::MarkdownBlockQuoteSingle,
        "markdown_block_quote_multi" => GeneratorConfig::MarkdownBlockQuoteMulti,
        "markdown_code" => GeneratorConfig::MarkdownCode,
        "name" => GeneratorConfig::Name,
        "name_with_title" => GeneratorConfig::NameWithTitle,
        "first_name" => GeneratorConfig::FirstName,
        "last_name" => GeneratorConfig::LastName,
        "title" => GeneratorConfig::Title,
        "suffix" => GeneratorConfig::Suffix,
        "email" => GeneratorConfig::Email,
        "safe_email" => GeneratorConfig::SafeEmail,
        "free_email_provider" => GeneratorConfig::FreeEmailProvider,
        "domain_suffix" => GeneratorConfig::DomainSuffix,
        "free_email" => GeneratorConfig::FreeEmail,
        "phone_number" => GeneratorConfig::PhoneNumber,
        "cell_number" => GeneratorConfig::CellNumber,
        "username" => GeneratorConfig::Username,
        "password" => GeneratorConfig::Password { min: 1, max: 10 },
        "country" => GeneratorConfig::Country,
        "country_code" => GeneratorConfig::CountryCode,
        "country_name" => GeneratorConfig::CountryName,
        "city" => GeneratorConfig::City,
        "city_prefix" => GeneratorConfig::CityPrefix,
        "city_suffix" => GeneratorConfig::CitySuffix,
        "state_name" => GeneratorConfig::StateName,
        "state_abbr" => GeneratorConfig::StateAbbr,
        "street_name" => GeneratorConfig::StreetName,
        "street_suffix" => GeneratorConfig::StreetSuffix,
        "zip_code" => GeneratorConfig::ZipCode,
        "post_code" => GeneratorConfig::PostCode,
        "building_number" => GeneratorConfig::BuildingNumber,
        "secondary_address" => GeneratorConfig::SecondaryAddress,
        "secondary_address_type" => GeneratorConfig::SecondaryAddressType,
        "latitude" => GeneratorConfig::Latitude,
        "longitude" => GeneratorConfig::Longitude,
        "geohash" => GeneratorConfig::Geohash { precision: 2 },
        "time_zone" => GeneratorConfig::TimeZone,
        "ip_address" => GeneratorConfig::IpAddress,
        "i_pv4" => GeneratorConfig::IPv4,
        "i_pv6" => GeneratorConfig::IPv6,
        "i_p" => GeneratorConfig::IP,
        "mac_address" => GeneratorConfig::MacAddress,
        "date_time" => GeneratorConfig::DateTime {
            min: "2020-01-01".to_string(),
            max: "2025-12-31".to_string(),
            workdays_only: false,
            work_hours_only: false,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "date_time_before" => GeneratorConfig::DateTimeBefore {
            before: "2020-01-01".to_string(),
            workdays_only: false,
            work_hours_only: false,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "date_time_after" => GeneratorConfig::DateTimeAfter {
            after: "2020-01-01".to_string(),
            workdays_only: false,
            work_hours_only: false,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "date_time_between" => GeneratorConfig::DateTimeBetween {
            start: "".to_string(),
            end: "2025-12-31".to_string(),
            workdays_only: false,
            work_hours_only: false,
            work_hour_start: "09:00".to_string(),
            work_hour_end: "18:00".to_string(),
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "date" => GeneratorConfig::Date {
            min: "2020-01-01".to_string(),
            max: "2025-12-31".to_string(),
            workdays_only: false,
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "time" => GeneratorConfig::Time,
        "duration" => GeneratorConfig::Duration,
        "sequential_date" => GeneratorConfig::SequentialDate {
            start: "".to_string(),
            step_seconds: 86400,
            workdays_only: false,
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "sequential_date_with_gaps" => GeneratorConfig::SequentialDateWithGaps {
            start: "".to_string(),
            step_seconds: 86400,
            miss_probability: 0.1,
            workdays_only: false,
            work_week: "1111100".to_string(),
            skip_dates: Vec::new(),
            work_dates: Vec::new(),
        },
        "company_name" => GeneratorConfig::CompanyName,
        "company_suffix" => GeneratorConfig::CompanySuffix,
        "job_title" => GeneratorConfig::JobTitle,
        "profession" => GeneratorConfig::Profession,
        "industry" => GeneratorConfig::Industry,
        "seniority" => GeneratorConfig::Seniority,
        "field" => GeneratorConfig::Field,
        "position" => GeneratorConfig::Position,
        "buzzword" => GeneratorConfig::Buzzword,
        "buzzword_middle" => GeneratorConfig::BuzzwordMiddle,
        "buzzword_tail" => GeneratorConfig::BuzzwordTail,
        "catch_phrase" => GeneratorConfig::CatchPhrase,
        "bs_verb" => GeneratorConfig::BsVerb,
        "bs_adj" => GeneratorConfig::BsAdj,
        "bs_noun" => GeneratorConfig::BsNoun,
        "bs" => GeneratorConfig::Bs,
        "currency_code" => GeneratorConfig::CurrencyCode,
        "currency_name" => GeneratorConfig::CurrencyName,
        "currency_symbol" => GeneratorConfig::CurrencySymbol,
        "bic" => GeneratorConfig::Bic,
        "isin" => GeneratorConfig::Isin,
        "credit_card_number" => GeneratorConfig::CreditCardNumber,
        "uuid_v1" => GeneratorConfig::UuidV1,
        "uuid_v3" => GeneratorConfig::UuidV3,
        "uuid_v4" => GeneratorConfig::UuidV4,
        "uuid_v5" => GeneratorConfig::UuidV5,
        "url" => GeneratorConfig::Url,
        "user_agent" => GeneratorConfig::UserAgent,
        "mime_type" => GeneratorConfig::MimeType,
        "semver" => GeneratorConfig::Semver,
        "semver_stable" => GeneratorConfig::SemverStable,
        "semver_unstable" => GeneratorConfig::SemverUnstable,
        "file_path" => GeneratorConfig::FilePath,
        "file_name" => GeneratorConfig::FileName,
        "file_extension" => GeneratorConfig::FileExtension,
        "dir_path" => GeneratorConfig::DirPath,
        "image_url" => GeneratorConfig::ImageUrl {
            width: 200,
            height: 200,
        },
        "image_url_with_seed" => GeneratorConfig::ImageUrlWithSeed {
            width: 200,
            height: 200,
            seed: 42,
        },
        "image_url_grayscale" => GeneratorConfig::ImageUrlGrayscale {
            width: 200,
            height: 200,
        },
        "image_url_blur" => GeneratorConfig::ImageUrlBlur {
            width: 200,
            height: 200,
            blur_amount: 2,
        },
        "image_url_custom" => GeneratorConfig::ImageUrlCustom {
            width: 200,
            height: 200,
            grayscale: false,
            blur_amount: Some(2),
            seed: Some(42),
        },
        "hex_color" => GeneratorConfig::HexColor,
        "rgb_color" => GeneratorConfig::RgbColor,
        "rgba_color" => GeneratorConfig::RgbaColor,
        "hsl_color" => GeneratorConfig::HslColor,
        "hsla_color" => GeneratorConfig::HslaColor,
        "color" => GeneratorConfig::Color,
        "ferroid_u_l_i_d" => GeneratorConfig::FerroidULID,
        "ferroid_twitter_id" => GeneratorConfig::FerroidTwitterId,
        "ferroid_instagram_id" => GeneratorConfig::FerroidInstagramId,
        "ferroid_mastodon_id" => GeneratorConfig::FerroidMastodonId,
        "ferroid_discord_id" => GeneratorConfig::FerroidDiscordId,
        "isbn" => GeneratorConfig::Isbn,
        "isbn10" => GeneratorConfig::Isbn10,
        "isbn13" => GeneratorConfig::Isbn13,
        "rfc_status_code" => GeneratorConfig::RfcStatusCode,
        "valid_status_code" => GeneratorConfig::ValidStatusCode,
        "licence_plate" => GeneratorConfig::LicencePlate,
        "health_insurance_code" => GeneratorConfig::HealthInsuranceCode,
        "foreign_key" => GeneratorConfig::ValueSet { values: Vec::new() },
        "sequence" => GeneratorConfig::Sequence {
            cycle: false,
            values: Vec::new(),
        },
        "weighted" => GeneratorConfig::Weighted {
            choices: Vec::new(),
        },
        _ => return None,
    };
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_specs_cover_every_variant() {
        assert_eq!(all_specs().len(), 143, "规格数与变体数应一致");
        // 标识唯一
        let mut names: Vec<&str> = all_specs().iter().map(|s| s.name).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total, "规格标识不得重复");
    }

    #[test]
    fn every_spec_has_label_and_default() {
        for spec in all_specs() {
            assert!(!spec.label.is_empty(), "{} 缺标签", spec.name);
            let cfg = default_of(spec.name);
            assert!(cfg.is_some(), "{} 缺默认构造", spec.name);
            // 默认构造必须能回到同一规格（往返一致）
            assert_eq!(spec_of(&cfg.unwrap()).name, spec.name);
        }
    }

    #[test]
    fn every_variant_resolves_to_spec() {
        for spec in all_specs() {
            let cfg = default_of(spec.name).expect("默认构造");
            assert_eq!(spec_of(&cfg).category, spec.category);
        }
    }
}

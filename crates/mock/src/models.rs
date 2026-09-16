use shared::models::QueryResult;
use serde::{Deserialize, Serialize};
use specta::Type;

// ==================== 核心配置 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockConfig {
    pub table_name: String,
    pub row_count: u32,
    pub seed: Option<u32>,
    pub locale: Locale,
    pub columns: Vec<ColumnDef>,
}

// ==================== 列定义 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDef {
    pub name: String,
    pub data_type: ColumnDataType,
    pub generator: GeneratorConfig,
    #[serde(default = "default_nullable_ratio")]
    pub nullable_ratio: f64,
    #[serde(default)]
    pub unique: bool,
    /// 列间依赖配置（可选）
    #[serde(default)]
    pub dependency: Option<ColumnDependency>,
}

fn default_nullable_ratio() -> f64 {
    0.0
}

// ==================== 列数据类型 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ColumnDataType {
    Integer,
    BigInt,
    Float,
    Double,
    Decimal { precision: u8, scale: u8 },
    Boolean,
    Varchar { length: Option<u32> },
    Text,
    Date,
    DateTime,
    Timestamp,
    Uuid,
    Blob,
}

impl ColumnDataType {
    pub fn to_duckdb_type(&self) -> String {
        match self {
            ColumnDataType::Integer => "INTEGER".to_string(),
            ColumnDataType::BigInt => "BIGINT".to_string(),
            ColumnDataType::Float => "FLOAT".to_string(),
            ColumnDataType::Double => "DOUBLE".to_string(),
            ColumnDataType::Decimal { precision, scale } => {
                format!("DECIMAL({}, {})", precision, scale)
            }
            ColumnDataType::Boolean => "BOOLEAN".to_string(),
            ColumnDataType::Varchar { length } => {
                if let Some(len) = length {
                    format!("VARCHAR({})", len)
                } else {
                    "VARCHAR".to_string()
                }
            }
            ColumnDataType::Text => "VARCHAR".to_string(),
            ColumnDataType::Date => "DATE".to_string(),
            ColumnDataType::DateTime => "TIMESTAMP".to_string(),
            ColumnDataType::Timestamp => "TIMESTAMP".to_string(),
            ColumnDataType::Uuid => "VARCHAR".to_string(),
            ColumnDataType::Blob => "BLOB".to_string(),
        }
    }
}

// ==================== 生成器配置 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GeneratorConfig {
    // 数值类
    AutoIncrement {
        start: i32,
        step: i32,
    },
    RandomInt {
        min: i32,
        max: i32,
    },
    RandomFloat {
        min: f64,
        max: f64,
        precision: u8,
    },
    RandomDecimal {
        min: f64,
        max: f64,
        scale: u8,
    },
    Digit,
    NumberWithFormat {
        fmt: String,
    },
    Normal {
        mean: f64,
        std_dev: f64,
    },
    LogNormal {
        median: f64,
        dispersion: f64,
    },
    RandomWalk {
        start: f64,
        step: f64,
        volatility: f64,
    },
    Boolean {
        ratio: u8,
    },

    // 字符串/文本类
    Constant {
        value: String,
    },
    Words {
        min: u32,
        max: u32,
    },
    Sentence {
        min: u32,
        max: u32,
    },
    Sentences {
        min: u32,
        max: u32,
    },
    Paragraph {
        count: u32,
    },
    Paragraphs {
        count: u32,
    },
    Word,
    Regex {
        pattern: String,
    },
    Template {
        template: String,
    },

    // Markdown
    MarkdownItalicWord,
    MarkdownBoldWord,
    MarkdownLink,
    MarkdownBulletPoints,
    MarkdownListItems,
    MarkdownBlockQuoteSingle,
    MarkdownBlockQuoteMulti,
    MarkdownCode,

    // 个人信息
    Name,
    NameWithTitle,
    FirstName,
    LastName,
    Title,
    Suffix,
    Email,
    SafeEmail,
    FreeEmailProvider,
    DomainSuffix,
    FreeEmail,
    PhoneNumber,
    CellNumber,
    Username,
    Password {
        min: u32,
        max: u32,
    },

    // 地址类
    Country,
    CountryCode,
    CountryName,
    City,
    CityPrefix,
    CitySuffix,
    StateName,
    StateAbbr,
    StreetName,
    StreetSuffix,
    ZipCode,
    PostCode,
    BuildingNumber,
    SecondaryAddress,
    SecondaryAddressType,
    Latitude,
    Longitude,
    Geohash {
        precision: u8,
    },
    TimeZone,
    IpAddress,
    IPv4,
    IPv6,
    IP,
    MacAddress,

    // 日期时间类
    DateTime {
        min: String,
        max: String,
    },
    DateTimeBefore {
        before: String,
    },
    DateTimeAfter {
        after: String,
    },
    DateTimeBetween {
        start: String,
        end: String,
    },
    Date {
        min: String,
        max: String,
    },
    Time,
    Duration,
    SequentialDate {
        start: String,
        step_seconds: i32,
    },
    SequentialDateWithGaps {
        start: String,
        step_seconds: i32,
        miss_probability: f64,
    },

    // 商业类
    CompanyName,
    CompanySuffix,
    JobTitle,
    Profession,
    Industry,
    Seniority,
    Field,
    Position,
    Buzzword,
    BuzzwordMiddle,
    BuzzwordTail,
    CatchPhrase,
    BsVerb,
    BsAdj,
    BsNoun,
    Bs,

    // 金融类
    CurrencyCode,
    CurrencyName,
    CurrencySymbol,
    Bic,
    Isin,
    CreditCardNumber,

    // 网络/技术类
    UuidV1,
    UuidV3,
    UuidV4,
    UuidV5,
    Url,
    UserAgent,
    MimeType,
    Semver,
    SemverStable,
    SemverUnstable,
    FilePath,
    FileName,
    FileExtension,
    DirPath,

    // Picsum 图片 URL
    ImageUrl {
        width: u32,
        height: u32,
    },
    ImageUrlWithSeed {
        width: u32,
        height: u32,
        seed: u32,
    },
    ImageUrlGrayscale {
        width: u32,
        height: u32,
    },
    ImageUrlBlur {
        width: u32,
        height: u32,
        blur_amount: u8,
    },
    ImageUrlCustom {
        width: u32,
        height: u32,
        grayscale: bool,
        blur_amount: Option<u8>,
        seed: Option<u32>,
    },

    // 颜色类
    HexColor,
    RgbColor,
    RgbaColor,
    HslColor,
    HslaColor,
    Color,

    // Ferroid ID 类
    FerroidULID,
    FerroidTwitterId,
    FerroidInstagramId,
    FerroidMastodonId,
    FerroidDiscordId,

    // 条形码与标准编码
    Isbn,
    Isbn10,
    Isbn13,
    RfcStatusCode,
    ValidStatusCode,

    // 汽车与行政
    LicencePlate,
    HealthInsuranceCode,

    // 约束类
    ForeignKey {
        values: Vec<String>,
    },
    Sequence {
        values: Vec<String>,
        cycle: bool,
    },
    Weighted {
        choices: Vec<(String, f64)>,
    },
}

// ==================== 语言/地区 ====================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Locale {
    ZhCn,
    En,
    JaJp,
    ZhTw,
    FrFr,
    DeDe,
    ItIt,
    PtBr,
    PtPt,
    NlNl,
    ArSa,
    TrTr,
    FaIr,
}

// ==================== 生成结果 ====================

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerateResult {
    pub table_name: String,
    pub temp_table_name: String,
    pub row_count: u32,
    pub preview: QueryResult,
    pub columns: Vec<String>,
    pub elapsed_ms: u32,
}

// ==================== 导出格式 ====================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MockExportFormat {
    Csv,
    Parquet,
    Xlsx,
    Table,
    SqlInsert,
}

// ==================== 导出请求 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockExportInput {
    pub temp_table_name: String,
    pub format: MockExportFormat,
    pub output_path: Option<String>,
    pub table_name: Option<String>,
}

// ==================== 列映射响应 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMappingResponse {
    pub column_name: String,
    pub generator: GeneratorConfig,
    pub confidence: String,
    pub sample_value: String,
}

// ==================== 导入结构请求 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportSchemaInput {
    pub conn_id: String,
    pub database: String,
    pub schema: Option<String>,
    pub tables: Vec<String>,
    #[serde(default = "default_connection_type")]
    pub connection_type: String,
    pub project_path: Option<String>,
}

fn default_connection_type() -> String {
    "global".to_string()
}

// ==================== 生成历史记录 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockHistoryRecord {
    pub id: String,
    pub table_name: String,
    pub row_count: u32,
    pub seed: Option<u32>,
    pub config_json: String,
    pub generated_at: String,
    pub status: String,
}

// ==================== 场景模板 ====================

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub locale: String,
    pub tables: Vec<TemplateTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateTable {
    pub name: String,
    pub row_count: u32,
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockSaveToScratchpadInput {
    pub temp_table_name: String,
    pub format: MockExportFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockPersistAssetInput {
    pub temp_table_name: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockPersistAssetResult {
    pub table_name: String,
    pub row_count: i32,
    pub column_count: i32,
}

// ==================== 多表场景生成结果 ====================

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockScenarioResult {
    pub template_id: String,
    pub template_name: String,
    pub table_count: u32,
    pub total_rows: u32,
    pub total_elapsed_ms: u32,
    pub tables: Vec<MockScenarioTableResult>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockScenarioTableResult {
    pub table_name: String,
    pub temp_table_name: String,
    pub row_count: u32,
    pub columns: Vec<String>,
    pub elapsed_ms: u32,
}

// ==================== 列依赖模型 ====================

/// 列依赖类型
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DependencyType {
    /// 计算表达式：引用其他列的计算（如 `price * quantity`）
    Expression,
    /// 外键引用：引用其他表的列
    ForeignKey,
    /// 模板引用：使用模板字符串拼接（如 `{first_name} {last_name}`）
    Template,
    /// 序列依赖：基于序列生成器
    Sequence,
    /// 加权依赖：基于加权随机选择
    Weighted,
}

/// 列间依赖定义
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDependency {
    /// 依赖类型
    pub dep_type: DependencyType,
    /// 依赖的源列名列表
    pub source_columns: Vec<String>,
    /// 表达式/模板字符串
    pub expression: Option<String>,
    /// 外键引用的目标表名
    pub ref_table: Option<String>,
    /// 外键引用的目标列名
    pub ref_column: Option<String>,
    /// 权重配置（用于 Weighted 类型）
    pub weights: Option<Vec<(String, f64)>>,
}

// ==================== 表间引用（场景生成用） ====================

/// 一张表的某个主键列在**本次生成**中的取值域（供子表引用采样）。
///
/// 由父列的 `AutoIncrement { start, step }` 与父表行数**算出来**，因此：
///
/// - **不读任何已有数据**（不查库、不开文件）——父表与子表在同一次多表生成里产出；
/// - **O(1) 内存**：等差数列，不需要把十万量级的主键装进内存；
/// - 域是「本次生成会产出的值」的精确集合（自增列逐行 `+step`，不受唯一性重试影响）。
///
/// 父键不是自增（uuid / 随机）时**算不出域**——那种情形直接拒绝并给可读理由
/// （见 `MockEngine::resolve_reference_domains`），不猜、也不去读已落地的数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDomain {
    /// 父表名（模板里的表名）
    pub table: String,
    /// 被引用的父列名
    pub column: String,
    /// 域下界（`AutoIncrement.start`）
    pub first: i64,
    /// 步长（`AutoIncrement.step`）
    pub step: i64,
    /// 取值个数（= 父表行数）
    pub count: u32,
}

impl ReferenceDomain {
    /// 域内第 `index` 个值（调用方保证 `index < count`）。
    pub fn value_at(&self, index: u32) -> i64 {
        self.first + self.step * i64::from(index)
    }

    /// 域上界（`count` 为 0 时等于下界——空域，生成前会被拦下）。
    pub fn last(&self) -> i64 {
        match self.count.checked_sub(1) {
            Some(last) => self.value_at(last),
            None => self.first,
        }
    }

    /// 引用目标文案：`users.id（1..1000）`（面板展示关系时用）。
    pub fn label(&self) -> String {
        if self.step == 1 {
            format!(
                "{}.{}（{}..{}）",
                self.table,
                self.column,
                self.first,
                self.last()
            )
        } else {
            format!(
                "{}.{}（{}..{}，步长 {}）",
                self.table,
                self.column,
                self.first,
                self.last(),
                self.step
            )
        }
    }
}

/// 依赖解析配置
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DependencyConfig {
    /// 拓扑排序后的列生成顺序
    pub sorted_columns: Vec<String>,
    /// 每列的依赖信息
    pub dependencies: std::collections::HashMap<String, ColumnDependency>,
}

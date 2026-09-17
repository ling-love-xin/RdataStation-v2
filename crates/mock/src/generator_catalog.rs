//! 生成器目录（M7）——由 `models.rs` 的 `GeneratorConfig` 穷尽派生。
//!
//! 本文件由 `tools/gen_mock_generator_catalog.py` 生成，**不要手改**：
//! 新增 / 删除 `GeneratorConfig` 变体后重跑脚本，`spec_of` 的穷尽 match 会强制补齐。
//!
//! 用途：面板展示（中文标签）、生成器选择器（分类浏览 + 搜索）、参数编辑表单。

use crate::models::GeneratorConfig;

/// 生成器分类（面板分组用；顺序即展示顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy)]
pub struct ParamField {
    /// 字段名（与 `GeneratorConfig` 变体字段同名；JSON 补丁按此键写回）
    pub key: &'static str,
    /// 中文标签
    pub label: &'static str,
    /// 参数类型
    pub kind: ParamKind,
}

/// 生成器规格。
#[derive(Debug, Clone, Copy)]
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

// ==================== 规格常量 ====================

/// `AutoIncrement`
static SPEC_AUTO_INCREMENT: GeneratorSpec = GeneratorSpec {
    name: "auto_increment",
    label: "自增序列",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "start",
            label: "起始值",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "step",
            label: "步长",
            kind: ParamKind::Int,
        },
    ],
};

/// `RandomInt`
static SPEC_RANDOM_INT: GeneratorSpec = GeneratorSpec {
    name: "random_int",
    label: "随机整数",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Int,
        },
    ],
};

/// `RandomFloat`
static SPEC_RANDOM_FLOAT: GeneratorSpec = GeneratorSpec {
    name: "random_float",
    label: "随机小数",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "precision",
            label: "精度",
            kind: ParamKind::Int,
        },
    ],
};

/// `RandomDecimal`
static SPEC_RANDOM_DECIMAL: GeneratorSpec = GeneratorSpec {
    name: "random_decimal",
    label: "随机定点数",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "scale",
            label: "小数位",
            kind: ParamKind::Int,
        },
    ],
};

/// `Digit`
static SPEC_DIGIT: GeneratorSpec = GeneratorSpec {
    name: "digit",
    label: "单个数字",
    category: GeneratorCategory::Numeric,
    params: &[],
};

/// `NumberWithFormat`
static SPEC_NUMBER_WITH_FORMAT: GeneratorSpec = GeneratorSpec {
    name: "number_with_format",
    label: "格式化数字",
    category: GeneratorCategory::Numeric,
    params: &[ParamField {
        key: "fmt",
        label: "格式",
        kind: ParamKind::Text,
    }],
};

/// `Normal`
static SPEC_NORMAL: GeneratorSpec = GeneratorSpec {
    name: "normal",
    label: "正态分布",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "mean",
            label: "均值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "std_dev",
            label: "标准差",
            kind: ParamKind::Float,
        },
    ],
};

/// `LogNormal`
static SPEC_LOG_NORMAL: GeneratorSpec = GeneratorSpec {
    name: "log_normal",
    label: "对数正态分布",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "median",
            label: "中位数",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "dispersion",
            label: "离散度",
            kind: ParamKind::Float,
        },
    ],
};

/// `RandomWalk`
static SPEC_RANDOM_WALK: GeneratorSpec = GeneratorSpec {
    name: "random_walk",
    label: "随机游走",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "start",
            label: "起始值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "step",
            label: "步长",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "volatility",
            label: "波动率",
            kind: ParamKind::Float,
        },
    ],
};

/// `Boolean`
static SPEC_BOOLEAN: GeneratorSpec = GeneratorSpec {
    name: "boolean",
    label: "布尔值",
    category: GeneratorCategory::Numeric,
    params: &[ParamField {
        key: "ratio",
        label: "占比（%）",
        kind: ParamKind::Int,
    }],
};

/// `Poisson`
static SPEC_POISSON: GeneratorSpec = GeneratorSpec {
    name: "poisson",
    label: "泊松分布",
    category: GeneratorCategory::Numeric,
    params: &[ParamField {
        key: "lambda",
        label: "强度 λ",
        kind: ParamKind::Float,
    }],
};

/// `Exponential`
static SPEC_EXPONENTIAL: GeneratorSpec = GeneratorSpec {
    name: "exponential",
    label: "指数分布",
    category: GeneratorCategory::Numeric,
    params: &[ParamField {
        key: "lambda",
        label: "强度 λ",
        kind: ParamKind::Float,
    }],
};

/// `Pareto`
static SPEC_PARETO: GeneratorSpec = GeneratorSpec {
    name: "pareto",
    label: "帕累托分布（长尾）",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "scale_value",
            label: "尺度（最小值）",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "alpha",
            label: "形状参数 α",
            kind: ParamKind::Float,
        },
    ],
};

/// `Beta`
static SPEC_BETA: GeneratorSpec = GeneratorSpec {
    name: "beta",
    label: "Beta 分布（比例）",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "alpha",
            label: "形状参数 α",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "beta",
            label: "形状参数 β",
            kind: ParamKind::Float,
        },
    ],
};

/// `Binomial`
static SPEC_BINOMIAL: GeneratorSpec = GeneratorSpec {
    name: "binomial",
    label: "二项分布",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "trials",
            label: "试验次数 n",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "probability",
            label: "概率 p",
            kind: ParamKind::Float,
        },
    ],
};

/// `TimeSeries`
static SPEC_TIME_SERIES: GeneratorSpec = GeneratorSpec {
    name: "time_series",
    label: "时序数值（趋势 + 周期）",
    category: GeneratorCategory::Numeric,
    params: &[
        ParamField {
            key: "start",
            label: "起始值",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "trend",
            label: "趋势（每行增量）",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "period",
            label: "周期（行数）",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "amplitude",
            label: "周期振幅",
            kind: ParamKind::Float,
        },
        ParamField {
            key: "noise",
            label: "噪声强度",
            kind: ParamKind::Float,
        },
    ],
};

/// `Constant`
static SPEC_CONSTANT: GeneratorSpec = GeneratorSpec {
    name: "constant",
    label: "固定值",
    category: GeneratorCategory::Text,
    params: &[ParamField {
        key: "value",
        label: "固定值",
        kind: ParamKind::Text,
    }],
};

/// `Words`
static SPEC_WORDS: GeneratorSpec = GeneratorSpec {
    name: "words",
    label: "随机词组",
    category: GeneratorCategory::Text,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Int,
        },
    ],
};

/// `Sentence`
static SPEC_SENTENCE: GeneratorSpec = GeneratorSpec {
    name: "sentence",
    label: "随机句子",
    category: GeneratorCategory::Text,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Int,
        },
    ],
};

/// `Sentences`
static SPEC_SENTENCES: GeneratorSpec = GeneratorSpec {
    name: "sentences",
    label: "多句文本",
    category: GeneratorCategory::Text,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Int,
        },
    ],
};

/// `Paragraph`
static SPEC_PARAGRAPH: GeneratorSpec = GeneratorSpec {
    name: "paragraph",
    label: "段落",
    category: GeneratorCategory::Text,
    params: &[ParamField {
        key: "count",
        label: "数量",
        kind: ParamKind::Int,
    }],
};

/// `Paragraphs`
static SPEC_PARAGRAPHS: GeneratorSpec = GeneratorSpec {
    name: "paragraphs",
    label: "多段文本",
    category: GeneratorCategory::Text,
    params: &[ParamField {
        key: "count",
        label: "数量",
        kind: ParamKind::Int,
    }],
};

/// `Word`
static SPEC_WORD: GeneratorSpec = GeneratorSpec {
    name: "word",
    label: "随机单词",
    category: GeneratorCategory::Text,
    params: &[],
};

/// `Regex`
static SPEC_REGEX: GeneratorSpec = GeneratorSpec {
    name: "regex",
    label: "正则表达式",
    category: GeneratorCategory::Text,
    params: &[ParamField {
        key: "pattern",
        label: "正则表达式",
        kind: ParamKind::Text,
    }],
};

/// `Template`
static SPEC_TEMPLATE: GeneratorSpec = GeneratorSpec {
    name: "template",
    label: "模板字符串",
    category: GeneratorCategory::Text,
    params: &[ParamField {
        key: "template",
        label: "模板",
        kind: ParamKind::Text,
    }],
};

/// `MarkdownItalicWord`
static SPEC_MARKDOWN_ITALIC_WORD: GeneratorSpec = GeneratorSpec {
    name: "markdown_italic_word",
    label: "Markdown 斜体词",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBoldWord`
static SPEC_MARKDOWN_BOLD_WORD: GeneratorSpec = GeneratorSpec {
    name: "markdown_bold_word",
    label: "Markdown 粗体词",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownLink`
static SPEC_MARKDOWN_LINK: GeneratorSpec = GeneratorSpec {
    name: "markdown_link",
    label: "Markdown 链接",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBulletPoints`
static SPEC_MARKDOWN_BULLET_POINTS: GeneratorSpec = GeneratorSpec {
    name: "markdown_bullet_points",
    label: "Markdown 无序列表",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownListItems`
static SPEC_MARKDOWN_LIST_ITEMS: GeneratorSpec = GeneratorSpec {
    name: "markdown_list_items",
    label: "Markdown 列表项",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBlockQuoteSingle`
static SPEC_MARKDOWN_BLOCK_QUOTE_SINGLE: GeneratorSpec = GeneratorSpec {
    name: "markdown_block_quote_single",
    label: "Markdown 单行引用",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBlockQuoteMulti`
static SPEC_MARKDOWN_BLOCK_QUOTE_MULTI: GeneratorSpec = GeneratorSpec {
    name: "markdown_block_quote_multi",
    label: "Markdown 多行引用",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownCode`
static SPEC_MARKDOWN_CODE: GeneratorSpec = GeneratorSpec {
    name: "markdown_code",
    label: "Markdown 代码块",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `Name`
static SPEC_NAME: GeneratorSpec = GeneratorSpec {
    name: "name",
    label: "姓名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `NameWithTitle`
static SPEC_NAME_WITH_TITLE: GeneratorSpec = GeneratorSpec {
    name: "name_with_title",
    label: "带称谓姓名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `FirstName`
static SPEC_FIRST_NAME: GeneratorSpec = GeneratorSpec {
    name: "first_name",
    label: "名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `LastName`
static SPEC_LAST_NAME: GeneratorSpec = GeneratorSpec {
    name: "last_name",
    label: "姓",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Title`
static SPEC_TITLE: GeneratorSpec = GeneratorSpec {
    name: "title",
    label: "称谓",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Suffix`
static SPEC_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "suffix",
    label: "名字后缀",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Email`
static SPEC_EMAIL: GeneratorSpec = GeneratorSpec {
    name: "email",
    label: "邮箱地址",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `SafeEmail`
static SPEC_SAFE_EMAIL: GeneratorSpec = GeneratorSpec {
    name: "safe_email",
    label: "安全邮箱",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `FreeEmailProvider`
static SPEC_FREE_EMAIL_PROVIDER: GeneratorSpec = GeneratorSpec {
    name: "free_email_provider",
    label: "免费邮箱服务商",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `DomainSuffix`
static SPEC_DOMAIN_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "domain_suffix",
    label: "域名后缀",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `FreeEmail`
static SPEC_FREE_EMAIL: GeneratorSpec = GeneratorSpec {
    name: "free_email",
    label: "免费邮箱",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `PhoneNumber`
static SPEC_PHONE_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "phone_number",
    label: "电话号码",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `CellNumber`
static SPEC_CELL_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "cell_number",
    label: "手机号",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Username`
static SPEC_USERNAME: GeneratorSpec = GeneratorSpec {
    name: "username",
    label: "用户名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Password`
static SPEC_PASSWORD: GeneratorSpec = GeneratorSpec {
    name: "password",
    label: "密码",
    category: GeneratorCategory::Person,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Int,
        },
    ],
};

/// `Country`
static SPEC_COUNTRY: GeneratorSpec = GeneratorSpec {
    name: "country",
    label: "国家",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CountryCode`
static SPEC_COUNTRY_CODE: GeneratorSpec = GeneratorSpec {
    name: "country_code",
    label: "国家代码",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CountryName`
static SPEC_COUNTRY_NAME: GeneratorSpec = GeneratorSpec {
    name: "country_name",
    label: "国家名",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `City`
static SPEC_CITY: GeneratorSpec = GeneratorSpec {
    name: "city",
    label: "城市",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CityPrefix`
static SPEC_CITY_PREFIX: GeneratorSpec = GeneratorSpec {
    name: "city_prefix",
    label: "城市前缀",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CitySuffix`
static SPEC_CITY_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "city_suffix",
    label: "城市后缀",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StateName`
static SPEC_STATE_NAME: GeneratorSpec = GeneratorSpec {
    name: "state_name",
    label: "省/州名",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StateAbbr`
static SPEC_STATE_ABBR: GeneratorSpec = GeneratorSpec {
    name: "state_abbr",
    label: "省/州缩写",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StreetName`
static SPEC_STREET_NAME: GeneratorSpec = GeneratorSpec {
    name: "street_name",
    label: "街道名",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StreetSuffix`
static SPEC_STREET_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "street_suffix",
    label: "街道后缀",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `ZipCode`
static SPEC_ZIP_CODE: GeneratorSpec = GeneratorSpec {
    name: "zip_code",
    label: "邮政编码",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `PostCode`
static SPEC_POST_CODE: GeneratorSpec = GeneratorSpec {
    name: "post_code",
    label: "邮政编号",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `BuildingNumber`
static SPEC_BUILDING_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "building_number",
    label: "楼牌号",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `SecondaryAddress`
static SPEC_SECONDARY_ADDRESS: GeneratorSpec = GeneratorSpec {
    name: "secondary_address",
    label: "次级地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `SecondaryAddressType`
static SPEC_SECONDARY_ADDRESS_TYPE: GeneratorSpec = GeneratorSpec {
    name: "secondary_address_type",
    label: "次级地址类型",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `Latitude`
static SPEC_LATITUDE: GeneratorSpec = GeneratorSpec {
    name: "latitude",
    label: "纬度",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `Longitude`
static SPEC_LONGITUDE: GeneratorSpec = GeneratorSpec {
    name: "longitude",
    label: "经度",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `Geohash`
static SPEC_GEOHASH: GeneratorSpec = GeneratorSpec {
    name: "geohash",
    label: "Geohash",
    category: GeneratorCategory::Address,
    params: &[ParamField {
        key: "precision",
        label: "精度",
        kind: ParamKind::Int,
    }],
};

/// `TimeZone`
static SPEC_TIME_ZONE: GeneratorSpec = GeneratorSpec {
    name: "time_zone",
    label: "时区",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IpAddress`
static SPEC_IP_ADDRESS: GeneratorSpec = GeneratorSpec {
    name: "ip_address",
    label: "IP 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IPv4`
static SPEC_I_PV4: GeneratorSpec = GeneratorSpec {
    name: "i_pv4",
    label: "IPv4 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IPv6`
static SPEC_I_PV6: GeneratorSpec = GeneratorSpec {
    name: "i_pv6",
    label: "IPv6 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IP`
static SPEC_I_P: GeneratorSpec = GeneratorSpec {
    name: "i_p",
    label: "IP（v4/v6）",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `MacAddress`
static SPEC_MAC_ADDRESS: GeneratorSpec = GeneratorSpec {
    name: "mac_address",
    label: "MAC 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `DateTime`
static SPEC_DATE_TIME: GeneratorSpec = GeneratorSpec {
    name: "date_time",
    label: "日期时间（区间）",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Text,
        },
    ],
};

/// `DateTimeBefore`
static SPEC_DATE_TIME_BEFORE: GeneratorSpec = GeneratorSpec {
    name: "date_time_before",
    label: "某时间之前",
    category: GeneratorCategory::DateTime,
    params: &[ParamField {
        key: "before",
        label: "早于",
        kind: ParamKind::Text,
    }],
};

/// `DateTimeAfter`
static SPEC_DATE_TIME_AFTER: GeneratorSpec = GeneratorSpec {
    name: "date_time_after",
    label: "某时间之后",
    category: GeneratorCategory::DateTime,
    params: &[ParamField {
        key: "after",
        label: "晚于",
        kind: ParamKind::Text,
    }],
};

/// `DateTimeBetween`
static SPEC_DATE_TIME_BETWEEN: GeneratorSpec = GeneratorSpec {
    name: "date_time_between",
    label: "两时间之间",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "start",
            label: "起始值",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "end",
            label: "结束",
            kind: ParamKind::Text,
        },
    ],
};

/// `Date`
static SPEC_DATE: GeneratorSpec = GeneratorSpec {
    name: "date",
    label: "日期（区间）",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "min",
            label: "最小值",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "max",
            label: "最大值",
            kind: ParamKind::Text,
        },
    ],
};

/// `Time`
static SPEC_TIME: GeneratorSpec = GeneratorSpec {
    name: "time",
    label: "时间",
    category: GeneratorCategory::DateTime,
    params: &[],
};

/// `Duration`
static SPEC_DURATION: GeneratorSpec = GeneratorSpec {
    name: "duration",
    label: "持续时间",
    category: GeneratorCategory::DateTime,
    params: &[],
};

/// `SequentialDate`
static SPEC_SEQUENTIAL_DATE: GeneratorSpec = GeneratorSpec {
    name: "sequential_date",
    label: "顺序日期",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "start",
            label: "起始值",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "step_seconds",
            label: "步长（秒）",
            kind: ParamKind::Int,
        },
    ],
};

/// `SequentialDateWithGaps`
static SPEC_SEQUENTIAL_DATE_WITH_GAPS: GeneratorSpec = GeneratorSpec {
    name: "sequential_date_with_gaps",
    label: "顺序日期（含缺口）",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "start",
            label: "起始值",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "step_seconds",
            label: "步长（秒）",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "miss_probability",
            label: "缺失概率",
            kind: ParamKind::Float,
        },
    ],
};

/// `CompanyName`
static SPEC_COMPANY_NAME: GeneratorSpec = GeneratorSpec {
    name: "company_name",
    label: "公司名",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `CompanySuffix`
static SPEC_COMPANY_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "company_suffix",
    label: "公司后缀",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `JobTitle`
static SPEC_JOB_TITLE: GeneratorSpec = GeneratorSpec {
    name: "job_title",
    label: "职位",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Profession`
static SPEC_PROFESSION: GeneratorSpec = GeneratorSpec {
    name: "profession",
    label: "职业",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Industry`
static SPEC_INDUSTRY: GeneratorSpec = GeneratorSpec {
    name: "industry",
    label: "行业",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Seniority`
static SPEC_SENIORITY: GeneratorSpec = GeneratorSpec {
    name: "seniority",
    label: "职级",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Field`
static SPEC_FIELD: GeneratorSpec = GeneratorSpec {
    name: "field",
    label: "领域",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Position`
static SPEC_POSITION: GeneratorSpec = GeneratorSpec {
    name: "position",
    label: "岗位",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Buzzword`
static SPEC_BUZZWORD: GeneratorSpec = GeneratorSpec {
    name: "buzzword",
    label: "行业术语",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BuzzwordMiddle`
static SPEC_BUZZWORD_MIDDLE: GeneratorSpec = GeneratorSpec {
    name: "buzzword_middle",
    label: "术语中段",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BuzzwordTail`
static SPEC_BUZZWORD_TAIL: GeneratorSpec = GeneratorSpec {
    name: "buzzword_tail",
    label: "术语尾段",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `CatchPhrase`
static SPEC_CATCH_PHRASE: GeneratorSpec = GeneratorSpec {
    name: "catch_phrase",
    label: "业务口号",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BsVerb`
static SPEC_BS_VERB: GeneratorSpec = GeneratorSpec {
    name: "bs_verb",
    label: "套话动词",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BsAdj`
static SPEC_BS_ADJ: GeneratorSpec = GeneratorSpec {
    name: "bs_adj",
    label: "套话形容词",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BsNoun`
static SPEC_BS_NOUN: GeneratorSpec = GeneratorSpec {
    name: "bs_noun",
    label: "套话名词",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Bs`
static SPEC_BS: GeneratorSpec = GeneratorSpec {
    name: "bs",
    label: "套话短语",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `CurrencyCode`
static SPEC_CURRENCY_CODE: GeneratorSpec = GeneratorSpec {
    name: "currency_code",
    label: "货币代码",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `CurrencyName`
static SPEC_CURRENCY_NAME: GeneratorSpec = GeneratorSpec {
    name: "currency_name",
    label: "货币名称",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `CurrencySymbol`
static SPEC_CURRENCY_SYMBOL: GeneratorSpec = GeneratorSpec {
    name: "currency_symbol",
    label: "货币符号",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `Bic`
static SPEC_BIC: GeneratorSpec = GeneratorSpec {
    name: "bic",
    label: "BIC 银行代码",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `Isin`
static SPEC_ISIN: GeneratorSpec = GeneratorSpec {
    name: "isin",
    label: "ISIN 证券代码",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `CreditCardNumber`
static SPEC_CREDIT_CARD_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "credit_card_number",
    label: "信用卡号",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `UuidV1`
static SPEC_UUID_V1: GeneratorSpec = GeneratorSpec {
    name: "uuid_v1",
    label: "UUID v1",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UuidV3`
static SPEC_UUID_V3: GeneratorSpec = GeneratorSpec {
    name: "uuid_v3",
    label: "UUID v3",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UuidV4`
static SPEC_UUID_V4: GeneratorSpec = GeneratorSpec {
    name: "uuid_v4",
    label: "UUID v4",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UuidV5`
static SPEC_UUID_V5: GeneratorSpec = GeneratorSpec {
    name: "uuid_v5",
    label: "UUID v5",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `Url`
static SPEC_URL: GeneratorSpec = GeneratorSpec {
    name: "url",
    label: "URL",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UserAgent`
static SPEC_USER_AGENT: GeneratorSpec = GeneratorSpec {
    name: "user_agent",
    label: "User-Agent",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `MimeType`
static SPEC_MIME_TYPE: GeneratorSpec = GeneratorSpec {
    name: "mime_type",
    label: "MIME 类型",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `Semver`
static SPEC_SEMVER: GeneratorSpec = GeneratorSpec {
    name: "semver",
    label: "语义化版本",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `SemverStable`
static SPEC_SEMVER_STABLE: GeneratorSpec = GeneratorSpec {
    name: "semver_stable",
    label: "语义化版本（稳定）",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `SemverUnstable`
static SPEC_SEMVER_UNSTABLE: GeneratorSpec = GeneratorSpec {
    name: "semver_unstable",
    label: "语义化版本（预发布）",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `FilePath`
static SPEC_FILE_PATH: GeneratorSpec = GeneratorSpec {
    name: "file_path",
    label: "文件路径",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `FileName`
static SPEC_FILE_NAME: GeneratorSpec = GeneratorSpec {
    name: "file_name",
    label: "文件名",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `FileExtension`
static SPEC_FILE_EXTENSION: GeneratorSpec = GeneratorSpec {
    name: "file_extension",
    label: "文件扩展名",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `DirPath`
static SPEC_DIR_PATH: GeneratorSpec = GeneratorSpec {
    name: "dir_path",
    label: "目录路径",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `ImageUrl`
static SPEC_IMAGE_URL: GeneratorSpec = GeneratorSpec {
    name: "image_url",
    label: "图片 URL",
    category: GeneratorCategory::Image,
    params: &[
        ParamField {
            key: "width",
            label: "宽",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "height",
            label: "高",
            kind: ParamKind::Int,
        },
    ],
};

/// `ImageUrlWithSeed`
static SPEC_IMAGE_URL_WITH_SEED: GeneratorSpec = GeneratorSpec {
    name: "image_url_with_seed",
    label: "图片 URL（固定种子）",
    category: GeneratorCategory::Image,
    params: &[
        ParamField {
            key: "width",
            label: "宽",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "height",
            label: "高",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "seed",
            label: "种子",
            kind: ParamKind::Int,
        },
    ],
};

/// `ImageUrlGrayscale`
static SPEC_IMAGE_URL_GRAYSCALE: GeneratorSpec = GeneratorSpec {
    name: "image_url_grayscale",
    label: "图片 URL（灰度）",
    category: GeneratorCategory::Image,
    params: &[
        ParamField {
            key: "width",
            label: "宽",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "height",
            label: "高",
            kind: ParamKind::Int,
        },
    ],
};

/// `ImageUrlBlur`
static SPEC_IMAGE_URL_BLUR: GeneratorSpec = GeneratorSpec {
    name: "image_url_blur",
    label: "图片 URL（模糊）",
    category: GeneratorCategory::Image,
    params: &[
        ParamField {
            key: "width",
            label: "宽",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "height",
            label: "高",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "blur_amount",
            label: "模糊程度",
            kind: ParamKind::Int,
        },
    ],
};

/// `ImageUrlCustom`
static SPEC_IMAGE_URL_CUSTOM: GeneratorSpec = GeneratorSpec {
    name: "image_url_custom",
    label: "图片 URL（自定义）",
    category: GeneratorCategory::Image,
    params: &[
        ParamField {
            key: "width",
            label: "宽",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "height",
            label: "高",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "grayscale",
            label: "灰度",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "blur_amount",
            label: "模糊程度",
            kind: ParamKind::Int,
        },
        ParamField {
            key: "seed",
            label: "种子",
            kind: ParamKind::Int,
        },
    ],
};

/// `HexColor`
static SPEC_HEX_COLOR: GeneratorSpec = GeneratorSpec {
    name: "hex_color",
    label: "HEX 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `RgbColor`
static SPEC_RGB_COLOR: GeneratorSpec = GeneratorSpec {
    name: "rgb_color",
    label: "RGB 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `RgbaColor`
static SPEC_RGBA_COLOR: GeneratorSpec = GeneratorSpec {
    name: "rgba_color",
    label: "RGBA 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `HslColor`
static SPEC_HSL_COLOR: GeneratorSpec = GeneratorSpec {
    name: "hsl_color",
    label: "HSL 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `HslaColor`
static SPEC_HSLA_COLOR: GeneratorSpec = GeneratorSpec {
    name: "hsla_color",
    label: "HSLA 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `Color`
static SPEC_COLOR: GeneratorSpec = GeneratorSpec {
    name: "color",
    label: "颜色名",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `FerroidULID`
static SPEC_FERROID_U_L_I_D: GeneratorSpec = GeneratorSpec {
    name: "ferroid_u_l_i_d",
    label: "ULID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidTwitterId`
static SPEC_FERROID_TWITTER_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_twitter_id",
    label: "Twitter ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidInstagramId`
static SPEC_FERROID_INSTAGRAM_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_instagram_id",
    label: "Instagram ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidMastodonId`
static SPEC_FERROID_MASTODON_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_mastodon_id",
    label: "Mastodon ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidDiscordId`
static SPEC_FERROID_DISCORD_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_discord_id",
    label: "Discord ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `Isbn`
static SPEC_ISBN: GeneratorSpec = GeneratorSpec {
    name: "isbn",
    label: "ISBN",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `Isbn10`
static SPEC_ISBN10: GeneratorSpec = GeneratorSpec {
    name: "isbn10",
    label: "ISBN-10",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `Isbn13`
static SPEC_ISBN13: GeneratorSpec = GeneratorSpec {
    name: "isbn13",
    label: "ISBN-13",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `RfcStatusCode`
static SPEC_RFC_STATUS_CODE: GeneratorSpec = GeneratorSpec {
    name: "rfc_status_code",
    label: "RFC 状态码",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `ValidStatusCode`
static SPEC_VALID_STATUS_CODE: GeneratorSpec = GeneratorSpec {
    name: "valid_status_code",
    label: "有效 HTTP 状态码",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `LicencePlate`
static SPEC_LICENCE_PLATE: GeneratorSpec = GeneratorSpec {
    name: "licence_plate",
    label: "车牌号",
    category: GeneratorCategory::Vehicle,
    params: &[],
};

/// `HealthInsuranceCode`
static SPEC_HEALTH_INSURANCE_CODE: GeneratorSpec = GeneratorSpec {
    name: "health_insurance_code",
    label: "医保编号",
    category: GeneratorCategory::Vehicle,
    params: &[],
};

/// `ForeignKey`
static SPEC_FOREIGN_KEY: GeneratorSpec = GeneratorSpec {
    name: "foreign_key",
    label: "外键取值",
    category: GeneratorCategory::Constraint,
    params: &[ParamField {
        key: "values",
        label: "取值集合",
        kind: ParamKind::Complex,
    }],
};

/// `Sequence`
static SPEC_SEQUENCE: GeneratorSpec = GeneratorSpec {
    name: "sequence",
    label: "序列取值",
    category: GeneratorCategory::Constraint,
    params: &[
        ParamField {
            key: "values",
            label: "取值集合",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "cycle",
            label: "循环",
            kind: ParamKind::Bool,
        },
    ],
};

/// `Weighted`
static SPEC_WEIGHTED: GeneratorSpec = GeneratorSpec {
    name: "weighted",
    label: "加权取值",
    category: GeneratorCategory::Constraint,
    params: &[ParamField {
        key: "choices",
        label: "加权选项",
        kind: ParamKind::Complex,
    }],
};

// ==================== 查询入口 ====================

/// 当前配置对应的规格（穷尽 match：新增变体将编译失败，强制补规格）。
pub fn spec_of(config: &GeneratorConfig) -> &'static GeneratorSpec {
    match config {
        GeneratorConfig::AutoIncrement { .. } => &SPEC_AUTO_INCREMENT,
        GeneratorConfig::RandomInt { .. } => &SPEC_RANDOM_INT,
        GeneratorConfig::RandomFloat { .. } => &SPEC_RANDOM_FLOAT,
        GeneratorConfig::RandomDecimal { .. } => &SPEC_RANDOM_DECIMAL,
        GeneratorConfig::Digit => &SPEC_DIGIT,
        GeneratorConfig::NumberWithFormat { .. } => &SPEC_NUMBER_WITH_FORMAT,
        GeneratorConfig::Normal { .. } => &SPEC_NORMAL,
        GeneratorConfig::LogNormal { .. } => &SPEC_LOG_NORMAL,
        GeneratorConfig::RandomWalk { .. } => &SPEC_RANDOM_WALK,
        GeneratorConfig::Boolean { .. } => &SPEC_BOOLEAN,
        GeneratorConfig::Poisson { .. } => &SPEC_POISSON,
        GeneratorConfig::Exponential { .. } => &SPEC_EXPONENTIAL,
        GeneratorConfig::Pareto { .. } => &SPEC_PARETO,
        GeneratorConfig::Beta { .. } => &SPEC_BETA,
        GeneratorConfig::Binomial { .. } => &SPEC_BINOMIAL,
        GeneratorConfig::TimeSeries { .. } => &SPEC_TIME_SERIES,
        GeneratorConfig::Constant { .. } => &SPEC_CONSTANT,
        GeneratorConfig::Words { .. } => &SPEC_WORDS,
        GeneratorConfig::Sentence { .. } => &SPEC_SENTENCE,
        GeneratorConfig::Sentences { .. } => &SPEC_SENTENCES,
        GeneratorConfig::Paragraph { .. } => &SPEC_PARAGRAPH,
        GeneratorConfig::Paragraphs { .. } => &SPEC_PARAGRAPHS,
        GeneratorConfig::Word => &SPEC_WORD,
        GeneratorConfig::Regex { .. } => &SPEC_REGEX,
        GeneratorConfig::Template { .. } => &SPEC_TEMPLATE,
        GeneratorConfig::MarkdownItalicWord => &SPEC_MARKDOWN_ITALIC_WORD,
        GeneratorConfig::MarkdownBoldWord => &SPEC_MARKDOWN_BOLD_WORD,
        GeneratorConfig::MarkdownLink => &SPEC_MARKDOWN_LINK,
        GeneratorConfig::MarkdownBulletPoints => &SPEC_MARKDOWN_BULLET_POINTS,
        GeneratorConfig::MarkdownListItems => &SPEC_MARKDOWN_LIST_ITEMS,
        GeneratorConfig::MarkdownBlockQuoteSingle => &SPEC_MARKDOWN_BLOCK_QUOTE_SINGLE,
        GeneratorConfig::MarkdownBlockQuoteMulti => &SPEC_MARKDOWN_BLOCK_QUOTE_MULTI,
        GeneratorConfig::MarkdownCode => &SPEC_MARKDOWN_CODE,
        GeneratorConfig::Name => &SPEC_NAME,
        GeneratorConfig::NameWithTitle => &SPEC_NAME_WITH_TITLE,
        GeneratorConfig::FirstName => &SPEC_FIRST_NAME,
        GeneratorConfig::LastName => &SPEC_LAST_NAME,
        GeneratorConfig::Title => &SPEC_TITLE,
        GeneratorConfig::Suffix => &SPEC_SUFFIX,
        GeneratorConfig::Email => &SPEC_EMAIL,
        GeneratorConfig::SafeEmail => &SPEC_SAFE_EMAIL,
        GeneratorConfig::FreeEmailProvider => &SPEC_FREE_EMAIL_PROVIDER,
        GeneratorConfig::DomainSuffix => &SPEC_DOMAIN_SUFFIX,
        GeneratorConfig::FreeEmail => &SPEC_FREE_EMAIL,
        GeneratorConfig::PhoneNumber => &SPEC_PHONE_NUMBER,
        GeneratorConfig::CellNumber => &SPEC_CELL_NUMBER,
        GeneratorConfig::Username => &SPEC_USERNAME,
        GeneratorConfig::Password { .. } => &SPEC_PASSWORD,
        GeneratorConfig::Country => &SPEC_COUNTRY,
        GeneratorConfig::CountryCode => &SPEC_COUNTRY_CODE,
        GeneratorConfig::CountryName => &SPEC_COUNTRY_NAME,
        GeneratorConfig::City => &SPEC_CITY,
        GeneratorConfig::CityPrefix => &SPEC_CITY_PREFIX,
        GeneratorConfig::CitySuffix => &SPEC_CITY_SUFFIX,
        GeneratorConfig::StateName => &SPEC_STATE_NAME,
        GeneratorConfig::StateAbbr => &SPEC_STATE_ABBR,
        GeneratorConfig::StreetName => &SPEC_STREET_NAME,
        GeneratorConfig::StreetSuffix => &SPEC_STREET_SUFFIX,
        GeneratorConfig::ZipCode => &SPEC_ZIP_CODE,
        GeneratorConfig::PostCode => &SPEC_POST_CODE,
        GeneratorConfig::BuildingNumber => &SPEC_BUILDING_NUMBER,
        GeneratorConfig::SecondaryAddress => &SPEC_SECONDARY_ADDRESS,
        GeneratorConfig::SecondaryAddressType => &SPEC_SECONDARY_ADDRESS_TYPE,
        GeneratorConfig::Latitude => &SPEC_LATITUDE,
        GeneratorConfig::Longitude => &SPEC_LONGITUDE,
        GeneratorConfig::Geohash { .. } => &SPEC_GEOHASH,
        GeneratorConfig::TimeZone => &SPEC_TIME_ZONE,
        GeneratorConfig::IpAddress => &SPEC_IP_ADDRESS,
        GeneratorConfig::IPv4 => &SPEC_I_PV4,
        GeneratorConfig::IPv6 => &SPEC_I_PV6,
        GeneratorConfig::IP => &SPEC_I_P,
        GeneratorConfig::MacAddress => &SPEC_MAC_ADDRESS,
        GeneratorConfig::DateTime { .. } => &SPEC_DATE_TIME,
        GeneratorConfig::DateTimeBefore { .. } => &SPEC_DATE_TIME_BEFORE,
        GeneratorConfig::DateTimeAfter { .. } => &SPEC_DATE_TIME_AFTER,
        GeneratorConfig::DateTimeBetween { .. } => &SPEC_DATE_TIME_BETWEEN,
        GeneratorConfig::Date { .. } => &SPEC_DATE,
        GeneratorConfig::Time => &SPEC_TIME,
        GeneratorConfig::Duration => &SPEC_DURATION,
        GeneratorConfig::SequentialDate { .. } => &SPEC_SEQUENTIAL_DATE,
        GeneratorConfig::SequentialDateWithGaps { .. } => &SPEC_SEQUENTIAL_DATE_WITH_GAPS,
        GeneratorConfig::CompanyName => &SPEC_COMPANY_NAME,
        GeneratorConfig::CompanySuffix => &SPEC_COMPANY_SUFFIX,
        GeneratorConfig::JobTitle => &SPEC_JOB_TITLE,
        GeneratorConfig::Profession => &SPEC_PROFESSION,
        GeneratorConfig::Industry => &SPEC_INDUSTRY,
        GeneratorConfig::Seniority => &SPEC_SENIORITY,
        GeneratorConfig::Field => &SPEC_FIELD,
        GeneratorConfig::Position => &SPEC_POSITION,
        GeneratorConfig::Buzzword => &SPEC_BUZZWORD,
        GeneratorConfig::BuzzwordMiddle => &SPEC_BUZZWORD_MIDDLE,
        GeneratorConfig::BuzzwordTail => &SPEC_BUZZWORD_TAIL,
        GeneratorConfig::CatchPhrase => &SPEC_CATCH_PHRASE,
        GeneratorConfig::BsVerb => &SPEC_BS_VERB,
        GeneratorConfig::BsAdj => &SPEC_BS_ADJ,
        GeneratorConfig::BsNoun => &SPEC_BS_NOUN,
        GeneratorConfig::Bs => &SPEC_BS,
        GeneratorConfig::CurrencyCode => &SPEC_CURRENCY_CODE,
        GeneratorConfig::CurrencyName => &SPEC_CURRENCY_NAME,
        GeneratorConfig::CurrencySymbol => &SPEC_CURRENCY_SYMBOL,
        GeneratorConfig::Bic => &SPEC_BIC,
        GeneratorConfig::Isin => &SPEC_ISIN,
        GeneratorConfig::CreditCardNumber => &SPEC_CREDIT_CARD_NUMBER,
        GeneratorConfig::UuidV1 => &SPEC_UUID_V1,
        GeneratorConfig::UuidV3 => &SPEC_UUID_V3,
        GeneratorConfig::UuidV4 => &SPEC_UUID_V4,
        GeneratorConfig::UuidV5 => &SPEC_UUID_V5,
        GeneratorConfig::Url => &SPEC_URL,
        GeneratorConfig::UserAgent => &SPEC_USER_AGENT,
        GeneratorConfig::MimeType => &SPEC_MIME_TYPE,
        GeneratorConfig::Semver => &SPEC_SEMVER,
        GeneratorConfig::SemverStable => &SPEC_SEMVER_STABLE,
        GeneratorConfig::SemverUnstable => &SPEC_SEMVER_UNSTABLE,
        GeneratorConfig::FilePath => &SPEC_FILE_PATH,
        GeneratorConfig::FileName => &SPEC_FILE_NAME,
        GeneratorConfig::FileExtension => &SPEC_FILE_EXTENSION,
        GeneratorConfig::DirPath => &SPEC_DIR_PATH,
        GeneratorConfig::ImageUrl { .. } => &SPEC_IMAGE_URL,
        GeneratorConfig::ImageUrlWithSeed { .. } => &SPEC_IMAGE_URL_WITH_SEED,
        GeneratorConfig::ImageUrlGrayscale { .. } => &SPEC_IMAGE_URL_GRAYSCALE,
        GeneratorConfig::ImageUrlBlur { .. } => &SPEC_IMAGE_URL_BLUR,
        GeneratorConfig::ImageUrlCustom { .. } => &SPEC_IMAGE_URL_CUSTOM,
        GeneratorConfig::HexColor => &SPEC_HEX_COLOR,
        GeneratorConfig::RgbColor => &SPEC_RGB_COLOR,
        GeneratorConfig::RgbaColor => &SPEC_RGBA_COLOR,
        GeneratorConfig::HslColor => &SPEC_HSL_COLOR,
        GeneratorConfig::HslaColor => &SPEC_HSLA_COLOR,
        GeneratorConfig::Color => &SPEC_COLOR,
        GeneratorConfig::FerroidULID => &SPEC_FERROID_U_L_I_D,
        GeneratorConfig::FerroidTwitterId => &SPEC_FERROID_TWITTER_ID,
        GeneratorConfig::FerroidInstagramId => &SPEC_FERROID_INSTAGRAM_ID,
        GeneratorConfig::FerroidMastodonId => &SPEC_FERROID_MASTODON_ID,
        GeneratorConfig::FerroidDiscordId => &SPEC_FERROID_DISCORD_ID,
        GeneratorConfig::Isbn => &SPEC_ISBN,
        GeneratorConfig::Isbn10 => &SPEC_ISBN10,
        GeneratorConfig::Isbn13 => &SPEC_ISBN13,
        GeneratorConfig::RfcStatusCode => &SPEC_RFC_STATUS_CODE,
        GeneratorConfig::ValidStatusCode => &SPEC_VALID_STATUS_CODE,
        GeneratorConfig::LicencePlate => &SPEC_LICENCE_PLATE,
        GeneratorConfig::HealthInsuranceCode => &SPEC_HEALTH_INSURANCE_CODE,
        GeneratorConfig::ForeignKey { .. } => &SPEC_FOREIGN_KEY,
        GeneratorConfig::Sequence { .. } => &SPEC_SEQUENCE,
        GeneratorConfig::Weighted { .. } => &SPEC_WEIGHTED,
    }
}

/// 全部生成器规格（选择器浏览用；顺序 = 分类顺序 + 变体声明顺序）。
pub fn all_specs() -> &'static [&'static GeneratorSpec] {
    ALL_SPECS
}

/// 规格表（static：避免每次调用构造临时数组）。
static ALL_SPECS: &[&GeneratorSpec] = &[
    &SPEC_AUTO_INCREMENT,
    &SPEC_RANDOM_INT,
    &SPEC_RANDOM_FLOAT,
    &SPEC_RANDOM_DECIMAL,
    &SPEC_DIGIT,
    &SPEC_NUMBER_WITH_FORMAT,
    &SPEC_NORMAL,
    &SPEC_LOG_NORMAL,
    &SPEC_RANDOM_WALK,
    &SPEC_BOOLEAN,
    &SPEC_POISSON,
    &SPEC_EXPONENTIAL,
    &SPEC_PARETO,
    &SPEC_BETA,
    &SPEC_BINOMIAL,
    &SPEC_TIME_SERIES,
    &SPEC_CONSTANT,
    &SPEC_WORDS,
    &SPEC_SENTENCE,
    &SPEC_SENTENCES,
    &SPEC_PARAGRAPH,
    &SPEC_PARAGRAPHS,
    &SPEC_WORD,
    &SPEC_REGEX,
    &SPEC_TEMPLATE,
    &SPEC_MARKDOWN_ITALIC_WORD,
    &SPEC_MARKDOWN_BOLD_WORD,
    &SPEC_MARKDOWN_LINK,
    &SPEC_MARKDOWN_BULLET_POINTS,
    &SPEC_MARKDOWN_LIST_ITEMS,
    &SPEC_MARKDOWN_BLOCK_QUOTE_SINGLE,
    &SPEC_MARKDOWN_BLOCK_QUOTE_MULTI,
    &SPEC_MARKDOWN_CODE,
    &SPEC_NAME,
    &SPEC_NAME_WITH_TITLE,
    &SPEC_FIRST_NAME,
    &SPEC_LAST_NAME,
    &SPEC_TITLE,
    &SPEC_SUFFIX,
    &SPEC_EMAIL,
    &SPEC_SAFE_EMAIL,
    &SPEC_FREE_EMAIL_PROVIDER,
    &SPEC_DOMAIN_SUFFIX,
    &SPEC_FREE_EMAIL,
    &SPEC_PHONE_NUMBER,
    &SPEC_CELL_NUMBER,
    &SPEC_USERNAME,
    &SPEC_PASSWORD,
    &SPEC_COUNTRY,
    &SPEC_COUNTRY_CODE,
    &SPEC_COUNTRY_NAME,
    &SPEC_CITY,
    &SPEC_CITY_PREFIX,
    &SPEC_CITY_SUFFIX,
    &SPEC_STATE_NAME,
    &SPEC_STATE_ABBR,
    &SPEC_STREET_NAME,
    &SPEC_STREET_SUFFIX,
    &SPEC_ZIP_CODE,
    &SPEC_POST_CODE,
    &SPEC_BUILDING_NUMBER,
    &SPEC_SECONDARY_ADDRESS,
    &SPEC_SECONDARY_ADDRESS_TYPE,
    &SPEC_LATITUDE,
    &SPEC_LONGITUDE,
    &SPEC_GEOHASH,
    &SPEC_TIME_ZONE,
    &SPEC_IP_ADDRESS,
    &SPEC_I_PV4,
    &SPEC_I_PV6,
    &SPEC_I_P,
    &SPEC_MAC_ADDRESS,
    &SPEC_DATE_TIME,
    &SPEC_DATE_TIME_BEFORE,
    &SPEC_DATE_TIME_AFTER,
    &SPEC_DATE_TIME_BETWEEN,
    &SPEC_DATE,
    &SPEC_TIME,
    &SPEC_DURATION,
    &SPEC_SEQUENTIAL_DATE,
    &SPEC_SEQUENTIAL_DATE_WITH_GAPS,
    &SPEC_COMPANY_NAME,
    &SPEC_COMPANY_SUFFIX,
    &SPEC_JOB_TITLE,
    &SPEC_PROFESSION,
    &SPEC_INDUSTRY,
    &SPEC_SENIORITY,
    &SPEC_FIELD,
    &SPEC_POSITION,
    &SPEC_BUZZWORD,
    &SPEC_BUZZWORD_MIDDLE,
    &SPEC_BUZZWORD_TAIL,
    &SPEC_CATCH_PHRASE,
    &SPEC_BS_VERB,
    &SPEC_BS_ADJ,
    &SPEC_BS_NOUN,
    &SPEC_BS,
    &SPEC_CURRENCY_CODE,
    &SPEC_CURRENCY_NAME,
    &SPEC_CURRENCY_SYMBOL,
    &SPEC_BIC,
    &SPEC_ISIN,
    &SPEC_CREDIT_CARD_NUMBER,
    &SPEC_UUID_V1,
    &SPEC_UUID_V3,
    &SPEC_UUID_V4,
    &SPEC_UUID_V5,
    &SPEC_URL,
    &SPEC_USER_AGENT,
    &SPEC_MIME_TYPE,
    &SPEC_SEMVER,
    &SPEC_SEMVER_STABLE,
    &SPEC_SEMVER_UNSTABLE,
    &SPEC_FILE_PATH,
    &SPEC_FILE_NAME,
    &SPEC_FILE_EXTENSION,
    &SPEC_DIR_PATH,
    &SPEC_IMAGE_URL,
    &SPEC_IMAGE_URL_WITH_SEED,
    &SPEC_IMAGE_URL_GRAYSCALE,
    &SPEC_IMAGE_URL_BLUR,
    &SPEC_IMAGE_URL_CUSTOM,
    &SPEC_HEX_COLOR,
    &SPEC_RGB_COLOR,
    &SPEC_RGBA_COLOR,
    &SPEC_HSL_COLOR,
    &SPEC_HSLA_COLOR,
    &SPEC_COLOR,
    &SPEC_FERROID_U_L_I_D,
    &SPEC_FERROID_TWITTER_ID,
    &SPEC_FERROID_INSTAGRAM_ID,
    &SPEC_FERROID_MASTODON_ID,
    &SPEC_FERROID_DISCORD_ID,
    &SPEC_ISBN,
    &SPEC_ISBN10,
    &SPEC_ISBN13,
    &SPEC_RFC_STATUS_CODE,
    &SPEC_VALID_STATUS_CODE,
    &SPEC_LICENCE_PLATE,
    &SPEC_HEALTH_INSURANCE_CODE,
    &SPEC_FOREIGN_KEY,
    &SPEC_SEQUENCE,
    &SPEC_WEIGHTED,
];

/// 某分类下的规格。
pub fn specs_in(category: GeneratorCategory) -> Vec<&'static GeneratorSpec> {
    all_specs()
        .iter()
        .copied()
        .filter(|s| s.category == category)
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
        },
        "date_time_before" => GeneratorConfig::DateTimeBefore {
            before: "2020-01-01".to_string(),
        },
        "date_time_after" => GeneratorConfig::DateTimeAfter {
            after: "2020-01-01".to_string(),
        },
        "date_time_between" => GeneratorConfig::DateTimeBetween {
            start: "".to_string(),
            end: "2025-12-31".to_string(),
        },
        "date" => GeneratorConfig::Date {
            min: "2020-01-01".to_string(),
            max: "2025-12-31".to_string(),
        },
        "time" => GeneratorConfig::Time,
        "duration" => GeneratorConfig::Duration,
        "sequential_date" => GeneratorConfig::SequentialDate {
            start: "".to_string(),
            step_seconds: 86400,
        },
        "sequential_date_with_gaps" => GeneratorConfig::SequentialDateWithGaps {
            start: "".to_string(),
            step_seconds: 86400,
            miss_probability: 0.1,
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
        "foreign_key" => GeneratorConfig::ForeignKey { values: Vec::new() },
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

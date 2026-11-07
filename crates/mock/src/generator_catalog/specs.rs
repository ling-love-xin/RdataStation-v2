// ==================== 规格常量 ====================

use super::*;

// ==================== 规格常量 ====================

/// `AutoIncrement`
pub(crate) static SPEC_AUTO_INCREMENT: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_RANDOM_INT: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_RANDOM_FLOAT: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_RANDOM_DECIMAL: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_DIGIT: GeneratorSpec = GeneratorSpec {
    name: "digit",
    label: "单个数字",
    category: GeneratorCategory::Numeric,
    params: &[],
};

/// `NumberWithFormat`
pub(crate) static SPEC_NUMBER_WITH_FORMAT: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_NORMAL: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_LOG_NORMAL: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_RANDOM_WALK: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_BOOLEAN: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_POISSON: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_EXPONENTIAL: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_PARETO: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_BETA: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_BINOMIAL: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_TIME_SERIES: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_CONSTANT: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_WORDS: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_SENTENCE: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_SENTENCES: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_PARAGRAPH: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_PARAGRAPHS: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_WORD: GeneratorSpec = GeneratorSpec {
    name: "word",
    label: "随机单词",
    category: GeneratorCategory::Text,
    params: &[],
};

/// `Regex`
pub(crate) static SPEC_REGEX: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_TEMPLATE: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_MARKDOWN_ITALIC_WORD: GeneratorSpec = GeneratorSpec {
    name: "markdown_italic_word",
    label: "Markdown 斜体词",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBoldWord`
pub(crate) static SPEC_MARKDOWN_BOLD_WORD: GeneratorSpec = GeneratorSpec {
    name: "markdown_bold_word",
    label: "Markdown 粗体词",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownLink`
pub(crate) static SPEC_MARKDOWN_LINK: GeneratorSpec = GeneratorSpec {
    name: "markdown_link",
    label: "Markdown 链接",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBulletPoints`
pub(crate) static SPEC_MARKDOWN_BULLET_POINTS: GeneratorSpec = GeneratorSpec {
    name: "markdown_bullet_points",
    label: "Markdown 无序列表",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownListItems`
pub(crate) static SPEC_MARKDOWN_LIST_ITEMS: GeneratorSpec = GeneratorSpec {
    name: "markdown_list_items",
    label: "Markdown 列表项",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBlockQuoteSingle`
pub(crate) static SPEC_MARKDOWN_BLOCK_QUOTE_SINGLE: GeneratorSpec = GeneratorSpec {
    name: "markdown_block_quote_single",
    label: "Markdown 单行引用",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownBlockQuoteMulti`
pub(crate) static SPEC_MARKDOWN_BLOCK_QUOTE_MULTI: GeneratorSpec = GeneratorSpec {
    name: "markdown_block_quote_multi",
    label: "Markdown 多行引用",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `MarkdownCode`
pub(crate) static SPEC_MARKDOWN_CODE: GeneratorSpec = GeneratorSpec {
    name: "markdown_code",
    label: "Markdown 代码块",
    category: GeneratorCategory::Markdown,
    params: &[],
};

/// `Name`
pub(crate) static SPEC_NAME: GeneratorSpec = GeneratorSpec {
    name: "name",
    label: "姓名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `NameWithTitle`
pub(crate) static SPEC_NAME_WITH_TITLE: GeneratorSpec = GeneratorSpec {
    name: "name_with_title",
    label: "带称谓姓名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `FirstName`
pub(crate) static SPEC_FIRST_NAME: GeneratorSpec = GeneratorSpec {
    name: "first_name",
    label: "名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `LastName`
pub(crate) static SPEC_LAST_NAME: GeneratorSpec = GeneratorSpec {
    name: "last_name",
    label: "姓",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Title`
pub(crate) static SPEC_TITLE: GeneratorSpec = GeneratorSpec {
    name: "title",
    label: "称谓",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Suffix`
pub(crate) static SPEC_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "suffix",
    label: "名字后缀",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Email`
pub(crate) static SPEC_EMAIL: GeneratorSpec = GeneratorSpec {
    name: "email",
    label: "邮箱地址",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `SafeEmail`
pub(crate) static SPEC_SAFE_EMAIL: GeneratorSpec = GeneratorSpec {
    name: "safe_email",
    label: "安全邮箱",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `FreeEmailProvider`
pub(crate) static SPEC_FREE_EMAIL_PROVIDER: GeneratorSpec = GeneratorSpec {
    name: "free_email_provider",
    label: "免费邮箱服务商",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `DomainSuffix`
pub(crate) static SPEC_DOMAIN_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "domain_suffix",
    label: "域名后缀",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `FreeEmail`
pub(crate) static SPEC_FREE_EMAIL: GeneratorSpec = GeneratorSpec {
    name: "free_email",
    label: "免费邮箱",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `PhoneNumber`
pub(crate) static SPEC_PHONE_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "phone_number",
    label: "电话号码",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `CellNumber`
pub(crate) static SPEC_CELL_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "cell_number",
    label: "手机号",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Username`
pub(crate) static SPEC_USERNAME: GeneratorSpec = GeneratorSpec {
    name: "username",
    label: "用户名",
    category: GeneratorCategory::Person,
    params: &[],
};

/// `Password`
pub(crate) static SPEC_PASSWORD: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_COUNTRY: GeneratorSpec = GeneratorSpec {
    name: "country",
    label: "国家",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CountryCode`
pub(crate) static SPEC_COUNTRY_CODE: GeneratorSpec = GeneratorSpec {
    name: "country_code",
    label: "国家代码",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CountryName`
pub(crate) static SPEC_COUNTRY_NAME: GeneratorSpec = GeneratorSpec {
    name: "country_name",
    label: "国家名",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `City`
pub(crate) static SPEC_CITY: GeneratorSpec = GeneratorSpec {
    name: "city",
    label: "城市",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CityPrefix`
pub(crate) static SPEC_CITY_PREFIX: GeneratorSpec = GeneratorSpec {
    name: "city_prefix",
    label: "城市前缀",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `CitySuffix`
pub(crate) static SPEC_CITY_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "city_suffix",
    label: "城市后缀",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StateName`
pub(crate) static SPEC_STATE_NAME: GeneratorSpec = GeneratorSpec {
    name: "state_name",
    label: "省/州名",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StateAbbr`
pub(crate) static SPEC_STATE_ABBR: GeneratorSpec = GeneratorSpec {
    name: "state_abbr",
    label: "省/州缩写",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StreetName`
pub(crate) static SPEC_STREET_NAME: GeneratorSpec = GeneratorSpec {
    name: "street_name",
    label: "街道名",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `StreetSuffix`
pub(crate) static SPEC_STREET_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "street_suffix",
    label: "街道后缀",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `ZipCode`
pub(crate) static SPEC_ZIP_CODE: GeneratorSpec = GeneratorSpec {
    name: "zip_code",
    label: "邮政编码",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `PostCode`
pub(crate) static SPEC_POST_CODE: GeneratorSpec = GeneratorSpec {
    name: "post_code",
    label: "邮政编号",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `BuildingNumber`
pub(crate) static SPEC_BUILDING_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "building_number",
    label: "楼牌号",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `SecondaryAddress`
pub(crate) static SPEC_SECONDARY_ADDRESS: GeneratorSpec = GeneratorSpec {
    name: "secondary_address",
    label: "次级地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `SecondaryAddressType`
pub(crate) static SPEC_SECONDARY_ADDRESS_TYPE: GeneratorSpec = GeneratorSpec {
    name: "secondary_address_type",
    label: "次级地址类型",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `Latitude`
pub(crate) static SPEC_LATITUDE: GeneratorSpec = GeneratorSpec {
    name: "latitude",
    label: "纬度",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `Longitude`
pub(crate) static SPEC_LONGITUDE: GeneratorSpec = GeneratorSpec {
    name: "longitude",
    label: "经度",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `Geohash`
pub(crate) static SPEC_GEOHASH: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_TIME_ZONE: GeneratorSpec = GeneratorSpec {
    name: "time_zone",
    label: "时区",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IpAddress`
pub(crate) static SPEC_IP_ADDRESS: GeneratorSpec = GeneratorSpec {
    name: "ip_address",
    label: "IP 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IPv4`
pub(crate) static SPEC_I_PV4: GeneratorSpec = GeneratorSpec {
    name: "i_pv4",
    label: "IPv4 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IPv6`
pub(crate) static SPEC_I_PV6: GeneratorSpec = GeneratorSpec {
    name: "i_pv6",
    label: "IPv6 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `IP`
pub(crate) static SPEC_I_P: GeneratorSpec = GeneratorSpec {
    name: "i_p",
    label: "IP（v4/v6）",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `MacAddress`
pub(crate) static SPEC_MAC_ADDRESS: GeneratorSpec = GeneratorSpec {
    name: "mac_address",
    label: "MAC 地址",
    category: GeneratorCategory::Address,
    params: &[],
};

/// `DateTime`
pub(crate) static SPEC_DATE_TIME: GeneratorSpec = GeneratorSpec {
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
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hours_only",
            label: "仅工作时段",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hour_start",
            label: "工作时段起（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_hour_end",
            label: "工作时段止（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `DateTimeBefore`
pub(crate) static SPEC_DATE_TIME_BEFORE: GeneratorSpec = GeneratorSpec {
    name: "date_time_before",
    label: "某时间之前",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "before",
            label: "早于",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hours_only",
            label: "仅工作时段",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hour_start",
            label: "工作时段起（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_hour_end",
            label: "工作时段止（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `DateTimeAfter`
pub(crate) static SPEC_DATE_TIME_AFTER: GeneratorSpec = GeneratorSpec {
    name: "date_time_after",
    label: "某时间之后",
    category: GeneratorCategory::DateTime,
    params: &[
        ParamField {
            key: "after",
            label: "晚于",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hours_only",
            label: "仅工作时段",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hour_start",
            label: "工作时段起（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_hour_end",
            label: "工作时段止（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `DateTimeBetween`
pub(crate) static SPEC_DATE_TIME_BETWEEN: GeneratorSpec = GeneratorSpec {
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
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hours_only",
            label: "仅工作时段",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_hour_start",
            label: "工作时段起（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_hour_end",
            label: "工作时段止（HH:MM）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `Date`
pub(crate) static SPEC_DATE: GeneratorSpec = GeneratorSpec {
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
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `Time`
pub(crate) static SPEC_TIME: GeneratorSpec = GeneratorSpec {
    name: "time",
    label: "时间",
    category: GeneratorCategory::DateTime,
    params: &[],
};

/// `Duration`
pub(crate) static SPEC_DURATION: GeneratorSpec = GeneratorSpec {
    name: "duration",
    label: "持续时间",
    category: GeneratorCategory::DateTime,
    params: &[],
};

/// `SequentialDate`
pub(crate) static SPEC_SEQUENTIAL_DATE: GeneratorSpec = GeneratorSpec {
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
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `SequentialDateWithGaps`
pub(crate) static SPEC_SEQUENTIAL_DATE_WITH_GAPS: GeneratorSpec = GeneratorSpec {
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
        ParamField {
            key: "workdays_only",
            label: "仅工作日",
            kind: ParamKind::Bool,
        },
        ParamField {
            key: "work_week",
            label: "工作周（周一~周日，1 上班）",
            kind: ParamKind::Text,
        },
        ParamField {
            key: "skip_dates",
            label: "跳过日期（节假日）",
            kind: ParamKind::Complex,
        },
        ParamField {
            key: "work_dates",
            label: "上班日期（调休）",
            kind: ParamKind::Complex,
        },
    ],
};

/// `CompanyName`
pub(crate) static SPEC_COMPANY_NAME: GeneratorSpec = GeneratorSpec {
    name: "company_name",
    label: "公司名",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `CompanySuffix`
pub(crate) static SPEC_COMPANY_SUFFIX: GeneratorSpec = GeneratorSpec {
    name: "company_suffix",
    label: "公司后缀",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `JobTitle`
pub(crate) static SPEC_JOB_TITLE: GeneratorSpec = GeneratorSpec {
    name: "job_title",
    label: "职位",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Profession`
pub(crate) static SPEC_PROFESSION: GeneratorSpec = GeneratorSpec {
    name: "profession",
    label: "职业",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Industry`
pub(crate) static SPEC_INDUSTRY: GeneratorSpec = GeneratorSpec {
    name: "industry",
    label: "行业",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Seniority`
pub(crate) static SPEC_SENIORITY: GeneratorSpec = GeneratorSpec {
    name: "seniority",
    label: "职级",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Field`
pub(crate) static SPEC_FIELD: GeneratorSpec = GeneratorSpec {
    name: "field",
    label: "领域",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Position`
pub(crate) static SPEC_POSITION: GeneratorSpec = GeneratorSpec {
    name: "position",
    label: "岗位",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Buzzword`
pub(crate) static SPEC_BUZZWORD: GeneratorSpec = GeneratorSpec {
    name: "buzzword",
    label: "行业术语",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BuzzwordMiddle`
pub(crate) static SPEC_BUZZWORD_MIDDLE: GeneratorSpec = GeneratorSpec {
    name: "buzzword_middle",
    label: "术语中段",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BuzzwordTail`
pub(crate) static SPEC_BUZZWORD_TAIL: GeneratorSpec = GeneratorSpec {
    name: "buzzword_tail",
    label: "术语尾段",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `CatchPhrase`
pub(crate) static SPEC_CATCH_PHRASE: GeneratorSpec = GeneratorSpec {
    name: "catch_phrase",
    label: "业务口号",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BsVerb`
pub(crate) static SPEC_BS_VERB: GeneratorSpec = GeneratorSpec {
    name: "bs_verb",
    label: "套话动词",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BsAdj`
pub(crate) static SPEC_BS_ADJ: GeneratorSpec = GeneratorSpec {
    name: "bs_adj",
    label: "套话形容词",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `BsNoun`
pub(crate) static SPEC_BS_NOUN: GeneratorSpec = GeneratorSpec {
    name: "bs_noun",
    label: "套话名词",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `Bs`
pub(crate) static SPEC_BS: GeneratorSpec = GeneratorSpec {
    name: "bs",
    label: "套话短语",
    category: GeneratorCategory::Business,
    params: &[],
};

/// `CurrencyCode`
pub(crate) static SPEC_CURRENCY_CODE: GeneratorSpec = GeneratorSpec {
    name: "currency_code",
    label: "货币代码",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `CurrencyName`
pub(crate) static SPEC_CURRENCY_NAME: GeneratorSpec = GeneratorSpec {
    name: "currency_name",
    label: "货币名称",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `CurrencySymbol`
pub(crate) static SPEC_CURRENCY_SYMBOL: GeneratorSpec = GeneratorSpec {
    name: "currency_symbol",
    label: "货币符号",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `Bic`
pub(crate) static SPEC_BIC: GeneratorSpec = GeneratorSpec {
    name: "bic",
    label: "BIC 银行代码",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `Isin`
pub(crate) static SPEC_ISIN: GeneratorSpec = GeneratorSpec {
    name: "isin",
    label: "ISIN 证券代码",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `CreditCardNumber`
pub(crate) static SPEC_CREDIT_CARD_NUMBER: GeneratorSpec = GeneratorSpec {
    name: "credit_card_number",
    label: "信用卡号",
    category: GeneratorCategory::Finance,
    params: &[],
};

/// `UuidV1`
pub(crate) static SPEC_UUID_V1: GeneratorSpec = GeneratorSpec {
    name: "uuid_v1",
    label: "UUID v1",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UuidV3`
pub(crate) static SPEC_UUID_V3: GeneratorSpec = GeneratorSpec {
    name: "uuid_v3",
    label: "UUID v3",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UuidV4`
pub(crate) static SPEC_UUID_V4: GeneratorSpec = GeneratorSpec {
    name: "uuid_v4",
    label: "UUID v4",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UuidV5`
pub(crate) static SPEC_UUID_V5: GeneratorSpec = GeneratorSpec {
    name: "uuid_v5",
    label: "UUID v5",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `Url`
pub(crate) static SPEC_URL: GeneratorSpec = GeneratorSpec {
    name: "url",
    label: "URL",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `UserAgent`
pub(crate) static SPEC_USER_AGENT: GeneratorSpec = GeneratorSpec {
    name: "user_agent",
    label: "User-Agent",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `MimeType`
pub(crate) static SPEC_MIME_TYPE: GeneratorSpec = GeneratorSpec {
    name: "mime_type",
    label: "MIME 类型",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `Semver`
pub(crate) static SPEC_SEMVER: GeneratorSpec = GeneratorSpec {
    name: "semver",
    label: "语义化版本",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `SemverStable`
pub(crate) static SPEC_SEMVER_STABLE: GeneratorSpec = GeneratorSpec {
    name: "semver_stable",
    label: "语义化版本（稳定）",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `SemverUnstable`
pub(crate) static SPEC_SEMVER_UNSTABLE: GeneratorSpec = GeneratorSpec {
    name: "semver_unstable",
    label: "语义化版本（预发布）",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `FilePath`
pub(crate) static SPEC_FILE_PATH: GeneratorSpec = GeneratorSpec {
    name: "file_path",
    label: "文件路径",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `FileName`
pub(crate) static SPEC_FILE_NAME: GeneratorSpec = GeneratorSpec {
    name: "file_name",
    label: "文件名",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `FileExtension`
pub(crate) static SPEC_FILE_EXTENSION: GeneratorSpec = GeneratorSpec {
    name: "file_extension",
    label: "文件扩展名",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `DirPath`
pub(crate) static SPEC_DIR_PATH: GeneratorSpec = GeneratorSpec {
    name: "dir_path",
    label: "目录路径",
    category: GeneratorCategory::Tech,
    params: &[],
};

/// `ImageUrl`
pub(crate) static SPEC_IMAGE_URL: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_IMAGE_URL_WITH_SEED: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_IMAGE_URL_GRAYSCALE: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_IMAGE_URL_BLUR: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_IMAGE_URL_CUSTOM: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_HEX_COLOR: GeneratorSpec = GeneratorSpec {
    name: "hex_color",
    label: "HEX 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `RgbColor`
pub(crate) static SPEC_RGB_COLOR: GeneratorSpec = GeneratorSpec {
    name: "rgb_color",
    label: "RGB 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `RgbaColor`
pub(crate) static SPEC_RGBA_COLOR: GeneratorSpec = GeneratorSpec {
    name: "rgba_color",
    label: "RGBA 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `HslColor`
pub(crate) static SPEC_HSL_COLOR: GeneratorSpec = GeneratorSpec {
    name: "hsl_color",
    label: "HSL 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `HslaColor`
pub(crate) static SPEC_HSLA_COLOR: GeneratorSpec = GeneratorSpec {
    name: "hsla_color",
    label: "HSLA 颜色",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `Color`
pub(crate) static SPEC_COLOR: GeneratorSpec = GeneratorSpec {
    name: "color",
    label: "颜色名",
    category: GeneratorCategory::Color,
    params: &[],
};

/// `FerroidULID`
pub(crate) static SPEC_FERROID_U_L_I_D: GeneratorSpec = GeneratorSpec {
    name: "ferroid_u_l_i_d",
    label: "ULID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidTwitterId`
pub(crate) static SPEC_FERROID_TWITTER_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_twitter_id",
    label: "Twitter ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidInstagramId`
pub(crate) static SPEC_FERROID_INSTAGRAM_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_instagram_id",
    label: "Instagram ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidMastodonId`
pub(crate) static SPEC_FERROID_MASTODON_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_mastodon_id",
    label: "Mastodon ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `FerroidDiscordId`
pub(crate) static SPEC_FERROID_DISCORD_ID: GeneratorSpec = GeneratorSpec {
    name: "ferroid_discord_id",
    label: "Discord ID",
    category: GeneratorCategory::FerroidId,
    params: &[],
};

/// `Isbn`
pub(crate) static SPEC_ISBN: GeneratorSpec = GeneratorSpec {
    name: "isbn",
    label: "ISBN",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `Isbn10`
pub(crate) static SPEC_ISBN10: GeneratorSpec = GeneratorSpec {
    name: "isbn10",
    label: "ISBN-10",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `Isbn13`
pub(crate) static SPEC_ISBN13: GeneratorSpec = GeneratorSpec {
    name: "isbn13",
    label: "ISBN-13",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `RfcStatusCode`
pub(crate) static SPEC_RFC_STATUS_CODE: GeneratorSpec = GeneratorSpec {
    name: "rfc_status_code",
    label: "RFC 状态码",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `ValidStatusCode`
pub(crate) static SPEC_VALID_STATUS_CODE: GeneratorSpec = GeneratorSpec {
    name: "valid_status_code",
    label: "有效 HTTP 状态码",
    category: GeneratorCategory::Code,
    params: &[],
};

/// `LicencePlate`
pub(crate) static SPEC_LICENCE_PLATE: GeneratorSpec = GeneratorSpec {
    name: "licence_plate",
    label: "车牌号",
    category: GeneratorCategory::Vehicle,
    params: &[],
};

/// `HealthInsuranceCode`
pub(crate) static SPEC_HEALTH_INSURANCE_CODE: GeneratorSpec = GeneratorSpec {
    name: "health_insurance_code",
    label: "医保编号",
    category: GeneratorCategory::Vehicle,
    params: &[],
};

/// `ForeignKey`
pub(crate) static SPEC_FOREIGN_KEY: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_SEQUENCE: GeneratorSpec = GeneratorSpec {
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
pub(crate) static SPEC_WEIGHTED: GeneratorSpec = GeneratorSpec {
    name: "weighted",
    label: "加权取值",
    category: GeneratorCategory::Constraint,
    params: &[ParamField {
        key: "choices",
        label: "加权选项",
        kind: ParamKind::Complex,
    }],
};

// ==================== 模板数据 ====================

use super::*;

/// 引用列：`col_ref!(列名, 类型, 生成器, "父表", "父列")`；末位可缀 `nullable`。
///
/// 关系挂在列上（`ColumnDependency::referenced`），是本模块表达关系的**唯一处**：
/// 父表必须在同一模板里，**不要求排在子表之前**（域由模板参数算出，与“哪张表先跑”无关），
/// 引用自身（自关联）也允许。
///
/// `generator` 不是冗余：它是**没有父表上下文**（单表生成）时的取值方式，
/// 取值域必须落在父域内（自检测试锁定这一点）。
macro_rules! col_ref {
    ($name:expr, $data_type:expr, $gen:expr, $parent:expr, $parent_col:expr) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.0,
            unique: false,
            dependency: Some(ColumnDependency::referenced($parent, $parent_col)),
        }
    };
    ($name:expr, $data_type:expr, $gen:expr, $parent:expr, $parent_col:expr, nullable) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.3,
            unique: false,
            dependency: Some(ColumnDependency::referenced($parent, $parent_col)),
        }
    };
}

macro_rules! rnd_int {
    ($min:expr, $max:expr) => {
        GeneratorConfig::RandomInt {
            min: $min,
            max: $max,
        }
    };
}

macro_rules! rnd_float {
    ($min:expr, $max:expr) => {
        GeneratorConfig::RandomFloat {
            min: $min,
            max: $max,
            precision: 2,
        }
    };
}

macro_rules! dt {
    ($min:expr, $max:expr) => {
        GeneratorConfig::date_time($min, $max)
    };
}

macro_rules! d {
    ($min:expr, $max:expr) => {
        GeneratorConfig::date($min, $max)
    };
}

pub(crate) fn ecommerce_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:ecommerce".to_string(),
        name: "电商系统".to_string(),
        description: "包含用户、商品、订单、订单明细四张表，模拟典型 B2C 电商数据".to_string(),
        category: "商业".to_string(),
        locale: "zh_cn".to_string(),
        seed: None,
        tables: vec![
            TemplateTable {
                name: "users".to_string(),
                row_count: 1000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "username",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::Username,
                        unique
                    ),
                    col!(
                        "email",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::SafeEmail,
                        unique
                    ),
                    col!(
                        "phone",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::CellNumber
                    ),
                    col!(
                        "full_name",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Name
                    ),
                    col!(
                        "birth_date",
                        ColumnDataType::DateTime,
                        d!("1960-01-01", "2005-12-31"),
                        nullable
                    ),
                    col!(
                        "city",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::City
                    ),
                    col!(
                        "province",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::StateName
                    ),
                    col!(
                        "zip_code",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::ZipCode
                    ),
                    col!(
                        "registered_at",
                        ColumnDataType::DateTime,
                        dt!("2018-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "products".to_string(),
                row_count: 500,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "sku",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::Words { min: 2, max: 5 }
                    ),
                    col!(
                        "category",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Industry
                    ),
                    col!(
                        "description",
                        ColumnDataType::Text,
                        GeneratorConfig::Sentence { min: 5, max: 15 },
                        nullable
                    ),
                    col!("price", ColumnDataType::Float, rnd_float!(1.0, 99999.0)),
                    col!("cost", ColumnDataType::Float, rnd_float!(0.5, 50000.0)),
                    col!(
                        "stock_quantity",
                        ColumnDataType::Integer,
                        rnd_int!(0, 10000)
                    ),
                    col!("weight_grams", ColumnDataType::Integer, rnd_int!(10, 50000)),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2020-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "orders".to_string(),
                row_count: 5000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "order_number",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col_ref!(
                        "user_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 1000),
                        "users",
                        "id"
                    ),
                    col!(
                        "status",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "pending".into(),
                                "processing".into(),
                                "shipped".into(),
                                "delivered".into(),
                                "cancelled".into()
                            ]
                        }
                    ),
                    col!(
                        "total_amount",
                        ColumnDataType::Float,
                        rnd_float!(10.0, 99999.0)
                    ),
                    col!(
                        "discount_amount",
                        ColumnDataType::Float,
                        rnd_float!(0.0, 5000.0)
                    ),
                    col!(
                        "payment_method",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::CreditCardNumber
                    ),
                    col!(
                        "shipping_city",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::City
                    ),
                    col!(
                        "tracking_number",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::UuidV4,
                        nullable
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2024-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                    col!(
                        "updated_at",
                        ColumnDataType::DateTime,
                        dt!("2024-01-01T00:00:00Z", "2025-12-31T23:59:59Z"),
                        nullable
                    ),
                ],
            },
            TemplateTable {
                name: "order_items".to_string(),
                row_count: 15000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "order_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 5000),
                        "orders",
                        "id"
                    ),
                    col_ref!(
                        "product_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 500),
                        "products",
                        "id"
                    ),
                    col!("quantity", ColumnDataType::Integer, rnd_int!(1, 10)),
                    col!(
                        "unit_price",
                        ColumnDataType::Float,
                        rnd_float!(1.0, 99999.0)
                    ),
                    col!("subtotal", ColumnDataType::Float, rnd_float!(1.0, 999999.0)),
                ],
            },
        ],
    }
}

pub(crate) fn hr_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:hr".to_string(),
        name: "人力资源系统".to_string(),
        description: "包含员工、部门、薪资三张表，模拟企业内部 HR 管理数据".to_string(),
        category: "企业管理".to_string(),
        locale: "zh_cn".to_string(),
        seed: None,
        tables: vec![
            TemplateTable {
                name: "employees".to_string(),
                row_count: 500,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "employee_code",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!(
                        "full_name",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Name
                    ),
                    col!(
                        "english_name",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::FirstName,
                        nullable
                    ),
                    col!(
                        "birth_date",
                        ColumnDataType::DateTime,
                        d!("1960-01-01", "2000-12-31")
                    ),
                    col!(
                        "phone",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::CellNumber
                    ),
                    col!(
                        "email",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::SafeEmail,
                        unique
                    ),
                    col_ref!(
                        "department_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 20),
                        "departments",
                        "id"
                    ),
                    col!(
                        "position",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::Position
                    ),
                    col!(
                        "hire_date",
                        ColumnDataType::DateTime,
                        d!("2010-01-01", "2025-12-31")
                    ),
                    col!(
                        "salary_grade",
                        ColumnDataType::Varchar { length: Some(10) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "P1".into(),
                                "P2".into(),
                                "P3".into(),
                                "P4".into(),
                                "P5".into(),
                                "P6".into(),
                                "M1".into(),
                                "M2".into(),
                                "M3".into()
                            ]
                        }
                    ),
                    col!(
                        "address",
                        ColumnDataType::Text,
                        GeneratorConfig::Sentence { min: 5, max: 15 },
                        nullable
                    ),
                ],
            },
            TemplateTable {
                name: "departments".to_string(),
                row_count: 20,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::CompanyName
                    ),
                    col_ref!(
                        "manager_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 500),
                        "employees",
                        "id",
                        nullable
                    ),
                    col_ref!(
                        "parent_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 10),
                        "departments",
                        "id",
                        nullable
                    ),
                    col!(
                        "budget",
                        ColumnDataType::Float,
                        rnd_float!(100000.0, 50000000.0)
                    ),
                    col!("headcount", ColumnDataType::Integer, rnd_int!(5, 200)),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        d!("2010-01-01", "2025-12-31")
                    ),
                ],
            },
            TemplateTable {
                name: "salaries".to_string(),
                row_count: 500,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "employee_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 500),
                        "employees",
                        "id"
                    ),
                    col!("year", ColumnDataType::Integer, rnd_int!(2022, 2025)),
                    col!("month", ColumnDataType::Integer, rnd_int!(1, 12)),
                    col!(
                        "base_salary",
                        ColumnDataType::Float,
                        rnd_float!(5000.0, 100000.0)
                    ),
                    col!(
                        "bonus",
                        ColumnDataType::Float,
                        rnd_float!(0.0, 50000.0),
                        nullable
                    ),
                    col!("allowance", ColumnDataType::Float, rnd_float!(0.0, 10000.0)),
                    col!("deduction", ColumnDataType::Float, rnd_float!(0.0, 5000.0)),
                    col!(
                        "net_salary",
                        ColumnDataType::Float,
                        rnd_float!(4000.0, 150000.0)
                    ),
                    col!(
                        "paid_at",
                        ColumnDataType::DateTime,
                        dt!("2022-01-05T00:00:00Z", "2025-12-10T23:59:59Z"),
                        nullable
                    ),
                ],
            },
        ],
    }
}

pub(crate) fn blog_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:blog".to_string(),
        name: "博客 / 内容平台".to_string(),
        description: "包含用户、文章、评论、标签四张表，模拟 UGC 内容平台数据".to_string(),
        category: "内容平台".to_string(),
        locale: "zh_cn".to_string(),
        seed: None,
        tables: vec![
            TemplateTable {
                name: "users".to_string(),
                row_count: 200,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "username",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::Username,
                        unique
                    ),
                    col!(
                        "display_name",
                        ColumnDataType::Varchar { length: Some(60) },
                        GeneratorConfig::Name
                    ),
                    col!(
                        "email",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::SafeEmail,
                        unique
                    ),
                    col!(
                        "joined_at",
                        ColumnDataType::DateTime,
                        dt!("2019-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "articles".to_string(),
                row_count: 1000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "title",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::Sentence { min: 3, max: 10 }
                    ),
                    col!(
                        "slug",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col_ref!(
                        "author_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 200),
                        "users",
                        "id"
                    ),
                    col!(
                        "content",
                        ColumnDataType::Text,
                        GeneratorConfig::Paragraph { count: 5 }
                    ),
                    col!(
                        "summary",
                        ColumnDataType::Varchar { length: Some(500) },
                        GeneratorConfig::Sentence { min: 10, max: 30 }
                    ),
                    col!(
                        "category",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Industry
                    ),
                    col!("view_count", ColumnDataType::Integer, rnd_int!(0, 100000)),
                    col!("like_count", ColumnDataType::Integer, rnd_int!(0, 5000)),
                    col!("comment_count", ColumnDataType::Integer, rnd_int!(0, 500)),
                    col!(
                        "published_at",
                        ColumnDataType::DateTime,
                        dt!("2023-01-01T00:00:00Z", "2025-12-31T23:59:59Z"),
                        nullable
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2022-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                    col!(
                        "updated_at",
                        ColumnDataType::DateTime,
                        dt!("2022-01-01T00:00:00Z", "2025-12-31T23:59:59Z"),
                        nullable
                    ),
                ],
            },
            TemplateTable {
                name: "comments".to_string(),
                row_count: 5000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "article_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 1000),
                        "articles",
                        "id"
                    ),
                    col_ref!(
                        "user_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 200),
                        "users",
                        "id"
                    ),
                    col_ref!(
                        "parent_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 5000),
                        "comments",
                        "id",
                        nullable
                    ),
                    col!(
                        "content",
                        ColumnDataType::Text,
                        GeneratorConfig::Sentence { min: 3, max: 15 }
                    ),
                    col!("like_count", ColumnDataType::Integer, rnd_int!(0, 100)),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2023-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "tags".to_string(),
                row_count: 100,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::Industry
                    ),
                    col!(
                        "slug",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!("article_count", ColumnDataType::Integer, rnd_int!(1, 500)),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        d!("2022-01-01", "2025-12-31")
                    ),
                ],
            },
        ],
    }
}

pub(crate) fn finance_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:finance".to_string(),
        name: "金融 / 交易系统".to_string(),
        description: "包含交易记录、账户信息、产品三张表，模拟金融交易数据".to_string(),
        category: "金融".to_string(),
        locale: "en".to_string(),
        seed: None,
        tables: vec![
            TemplateTable {
                name: "transactions".to_string(),
                row_count: 100000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "txn_hash",
                        ColumnDataType::Varchar { length: Some(64) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col_ref!(
                        "account_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 500),
                        "accounts",
                        "id"
                    ),
                    col_ref!(
                        "product_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 100),
                        "products",
                        "id"
                    ),
                    col!(
                        "amount",
                        ColumnDataType::Float,
                        rnd_float!(0.01, 10000000.0)
                    ),
                    col!(
                        "exchange_rate",
                        ColumnDataType::Float,
                        rnd_float!(0.01, 10.0)
                    ),
                    col!("fee", ColumnDataType::Float, rnd_float!(0.0, 1000.0)),
                    col!(
                        "description",
                        ColumnDataType::Varchar { length: Some(500) },
                        GeneratorConfig::Sentence { min: 3, max: 10 },
                        nullable
                    ),
                    col!(
                        "ip_address",
                        ColumnDataType::Varchar { length: Some(45) },
                        GeneratorConfig::IpAddress,
                        nullable
                    ),
                    col!(
                        "executed_at",
                        ColumnDataType::DateTime,
                        dt!("2024-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2024-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "accounts".to_string(),
                row_count: 500,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "account_number",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!(
                        "account_name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::CompanyName
                    ),
                    col!(
                        "account_type",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "savings".into(),
                                "checking".into(),
                                "investment".into(),
                                "credit".into(),
                                "loan".into()
                            ]
                        }
                    ),
                    col!(
                        "balance",
                        ColumnDataType::Float,
                        rnd_float!(-1000000.0, 100000000.0)
                    ),
                    col!(
                        "currency",
                        ColumnDataType::Varchar { length: Some(3) },
                        GeneratorConfig::CurrencyCode
                    ),
                    col!("interest_rate", ColumnDataType::Float, rnd_float!(0.0, 0.2)),
                    col!(
                        "opened_at",
                        ColumnDataType::DateTime,
                        d!("2015-01-01", "2025-12-31")
                    ),
                    col!(
                        "status",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "active".into(),
                                "dormant".into(),
                                "frozen".into(),
                                "closed".into()
                            ]
                        }
                    ),
                ],
            },
            TemplateTable {
                name: "products".to_string(),
                row_count: 100,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "isin_code",
                        ColumnDataType::Varchar { length: Some(12) },
                        GeneratorConfig::Isin,
                        unique
                    ),
                    col!(
                        "ticker",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!(
                        "product_name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::CompanyName
                    ),
                    col!(
                        "product_type",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "stock".into(),
                                "bond".into(),
                                "fund".into(),
                                "etf".into(),
                                "option".into(),
                                "future".into(),
                                "crypto".into(),
                                "forex".into()
                            ]
                        }
                    ),
                    col!(
                        "market_price",
                        ColumnDataType::Float,
                        rnd_float!(0.01, 100000.0)
                    ),
                    col!("nav", ColumnDataType::Float, rnd_float!(0.0, 100000.0)),
                    col!(
                        "market_cap",
                        ColumnDataType::Float,
                        rnd_float!(0.0, 1000000000000.0)
                    ),
                    col!(
                        "risk_level",
                        ColumnDataType::Varchar { length: Some(10) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "low".into(),
                                "medium".into(),
                                "high".into(),
                                "extreme".into()
                            ]
                        }
                    ),
                    col!(
                        "listed_date",
                        ColumnDataType::DateTime,
                        d!("2000-01-01", "2025-12-31")
                    ),
                ],
            },
        ],
    }
}

pub(crate) fn social_media_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:social_media".to_string(),
        name: "社交平台".to_string(),
        description: "包含用户、帖子、关注关系、点赞四张表，模拟社交平台数据".to_string(),
        category: "社交".to_string(),
        locale: "zh_cn".to_string(),
        seed: None,
        tables: vec![
            TemplateTable {
                name: "users".to_string(),
                row_count: 1000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "username",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::Username,
                        unique
                    ),
                    col!(
                        "display_name",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Name
                    ),
                    col!(
                        "email",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::SafeEmail,
                        unique
                    ),
                    col!(
                        "bio",
                        ColumnDataType::Text,
                        GeneratorConfig::Sentence { min: 3, max: 10 },
                        nullable
                    ),
                    col!(
                        "avatar_url",
                        ColumnDataType::Varchar { length: Some(500) },
                        GeneratorConfig::ImageUrl {
                            width: 256,
                            height: 256
                        }
                    ),
                    col!(
                        "follower_count",
                        ColumnDataType::Integer,
                        rnd_int!(0, 100000)
                    ),
                    col!(
                        "following_count",
                        ColumnDataType::Integer,
                        rnd_int!(0, 5000)
                    ),
                    col!("post_count", ColumnDataType::Integer, rnd_int!(0, 10000)),
                    col!(
                        "is_verified",
                        ColumnDataType::Boolean,
                        GeneratorConfig::Boolean { ratio: 50 }
                    ),
                    col!(
                        "city",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::City
                    ),
                    col!(
                        "joined_at",
                        ColumnDataType::DateTime,
                        dt!("2018-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "posts".to_string(),
                row_count: 10000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "user_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 1000),
                        "users",
                        "id"
                    ),
                    col!(
                        "content",
                        ColumnDataType::Text,
                        GeneratorConfig::Sentence { min: 3, max: 20 }
                    ),
                    col!(
                        "image_url",
                        ColumnDataType::Varchar { length: Some(500) },
                        GeneratorConfig::ImageUrl {
                            width: 1024,
                            height: 768
                        },
                        nullable
                    ),
                    col!("like_count", ColumnDataType::Integer, rnd_int!(0, 50000)),
                    col!("comment_count", ColumnDataType::Integer, rnd_int!(0, 5000)),
                    col!("reshare_count", ColumnDataType::Integer, rnd_int!(0, 10000)),
                    col!("view_count", ColumnDataType::Integer, rnd_int!(0, 500000)),
                    col!(
                        "is_pinned",
                        ColumnDataType::Boolean,
                        GeneratorConfig::Boolean { ratio: 20 }
                    ),
                    col!(
                        "ip_address",
                        ColumnDataType::Varchar { length: Some(45) },
                        GeneratorConfig::IpAddress,
                        nullable
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2023-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "follows".to_string(),
                row_count: 50000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "follower_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 1000),
                        "users",
                        "id"
                    ),
                    col_ref!(
                        "following_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 1000),
                        "users",
                        "id"
                    ),
                    col!(
                        "followed_at",
                        ColumnDataType::DateTime,
                        dt!("2020-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
            TemplateTable {
                name: "likes".to_string(),
                row_count: 100000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "user_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 1000),
                        "users",
                        "id"
                    ),
                    col_ref!(
                        "post_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 10000),
                        "posts",
                        "id"
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        dt!("2023-01-01T00:00:00Z", "2025-12-31T23:59:59Z")
                    ),
                ],
            },
        ],
    }
}

pub(crate) fn company_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:company".to_string(),
        name: "企业组织架构".to_string(),
        description: "包含公司、子公司、部门层级、项目、客户五张表，模拟集团企业数据".to_string(),
        category: "企业管理".to_string(),
        locale: "zh_cn".to_string(),
        seed: None,
        tables: vec![
            TemplateTable {
                name: "companies".to_string(),
                row_count: 50,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::CompanyName
                    ),
                    col!(
                        "legal_name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::CompanyName
                    ),
                    col!(
                        "bic",
                        ColumnDataType::Varchar { length: Some(11) },
                        GeneratorConfig::Bic,
                        nullable
                    ),
                    col!(
                        "tax_id",
                        ColumnDataType::Varchar { length: Some(30) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!(
                        "industry",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Industry
                    ),
                    col!(
                        "employee_count",
                        ColumnDataType::Integer,
                        rnd_int!(10, 50000)
                    ),
                    col!(
                        "annual_revenue",
                        ColumnDataType::Float,
                        rnd_float!(1000000.0, 50000000000.0)
                    ),
                    col!(
                        "headquarters_city",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::City
                    ),
                    col!(
                        "founded_year",
                        ColumnDataType::Integer,
                        rnd_int!(1950, 2024)
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        d!("2010-01-01", "2025-12-31")
                    ),
                ],
            },
            TemplateTable {
                name: "subsidiaries".to_string(),
                row_count: 200,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "parent_company_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 50),
                        "companies",
                        "id"
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::CompanyName
                    ),
                    col!(
                        "ownership_pct",
                        ColumnDataType::Float,
                        rnd_float!(0.0, 100.0)
                    ),
                    col!(
                        "employee_count",
                        ColumnDataType::Integer,
                        rnd_int!(5, 10000)
                    ),
                    col!(
                        "city",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::City
                    ),
                    col!(
                        "established_at",
                        ColumnDataType::DateTime,
                        d!("2000-01-01", "2025-12-31")
                    ),
                ],
            },
            TemplateTable {
                name: "departments".to_string(),
                row_count: 500,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "company_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 50),
                        "companies",
                        "id"
                    ),
                    col_ref!(
                        "parent_dept_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 500),
                        "departments",
                        "id",
                        nullable
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Words { min: 1, max: 3 }
                    ),
                    col!(
                        "manager_name",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Name
                    ),
                    col!("headcount", ColumnDataType::Integer, rnd_int!(3, 500)),
                    col!(
                        "budget",
                        ColumnDataType::Float,
                        rnd_float!(100000.0, 100000000.0)
                    ),
                    col!("level", ColumnDataType::Integer, rnd_int!(1, 5)),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        d!("2010-01-01", "2025-12-31")
                    ),
                ],
            },
            TemplateTable {
                name: "projects".to_string(),
                row_count: 1000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "company_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 50),
                        "companies",
                        "id"
                    ),
                    col_ref!(
                        "dept_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 500),
                        "departments",
                        "id"
                    ),
                    col!(
                        "code",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::UuidV4,
                        unique
                    ),
                    col!(
                        "name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::Words { min: 2, max: 6 }
                    ),
                    col!(
                        "description",
                        ColumnDataType::Text,
                        GeneratorConfig::Sentence { min: 5, max: 15 },
                        nullable
                    ),
                    col!(
                        "budget",
                        ColumnDataType::Float,
                        rnd_float!(10000.0, 50000000.0)
                    ),
                    col!(
                        "actual_cost",
                        ColumnDataType::Float,
                        rnd_float!(5000.0, 55000000.0),
                        nullable
                    ),
                    col!(
                        "status",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::ValueSet {
                            values: vec![
                                "planning".into(),
                                "active".into(),
                                "on_hold".into(),
                                "completed".into(),
                                "cancelled".into()
                            ]
                        }
                    ),
                    col!(
                        "start_date",
                        ColumnDataType::DateTime,
                        d!("2020-01-01", "2025-06-30")
                    ),
                    col!(
                        "end_date",
                        ColumnDataType::DateTime,
                        d!("2023-01-01", "2026-12-31"),
                        nullable
                    ),
                    col!(
                        "created_at",
                        ColumnDataType::DateTime,
                        d!("2019-01-01", "2025-12-31")
                    ),
                ],
            },
            TemplateTable {
                name: "clients".to_string(),
                row_count: 2000,
                columns: vec![
                    col!(
                        "id",
                        ColumnDataType::Integer,
                        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                        unique
                    ),
                    col_ref!(
                        "company_id",
                        ColumnDataType::Integer,
                        rnd_int!(1, 50),
                        "companies",
                        "id"
                    ),
                    col!(
                        "client_name",
                        ColumnDataType::Varchar { length: Some(200) },
                        GeneratorConfig::CompanyName
                    ),
                    col!(
                        "contact_person",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Name
                    ),
                    col!(
                        "email",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::SafeEmail
                    ),
                    col!(
                        "phone",
                        ColumnDataType::Varchar { length: Some(20) },
                        GeneratorConfig::CellNumber
                    ),
                    col!(
                        "industry",
                        ColumnDataType::Varchar { length: Some(100) },
                        GeneratorConfig::Industry
                    ),
                    col!(
                        "city",
                        ColumnDataType::Varchar { length: Some(50) },
                        GeneratorConfig::City
                    ),
                    col!(
                        "annual_revenue",
                        ColumnDataType::Float,
                        rnd_float!(50000.0, 10000000000.0)
                    ),
                    col!(
                        "is_vip",
                        ColumnDataType::Boolean,
                        GeneratorConfig::Boolean { ratio: 30 }
                    ),
                    col!(
                        "contracted_at",
                        ColumnDataType::DateTime,
                        d!("2018-01-01", "2025-12-31")
                    ),
                ],
            },
        ],
    }
}

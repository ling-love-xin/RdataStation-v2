use crate::models::{
    ColumnDataType, ColumnDef, ColumnDependency, GeneratorConfig, ScenarioTemplate, TemplateTable,
};

macro_rules! col {
    ($name:expr, $data_type:expr, $gen:expr) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        }
    };
    ($name:expr, $data_type:expr, $gen:expr, nullable) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.3,
            unique: false,
            dependency: None,
        }
    };
    ($name:expr, $data_type:expr, $gen:expr, unique) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.0,
            unique: true,
            dependency: None,
        }
    };
}

/// 引用列：`col_ref!(列名, 类型, 生成器, "父表", "父列")`；末位可缀 `nullable`。
///
/// 关系挂在列上（`ColumnDependency::foreign_key`），是本模块表达关系的**唯一处**：
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
            dependency: Some(ColumnDependency::foreign_key($parent, $parent_col)),
        }
    };
    ($name:expr, $data_type:expr, $gen:expr, $parent:expr, $parent_col:expr, nullable) => {
        ColumnDef {
            name: $name.to_string(),
            data_type: $data_type,
            generator: $gen,
            nullable_ratio: 0.3,
            unique: false,
            dependency: Some(ColumnDependency::foreign_key($parent, $parent_col)),
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
        GeneratorConfig::DateTime {
            min: $min.to_string(),
            max: $max.to_string(),
        }
    };
}

macro_rules! d {
    ($min:expr, $max:expr) => {
        GeneratorConfig::Date {
            min: $min.to_string(),
            max: $max.to_string(),
        }
    };
}

fn ecommerce_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:ecommerce".to_string(),
        name: "电商系统".to_string(),
        description: "包含用户、商品、订单、订单明细四张表，模拟典型 B2C 电商数据".to_string(),
        category: "商业".to_string(),
        locale: "zh_cn".to_string(),
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
                        GeneratorConfig::ForeignKey {
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

fn hr_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:hr".to_string(),
        name: "人力资源系统".to_string(),
        description: "包含员工、部门、薪资三张表，模拟企业内部 HR 管理数据".to_string(),
        category: "企业管理".to_string(),
        locale: "zh_cn".to_string(),
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
                        GeneratorConfig::ForeignKey {
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

fn blog_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:blog".to_string(),
        name: "博客 / 内容平台".to_string(),
        description: "包含用户、文章、评论、标签四张表，模拟 UGC 内容平台数据".to_string(),
        category: "内容平台".to_string(),
        locale: "zh_cn".to_string(),
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

fn finance_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:finance".to_string(),
        name: "金融 / 交易系统".to_string(),
        description: "包含交易记录、账户信息、产品三张表，模拟金融交易数据".to_string(),
        category: "金融".to_string(),
        locale: "en".to_string(),
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
                        GeneratorConfig::ForeignKey {
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
                        GeneratorConfig::ForeignKey {
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
                        GeneratorConfig::ForeignKey {
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
                        GeneratorConfig::ForeignKey {
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

fn social_media_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:social_media".to_string(),
        name: "社交平台".to_string(),
        description: "包含用户、帖子、关注关系、点赞四张表，模拟社交平台数据".to_string(),
        category: "社交".to_string(),
        locale: "zh_cn".to_string(),
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

fn company_template() -> ScenarioTemplate {
    ScenarioTemplate {
        id: "builtin:company".to_string(),
        name: "企业组织架构".to_string(),
        description: "包含公司、子公司、部门层级、项目、客户五张表，模拟集团企业数据".to_string(),
        category: "企业管理".to_string(),
        locale: "zh_cn".to_string(),
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
                        GeneratorConfig::ForeignKey {
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

pub fn get_builtin_templates() -> Vec<ScenarioTemplate> {
    vec![
        ecommerce_template(),
        hr_template(),
        blog_template(),
        finance_template(),
        social_media_template(),
        company_template(),
    ]
}

pub fn get_template_by_id(id: &str) -> Option<ScenarioTemplate> {
    get_builtin_templates().into_iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::error::CoreError;

    #[test]
    fn test_all_templates_exist() {
        let templates = get_builtin_templates();
        assert_eq!(templates.len(), 6);
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"builtin:ecommerce"));
        assert!(ids.contains(&"builtin:hr"));
        assert!(ids.contains(&"builtin:blog"));
        assert!(ids.contains(&"builtin:finance"));
        assert!(ids.contains(&"builtin:social_media"));
        assert!(ids.contains(&"builtin:company"));
    }

    #[test]
    fn test_get_template_by_id_found() -> Result<(), CoreError> {
        let t = get_template_by_id("builtin:ecommerce")
            .ok_or_else(|| CoreError::from("template not found: builtin:ecommerce"))?;
        assert_eq!(t.name, "电商系统");
        assert!(!t.tables.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_template_by_id_not_found() {
        assert!(get_template_by_id("nonexistent").is_none());
    }

    #[test]
    fn test_ecommerce_template_has_four_tables() -> Result<(), CoreError> {
        let t = get_template_by_id("builtin:ecommerce")
            .ok_or_else(|| CoreError::from("template not found: builtin:ecommerce"))?;
        assert_eq!(t.tables.len(), 4);
        let table_names: Vec<&str> = t.tables.iter().map(|tb| tb.name.as_str()).collect();
        assert!(table_names.contains(&"users"));
        assert!(table_names.contains(&"products"));
        assert!(table_names.contains(&"orders"));
        assert!(table_names.contains(&"order_items"));
        Ok(())
    }

    #[test]
    fn test_hr_template_has_tables() {
        let t = get_template_by_id("builtin:hr").unwrap();
        assert!(!t.tables.is_empty());
        assert_eq!(t.locale, "zh_cn");
    }

    #[test]
    fn test_all_template_columns_have_valid_types() {
        for template in get_builtin_templates() {
            for table in &template.tables {
                assert!(
                    !table.columns.is_empty(),
                    "{}::{} has no columns",
                    template.id,
                    table.name
                );
                for col in &table.columns {
                    assert!(
                        !col.name.is_empty(),
                        "{}.{} column has empty name",
                        table.name,
                        col.name
                    );
                    assert!(
                        col.nullable_ratio >= 0.0 && col.nullable_ratio <= 1.0,
                        "{}.{} nullable_ratio out of range: {}",
                        table.name,
                        col.name,
                        col.nullable_ratio
                    );
                }
            }
        }
    }

    // ==================== 表间引用（关系自检） ====================

    /// 名字里带 `_id` 但**故意不是引用**的列（改了就得同步改这里，并写清理由）。
    ///
    /// 这些列是「标识」而不是「指向另一张表的行」（如税号）。
    const NOT_A_REFERENCE: &[(&str, &str, &str)] = &[("builtin:company", "companies", "tax_id")];

    /// 模板里声明的引用都要能解析成取值域（父表 / 父列存在、父列可算域）。
    ///
    /// 这是「关系能生成」的前提：解析不了的引用在生成前就被拦下，
    /// 内置模板更不应该存在解析不了的引用。
    #[test]
    fn every_declared_reference_resolves() {
        for template in get_builtin_templates() {
            let domains = crate::engine::MockEngine::resolve_reference_domains(&template)
                .unwrap_or_else(|e| panic!("{} 的引用解析失败: {e}", template.id));
            let declared = declared_references(&template).len();
            assert!(
                !domains.is_empty() || declared == 0,
                "{} 声明了 {declared} 条引用，却一条域也没解析出来",
                template.id
            );
            for domain in &domains {
                assert!(domain.count > 0, "{domain:?} 的行数为 0");
            }
        }
    }

    /// 引用列的 `generator` 取值域必须**落在父域内**。
    ///
    /// 为何要管这个：场景生成走父域采样，但**单表生成没有父表上下文**，按它自己的
    /// `generator` 取值——那时产出的值也得落在父表可能的取值范围内，
    /// 否则同一列在两种生成方式下值域不同，用户换个入口就看到不一样的数据。
    #[test]
    fn reference_generators_stay_inside_the_parent_domain() {
        for template in get_builtin_templates() {
            let domains = crate::engine::MockEngine::resolve_reference_domains(&template)
                .expect("引用解析应当成功");
            for (child_table, column, parent_table, parent_column) in declared_references(&template)
            {
                let domain = domains
                    .iter()
                    .find(|d| d.table == parent_table && d.column == parent_column)
                    .unwrap_or_else(|| panic!("{} 的父域缺失", template.id));
                let GeneratorConfig::RandomInt { min, max } = column.generator else {
                    panic!(
                        "{}.{} 是引用列，生成器需是整数区间（单表生成时按它取值），实际: {:?}",
                        child_table, column.name, column.generator
                    );
                };
                assert!(
                    i64::from(min) >= domain.first && i64::from(max) <= domain.last(),
                    "{}.{} 的取值域 {}..{} 超出了父域 {}..{}（{}）",
                    child_table,
                    column.name,
                    min,
                    max,
                    domain.first,
                    domain.last(),
                    domain.label()
                );
            }
        }
    }

    /// 名字里带 `_id` 的列（除自带主键 `id`）**要么声明为引用，要么在白名单里**。
    ///
    /// 为何要这条：引用一旦漏声明，生成出来的就是「看着像外键、实际是随机数」的列，
    /// 关联查询会落空——这类问题只能在评审时看出来。让测试顶出来，
    /// 逼作者在「声明引用」与「明确写进白名单」之间选一个。
    #[test]
    fn every_id_column_is_declared_or_explicitly_not_a_reference() {
        let mut undeclared = Vec::new();
        for template in get_builtin_templates() {
            for table in &template.tables {
                for col in &table.columns {
                    if col.name == "id" || !col.name.ends_with("_id") || is_reference(col) {
                        continue;
                    }
                    let allowed = NOT_A_REFERENCE.iter().any(|(id, table_name, col_name)| {
                        *id == template.id && *table_name == table.name && *col_name == col.name
                    });
                    if !allowed {
                        undeclared.push(format!("{}.{}.{}", template.id, table.name, col.name));
                    }
                }
            }
        }
        assert!(
            undeclared.is_empty(),
            "这些 `*_id` 列既没声明引用、也不在 NOT_A_REFERENCE 白名单里：{undeclared:?}"
        );
    }

    /// 白名单里不能有幽灵条目（列已改名 / 已删 / 其实已经是引用）。
    #[test]
    fn the_not_a_reference_allowlist_has_no_stale_entries() {
        for (id, table_name, col_name) in NOT_A_REFERENCE {
            let template = get_template_by_id(id).unwrap_or_else(|| panic!("{id} 不存在"));
            let table = template
                .tables
                .iter()
                .find(|t| t.name == *table_name)
                .unwrap_or_else(|| panic!("{id} 里没有表 {table_name}"));
            let column = table
                .columns
                .iter()
                .find(|c| c.name == *col_name)
                .unwrap_or_else(|| panic!("{id}.{table_name} 里没有列 {col_name}"));
            assert!(
                !is_reference(column),
                "{id}.{table_name}.{col_name} 已声明为引用，应从白名单里删掉"
            );
        }
    }

    /// 列是否声明了跨表引用。
    fn is_reference(col: &ColumnDef) -> bool {
        col.dependency.is_some()
    }

    /// 模板里全部 `(子表, 列, 父表, 父列)` 引用声明。
    fn declared_references(template: &ScenarioTemplate) -> Vec<(&str, &ColumnDef, &str, &str)> {
        let mut out = Vec::new();
        for table in &template.tables {
            for col in &table.columns {
                let Some(dep) = col.dependency.as_ref() else {
                    continue;
                };
                out.push((
                    table.name.as_str(),
                    col,
                    dep.ref_table.as_str(),
                    dep.ref_column.as_str(),
                ));
            }
        }
        out
    }

    #[test]
    fn test_ecommerce_users_has_username_email() -> Result<(), CoreError> {
        let t = get_template_by_id("builtin:ecommerce")
            .ok_or_else(|| CoreError::from("template not found: builtin:ecommerce"))?;
        let users = t
            .tables
            .iter()
            .find(|tb| tb.name == "users")
            .ok_or_else(|| CoreError::from("table 'users' not found"))?;
        let names: Vec<&str> = users.columns.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"username"));
        assert!(names.contains(&"email"));
        Ok(())
    }
}

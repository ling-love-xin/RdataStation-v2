use serde::{Deserialize, Serialize};
use specta::Type;

/// 驱动类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DriverType {
    /// 原生驱动（Rust实现）
    Native,
    /// WASM插件驱动
    Wasm,
    /// JDBC驱动
    Jdbc,
    /// ODBC驱动
    Odbc,
    /// ADBC驱动（Apache Arrow Database Connectivity）
    Adbc,
    /// HTTP API 驱动
    Http,
    /// Python 生态驱动
    Python,
    /// JavaScript/Node 驱动
    Js,
}

/// 驱动图标
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverIcon {
    /// 图标类型
    pub r#type: String,
    /// 图标内容
    pub content: String,
}

/// 驱动配置表单字段
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverFormField {
    /// 字段名称
    pub name: String,
    /// 字段标签
    pub label: String,
    /// 字段类型
    pub field_type: String,
    /// 是否必填
    pub required: bool,
    /// 默认值
    #[specta(skip)]
    pub default_value: Option<serde_json::Value>,
    /// 选项（用于下拉框等）
    pub options: Option<Vec<(String, String)>>,
    /// 描述
    pub description: Option<String>,
}

/// 驱动元信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverMetadata {
    /// 驱动ID
    pub id: String,
    /// 驱动名称
    pub name: String,
    /// 驱动版本
    pub version: String,
    /// 驱动类型
    pub r#type: DriverType,
    /// 驱动描述
    pub description: String,
    /// 驱动图标
    pub icon: Option<DriverIcon>,
    /// 支持的特性
    pub features: Vec<String>,
    /// 配置表单
    pub config_form: Vec<DriverFormField>,
    /// 连接URL模板
    pub url_template: String,
    /// 默认端口
    pub default_port: Option<u16>,
    /// 作者
    pub author: Option<String>,
    /// 许可证
    pub license: Option<String>,
    /// 主页
    pub homepage: Option<String>,
}

impl DriverMetadata {
    /// 创建MySQL驱动元信息
    pub fn mysql() -> Self {
        Self {
            id: "mysql".to_string(),
            name: "MySQL".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "MySQL database driver".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#4479A1\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "streaming".to_string(),
                "ssl".to_string(),
                "ssh".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "host".to_string(),
                    label: "Host".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("localhost".to_string())),
                    options: None,
                    description: Some("MySQL server hostname".to_string()),
                },
                DriverFormField {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::Number(serde_json::Number::from(3306))),
                    options: None,
                    description: Some("MySQL server port".to_string()),
                },
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database".to_string(),
                    field_type: "text".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("Database name".to_string()),
                },
                DriverFormField {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("root".to_string())),
                    options: None,
                    description: Some("MySQL username".to_string()),
                },
                DriverFormField {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("MySQL password".to_string()),
                },
            ],
            url_template: "mysql://{username}:{password}@{host}:{port}/{database}".to_string(),
            default_port: Some(3306),
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }

    /// 创建PostgreSQL驱动元信息
    pub fn postgres() -> Self {
        Self {
            id: "postgres".to_string(),
            name: "PostgreSQL".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "PostgreSQL database driver".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#336791\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "streaming".to_string(),
                "ssl".to_string(),
                "ssh".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "host".to_string(),
                    label: "Host".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("localhost".to_string())),
                    options: None,
                    description: Some("PostgreSQL server hostname".to_string()),
                },
                DriverFormField {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::Number(serde_json::Number::from(5432))),
                    options: None,
                    description: Some("PostgreSQL server port".to_string()),
                },
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("postgres".to_string())),
                    options: None,
                    description: Some("Database name".to_string()),
                },
                DriverFormField {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("postgres".to_string())),
                    options: None,
                    description: Some("PostgreSQL username".to_string()),
                },
                DriverFormField {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("PostgreSQL password".to_string()),
                },
            ],
            url_template: "postgres://{username}:{password}@{host}:{port}/{database}".to_string(),
            default_port: Some(5432),
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }

    /// 创建SQLite驱动元信息
    pub fn sqlite() -> Self {
        Self {
            id: "sqlite".to_string(),
            name: "SQLite".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "SQLite database driver".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#003B57\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "file".to_string(),
                "in-memory".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database File".to_string(),
                    field_type: "file".to_string(),
                    required: true,
                    default_value: None,
                    options: None,
                    description: Some("SQLite database file path".to_string()),
                },
            ],
            url_template: "sqlite://{database}".to_string(),
            default_port: None,
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }

    /// 创建DuckDB驱动元信息
    pub fn duckdb() -> Self {
        Self {
            id: "duckdb".to_string(),
            name: "DuckDB".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "DuckDB database driver".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#A67C00\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "streaming".to_string(),
                "arrow".to_string(),
                "federated".to_string(),
                "file".to_string(),
                "in-memory".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database File".to_string(),
                    field_type: "file".to_string(),
                    required: false,
                    default_value: Some(serde_json::Value::String(":memory:".to_string())),
                    options: None,
                    description: Some("DuckDB database file path (leave blank for in-memory)".to_string()),
                },
            ],
            url_template: "duckdb://{database}".to_string(),
            default_port: None,
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }

    /// 创建ClickHouse驱动元信息
    pub fn clickhouse() -> Self {
        Self {
            id: "clickhouse".to_string(),
            name: "ClickHouse".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "ClickHouse database driver".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#27619E\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "streaming".to_string(),
                "arrow".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "host".to_string(),
                    label: "Host".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("localhost".to_string())),
                    options: None,
                    description: Some("ClickHouse server hostname".to_string()),
                },
                DriverFormField {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::Number(serde_json::Number::from(9000))),
                    options: None,
                    description: Some("ClickHouse server port".to_string()),
                },
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database".to_string(),
                    field_type: "text".to_string(),
                    required: false,
                    default_value: Some(serde_json::Value::String("default".to_string())),
                    options: None,
                    description: Some("Database name".to_string()),
                },
                DriverFormField {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("default".to_string())),
                    options: None,
                    description: Some("ClickHouse username".to_string()),
                },
                DriverFormField {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("ClickHouse password".to_string()),
                },
            ],
            url_template: "clickhouse://{username}:{password}@{host}:{port}/{database}".to_string(),
            default_port: Some(9000),
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }

    /// 创建 MySQL 官方原生驱动元信息（mysql_async）
    pub fn mysql_native() -> Self {
        Self {
            id: "mysql_native".to_string(),
            name: "MySQL (Official)".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "MySQL official pure-Rust async driver (mysql_async)".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#4479A1\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "streaming".to_string(),
                "ssl".to_string(),
                "ssh".to_string(),
                "protocol_compression".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "host".to_string(),
                    label: "Host".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("localhost".to_string())),
                    options: None,
                    description: Some("MySQL server hostname".to_string()),
                },
                DriverFormField {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::Number(serde_json::Number::from(3306))),
                    options: None,
                    description: Some("MySQL server port".to_string()),
                },
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database".to_string(),
                    field_type: "text".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("Database name".to_string()),
                },
                DriverFormField {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("root".to_string())),
                    options: None,
                    description: Some("MySQL username".to_string()),
                },
                DriverFormField {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("MySQL password".to_string()),
                },
            ],
            url_template: "mysql://{username}:{password}@{host}:{port}/{database}".to_string(),
            default_port: Some(3306),
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }

    /// 创建 PostgreSQL 官方原生驱动元信息（tokio-postgres）
    pub fn postgres_native() -> Self {
        Self {
            id: "postgres_native".to_string(),
            name: "PostgreSQL (Official)".to_string(),
            version: "1.0.0".to_string(),
            r#type: DriverType::Native,
            description: "PostgreSQL official async driver (tokio-postgres)".to_string(),
            icon: Some(DriverIcon {
                r#type: "svg".to_string(),
                content: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"#336791\"><path d=\"M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z\"/></svg>".to_string(),
            }),
            features: vec![
                "transaction".to_string(),
                "streaming".to_string(),
                "ssl".to_string(),
                "ssh".to_string(),
                "pipeline".to_string(),
                "copy_protocol".to_string(),
                "listen_notify".to_string(),
            ],
            config_form: vec![
                DriverFormField {
                    name: "host".to_string(),
                    label: "Host".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("localhost".to_string())),
                    options: None,
                    description: Some("PostgreSQL server hostname".to_string()),
                },
                DriverFormField {
                    name: "port".to_string(),
                    label: "Port".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::Number(serde_json::Number::from(5432))),
                    options: None,
                    description: Some("PostgreSQL server port".to_string()),
                },
                DriverFormField {
                    name: "database".to_string(),
                    label: "Database".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("postgres".to_string())),
                    options: None,
                    description: Some("Database name".to_string()),
                },
                DriverFormField {
                    name: "username".to_string(),
                    label: "Username".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some(serde_json::Value::String("postgres".to_string())),
                    options: None,
                    description: Some("PostgreSQL username".to_string()),
                },
                DriverFormField {
                    name: "password".to_string(),
                    label: "Password".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    options: None,
                    description: Some("PostgreSQL password".to_string()),
                },
            ],
            url_template: "postgres://{username}:{password}@{host}:{port}/{database}".to_string(),
            default_port: Some(5432),
            author: Some("RdataStation Team".to_string()),
            license: Some("MIT".to_string()),
            homepage: Some("https://rdatastation.com".to_string()),
        }
    }
}

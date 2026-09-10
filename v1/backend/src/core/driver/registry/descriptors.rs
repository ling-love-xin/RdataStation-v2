//! 驱动描述符定义
//!
//! 提供类似 DBeaver 的驱动描述模型，包括驱动字段、选项类型、
//! 以及四种内置数据库（MySQL/PostgreSQL/SQLite/DuckDB）的驱动描述符。

use serde::{Deserialize, Serialize};
use specta::Type;

/// 驱动选项定义
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverOption {
    /// 选项键
    pub key: String,
    /// 显示名称
    pub label: String,
    /// 默认值
    pub default_value: String,
    /// 选项类型
    pub option_type: DriverOptionType,
    /// 是否必需
    pub required: bool,
    /// 描述
    pub description: Option<String>,
}

impl DriverOption {
    /// 创建新的驱动选项
    pub fn new(key: impl Into<String>, default_value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: String::new(),
            default_value: default_value.into(),
            option_type: DriverOptionType::String,
            required: false,
            description: None,
        }
    }

    /// 设置显示名称
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// 设置选项类型
    pub fn with_type(mut self, option_type: DriverOptionType) -> Self {
        self.option_type = option_type;
        self
    }

    /// 设置是否必需
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// 设置描述
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// 驱动选项类型
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DriverOptionType {
    /// 字符串输入
    String,
    /// 数字输入
    Number,
    /// 布尔开关
    Boolean,
    /// 下拉选择
    Select { options: Vec<String> },
    /// 文件选择
    File,
}

/// 驱动字段定义（用于前端表单渲染）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverField {
    /// 字段键
    pub key: String,
    /// 显示标签
    pub label: String,
    /// 字段类型
    pub field_type: DriverFieldType,
    /// 是否必需
    pub required: bool,
    /// 默认值
    pub default_value: Option<String>,
    /// 占位符文本
    pub placeholder: Option<String>,
}

/// 驱动字段类型
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DriverFieldType {
    /// 文本输入
    Text,
    /// 密码输入
    Password,
    /// 数字输入
    Number,
    /// 文件选择
    File,
    /// 下拉选择
    Select { options: Vec<(String, String)> },
}

/// 驱动种类：区分驱动实现方式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DriverKind {
    Native,
    Jdbc,
    Odbc,
    Wasm,
    Adbc,
    Http,
    Python,
    Js,
}

impl DriverKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DriverKind::Native => "native",
            DriverKind::Jdbc => "jdbc",
            DriverKind::Odbc => "odbc",
            DriverKind::Wasm => "wasm",
            DriverKind::Adbc => "adbc",
            DriverKind::Http => "http",
            DriverKind::Python => "python",
            DriverKind::Js => "js",
        }
    }
}

/// 驱动描述符（类似 DBeaver 的驱动定义）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverDescriptor {
    /// 驱动 ID
    pub id: String,
    /// 显示名称
    pub name: String,
    /// 驱动描述
    pub description: String,
    /// 驱动种类（原生/JDBC/ODBC/WASM/...）
    pub driver_kind: DriverKind,
    /// 目标数据库类型（非 Native 驱动标记连接的数据库类型，如 JDBC 驱动可为 "mysql"）
    pub target_database: Option<String>,
    /// 驱动分类（relational / file-based / nosql / analytics / cloud）
    pub category: String,
    /// 默认端口
    pub default_port: Option<u16>,
    /// 是否需要数据库名
    pub require_database: bool,
    /// 是否需要文件路径
    pub require_file: bool,
    /// 是否支持 SSL/TLS
    pub supports_ssl: bool,
    /// 是否支持 SSH 隧道
    pub supports_ssh_tunnel: bool,
    /// 是否支持 HTTP 代理
    pub supports_http_proxy: bool,
    /// 是否支持 SOCKS 代理
    pub supports_socks_proxy: bool,
    /// URL 模板（用于 to_url 扩展，支持新驱动类型）
    /// 例如: "postgres://{username}:{password}@{host}:{port}/{database}"
    pub url_template: Option<String>,
    /// 表单字段定义
    pub fields: Vec<DriverField>,
    /// 额外选项
    pub extra_options: Vec<DriverOption>,
    pub icon: Option<String>,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub supported_auth_types: Vec<String>,
}

impl DriverDescriptor {
    /// 创建新的驱动描述符（默认为 Native 驱动）
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            driver_kind: DriverKind::Native,
            target_database: None,
            default_port: None,
            require_database: false,
            require_file: false,
            supports_ssl: false,
            supports_ssh_tunnel: false,
            supports_http_proxy: false,
            supports_socks_proxy: false,
            url_template: None,
            fields: Vec::new(),
            extra_options: Vec::new(),
            category: String::new(),
            icon: None,
            enabled: true,
            capabilities: Vec::new(),
            supported_auth_types: Vec::new(),
        }
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// 创建外部驱动描述符（JDBC/ODBC/WASM 等非 Native 驱动）
    pub fn new_external(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: DriverKind,
        target_db: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            driver_kind: kind,
            target_database: Some(target_db.into()),
            default_port: None,
            require_database: false,
            require_file: false,
            supports_ssl: false,
            supports_ssh_tunnel: false,
            supports_http_proxy: false,
            supports_socks_proxy: false,
            url_template: None,
            fields: Vec::new(),
            extra_options: Vec::new(),
            category: String::new(),
            icon: None,
            enabled: true,
            capabilities: Vec::new(),
            supported_auth_types: Vec::new(),
        }
    }

    /// 设置描述
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// 设置默认端口
    pub fn with_default_port(mut self, port: u16) -> Self {
        self.default_port = Some(port);
        self
    }

    /// 设置需要数据库名
    pub fn requires_database(mut self) -> Self {
        self.require_database = true;
        self
    }

    /// 设置需要文件路径
    pub fn requires_file(mut self) -> Self {
        self.require_file = true;
        self
    }

    /// 设置支持 SSL
    pub fn with_ssl_support(mut self) -> Self {
        self.supports_ssl = true;
        self
    }

    /// 设置支持 SSH 隧道
    pub fn with_ssh_tunnel_support(mut self) -> Self {
        self.supports_ssh_tunnel = true;
        self
    }

    /// 设置支持 HTTP 代理
    pub fn with_http_proxy_support(mut self) -> Self {
        self.supports_http_proxy = true;
        self
    }

    /// 设置支持 SOCKS 代理
    pub fn with_socks_proxy_support(mut self) -> Self {
        self.supports_socks_proxy = true;
        self
    }

    /// 设置 URL 模板（用于新驱动类型的 URL 构建）
    pub fn with_url_template(mut self, template: impl Into<String>) -> Self {
        self.url_template = Some(template.into());
        self
    }

    /// 设置目标数据库类型（Native 驱动等同于 id，外部驱动可指向不同数据库）
    pub fn with_target_database(mut self, target: impl Into<String>) -> Self {
        self.target_database = Some(target.into());
        self
    }

    /// 添加表单字段
    pub fn with_field(mut self, field: DriverField) -> Self {
        self.fields.push(field);
        self
    }

    /// 添加额外选项
    pub fn with_extra_option(mut self, option: DriverOption) -> Self {
        self.extra_options.push(option);
        self
    }

    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn with_supported_auth_types(mut self, types: Vec<String>) -> Self {
        self.supported_auth_types = types;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

// =============================================================================
// 内置驱动描述符工厂（MySQL / PostgreSQL / SQLite / DuckDB）
// =============================================================================

/// MySQL 驱动描述符
pub fn mysql_driver() -> DriverDescriptor {
    DriverDescriptor::new("mysql", "MySQL")
        .with_description("MySQL 关系型数据库")
        .with_category("relational")
        .with_target_database("mysql")
        .with_default_port(3306)
        .requires_database()
        .with_ssl_support()
        .with_ssh_tunnel_support()
        .with_http_proxy_support()
        .with_socks_proxy_support()
        .with_url_template("mysql://{username}:{password}@{host}:{port}/{database}")
        .with_icon("\u{1F42C}")
        .with_capabilities(vec![
            "tree".to_string(),
            "health_check".to_string(),
            "transactions".to_string(),
            "index_analysis".to_string(),
            "sql_autocomplete".to_string(),
            "table_editor".to_string(),
        ])
        .with_supported_auth_types(vec!["password".to_string(), "ssl".to_string()])
        .with_field(DriverField {
            key: "host".to_string(),
            label: "主机".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("localhost".to_string()),
            placeholder: Some("localhost 或 IP 地址".to_string()),
        })
        .with_field(DriverField {
            key: "port".to_string(),
            label: "端口".to_string(),
            field_type: DriverFieldType::Number,
            required: true,
            default_value: Some("3306".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "database".to_string(),
            label: "数据库".to_string(),
            field_type: DriverFieldType::Text,
            required: false,
            default_value: None,
            placeholder: Some("可选，留空显示所有数据库".to_string()),
        })
        .with_field(DriverField {
            key: "username".to_string(),
            label: "用户名".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("root".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "password".to_string(),
            label: "密码".to_string(),
            field_type: DriverFieldType::Password,
            required: false,
            default_value: None,
            placeholder: Some("可选".to_string()),
        })
        .with_extra_option(
            DriverOption::new("ssl_mode", "PREFERRED")
                .with_label("SSL 模式")
                .with_type(DriverOptionType::Select {
                    options: vec![
                        "DISABLED".to_string(),
                        "PREFERRED".to_string(),
                        "REQUIRED".to_string(),
                        "VERIFY_CA".to_string(),
                        "VERIFY_IDENTITY".to_string(),
                    ],
                })
                .with_description("SSL 连接模式"),
        )
}

/// PostgreSQL 驱动描述符
pub fn postgres_driver() -> DriverDescriptor {
    DriverDescriptor::new("postgres", "PostgreSQL")
        .with_description("PostgreSQL 关系型数据库")
        .with_category("relational")
        .with_target_database("postgres")
        .with_default_port(5432)
        .requires_database()
        .with_ssl_support()
        .with_ssh_tunnel_support()
        .with_http_proxy_support()
        .with_socks_proxy_support()
        .with_url_template("postgres://{username}:{password}@{host}:{port}/{database}")
        .with_icon("\u{1F418}")
        .with_capabilities(vec![
            "tree".to_string(),
            "health_check".to_string(),
            "transactions".to_string(),
            "index_analysis".to_string(),
            "sql_autocomplete".to_string(),
            "schema_browser".to_string(),
            "table_editor".to_string(),
        ])
        .with_supported_auth_types(vec![
            "password".to_string(),
            "ssl".to_string(),
            "kerberos".to_string(),
        ])
        .with_field(DriverField {
            key: "host".to_string(),
            label: "主机".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("localhost".to_string()),
            placeholder: Some("localhost 或 IP 地址".to_string()),
        })
        .with_field(DriverField {
            key: "port".to_string(),
            label: "端口".to_string(),
            field_type: DriverFieldType::Number,
            required: true,
            default_value: Some("5432".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "database".to_string(),
            label: "数据库".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("postgres".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "username".to_string(),
            label: "用户名".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("postgres".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "password".to_string(),
            label: "密码".to_string(),
            field_type: DriverFieldType::Password,
            required: false,
            default_value: None,
            placeholder: Some("可选".to_string()),
        })
        .with_extra_option(
            DriverOption::new("ssl_mode", "prefer")
                .with_label("SSL 模式")
                .with_type(DriverOptionType::Select {
                    options: vec![
                        "disable".to_string(),
                        "allow".to_string(),
                        "prefer".to_string(),
                        "require".to_string(),
                        "verify-ca".to_string(),
                        "verify-full".to_string(),
                    ],
                })
                .with_description("SSL 连接模式"),
        )
}

/// SQLite 驱动描述符
pub fn sqlite_driver() -> DriverDescriptor {
    DriverDescriptor::new("sqlite", "SQLite")
        .with_description("SQLite 嵌入式数据库")
        .with_category("file-based")
        .with_target_database("sqlite")
        .requires_file()
        .with_url_template("sqlite://{file_path}")
        .with_icon("\u{1FAB6}")
        .with_capabilities(vec![
            "tree".to_string(),
            "health_check".to_string(),
            "transactions".to_string(),
            "sql_autocomplete".to_string(),
            "table_editor".to_string(),
        ])
        .with_supported_auth_types(vec!["password".to_string()])
        .with_field(DriverField {
            key: "file_path".to_string(),
            label: "数据库文件".to_string(),
            field_type: DriverFieldType::File,
            required: true,
            default_value: None,
            placeholder: Some("选择 .db 或 .sqlite 文件".to_string()),
        })
        .with_extra_option(
            DriverOption::new("mode", "rwc")
                .with_label("打开模式")
                .with_type(DriverOptionType::Select {
                    options: vec!["ro".to_string(), "rw".to_string(), "rwc".to_string()],
                })
                .with_description("数据库文件打开模式：只读/读写/读写创建"),
        )
}

/// DuckDB 驱动描述符
pub fn duckdb_driver() -> DriverDescriptor {
    DriverDescriptor::new("duckdb", "DuckDB")
        .with_description("DuckDB 分析型数据库")
        .with_category("file-based")
        .with_target_database("duckdb")
        .requires_file()
        .with_url_template("duckdb://{file_path}")
        .with_icon("\u{1F986}")
        .with_capabilities(vec![
            "tree".to_string(),
            "health_check".to_string(),
            "sql_autocomplete".to_string(),
            "analytics".to_string(),
            "federation".to_string(),
            "table_editor".to_string(),
        ])
        .with_supported_auth_types(vec!["password".to_string()])
        .with_field(DriverField {
            key: "file_path".to_string(),
            label: "数据库文件".to_string(),
            field_type: DriverFieldType::File,
            required: true,
            default_value: None,
            placeholder: Some("选择 .duckdb 文件或 :memory:".to_string()),
        })
        .with_extra_option(
            DriverOption::new("memory_limit", "")
                .with_label("内存限制")
                .with_type(DriverOptionType::String)
                .with_description("例如: 1GB, 512MB（留空表示无限制）"),
        )
}

/// 获取所有支持的驱动
pub fn get_all_drivers() -> Vec<DriverDescriptor> {
    super::DriverRegistry::all_descriptors()
}

/// 根据 ID 获取驱动描述符
pub fn get_driver(id: &str) -> Option<DriverDescriptor> {
    super::DriverRegistry::get(id).map(|f| f.descriptor())
}

// =============================================================================
// 官方原生驱动描述符（mysql_native / postgres_native）
// =============================================================================

/// MySQL 官方原生驱动描述符（mysql_async）
pub fn mysql_native_driver() -> DriverDescriptor {
    DriverDescriptor::new("mysql_native", "MySQL (Native)")
        .with_description("MySQL 官方纯 Rust 异步驱动 (mysql_async)，支持协议压缩、原生认证插件")
        .with_category("relational")
        .with_target_database("mysql")
        .with_default_port(3306)
        .requires_database()
        .with_ssl_support()
        .with_ssh_tunnel_support()
        .with_http_proxy_support()
        .with_socks_proxy_support()
        .with_url_template("mysql://{username}:{password}@{host}:{port}/{database}")
        .with_icon("\u{1F42C}")
        .with_capabilities(vec![
            "tree".to_string(),
            "health_check".to_string(),
            "transactions".to_string(),
            "index_analysis".to_string(),
            "sql_autocomplete".to_string(),
            "table_editor".to_string(),
        ])
        .with_supported_auth_types(vec!["password".to_string(), "ssl".to_string()])
        .with_field(DriverField {
            key: "host".to_string(),
            label: "主机".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("localhost".to_string()),
            placeholder: Some("localhost 或 IP 地址".to_string()),
        })
        .with_field(DriverField {
            key: "port".to_string(),
            label: "端口".to_string(),
            field_type: DriverFieldType::Number,
            required: true,
            default_value: Some("3306".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "database".to_string(),
            label: "数据库".to_string(),
            field_type: DriverFieldType::Text,
            required: false,
            default_value: None,
            placeholder: Some("可选，留空显示所有数据库".to_string()),
        })
        .with_field(DriverField {
            key: "username".to_string(),
            label: "用户名".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("root".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "password".to_string(),
            label: "密码".to_string(),
            field_type: DriverFieldType::Password,
            required: false,
            default_value: None,
            placeholder: Some("可选".to_string()),
        })
}

/// PostgreSQL 官方原生驱动描述符（tokio-postgres）
pub fn postgres_native_driver() -> DriverDescriptor {
    DriverDescriptor::new("postgres_native", "PostgreSQL (Native)")
        .with_description(
            "PostgreSQL 官方异步驱动 (tokio-postgres)，支持 Pipeline、COPY 协议、LISTEN/NOTIFY",
        )
        .with_category("relational")
        .with_target_database("postgres")
        .with_default_port(5432)
        .requires_database()
        .with_ssl_support()
        .with_ssh_tunnel_support()
        .with_http_proxy_support()
        .with_socks_proxy_support()
        .with_url_template("postgres://{username}:{password}@{host}:{port}/{database}")
        .with_icon("\u{1F418}")
        .with_capabilities(vec![
            "tree".to_string(),
            "health_check".to_string(),
            "transactions".to_string(),
            "index_analysis".to_string(),
            "sql_autocomplete".to_string(),
            "schema_browser".to_string(),
            "table_editor".to_string(),
        ])
        .with_supported_auth_types(vec![
            "password".to_string(),
            "ssl".to_string(),
            "kerberos".to_string(),
        ])
        .with_field(DriverField {
            key: "host".to_string(),
            label: "主机".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("localhost".to_string()),
            placeholder: Some("localhost 或 IP 地址".to_string()),
        })
        .with_field(DriverField {
            key: "port".to_string(),
            label: "端口".to_string(),
            field_type: DriverFieldType::Number,
            required: true,
            default_value: Some("5432".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "database".to_string(),
            label: "数据库".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("postgres".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "username".to_string(),
            label: "用户名".to_string(),
            field_type: DriverFieldType::Text,
            required: true,
            default_value: Some("postgres".to_string()),
            placeholder: None,
        })
        .with_field(DriverField {
            key: "password".to_string(),
            label: "密码".to_string(),
            field_type: DriverFieldType::Password,
            required: false,
            default_value: None,
            placeholder: Some("可选".to_string()),
        })
}

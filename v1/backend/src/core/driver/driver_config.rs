//! 驱动配置管理
//!
//! 支持从配置文件或代码自动扫描注册驱动

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::error::CoreError;

/// 驱动配置（用于配置文件）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverConfig {
    /// 驱动 ID
    pub id: String,
    /// 驱动名称
    pub name: String,
    /// 驱动描述
    pub description: String,
    /// 驱动类型: "builtin" | "plugin" | "external"
    pub driver_type: String,
    /// 默认端口
    pub default_port: Option<u16>,
    /// 是否需要数据库名
    #[serde(default)]
    pub require_database: bool,
    /// 是否需要文件路径
    #[serde(default)]
    pub require_file: bool,
    /// 是否支持 SSL
    #[serde(default)]
    pub supports_ssl: bool,
    /// 是否支持 SSH 隧道
    #[serde(default)]
    pub supports_ssh_tunnel: bool,
    /// 是否支持 HTTP 代理
    #[serde(default)]
    pub supports_http_proxy: bool,
    /// 是否支持 SOCKS 代理
    #[serde(default)]
    pub supports_socks_proxy: bool,
    /// 表单字段定义
    #[serde(default)]
    pub fields: Vec<DriverFieldConfig>,
    /// 额外选项
    #[serde(default)]
    pub extra_options: Vec<DriverOptionConfig>,
}

/// 驱动字段配置
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverFieldConfig {
    pub key: String,
    pub label: String,
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    pub default_value: Option<String>,
    pub placeholder: Option<String>,
}

/// 驱动选项配置
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverOptionConfig {
    pub key: String,
    pub label: String,
    pub default_value: String,
    pub option_type: String,
    #[serde(default)]
    pub required: bool,
    pub description: Option<String>,
    /// 下拉选项（当 option_type 为 select 时使用）
    pub options: Option<Vec<String>>,
}

/// 驱动注册表配置
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverRegistryConfig {
    /// 驱动列表
    pub drivers: Vec<DriverConfig>,
    /// 自动扫描路径
    #[serde(default)]
    pub scan_paths: Vec<String>,
    /// 是否启用内置驱动
    #[serde(default = "default_true")]
    pub enable_builtin: bool,
}

fn default_true() -> bool {
    true
}

impl DriverRegistryConfig {
    /// 加载配置文件（当前使用默认配置，后续集成 toml/serde 反序列化）
    pub fn from_file(_path: &str) -> Result<Self, CoreError> {
        Ok(Self::default_config())
    }

    pub fn to_file(&self, _path: &str) -> Result<(), CoreError> {
        Ok(())
    }

    /// 创建默认配置
    pub fn default_config() -> Self {
        Self {
            drivers: vec![
                Self::mysql_config(),
                Self::mysql_native_config(),
                Self::postgres_config(),
                Self::postgres_native_config(),
                Self::sqlite_config(),
                Self::duckdb_config(),
            ],
            scan_paths: vec![
                "./drivers".to_string(),
            ],
            enable_builtin: true,
        }
    }

    fn mysql_config() -> DriverConfig {
        DriverConfig {
            id: "mysql".to_string(),
            name: "MySQL".to_string(),
            description: "MySQL 关系型数据库".to_string(),
            driver_type: "builtin".to_string(),
            default_port: Some(3306),
            require_database: true,
            require_file: false,
            supports_ssl: true,
            supports_ssh_tunnel: true,
            supports_http_proxy: true,
            supports_socks_proxy: true,
            fields: vec![
                DriverFieldConfig {
                    key: "host".to_string(),
                    label: "主机".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("localhost".to_string()),
                    placeholder: Some("localhost 或 IP 地址".to_string()),
                },
                DriverFieldConfig {
                    key: "port".to_string(),
                    label: "端口".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some("3306".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "database".to_string(),
                    label: "数据库".to_string(),
                    field_type: "text".to_string(),
                    required: false,
                    default_value: None,
                    placeholder: Some("可选，留空显示所有数据库".to_string()),
                },
                DriverFieldConfig {
                    key: "username".to_string(),
                    label: "用户名".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("root".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "password".to_string(),
                    label: "密码".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    placeholder: Some("可选".to_string()),
                },
            ],
            extra_options: vec![
                DriverOptionConfig {
                    key: "ssl_mode".to_string(),
                    label: "SSL 模式".to_string(),
                    default_value: "PREFERRED".to_string(),
                    option_type: "select".to_string(),
                    required: false,
                    description: Some("SSL 连接模式".to_string()),
                    options: Some(vec![
                        "DISABLED".to_string(),
                        "PREFERRED".to_string(),
                        "REQUIRED".to_string(),
                        "VERIFY_CA".to_string(),
                        "VERIFY_IDENTITY".to_string(),
                    ]),
                },
            ],
        }
    }

    fn postgres_config() -> DriverConfig {
        DriverConfig {
            id: "postgres".to_string(),
            name: "PostgreSQL".to_string(),
            description: "PostgreSQL 关系型数据库".to_string(),
            driver_type: "builtin".to_string(),
            default_port: Some(5432),
            require_database: true,
            require_file: false,
            supports_ssl: true,
            supports_ssh_tunnel: true,
            supports_http_proxy: true,
            supports_socks_proxy: true,
            fields: vec![
                DriverFieldConfig {
                    key: "host".to_string(),
                    label: "主机".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("localhost".to_string()),
                    placeholder: Some("localhost 或 IP 地址".to_string()),
                },
                DriverFieldConfig {
                    key: "port".to_string(),
                    label: "端口".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some("5432".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "database".to_string(),
                    label: "数据库".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("postgres".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "username".to_string(),
                    label: "用户名".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("postgres".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "password".to_string(),
                    label: "密码".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    placeholder: Some("可选".to_string()),
                },
            ],
            extra_options: vec![
                DriverOptionConfig {
                    key: "ssl_mode".to_string(),
                    label: "SSL 模式".to_string(),
                    default_value: "prefer".to_string(),
                    option_type: "select".to_string(),
                    required: false,
                    description: Some("SSL 连接模式".to_string()),
                    options: Some(vec![
                        "disable".to_string(),
                        "allow".to_string(),
                        "prefer".to_string(),
                        "require".to_string(),
                        "verify-ca".to_string(),
                        "verify-full".to_string(),
                    ]),
                },
            ],
        }
    }

    fn sqlite_config() -> DriverConfig {
        DriverConfig {
            id: "sqlite".to_string(),
            name: "SQLite".to_string(),
            description: "SQLite 嵌入式数据库".to_string(),
            driver_type: "builtin".to_string(),
            default_port: None,
            require_database: false,
            require_file: true,
            supports_ssl: false,
            supports_ssh_tunnel: false,
            supports_http_proxy: false,
            supports_socks_proxy: false,
            fields: vec![
                DriverFieldConfig {
                    key: "file_path".to_string(),
                    label: "数据库文件".to_string(),
                    field_type: "file".to_string(),
                    required: true,
                    default_value: None,
                    placeholder: Some("选择 .db 或 .sqlite 文件".to_string()),
                },
            ],
            extra_options: vec![
                DriverOptionConfig {
                    key: "mode".to_string(),
                    label: "打开模式".to_string(),
                    default_value: "rwc".to_string(),
                    option_type: "select".to_string(),
                    required: false,
                    description: Some("数据库文件打开模式：只读/读写/读写创建".to_string()),
                    options: Some(vec![
                        "ro".to_string(),
                        "rw".to_string(),
                        "rwc".to_string(),
                    ]),
                },
            ],
        }
    }

    fn duckdb_config() -> DriverConfig {
        DriverConfig {
            id: "duckdb".to_string(),
            name: "DuckDB".to_string(),
            description: "DuckDB 分析型数据库".to_string(),
            driver_type: "builtin".to_string(),
            default_port: None,
            require_database: false,
            require_file: true,
            supports_ssl: false,
            supports_ssh_tunnel: false,
            supports_http_proxy: false,
            supports_socks_proxy: false,
            fields: vec![
                DriverFieldConfig {
                    key: "file_path".to_string(),
                    label: "数据库文件".to_string(),
                    field_type: "file".to_string(),
                    required: true,
                    default_value: None,
                    placeholder: Some("选择 .duckdb 文件或 :memory:".to_string()),
                },
            ],
            extra_options: vec![
                DriverOptionConfig {
                    key: "memory_limit".to_string(),
                    label: "内存限制".to_string(),
                    default_value: "".to_string(),
                    option_type: "string".to_string(),
                    required: false,
                    description: Some("例如: 1GB, 512MB（留空表示无限制）".to_string()),
                    options: None,
                },
            ],
        }
    }

    fn mysql_native_config() -> DriverConfig {
        DriverConfig {
            id: "mysql_native".to_string(),
            name: "MySQL (Official)".to_string(),
            description: "MySQL 官方纯 Rust 异步驱动 (mysql_async)".to_string(),
            driver_type: "builtin".to_string(),
            default_port: Some(3306),
            require_database: true,
            require_file: false,
            supports_ssl: true,
            supports_ssh_tunnel: true,
            supports_http_proxy: true,
            supports_socks_proxy: true,
            fields: vec![
                DriverFieldConfig {
                    key: "host".to_string(),
                    label: "主机".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("localhost".to_string()),
                    placeholder: Some("localhost 或 IP 地址".to_string()),
                },
                DriverFieldConfig {
                    key: "port".to_string(),
                    label: "端口".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some("3306".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "database".to_string(),
                    label: "数据库".to_string(),
                    field_type: "text".to_string(),
                    required: false,
                    default_value: None,
                    placeholder: Some("可选，留空显示所有数据库".to_string()),
                },
                DriverFieldConfig {
                    key: "username".to_string(),
                    label: "用户名".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("root".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "password".to_string(),
                    label: "密码".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    placeholder: Some("可选".to_string()),
                },
            ],
            extra_options: vec![
                DriverOptionConfig {
                    key: "ssl_mode".to_string(),
                    label: "SSL 模式".to_string(),
                    default_value: "PREFERRED".to_string(),
                    option_type: "select".to_string(),
                    required: false,
                    description: Some("SSL 连接模式".to_string()),
                    options: Some(vec![
                        "DISABLED".to_string(),
                        "PREFERRED".to_string(),
                        "REQUIRED".to_string(),
                        "VERIFY_CA".to_string(),
                        "VERIFY_IDENTITY".to_string(),
                    ]),
                },
            ],
        }
    }

    fn postgres_native_config() -> DriverConfig {
        DriverConfig {
            id: "postgres_native".to_string(),
            name: "PostgreSQL (Official)".to_string(),
            description: "PostgreSQL 官方异步驱动 (tokio-postgres)".to_string(),
            driver_type: "builtin".to_string(),
            default_port: Some(5432),
            require_database: true,
            require_file: false,
            supports_ssl: true,
            supports_ssh_tunnel: true,
            supports_http_proxy: true,
            supports_socks_proxy: true,
            fields: vec![
                DriverFieldConfig {
                    key: "host".to_string(),
                    label: "主机".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("localhost".to_string()),
                    placeholder: Some("localhost 或 IP 地址".to_string()),
                },
                DriverFieldConfig {
                    key: "port".to_string(),
                    label: "端口".to_string(),
                    field_type: "number".to_string(),
                    required: true,
                    default_value: Some("5432".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "database".to_string(),
                    label: "数据库".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("postgres".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "username".to_string(),
                    label: "用户名".to_string(),
                    field_type: "text".to_string(),
                    required: true,
                    default_value: Some("postgres".to_string()),
                    placeholder: None,
                },
                DriverFieldConfig {
                    key: "password".to_string(),
                    label: "密码".to_string(),
                    field_type: "password".to_string(),
                    required: false,
                    default_value: None,
                    placeholder: Some("可选".to_string()),
                },
            ],
            extra_options: vec![
                DriverOptionConfig {
                    key: "ssl_mode".to_string(),
                    label: "SSL 模式".to_string(),
                    default_value: "prefer".to_string(),
                    option_type: "select".to_string(),
                    required: false,
                    description: Some("SSL 连接模式".to_string()),
                    options: Some(vec![
                        "disable".to_string(),
                        "allow".to_string(),
                        "prefer".to_string(),
                        "require".to_string(),
                        "verify-ca".to_string(),
                        "verify-full".to_string(),
                    ]),
                },
            ],
        }
    }
}

/// 驱动发现 trait
///
/// 用于自动扫描和注册驱动
pub trait DriverDiscovery {
    /// 扫描指定路径下的驱动
    fn scan_drivers(&self, paths: &[String]) -> Vec<DriverConfig>;
}

/// 内置驱动发现器
pub struct BuiltinDriverDiscovery;

impl DriverDiscovery for BuiltinDriverDiscovery {
    fn scan_drivers(&self, _paths: &[String]) -> Vec<DriverConfig> {
        // 内置驱动直接返回默认配置
        DriverRegistryConfig::default_config().drivers
    }
}

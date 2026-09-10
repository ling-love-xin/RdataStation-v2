//! 连接配置模型
//!
//! 一个结构支持所有数据库类型，前端根据 driver 字段动态渲染表单。

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

use connection::config::ConnectionMethod;
use shared::error::{CommonError, CoreError};

/// 连接配置（统一模型）
///
/// 一个结构支持所有数据库类型，前端根据 driver 字段动态渲染表单
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverConnectionConfig {
    /// 驱动类型: "mysql" | "postgres" | "sqlite" | "duckdb"
    pub driver: String,
    /// 连接名称（显示用）
    pub name: Option<String>,
    /// 主机地址（网络数据库）
    pub host: Option<String>,
    /// 端口（网络数据库）
    pub port: Option<u16>,
    /// 数据库名
    pub database: Option<String>,
    /// 用户名
    pub username: Option<String>,
    /// 密码
    pub password: Option<String>,
    /// 文件路径（SQLite/DuckDB 等文件型数据库）
    pub file_path: Option<String>,
    /// 连接方式覆盖（避免重新构建 URL）
    pub url_override: Option<String>,
    /// URL 模板（从 DriverDescriptor.url_template 传入，用于模板化构建 URL）
    pub url_template: Option<String>,
    /// 连接方式（SSL/SSH/Proxy）
    #[serde(default)]
    pub connection_method: ConnectionMethod,
    /// 额外连接选项（URL query params）
    pub options: HashMap<String, String>,
    /// 连接超时（秒，来自 AdvancedTab 性能策略 / 高级选项）
    pub connect_timeout: Option<u32>,
    /// 查询超时（秒，来自 AdvancedTab 性能策略）
    pub query_timeout: Option<u32>,
    /// 连接池大小（来自 AdvancedTab 性能策略）
    pub pool_size: Option<u32>,
    /// 心跳间隔（秒，来自 AdvancedTab 性能策略）
    pub heartbeat_interval: Option<u32>,
    /// 最大重连次数（来自 AdvancedTab 性能策略）
    pub max_reconnect: Option<u32>,
    /// 字符编码（来自 AdvancedTab Schema + 编码选择器，如 "UTF-8"/"GBK"）
    pub encoding: Option<String>,
    /// 驱动属性（来自 DriverPropsTab，key=value）
    pub driver_properties: HashMap<String, String>,
}

impl DriverConnectionConfig {
    /// 创建新的连接配置
    pub fn new(driver: impl Into<String>) -> Self {
        Self {
            driver: driver.into(),
            name: None,
            host: None,
            port: None,
            database: None,
            username: None,
            password: None,
            file_path: None,
            url_override: None,
            url_template: None,
            connection_method: ConnectionMethod::Direct,
            options: HashMap::new(),
            connect_timeout: None,
            query_timeout: None,
            pool_size: None,
            heartbeat_interval: None,
            max_reconnect: None,
            encoding: None,
            driver_properties: HashMap::new(),
        }
    }

    /// 设置连接名称
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 设置主机
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// 设置端口
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// 设置数据库
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// 设置用户名
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// 设置密码
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// 设置文件路径
    pub fn with_file_path(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// 添加额外选项
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// 设置连接超时
    pub fn with_connect_timeout(mut self, secs: u32) -> Self {
        self.connect_timeout = Some(secs);
        self
    }

    /// 设置查询超时
    pub fn with_query_timeout(mut self, secs: u32) -> Self {
        self.query_timeout = Some(secs);
        self
    }

    /// 设置连接池大小
    pub fn with_pool_size(mut self, size: u32) -> Self {
        self.pool_size = Some(size);
        self
    }

    /// 设置心跳间隔
    pub fn with_heartbeat_interval(mut self, secs: u32) -> Self {
        self.heartbeat_interval = Some(secs);
        self
    }

    /// 设置最大重连
    pub fn with_max_reconnect(mut self, retries: u32) -> Self {
        self.max_reconnect = Some(retries);
        self
    }

    /// 设置字符编码
    pub fn with_encoding(mut self, encoding: impl Into<String>) -> Self {
        self.encoding = Some(encoding.into());
        self
    }

    /// 添加驱动属性
    pub fn with_driver_property(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.driver_properties.insert(key.into(), value.into());
        self
    }

    /// 设置连接方式（SSL/SSH/Proxy）
    pub fn with_connection_method(mut self, method: ConnectionMethod) -> Self {
        self.connection_method = method;
        self
    }

    /// 设置预构建的 URL（绕过 to_url 自动构建）
    pub fn with_url_override(mut self, url: impl Into<String>) -> Self {
        self.url_override = Some(url.into());
        self
    }

    /// 设置 URL 模板（从 DriverDescriptor.url_template 传入）
    pub fn with_url_template(mut self, template: impl Into<String>) -> Self {
        self.url_template = Some(template.into());
        self
    }

    /// 转换为数据库连接 URL
    ///
    /// 优先级: url_override > url_template > hardcoded match (legacy fallback)
    /// 无论哪个路径，始终追加 driver_properties / options / encoding 等查询参数
    pub fn to_url(&self) -> Result<String, CoreError> {
        if let Some(ref url) = self.url_override {
            let mut url = url.clone();
            self.append_query_params(&mut url);
            return Ok(url);
        }
        // Preferred: use url_template from driver descriptor
        if let Some(ref template) = self.url_template {
            return self.build_from_template(template);
        }
        // Legacy fallback
        match self.driver.as_str() {
            "mysql" | "mysql_native" => self.build_mysql_url(),
            "postgres" | "postgres_native" => self.build_postgres_url(),
            "sqlite" => self.build_sqlite_url(),
            "duckdb" => self.build_duckdb_url(),
            _ => Err(CoreError::common(CommonError::Internal(format!(
                "Unsupported driver: {}",
                self.driver
            )))),
        }
    }

    /// 追加所有查询参数（options + encoding + driver_properties + connect_timeout）到 URL
    fn append_query_params(&self, url: &mut String) {
        let mut params: Vec<String> = self
            .options
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        if let Some(ref enc) = self.encoding {
            if !self.options.contains_key("charset") {
                let charset = match enc.as_str() {
                    "GBK" => "gbk",
                    "Latin-1" => "latin1",
                    _ => "utf8mb4",
                };
                params.push(format!("charset={}", charset));
            }
        }

        if let Some(ct) = self.connect_timeout {
            if !self.options.contains_key("connect_timeout")
                && !self.options.contains_key("connectTimeout")
            {
                params.push(format!("connect_timeout={}", ct));
            }
        }

        for (k, v) in &self.driver_properties {
            if !self.options.contains_key(k) {
                params.push(format!("{}={}", k, v));
            }
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
    }
    fn build_from_template(&self, template: &str) -> Result<String, CoreError> {
        let username = self.username.as_deref().unwrap_or("");
        let password = self.password.as_deref().unwrap_or("");
        let host = self.host.as_deref().unwrap_or("localhost");
        let port = self.port.map(|p| p.to_string()).unwrap_or_default();
        let database = self.database.as_deref().unwrap_or("");
        let file_path = self.file_path.as_deref().unwrap_or("");

        let mut url = template
            .replace("{username}", username)
            .replace("{password}", password)
            .replace("{host}", host)
            .replace("{port}", &port)
            .replace("{database}", database)
            .replace("{file_path}", file_path);

        self.append_query_params(&mut url);

        Ok(url)
    }

    fn build_mysql_url(&self) -> Result<String, CoreError> {
        let host = self.host.as_ref().ok_or("Host is required for MySQL")?;
        let port = self.port.unwrap_or(3306);
        let username = self.username.as_deref().unwrap_or("root");
        let password = self.password.as_deref().unwrap_or("");

        let mut url = format!("mysql://{}:{}@{}:{}", username, password, host, port);

        if let Some(db) = &self.database {
            url.push('/');
            url.push_str(db);
        }

        self.append_query_params(&mut url);

        Ok(url)
    }

    /// 构建 PostgreSQL URL
    fn build_postgres_url(&self) -> Result<String, CoreError> {
        let host = self
            .host
            .as_ref()
            .ok_or("Host is required for PostgreSQL")?;
        let port = self.port.unwrap_or(5432);
        let username = self.username.as_deref().unwrap_or("postgres");
        let password = self.password.as_deref().unwrap_or("");
        let database = self.database.as_deref().unwrap_or("postgres");

        let mut url = format!(
            "postgres://{}:{}@{}:{}/{}",
            username, password, host, port, database
        );

        self.append_query_params(&mut url);

        Ok(url)
    }

    /// 构建 SQLite URL
    fn build_sqlite_url(&self) -> Result<String, CoreError> {
        let path = self
            .file_path
            .as_ref()
            .ok_or("File path is required for SQLite")?;
        Ok(format!("sqlite://{}", path))
    }

    /// 构建 DuckDB URL
    fn build_duckdb_url(&self) -> Result<String, CoreError> {
        let path = self
            .file_path
            .as_ref()
            .ok_or("File path is required for DuckDB")?;
        Ok(format!("duckdb://{}", path))
    }
}

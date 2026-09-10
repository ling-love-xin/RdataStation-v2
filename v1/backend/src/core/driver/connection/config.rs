//! 连接配置定义
//!
//! 定义统一的连接配置结构，支持多种连接方式

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

/// 协议链路中的单跳
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChainHop {
    /// SSH 隧道跳
    Ssh(SshConfig),
    /// SSL/TLS 加密跳
    Ssl(SslConfig),
    /// HTTP/HTTPS 代理跳
    HttpProxy(ProxyConfig),
    /// SOCKS4/5 代理跳
    SocksProxy(ProxyConfig),
}

impl ChainHop {
    pub fn hop_name(&self) -> &'static str {
        match self {
            ChainHop::Ssh(_) => "ssh",
            ChainHop::Ssl(_) => "ssl",
            ChainHop::HttpProxy(_) => "http_proxy",
            ChainHop::SocksProxy(_) => "socks_proxy",
        }
    }
}

/// 连接方式枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ConnectionMethod {
    /// 直接连接（无加密/无代理）
    #[default]
    Direct,
    /// SSL/TLS 加密连接
    Ssl(SslConfig),
    /// SSH 隧道连接
    Ssh(SshConfig),
    /// HTTP/HTTPS 代理连接
    HttpProxy(ProxyConfig),
    /// SOCKS4/5 代理连接
    SocksProxy(ProxyConfig),
    /// 协议链路（外层 → 内层顺序，如 Proxy → SSH → SSL → DB）
    Chain(Vec<ChainHop>),
}

/// SSL/TLS 配置
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, Type)]
pub struct SslConfig {
    /// 是否验证服务器证书
    #[serde(default = "default_true")]
    pub verify_server_cert: bool,
    /// CA 证书路径（可选，用于自定义 CA）
    pub ca_cert_path: Option<String>,
    /// 客户端证书路径（可选，用于双向认证）
    pub client_cert_path: Option<String>,
    /// 客户端私钥路径（可选，用于双向认证）
    pub client_key_path: Option<String>,
    /// 允许的 TLS 版本
    #[serde(default = "default_tls_version")]
    pub min_tls_version: TlsVersion,
}

/// TLS 版本
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TlsVersion {
    Tls1_0,
    Tls1_1,
    #[default]
    Tls1_2,
    Tls1_3,
}

fn default_tls_version() -> TlsVersion {
    TlsVersion::Tls1_2
}

/// SSH 隧道配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct SshConfig {
    /// SSH 服务器主机
    pub host: String,
    /// SSH 服务器端口
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// SSH 用户名
    pub username: String,
    /// SSH 认证方式
    #[serde(flatten)]
    pub auth: SshAuth,
    /// 远程数据库主机（通过 SSH 隧道后连接的目标）
    pub remote_host: String,
    /// 远程数据库端口（通过 SSH 隧道后连接的目标）
    pub remote_port: u16,
    /// 本地绑定端口（0 表示自动分配）
    #[serde(default)]
    pub local_port: u16,
    /// SSH 连接超时（秒）
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
}

fn default_ssh_port() -> u16 {
    22
}

/// SSH 认证方式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(tag = "auth_type", rename_all = "snake_case")]
pub enum SshAuth {
    /// 密码认证
    Password { password: String },
    /// 私钥认证
    PrivateKey {
        /// 私钥路径
        key_path: String,
        /// 私钥密码（可选）
        passphrase: Option<String>,
    },
    /// SSH Agent 认证
    Agent,
}

/// 代理配置（HTTP/HTTPS/SOCKS）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct ProxyConfig {
    /// 代理服务器主机
    pub host: String,
    /// 代理服务器端口
    pub port: u16,
    /// 代理认证（可选）
    pub auth: Option<ProxyAuth>,
    /// 不经过代理的主机列表（可选）
    #[serde(default)]
    pub no_proxy: Vec<String>,
    /// 连接超时（秒）
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
}

/// 代理认证
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct ProxyAuth {
    /// 用户名
    pub username: String,
    /// 密码
    pub password: String,
}

fn default_timeout_secs() -> u32 {
    30
}

fn default_true() -> bool {
    true
}

/// 统一连接配置
///
/// 用于配置数据库连接的底层连接方式
#[derive(Debug, Clone, Serialize, Deserialize, Default, Type)]
pub struct ConnectionConfig {
    /// 目标主机
    pub host: String,
    /// 目标端口
    pub port: u16,
    /// 连接方式
    #[serde(flatten)]
    pub method: ConnectionMethod,
    /// 额外选项
    #[serde(default)]
    pub options: HashMap<String, String>,
}

impl ConnectionConfig {
    /// 创建直接连接配置
    pub fn direct(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            method: ConnectionMethod::Direct,
            options: HashMap::new(),
        }
    }

    /// 创建 SSL 连接配置
    pub fn ssl(host: impl Into<String>, port: u16, ssl_config: SslConfig) -> Self {
        Self {
            host: host.into(),
            port,
            method: ConnectionMethod::Ssl(ssl_config),
            options: HashMap::new(),
        }
    }

    /// 创建 SSH 隧道连接配置
    pub fn ssh(remote_host: impl Into<String>, remote_port: u16, ssh_config: SshConfig) -> Self {
        Self {
            host: remote_host.into(),
            port: remote_port,
            method: ConnectionMethod::Ssh(ssh_config),
            options: HashMap::new(),
        }
    }

    /// 创建 HTTP 代理连接配置
    pub fn http_proxy(host: impl Into<String>, port: u16, proxy_config: ProxyConfig) -> Self {
        Self {
            host: host.into(),
            port,
            method: ConnectionMethod::HttpProxy(proxy_config),
            options: HashMap::new(),
        }
    }

    /// 创建 SOCKS 代理连接配置
    pub fn socks_proxy(host: impl Into<String>, port: u16, proxy_config: ProxyConfig) -> Self {
        Self {
            host: host.into(),
            port,
            method: ConnectionMethod::SocksProxy(proxy_config),
            options: HashMap::new(),
        }
    }

    /// 创建协议链连接配置
    pub fn chain(host: impl Into<String>, port: u16, hops: Vec<ChainHop>) -> Self {
        Self {
            host: host.into(),
            port,
            method: ConnectionMethod::Chain(hops),
            options: HashMap::new(),
        }
    }

    /// 获取连接方式名称
    pub fn method_name(&self) -> &'static str {
        match &self.method {
            ConnectionMethod::Direct => "direct",
            ConnectionMethod::Ssl(_) => "ssl",
            ConnectionMethod::Ssh(_) => "ssh",
            ConnectionMethod::HttpProxy(_) => "http_proxy",
            ConnectionMethod::SocksProxy(_) => "socks_proxy",
            ConnectionMethod::Chain(_) => "chain",
        }
    }

    /// 检查是否使用加密连接
    pub fn is_encrypted(&self) -> bool {
        match &self.method {
            ConnectionMethod::Ssl(_) | ConnectionMethod::Ssh(_) => true,
            ConnectionMethod::Chain(hops) => hops
                .iter()
                .any(|h| matches!(h, ChainHop::Ssl(_) | ChainHop::Ssh(_))),
            _ => false,
        }
    }

    /// 检查是否使用代理
    pub fn is_proxied(&self) -> bool {
        match &self.method {
            ConnectionMethod::HttpProxy(_) | ConnectionMethod::SocksProxy(_) => true,
            ConnectionMethod::Chain(hops) => hops
                .iter()
                .any(|h| matches!(h, ChainHop::HttpProxy(_) | ChainHop::SocksProxy(_))),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_config_direct() {
        let config = ConnectionConfig::direct("localhost", 3306);
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 3306);
        assert!(matches!(config.method, ConnectionMethod::Direct));
        assert!(!config.is_encrypted());
        assert!(!config.is_proxied());
    }

    #[test]
    fn test_connection_config_ssl() {
        let ssl_config = SslConfig {
            verify_server_cert: true,
            ..Default::default()
        };
        let config = ConnectionConfig::ssl("localhost", 3306, ssl_config);
        assert!(config.is_encrypted());
        assert!(!config.is_proxied());
    }

    #[test]
    fn test_ssh_config() {
        let ssh_config = SshConfig {
            host: "ssh.example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: SshAuth::Password {
                password: "pass".to_string(),
            },
            remote_host: "db.internal".to_string(),
            remote_port: 3306,
            local_port: 0,
            timeout_secs: 30,
        };
        assert_eq!(ssh_config.port, 22);
        assert!(matches!(ssh_config.auth, SshAuth::Password { .. }));
    }
}

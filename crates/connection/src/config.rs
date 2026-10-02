//! 连接配置定义
//!
//! 定义统一的连接配置结构，支持多种连接方式

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

use crate::url_params::SslMode;

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

/// 结构化 TLS 请求：URL 表达不了的那部分 TLS 配置。
///
/// 为什么需要：连接串只能表达「要不要加密」（`ssl-mode` / `sslmode` / `require_ssl`），
/// **证书文件路径与校验意图无法写进 URL**（`mysql_async` 与 `tokio-postgres` 均无对应参数），
/// 所以在 URL 注入之外，还要把同一份请求结构地交给驱动：native 驱动的 TLS 连接器从
/// 这里构造（见 `engine::driver::native::{mysql_native, postgres_native}`），sqlx 驱动仍读 URL 参数。
///
/// 派生规则唯一来源：`workbench::services::connection_service::tls_request_of`
/// （网络档案 SSL 跳优先 → 「连接安全」内联覆盖）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsRequest {
    pub mode: SslMode,
    pub ssl: SslConfig,
}

impl TlsRequest {
    pub fn new(mode: SslMode, ssl: SslConfig) -> Self {
        Self { mode, ssl }
    }

    /// 要校验证书链吗（`verify-ca` / `verify-full`）。
    pub fn verifies_chain(&self) -> bool {
        self.mode.requires_verification()
    }

    /// 要校验主机名吗（仅 `verify-full`）。
    pub fn verifies_hostname(&self) -> bool {
        self.mode == SslMode::VerifyFull
    }

    /// 非空证书路径（空白视为未填，与表单清洗同一口径）。
    pub fn ca_path(&self) -> Option<&str> {
        non_empty(self.ssl.ca_cert_path.as_deref())
    }

    pub fn client_cert_path(&self) -> Option<&str> {
        non_empty(self.ssl.client_cert_path.as_deref())
    }

    pub fn client_key_path(&self) -> Option<&str> {
        non_empty(self.ssl.client_key_path.as_deref())
    }
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
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
    /// SSH Host Key 校验策略（TOFU 默认；Strict 高安全；AllowUnknown 内网退路）
    #[serde(default = "default_host_key_check")]
    pub host_key_check: super::known_hosts::HostKeyCheck,
}

fn default_host_key_check() -> super::known_hosts::HostKeyCheck {
    super::known_hosts::HostKeyCheck::Tofu
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
            host_key_check: crate::known_hosts::HostKeyCheck::Tofu,
        };
        assert_eq!(ssh_config.port, 22);
        assert!(matches!(ssh_config.auth, SshAuth::Password { .. }));
        // 决策 #37：默认 TOFU
        assert_eq!(
            ssh_config.host_key_check,
            crate::known_hosts::HostKeyCheck::Tofu
        );
    }

    fn ssh_config() -> SshConfig {
        SshConfig {
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
            host_key_check: crate::known_hosts::HostKeyCheck::Tofu,
        }
    }

    fn proxy_config() -> ProxyConfig {
        ProxyConfig {
            host: "proxy.example.com".to_string(),
            port: 8080,
            auth: None,
            no_proxy: Vec::new(),
            timeout_secs: 30,
        }
    }

    #[test]
    fn test_connection_method_default_is_direct() {
        assert_eq!(ConnectionMethod::default(), ConnectionMethod::Direct);
    }

    #[test]
    fn test_method_name_matrix() {
        assert_eq!(ConnectionConfig::direct("h", 1).method_name(), "direct");
        assert_eq!(
            ConnectionConfig::ssl("h", 1, SslConfig::default()).method_name(),
            "ssl"
        );
        assert_eq!(
            ConnectionConfig::ssh("h", 1, ssh_config()).method_name(),
            "ssh"
        );
        assert_eq!(
            ConnectionConfig::http_proxy("h", 1, proxy_config()).method_name(),
            "http_proxy"
        );
        assert_eq!(
            ConnectionConfig::socks_proxy("h", 1, proxy_config()).method_name(),
            "socks_proxy"
        );
        assert_eq!(ConnectionConfig::chain("h", 1, vec![]).method_name(), "chain");
    }

    #[test]
    fn test_encrypted_and_proxied_matrix() {
        let direct = ConnectionConfig::direct("h", 1);
        assert!(!direct.is_encrypted() && !direct.is_proxied());

        let ssl = ConnectionConfig::ssl("h", 1, SslConfig::default());
        assert!(ssl.is_encrypted() && !ssl.is_proxied());

        let ssh = ConnectionConfig::ssh("h", 1, ssh_config());
        assert!(ssh.is_encrypted() && !ssh.is_proxied());

        let http = ConnectionConfig::http_proxy("h", 1, proxy_config());
        assert!(!http.is_encrypted() && http.is_proxied());

        let socks = ConnectionConfig::socks_proxy("h", 1, proxy_config());
        assert!(!socks.is_encrypted() && socks.is_proxied());

        // 链式：任一跳含加密/代理即整体成立
        let chain_ssl = ConnectionConfig::chain("h", 1, vec![ChainHop::Ssl(SslConfig::default())]);
        assert!(chain_ssl.is_encrypted() && !chain_ssl.is_proxied());

        let chain_mixed = ConnectionConfig::chain(
            "h",
            1,
            vec![
                ChainHop::SocksProxy(proxy_config()),
                ChainHop::Ssl(SslConfig::default()),
            ],
        );
        assert!(chain_mixed.is_encrypted() && chain_mixed.is_proxied());
    }

    #[test]
    fn test_chain_hop_name() {
        assert_eq!(ChainHop::Ssh(ssh_config()).hop_name(), "ssh");
        assert_eq!(ChainHop::Ssl(SslConfig::default()).hop_name(), "ssl");
        assert_eq!(ChainHop::HttpProxy(proxy_config()).hop_name(), "http_proxy");
        assert_eq!(ChainHop::SocksProxy(proxy_config()).hop_name(), "socks_proxy");
    }

    #[test]
    fn test_tls_request_helpers() {
        use crate::url_params::SslMode;

        // verify-ca：校验证书链、不校验主机名
        let req = TlsRequest::new(
            SslMode::VerifyCa,
            SslConfig {
                verify_server_cert: true,
                ca_cert_path: Some("  /ca.pem  ".to_string()),
                client_cert_path: Some("   ".to_string()),
                client_key_path: None,
                min_tls_version: TlsVersion::Tls1_2,
            },
        );
        assert!(req.verifies_chain());
        assert!(!req.verifies_hostname());
        assert_eq!(req.ca_path(), Some("/ca.pem"), "空白应被 trim");
        assert_eq!(req.client_cert_path(), None, "纯空白视为未填");
        assert_eq!(req.client_key_path(), None);

        // verify-full：额外校验主机名
        let full = TlsRequest::new(
            SslMode::VerifyFull,
            SslConfig {
                client_key_path: Some("/k.pem".to_string()),
                ..Default::default()
            },
        );
        assert!(full.verifies_hostname());
        assert_eq!(full.client_key_path(), Some("/k.pem"));

        // disable：不校验证书链
        let off = TlsRequest::new(SslMode::Disable, SslConfig::default());
        assert!(!off.verifies_chain());
    }

    #[test]
    fn test_serde_defaults_for_ssl_and_proxy() {
        // verify_server_cert 与 min_tls_version 走 serde 默认（true / tls1_2）
        let ssl: SslConfig = serde_json::from_str("{}").unwrap();
        assert!(ssl.verify_server_cert, "verify_server_cert 默认 true");
        assert_eq!(ssl.min_tls_version, TlsVersion::Tls1_2);

        // 代理超时默认 30；auth / no_proxy 缺省为空
        let proxy: ProxyConfig = serde_json::from_str(r#"{"host":"proxy","port":8080}"#).unwrap();
        assert_eq!(proxy.timeout_secs, 30, "代理超时默认 30");
        assert!(proxy.no_proxy.is_empty());
        assert!(proxy.auth.is_none());
    }

    #[test]
    fn test_ssh_config_serde_roundtrip() {
        let ssh = ssh_config();
        let json = serde_json::to_string(&ssh).unwrap();
        let back: SshConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ssh, "SSH 配置序列化往返应一致（含 flatten 的 auth）");
    }

    #[test]
    fn test_connection_method_serde_roundtrip() {
        // 协议链（多跳）往返
        let cfg = ConnectionConfig::chain(
            "h",
            5432,
            vec![ChainHop::Ssh(ssh_config()), ChainHop::HttpProxy(proxy_config())],
        );
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ConnectionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.host, "h");
        assert_eq!(back.port, 5432);
        match &back.method {
            ConnectionMethod::Chain(hops) => {
                assert_eq!(hops.len(), 2);
                assert_eq!(hops[0].hop_name(), "ssh");
                assert_eq!(hops[1].hop_name(), "http_proxy");
                assert!(matches!(hops[0], ChainHop::Ssh(_)), "第一跳应为 SSH");
            }
            other => panic!("期望 Chain，实际 {:?}", other),
        }

        // 直连（unit 变体）往返
        let direct = ConnectionConfig::direct("h", 3306);
        let json = serde_json::to_string(&direct).unwrap();
        let back: ConnectionConfig = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.method, ConnectionMethod::Direct));
    }
}

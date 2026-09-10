//! 连接器 trait 和实现
//!
//! 提供统一的连接器接口，支持多种连接方式：
//! - Direct: TCP 直连
//! - SSH: 基于 russh 的真实 SSH 隧道（端口转发）
//! - SSL: 基于 native-tls 的 TLS 加密（支持 CA/客户端证书 mTLS）
//! - HTTP Proxy: HTTP CONNECT 隧道
//! - SOCKS5 Proxy: 基于 tokio-socks 的 SOCKS5 代理

use async_trait::async_trait;
use russh_keys::key::PrivateKeyWithHashAlg;
use russh_keys::PublicKeyBase64;
use std::sync::Arc;
use tokio::net::TcpStream;

use super::config::{
    ConnectionConfig, ConnectionMethod, ProxyConfig, SshAuth, SshConfig, SslConfig,
};
use super::stream::ConnectionStream;
use crate::core::error::{ConnectionError, CoreError};

/// 隧道生命周期守卫
///
/// 持有后台 bridge 任务的 JoinHandle 和优雅关闭信号。
/// 当 `TunnelGuard` 被 drop 时，自动发送关闭信号并 abort 后台任务，
/// 释放 TCP listener 和所有关联资源。
///
/// 使用模式：
/// ```ignore
/// let guard = establish_tunnel(...).await?;
/// let url = rewrite_url(original, "127.0.0.1", guard.port());
/// // 数据库连接存活期间保持 guard 不被 drop
/// // disconnect 时 drop(guard) 自动清理
/// ```
pub struct TunnelGuard {
    port: u16,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
    label: String,
}

impl TunnelGuard {
    pub fn new(
        port: u16,
        shutdown_tx: tokio::sync::oneshot::Sender<()>,
        task: tokio::task::JoinHandle<()>,
        label: impl Into<String>,
    ) -> Self {
        Self {
            port,
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
            label: label.into(),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Drop for TunnelGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            tracing::debug!(target: "tunnel", label = %self.label, port = self.port, "TunnelGuard 发送关闭信号");
        }
        if let Some(task) = self.task.take() {
            task.abort();
            tracing::debug!(target: "tunnel", label = %self.label, port = self.port, "TunnelGuard 中止后台任务");
        }
    }
}

/// 连接器 trait
///
/// 定义统一的连接接口，所有连接方式都需要实现此 trait
#[async_trait]
pub trait Connector: Send + Sync {
    /// 建立连接
    async fn connect(&self, config: &ConnectionConfig) -> Result<ConnectionStream, CoreError>;

    /// 检查是否支持此连接方式
    fn supports(&self, method: &ConnectionMethod) -> bool;

    /// 获取连接器名称
    fn name(&self) -> &'static str;
}

/// 连接句柄
pub struct Connection {
    pub stream: ConnectionStream,
    pub config: ConnectionConfig,
    pub established_at: std::time::Instant,
}

impl Connection {
    pub fn new(stream: ConnectionStream, config: ConnectionConfig) -> Self {
        Self {
            stream,
            config,
            established_at: std::time::Instant::now(),
        }
    }

    pub fn duration(&self) -> std::time::Duration {
        self.established_at.elapsed()
    }

    pub fn is_encrypted(&self) -> bool {
        self.stream.is_encrypted()
    }
}

/// 直接连接连接器
pub struct DirectConnector;

#[async_trait]
impl Connector for DirectConnector {
    async fn connect(&self, config: &ConnectionConfig) -> Result<ConnectionStream, CoreError> {
        let addr = format!("{}:{}", config.host, config.port);
        let stream = TcpStream::connect(&addr).await.map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: addr.clone(),
                reason: e.to_string(),
            })
        })?;

        Ok(ConnectionStream::tcp(stream))
    }

    fn supports(&self, method: &ConnectionMethod) -> bool {
        matches!(method, ConnectionMethod::Direct)
    }

    fn name(&self) -> &'static str {
        "direct"
    }
}

/// SSL/TLS 连接连接器
pub struct SslConnector;

#[async_trait]
impl Connector for SslConnector {
    async fn connect(&self, config: &ConnectionConfig) -> Result<ConnectionStream, CoreError> {
        let ConnectionMethod::Ssl(ssl_config) = &config.method else {
            return Err(CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: format!("{}:{}", config.host, config.port),
                reason: "Expected SSL connection method".to_string(),
            }));
        };

        let addr = format!("{}:{}", config.host, config.port);
        let tcp_stream = TcpStream::connect(&addr).await.map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: addr.clone(),
                reason: e.to_string(),
            })
        })?;

        let tls_stream = establish_tls(tcp_stream, &config.host, ssl_config).await?;

        Ok(ConnectionStream::tls(tls_stream))
    }

    fn supports(&self, method: &ConnectionMethod) -> bool {
        matches!(method, ConnectionMethod::Ssl(_))
    }

    fn name(&self) -> &'static str {
        "ssl"
    }
}

/// 建立 TLS 加密连接
///
/// 支持：
/// - 服务器证书校验（verify_server_cert）
/// - CA 证书加载（ca_cert_path）
/// - 客户端证书双向认证 mTLS（client_cert_path + client_key_path）
async fn establish_tls(
    stream: TcpStream,
    domain: &str,
    config: &SslConfig,
) -> Result<tokio_native_tls::TlsStream<TcpStream>, CoreError> {
    use native_tls::TlsConnector;
    use tokio_native_tls::TlsConnector as TokioTlsConnector;

    let mut builder = TlsConnector::builder();

    builder
        .danger_accept_invalid_certs(!config.verify_server_cert)
        .min_protocol_version(Some(map_tls_version(config.min_tls_version)))
        .max_protocol_version(
            Some(native_tls::Protocol::Tlsv12), // native-tls 默认上限
        );

    // 加载 CA 证书（用于验证服务器证书）
    if let Some(ref ca_path) = config.ca_cert_path {
        let ca_pem = std::fs::read(ca_path).map_err(|e| {
            CoreError::connection(ConnectionError::Tls {
                conn_id: domain.to_string(),
                reason: format!("读取 CA 证书文件 '{}' 失败: {}", ca_path, e),
            })
        })?;
        let ca_cert = native_tls::Certificate::from_pem(&ca_pem).map_err(|e| {
            CoreError::connection(ConnectionError::Tls {
                conn_id: domain.to_string(),
                reason: format!("解析 CA 证书失败: {}", e),
            })
        })?;
        builder.add_root_certificate(ca_cert);
    }

    // 加载客户端证书和私钥（mTLS 双向认证）
    if let (Some(ref cert_path), Some(ref key_path)) =
        (&config.client_cert_path, &config.client_key_path)
    {
        let cert_pem = std::fs::read(cert_path).map_err(|e| {
            CoreError::connection(ConnectionError::Tls {
                conn_id: domain.to_string(),
                reason: format!("读取客户端证书文件失败: {}", e),
            })
        })?;

        let key_pem = std::fs::read(key_path).map_err(|e| {
            CoreError::connection(ConnectionError::Tls {
                conn_id: domain.to_string(),
                reason: format!("读取客户端私钥文件失败: {}", e),
            })
        })?;

        let identity = native_tls::Identity::from_pkcs8(&cert_pem, &key_pem).map_err(|e| {
            CoreError::connection(ConnectionError::Tls {
                conn_id: domain.to_string(),
                reason: format!("加载客户端 PKCS8 证书失败: {}", e),
            })
        });
        // also try PEM format with PKCS12 as fallback
        let identity = match identity {
            Ok(id) => id,
            Err(_) => {
                // try PKCS#12
                native_tls::Identity::from_pkcs12(&cert_pem, "").map_err(|_e| {
                    CoreError::connection(ConnectionError::Tls {
                        conn_id: domain.to_string(),
                        reason: "客户端证书格式不支持，仅支持 PKCS#8 DER 或 PKCS#12".to_string(),
                    })
                })?
            }
        };

        builder.identity(identity);
    }

    let connector = builder.build().map_err(|e| {
        CoreError::connection(ConnectionError::Tls {
            conn_id: domain.to_string(),
            reason: format!("构建 TLS connector 失败: {}", e),
        })
    })?;

    let tokio_connector = TokioTlsConnector::from(connector);
    let tls_stream = tokio_connector.connect(domain, stream).await.map_err(|e| {
        CoreError::connection(ConnectionError::Tls {
            conn_id: domain.to_string(),
            reason: e.to_string(),
        })
    })?;

    Ok(tls_stream)
}

/// 映射 TlsVersion 枚举到 native_tls::Protocol
fn map_tls_version(
    version: crate::core::driver::connection::config::TlsVersion,
) -> native_tls::Protocol {
    use crate::core::driver::connection::config::TlsVersion;
    match version {
        TlsVersion::Tls1_0 => native_tls::Protocol::Tlsv10,
        TlsVersion::Tls1_1 => native_tls::Protocol::Tlsv11,
        TlsVersion::Tls1_2 => native_tls::Protocol::Tlsv12,
        TlsVersion::Tls1_3 => native_tls::Protocol::Tlsv13,
    }
}

/// SSH 隧道连接器（已废弃，SSH 隧道统一由 service 层 apply_network_method 处理）
pub struct SshTunnelConnector;

#[async_trait]
impl Connector for SshTunnelConnector {
    async fn connect(&self, _config: &ConnectionConfig) -> Result<ConnectionStream, CoreError> {
        Err(CoreError::connection(ConnectionError::NotSupported(
            "SSH 隧道请通过 ConnectionService.apply_network_method() 处理".to_string(),
        )))
    }

    fn supports(&self, method: &ConnectionMethod) -> bool {
        matches!(method, ConnectionMethod::Ssh(_))
    }

    fn name(&self) -> &'static str {
        "ssh_tunnel"
    }
}

/// SSH 客户端处理器
///
/// 处理 SSH 连接期间的服务端事件，集成 known_hosts 校验
struct SshClientHandler {
    host: String,
    port: u16,
    known_hosts: super::known_hosts::KnownHosts,
}

#[async_trait]
impl russh::client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key.fingerprint(russh::keys::HashAlg::Sha256);
        let key_b64 = server_public_key.public_key_base64();
        let key_type = key_b64.split_whitespace().next().unwrap_or("unknown");

        let verified = self
            .known_hosts
            .verify(&self.host, self.port, server_public_key);

        if verified {
            tracing::info!(
                target: "ssh_tunnel",
                fingerprint = %fingerprint,
                key_type = %key_type,
                host = %self.host,
                "Host Key 校验通过"
            );
        } else {
            tracing::error!(
                target: "ssh_tunnel",
                fingerprint = %fingerprint,
                key_type = %key_type,
                host = %self.host,
                "Host Key 校验失败，连接拒绝"
            );
        }

        Ok(verified)
    }
}

/// 建立 SSH 隧道（本地端口转发）
///
/// 流程：
/// 1. TCP 连接到 SSH 服务器
/// 2. russh SSH 协议握手
/// 3. 用户认证（密码/私钥/Agent）
/// 4. 绑定本地端口，后台任务接受本地连接并通过 SSH Channel 转发到目标
/// 5. 返回连接到本地隧道端口的 TcpStream
pub async fn establish_ssh_tunnel(
    _config: &ConnectionConfig,
    ssh_config: &SshConfig,
) -> Result<TunnelGuard, CoreError> {
    let ssh_addr = format!("{}:{}", ssh_config.host, ssh_config.port);

    let config = Arc::new(russh::client::Config::default());
    let known_hosts = super::known_hosts::create_known_hosts_checker(true);
    let handler = SshClientHandler {
        host: ssh_config.host.clone(),
        port: ssh_config.port,
        known_hosts,
    };

    let session = russh::client::connect(config, ssh_addr.as_str(), handler)
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: ssh_addr.clone(),
                reason: format!("SSH 握手失败: {}", e),
            })
        })?;

    let session = Arc::new(tokio::sync::Mutex::new(session));

    match &ssh_config.auth {
        SshAuth::Password { password } => {
            session
                .lock()
                .await
                .authenticate_password(&ssh_config.username, password)
                .await
                .map_err(|_| {
                    CoreError::connection(ConnectionError::AuthenticationFailed {
                        conn_id: ssh_addr.clone(),
                        username: ssh_config.username.clone(),
                    })
                })?;
        }
        SshAuth::PrivateKey {
            key_path,
            passphrase,
        } => {
            let key =
                russh::keys::load_secret_key(key_path, passphrase.as_deref()).map_err(|e| {
                    CoreError::connection(ConnectionError::InvalidConfig {
                        conn_id: ssh_addr.clone(),
                        reason: format!("无法加载SSH私钥 '{}': {}", key_path, e),
                    })
                })?;
            let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key), None).map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: ssh_addr.clone(),
                    reason: format!("SSH私钥哈希算法协商失败: {}", e),
                })
            })?;
            session
                .lock()
                .await
                .authenticate_publickey(&ssh_config.username, key_with_hash)
                .await
                .map_err(|_| {
                    CoreError::connection(ConnectionError::AuthenticationFailed {
                        conn_id: ssh_addr.clone(),
                        username: ssh_config.username.clone(),
                    })
                })?;
        }
        #[cfg(unix)]
        SshAuth::Agent => {
            let mut agent = connect_ssh_agent(&ssh_addr).await?;

            let identities = agent.request_identities().await.map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: ssh_addr.clone(),
                    reason: format!("无法获取 SSH Agent 身份列表: {}", e),
                })
            })?;

            if identities.is_empty() {
                return Err(CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: ssh_addr.clone(),
                    reason: "SSH Agent 中没有可用身份，请先通过 ssh-add 添加密钥".to_string(),
                }));
            }

            tracing::info!(
                target: "ssh_tunnel",
                count = identities.len(),
                "SSH Agent 中找到 {} 个身份",
                identities.len()
            );

            let mut authenticated = false;
            for pubkey in &identities {
                match session
                    .lock()
                    .await
                    .authenticate_publickey_with(&ssh_config.username, pubkey.clone(), &mut agent)
                    .await
                {
                    Ok(true) => {
                        authenticated = true;
                        tracing::info!(
                            target: "ssh_tunnel",
                            username = %ssh_config.username,
                            "SSH Agent 认证成功"
                        );
                        break;
                    }
                    Ok(false) => {
                        tracing::debug!(
                            target: "ssh_tunnel",
                            "SSH Agent 身份未通过认证，尝试下一个"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "ssh_tunnel",
                            "SSH Agent 认证过程出错: {}", e
                        );
                    }
                }
            }

            if !authenticated {
                return Err(CoreError::connection(
                    ConnectionError::AuthenticationFailed {
                        conn_id: ssh_addr.clone(),
                        username: ssh_config.username.clone(),
                    },
                ));
            }
        }
        #[cfg(not(unix))]
        SshAuth::Agent => {
            connect_ssh_agent(&ssh_addr).await?;
            unreachable!("Windows 上 connect_ssh_agent 总是返回 Err");
        }
    }

    let local_bind = if ssh_config.local_port == 0 {
        "127.0.0.1:0".to_string()
    } else {
        format!("127.0.0.1:{}", ssh_config.local_port)
    };

    let listener = tokio::net::TcpListener::bind(&local_bind)
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: local_bind.clone(),
                reason: format!("绑定本地端口失败: {}", e),
            })
        })?;

    let local_port = listener
        .local_addr()
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: local_bind.clone(),
                reason: format!("获取本地地址失败: {}", e),
            })
        })?
        .port();

    let remote_host = ssh_config.remote_host.clone();
    let remote_port = ssh_config.remote_port;
    let forward_session = session.clone();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let remote_label = format!("{}:{}", remote_host, remote_port);
    let remote_label2 = remote_label.clone();

    let task = tokio::spawn(async move {
        tracing::info!(
            target: "ssh_tunnel",
            remote = %remote_label,
            local_port,
            "SSH 隧道后台任务启动 (accept 循环)"
        );
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((local_stream, _)) => {
                            let fwd = forward_session.clone();
                            let rh = remote_host.clone();
                            let rp = remote_port;
                            tokio::spawn(async move {
                                match fwd.lock().await
                                    .channel_open_direct_tcpip(&rh, rp as u32, "127.0.0.1", 0)
                                    .await
                                {
                                    Ok(channel) => {
                                        let mut channel_stream = channel.into_stream();
                                        let (mut local_read, mut local_write) =
                                            tokio::io::split(local_stream);
                                        let (mut channel_read, mut channel_write) =
                                            tokio::io::split(&mut channel_stream);
                                        let t1 = tokio::io::copy(&mut local_read, &mut channel_write);
                                        let t2 = tokio::io::copy(&mut channel_read, &mut local_write);
                                        let _ = tokio::join!(t1, t2);
                                        tracing::debug!(target: "ssh_tunnel", "SSH 通道桥接结束");
                                    }
                                    Err(e) => {
                                        tracing::warn!(target: "ssh_tunnel", "SSH 通道打开失败: {}", e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(target: "ssh_tunnel", "接受本地隧道连接失败: {}", e);
                            break;
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    tracing::info!(target: "ssh_tunnel", local_port, "SSH 隧道收到关闭信号，退出 accept 循环");
                    break;
                }
            }
        }
        drop(listener);
        drop(forward_session);
        tracing::info!(target: "ssh_tunnel", local_port, "SSH 隧道后台任务已退出");
    });

    tracing::info!(
        target: "ssh_tunnel",
        host = %ssh_config.host,
        port = local_port,
        remote = %remote_label2,
        "SSH 隧道已建立"
    );

    Ok(TunnelGuard::new(local_port, shutdown_tx, task, "ssh"))
}

/// 连接到 SSH Agent（跨平台）
///
/// - Unix (Linux/macOS): 通过 `SSH_AUTH_SOCK` 环境变量连接 OpenSSH Agent
/// - Windows: 暂不支持（后续版本将通过 Pageant 集成）
#[cfg(unix)]
async fn connect_ssh_agent(
    ssh_addr: &str,
) -> Result<
    russh_keys::agent::client::AgentClient<
        Box<dyn russh_keys::agent::client::AgentStream + Send + Unpin + 'static>,
    >,
    CoreError,
> {
    russh_keys::agent::client::AgentClient::connect_env()
        .await
        .map(|c| c.dynamic())
        .map_err(|e| {
            CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: ssh_addr.to_string(),
                reason: format!(
                    "无法连接到 SSH Agent (SSH_AUTH_SOCK): {}. 请确保 ssh-agent 正在运行",
                    e
                ),
            })
        })
}

#[cfg(not(unix))]
async fn connect_ssh_agent(_ssh_addr: &str) -> Result<(), CoreError> {
    Err(CoreError::connection(ConnectionError::NotSupported(
        "SSH Agent 认证在当前平台暂未支持，请使用密码或私钥认证".to_string(),
    )))
}

/// HTTP 代理连接器
pub struct HttpProxyConnector;

#[async_trait]
impl Connector for HttpProxyConnector {
    async fn connect(&self, config: &ConnectionConfig) -> Result<ConnectionStream, CoreError> {
        let ConnectionMethod::HttpProxy(proxy_config) = &config.method else {
            return Err(CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: format!("{}:{}", config.host, config.port),
                reason: "Expected HTTP proxy connection method".to_string(),
            }));
        };

        let stream = establish_http_proxy(config, proxy_config).await?;
        Ok(ConnectionStream::HttpProxy(stream))
    }

    fn supports(&self, method: &ConnectionMethod) -> bool {
        matches!(method, ConnectionMethod::HttpProxy(_))
    }

    fn name(&self) -> &'static str {
        "http_proxy"
    }
}

pub async fn establish_http_proxy(
    config: &ConnectionConfig,
    proxy_config: &ProxyConfig,
) -> Result<TcpStream, CoreError> {
    let proxy_addr = format!("{}:{}", proxy_config.host, proxy_config.port);
    let mut stream = TcpStream::connect(&proxy_addr).await.map_err(|e| {
        CoreError::connection(ConnectionError::Network {
            conn_id: proxy_addr.clone(),
            reason: format!("Failed to connect to HTTP proxy: {}", e),
        })
    })?;

    let target = format!("{}:{}", config.host, config.port);
    let auth_header = if let Some(auth) = &proxy_config.auth {
        let credentials = base64::encode(format!("{}:{}", auth.username, auth.password));
        format!("Proxy-Authorization: Basic {}\r\n", credentials)
    } else {
        String::new()
    };

    let connect_request = format!(
        "CONNECT {} HTTP/1.1\r\nHost: {}\r\n{}\r\n",
        target, target, auth_header
    );

    tokio::io::AsyncWriteExt::write_all(&mut stream, connect_request.as_bytes())
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: proxy_addr.clone(),
                reason: format!("Failed to send CONNECT request: {}", e),
            })
        })?;

    let mut buffer = vec![0u8; 1024];
    let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer)
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: proxy_addr.clone(),
                reason: format!("Failed to read proxy response: {}", e),
            })
        })?;

    let response = String::from_utf8_lossy(&buffer[..n]);
    if !response.contains("200") {
        return Err(CoreError::connection(ConnectionError::Network {
            conn_id: proxy_addr,
            reason: format!("Proxy connection failed: {}", response),
        }));
    }

    Ok(stream)
}

/// SOCKS 代理连接器
pub struct SocksProxyConnector;

#[async_trait]
impl Connector for SocksProxyConnector {
    async fn connect(&self, config: &ConnectionConfig) -> Result<ConnectionStream, CoreError> {
        let ConnectionMethod::SocksProxy(proxy_config) = &config.method else {
            return Err(CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: format!("{}:{}", config.host, config.port),
                reason: "Expected SOCKS proxy connection method".to_string(),
            }));
        };

        let stream = establish_socks_proxy(config, proxy_config).await?;
        Ok(ConnectionStream::SocksProxy(stream))
    }

    fn supports(&self, method: &ConnectionMethod) -> bool {
        matches!(method, ConnectionMethod::SocksProxy(_))
    }

    fn name(&self) -> &'static str {
        "socks_proxy"
    }
}

pub async fn establish_socks_proxy(
    config: &ConnectionConfig,
    proxy_config: &ProxyConfig,
) -> Result<TcpStream, CoreError> {
    use tokio_socks::tcp::Socks5Stream;

    let proxy_addr = format!("{}:{}", proxy_config.host, proxy_config.port);
    let target_addr = format!("{}:{}", config.host, config.port);

    let stream = if let Some(auth) = &proxy_config.auth {
        Socks5Stream::connect_with_password(
            proxy_addr.as_str(),
            target_addr.as_str(),
            &auth.username,
            &auth.password,
        )
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: proxy_addr.clone(),
                reason: format!("SOCKS5 connection failed: {}", e),
            })
        })?
        .into_inner()
    } else {
        Socks5Stream::connect(proxy_addr.as_str(), target_addr.as_str())
            .await
            .map_err(|e| {
                CoreError::connection(ConnectionError::Network {
                    conn_id: proxy_addr.clone(),
                    reason: format!("SOCKS5 connection failed: {}", e),
                })
            })?
            .into_inner()
    };

    Ok(stream)
}

mod base64 {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    pub fn encode(input: impl AsRef<[u8]>) -> String {
        STANDARD.encode(input.as_ref())
    }
}

/// SSL 证书过期信息
#[derive(Debug, Clone)]
pub struct SslCertInfo {
    /// 证书文件路径
    pub path: String,
    /// 证书主题（CN）
    pub subject: String,
    /// 颁发者
    pub issuer: String,
    /// 生效时间
    pub not_before: String,
    /// 过期时间
    pub not_after: String,
    /// 距离过期天数（负数 = 已过期）
    pub days_until_expiry: i64,
    /// 是否已过期
    pub is_expired: bool,
}

/// 检查 PEM/DER 格式 X.509 证书的过期状态
///
/// 读取证书文件，解析 not_before / not_after，计算距离过期的天数。
/// 可同时用于 CA 证书和客户端证书的过期检测。
pub fn check_cert_expiry(cert_path: &str) -> Result<SslCertInfo, CoreError> {
    let pem_data = std::fs::read(cert_path).map_err(|e| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: cert_path.to_string(),
            reason: format!("无法读取证书文件: {}", e),
        })
    })?;

    let der_storage: Vec<u8>;
    #[allow(deprecated)]
    let der_ref: &[u8] = if let Ok((_, pem)) = x509_parser::pem::pem_to_der(&pem_data) {
        der_storage = pem.contents;
        &der_storage
    } else {
        &pem_data
    };

    let (_, cert) = x509_parser::parse_x509_certificate(der_ref).map_err(|e| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: cert_path.to_string(),
            reason: format!("证书解析失败: {}", e),
        })
    })?;

    let subject = cert.subject().to_string();
    let issuer = cert.issuer().to_string();

    let not_before = cert.validity().not_before.to_datetime();
    let not_after = cert.validity().not_after.to_datetime();

    let not_before_unix = not_before.unix_timestamp();
    let not_after_unix = not_after.unix_timestamp();
    let now_unix = chrono::Utc::now().timestamp();

    let days_until_expiry = (not_after_unix - now_unix) / 86400;
    let is_expired = days_until_expiry < 0;

    let not_before_str = chrono::DateTime::from_timestamp(not_before_unix, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| not_before.to_string());
    let not_after_str = chrono::DateTime::from_timestamp(not_after_unix, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| not_after.to_string());

    Ok(SslCertInfo {
        path: cert_path.to_string(),
        subject,
        issuer,
        not_before: not_before_str,
        not_after: not_after_str,
        days_until_expiry,
        is_expired,
    })
}

/// 在 TcpStream 之上建立 TLS 加密层
///
/// 用于 Proxy → SSL 嵌套：代理 CONNECT 成功后，对透传的 TCP 流进行 TLS 封装。
/// sqlx 后续通过隧道端口连接时，数据在代理隧道内已经过 TLS 加密。
pub async fn wrap_tls_stream(
    tcp_stream: tokio::net::TcpStream,
    ssl_config: &super::config::SslConfig,
    server_host: &str,
) -> Result<tokio_native_tls::TlsStream<tokio::net::TcpStream>, CoreError> {
    let mut builder = native_tls::TlsConnector::builder();

    if ssl_config.verify_server_cert {
        if let Some(ca_path) = &ssl_config.ca_cert_path {
            let ca_cert = std::fs::read(ca_path).map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: ca_path.clone(),
                    reason: format!("无法读取 CA 证书: {}", e),
                })
            })?;
            let cert = native_tls::Certificate::from_pem(&ca_cert).map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: ca_path.clone(),
                    reason: format!("CA 证书解析失败: {}", e),
                })
            })?;
            builder.add_root_certificate(cert);
        }
    } else {
        builder.danger_accept_invalid_certs(true);
    }

    if let (Some(cert_path), Some(key_path)) =
        (&ssl_config.client_cert_path, &ssl_config.client_key_path)
    {
        let client_cert_data = std::fs::read(cert_path).map_err(|e| {
            CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: cert_path.clone(),
                reason: format!("无法读取客户端证书: {}", e),
            })
        })?;
        let client_key_data = std::fs::read(key_path).map_err(|e| {
            CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: key_path.clone(),
                reason: format!("无法读取客户端私钥: {}", e),
            })
        })?;

        let identity = native_tls::Identity::from_pkcs8(&client_cert_data, &client_key_data)
            .or_else(|_| native_tls::Identity::from_pkcs12(&client_cert_data, ""))
            .map_err(|e| {
                CoreError::connection(ConnectionError::InvalidConfig {
                    conn_id: cert_path.clone(),
                    reason: format!("客户端证书/私钥加载失败: {}", e),
                })
            })?;
        builder.identity(identity);
    }

    let connector = builder.build().map_err(|e| {
        CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: server_host.to_string(),
            reason: format!("TLS Connector 构建失败: {}", e),
        })
    })?;

    let tls_connector = tokio_native_tls::TlsConnector::from(connector);
    let tls_stream = tls_connector
        .connect(server_host, tcp_stream)
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: server_host.to_string(),
                reason: format!("TLS 握手失败: {}", e),
            })
        })?;

    tracing::info!(
        target: "tls_wrapper",
        host = %server_host,
        "Proxy → SSL 嵌套 TLS 层已建立"
    );

    Ok(tls_stream)
}

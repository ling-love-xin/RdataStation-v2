//! 连接器 trait 和实现
//!
//! 提供统一的连接器接口，支持多种连接方式：
//! - Direct: TCP 直连
//! - SSH: 基于 russh 的真实 SSH 隧道（端口转发）
//! - SSL: 基于 native-tls 的 TLS 加密（支持 CA/客户端证书 mTLS）
//! - HTTP Proxy: HTTP CONNECT 隧道
//! - SOCKS5 Proxy: 基于 tokio-socks 的 SOCKS5 代理

use async_trait::async_trait;
use tokio::net::TcpStream;

use super::config::{ConnectionConfig, ConnectionMethod};
use super::stream::ConnectionStream;
use shared::error::{ConnectionError, CoreError};

mod proxy;
mod ssh;
mod tls;

pub use proxy::{
    HttpProxyConnector, SocksProxyConnector, establish_http_proxy, establish_socks_proxy,
};
pub use ssh::{SshTunnelConnector, establish_ssh_tunnel};
pub use tls::{SslConnector, wrap_tls_stream};

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


mod base64 {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    pub fn encode(input: impl AsRef<[u8]>) -> String {
        STANDARD.encode(input.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::tls::map_tls_version;
    use crate::config::{ProxyConfig, SshAuth, SshConfig, SslConfig, TlsVersion};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    fn test_map_tls_version() {
        assert!(matches!(
            map_tls_version(TlsVersion::Tls1_0),
            native_tls::Protocol::Tlsv10
        ));
        assert!(matches!(
            map_tls_version(TlsVersion::Tls1_1),
            native_tls::Protocol::Tlsv11
        ));
        assert!(matches!(
            map_tls_version(TlsVersion::Tls1_2),
            native_tls::Protocol::Tlsv12
        ));
        assert!(matches!(
            map_tls_version(TlsVersion::Tls1_3),
            native_tls::Protocol::Tlsv13
        ));
    }

    #[test]
    fn test_connector_supports_matrix_and_names() {
        let direct = DirectConnector;
        let ssl = SslConnector;
        let ssh = SshTunnelConnector;
        let http = HttpProxyConnector;
        let socks = SocksProxyConnector;

        let direct_m = ConnectionMethod::Direct;
        let ssl_m = ConnectionMethod::Ssl(SslConfig::default());
        let ssh_m = ConnectionMethod::Ssh(ssh_config());
        let http_m = ConnectionMethod::HttpProxy(proxy_config());
        let socks_m = ConnectionMethod::SocksProxy(proxy_config());

        assert!(direct.supports(&direct_m) && !direct.supports(&ssl_m));
        assert!(ssl.supports(&ssl_m) && !ssl.supports(&direct_m));
        // 占位器语义：supports(Ssh) 为 true（工厂能命中），真正报错在 connect
        assert!(ssh.supports(&ssh_m), "SshTunnelConnector::supports(Ssh) 应为 true（占位）");
        assert!(http.supports(&http_m) && !http.supports(&socks_m));
        assert!(socks.supports(&socks_m) && !socks.supports(&http_m));

        assert_eq!(direct.name(), "direct");
        assert_eq!(ssl.name(), "ssl");
        assert_eq!(ssh.name(), "ssh_tunnel");
        assert_eq!(http.name(), "http_proxy");
        assert_eq!(socks.name(), "socks_proxy");
    }

    #[tokio::test]
    async fn test_ssh_connector_connect_returns_not_supported() {
        // 回归：SSH 隧道必须走 chain，绝不静默直连
        let connector = SshTunnelConnector;
        let cfg = ConnectionConfig::ssh("db.internal", 3306, ssh_config());
        let err = match connector.connect(&cfg).await {
            Err(e) => e,
            Ok(_) => panic!("connect(Ssh) 应报错，不应返回流"),
        };
        assert!(
            matches!(err, CoreError::Connection(ConnectionError::NotSupported(_))),
            "connect(Ssh) 应返回 NotSupported，实际 {err}"
        );
    }

    #[tokio::test]
    async fn test_connectors_wrong_method_errors() {
        let direct_cfg = ConnectionConfig::direct("h", 5432);

        let err = match SslConnector.connect(&direct_cfg).await {
            Err(e) => e,
            Ok(_) => panic!("SslConnector 不应接受直连配置"),
        };
        assert!(err.to_string().contains("Expected SSL connection method"), "{err}");

        let err = match HttpProxyConnector.connect(&direct_cfg).await {
            Err(e) => e,
            Ok(_) => panic!("HttpProxyConnector 不应接受直连配置"),
        };
        assert!(err.to_string().contains("Expected HTTP proxy"), "{err}");

        let err = match SocksProxyConnector.connect(&direct_cfg).await {
            Err(e) => e,
            Ok(_) => panic!("SocksProxyConnector 不应接受直连配置"),
        };
        assert!(err.to_string().contains("Expected SOCKS proxy"), "{err}");
    }

    /// 起一个最小 HTTP 代理端：读取 CONNECT 请求头，返回指定响应，并回传请求头文本。
    async fn spawn_proxy_responder(
        response: &'static [u8],
    ) -> (
        u16,
        tokio::sync::oneshot::Receiver<String>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind proxy");
        let port = listener.local_addr().expect("port").port();
        let (head_tx, head_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut head = Vec::new();
            let mut chunk = [0u8; 512];
            loop {
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        head.extend_from_slice(&chunk[..n]);
                        if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() > 16 * 1024 {
                            break;
                        }
                    }
                }
            }
            let _ = head_tx.send(String::from_utf8_lossy(&head).into_owned());
            let _ = stream.write_all(response).await;
        });
        (port, head_rx, handle)
    }

    #[tokio::test]
    async fn test_http_proxy_connect_success_and_auth_header() {
        let (port, head_rx, handle) =
            spawn_proxy_responder(b"HTTP/1.1 200 Connection Established\r\n\r\n").await;

        let cfg = ConnectionConfig::direct("db.example", 3306);
        let proxy = ProxyConfig {
            host: "127.0.0.1".to_string(),
            port,
            auth: Some(crate::config::ProxyAuth {
                username: "u".to_string(),
                password: "p".to_string(),
            }),
            no_proxy: Vec::new(),
            timeout_secs: 5,
        };

        let stream = establish_http_proxy(&cfg, &proxy)
            .await
            .expect("CONNECT 应成功");
        let head = head_rx.await.expect("代理应收到请求头");
        assert!(
            head.starts_with("CONNECT db.example:3306 HTTP/1.1"),
            "CONNECT 行不正确: {head}"
        );
        // base64("u:p") = "dTpw"
        assert!(
            head.contains("Proxy-Authorization: Basic dTpw"),
            "应携带 Basic 认证头: {head}"
        );
        drop(stream);
        handle.abort();
    }

    #[tokio::test]
    async fn test_http_proxy_non_200_response_errors() {
        let (port, _head_rx, handle) =
            spawn_proxy_responder(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;

        let cfg = ConnectionConfig::direct("db.example", 3306);
        let proxy = ProxyConfig {
            host: "127.0.0.1".to_string(),
            port,
            auth: None,
            no_proxy: Vec::new(),
            timeout_secs: 5,
        };

        let err = establish_http_proxy(&cfg, &proxy).await.unwrap_err();
        assert!(
            err.to_string().contains("403"),
            "错误应透出代理拒绝原因: {err}"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn test_http_proxy_403_with_200_in_headers_still_errors() {
        // 回归：成功判据改为解析状态行（此前 `contains("200")` 会把
        // 拒绝响应里恰好含 "200" 的头部/正文误判为成功）
        let (port, _head_rx, handle) = spawn_proxy_responder(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 200\r\n\r\n",
        )
        .await;

        let cfg = ConnectionConfig::direct("db.example", 3306);
        let proxy = ProxyConfig {
            host: "127.0.0.1".to_string(),
            port,
            auth: None,
            no_proxy: Vec::new(),
            timeout_secs: 5,
        };

        let err = establish_http_proxy(&cfg, &proxy).await.unwrap_err();
        assert!(
            err.to_string().contains("403"),
            "403 状态行必须报错，即便响应头含 200: {err}"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn test_tunnel_guard_drop_sends_shutdown() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async {});
        let guard = TunnelGuard::new(8080, tx, task, "ssh-test");
        assert_eq!(guard.port(), 8080);
        assert_eq!(guard.label(), "ssh-test");

        drop(guard);
        // Drop 内 tx.send 先行、abort 在后：rx 必然 resolve
        tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .expect("drop 应发送关闭信号（2s 内）")
            .expect("关闭信号已送达");
    }

    #[tokio::test]
    async fn test_connection_handle_fields() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();

        let conn = Connection::new(
            ConnectionStream::tcp(client),
            ConnectionConfig::direct("h", 1),
        );
        assert!(!conn.is_encrypted());
        assert!(conn.duration() >= Duration::ZERO);
    }
}

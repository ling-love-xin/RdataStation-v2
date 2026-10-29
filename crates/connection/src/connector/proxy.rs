//! 代理连接器：HTTP CONNECT 与 SOCKS5 隧道。

use async_trait::async_trait;
use tokio::net::TcpStream;

use crate::connector::base64;
use crate::config::{ConnectionConfig, ConnectionMethod, ProxyConfig};
use crate::stream::ConnectionStream;
use crate::connector::Connector;
use shared::error::{ConnectionError, CoreError};

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
    // 成功判据：解析首行状态行（`HTTP/1.1 200 ...`）。不能对整段响应做子串匹配——
    // `contains("200")` 会把头部/正文里恰好含 "200" 的拒绝响应（如
    // `Content-Length: 200`）误判为成功（回归见 test_http_proxy_403_with_200_in_headers_still_errors）。
    let status_ok = response
        .lines()
        .next()
        .map(|line| {
            let mut parts = line.split_whitespace();
            matches!(parts.next(), Some("HTTP/1.1") | Some("HTTP/1.0"))
                && parts.next() == Some("200")
        })
        .unwrap_or(false);
    if !status_ok {
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
//! TLS 连接器：`SslConnector` + TLS 封装工具（establish_tls / wrap_tls_stream）。

use async_trait::async_trait;
use tokio::net::TcpStream;

use crate::config::{ConnectionConfig, ConnectionMethod, SslConfig};
use crate::stream::ConnectionStream;
use crate::connector::Connector;
use shared::error::{ConnectionError, CoreError};

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
    if let (Some(cert_path), Some(key_path)) =
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
pub(crate) fn map_tls_version(version: crate::config::TlsVersion) -> native_tls::Protocol {
    use crate::config::TlsVersion;
    match version {
        TlsVersion::Tls1_0 => native_tls::Protocol::Tlsv10,
        TlsVersion::Tls1_1 => native_tls::Protocol::Tlsv11,
        TlsVersion::Tls1_2 => native_tls::Protocol::Tlsv12,
        TlsVersion::Tls1_3 => native_tls::Protocol::Tlsv13,
    }
}


/// 在 TcpStream 之上建立 TLS 加密层
pub async fn wrap_tls_stream(
    tcp_stream: tokio::net::TcpStream,
    ssl_config: &crate::config::SslConfig,
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
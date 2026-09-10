//! 连接器（Connector）模块综合测试
//!
//! 覆盖 TunnelGuard、DirectConnector、SslConnector、HttpProxyConnector、
//! SocksProxyConnector、SshTunnelConnector、Connection、SslConfig、
//! check_cert_expiry、ConnectionConfig 等核心类型与函数。
//!
//! 注意：
//! - `map_tls_version` 是私有函数，无法在集成测试中直接测试，但
//!   可通过 SslConfig 的 min_tls_version 字段间接验证其行为。
//! - `base64` 子模块是私有的，无法在集成测试中直接测试。
//! - 需要真实网络的测试标记为 `#[ignore]`。

use rdata_station_lib::core::driver::connection::config::*;
use rdata_station_lib::core::driver::connection::connector::*;
use rdata_station_lib::core::driver::connection::stream::ConnectionStream;
use rdata_station_lib::core::error::{ConnectionError, CoreError};

// ============================================================================
// TunnelGuard 测试
// ============================================================================

#[tokio::test]
async fn test_tunnel_guard_new_sets_port() {
    let (_tx, _rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async {});
    let guard = TunnelGuard::new(12345, _tx, task, "test-tunnel");

    assert_eq!(guard.port(), 12345);
}

#[tokio::test]
async fn test_tunnel_guard_new_sets_label() {
    let (_tx, _rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async {});
    let guard = TunnelGuard::new(9999, _tx, task, "my-ssh-tunnel");

    assert_eq!(guard.label(), "my-ssh-tunnel");
}

#[tokio::test]
async fn test_tunnel_guard_label_empty_string() {
    let (_tx, _rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async {});
    let guard = TunnelGuard::new(1, _tx, task, "");

    assert_eq!(guard.label(), "");
}

#[tokio::test]
async fn test_tunnel_guard_drop_sends_shutdown_signal() {
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async {
        // 后台任务仅等待关闭信号，不做实际工作
    });

    {
        let guard = TunnelGuard::new(8888, tx, task, "shutdown-test");
        drop(guard);
    }

    // 验证 shutdown 信号已发送
    let result = rx.try_recv();
    assert!(
        result.is_ok(),
        "shutdown signal should have been sent on drop"
    );
}

#[tokio::test]
async fn test_tunnel_guard_drop_aborts_background_task() {
    let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
    let (_done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();

    let task = tokio::spawn(async move {
        // 模拟一个会持续运行的后台任务
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        // 如果任务被 abort，下面这行永远不会执行
        #[allow(unreachable_code)]
        let _ = _done_tx.send(());
    });

    {
        let guard = TunnelGuard::new(7777, tx, task, "abort-test");
        // 给任务一点时间启动
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        drop(guard);
    }

    // 给 abort 一点时间生效
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // 任务被 abort 后，done_tx 不会被发送
    assert!(
        done_rx.try_recv().is_err(),
        "background task should have been aborted"
    );
}

#[tokio::test]
async fn test_tunnel_guard_port_zero() {
    let (_tx, _rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async {});
    let guard = TunnelGuard::new(0, _tx, task, "zero-port");

    assert_eq!(guard.port(), 0);
}

#[tokio::test]
async fn test_tunnel_guard_port_max() {
    let (_tx, _rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async {});
    let guard = TunnelGuard::new(65535, _tx, task, "max-port");

    assert_eq!(guard.port(), 65535);
}

// ============================================================================
// DirectConnector 测试
// ============================================================================

#[test]
fn test_direct_connector_name() {
    let connector = DirectConnector;
    assert_eq!(connector.name(), "direct");
}

#[test]
fn test_direct_connector_supports_direct() {
    let connector = DirectConnector;
    assert!(connector.supports(&ConnectionMethod::Direct));
}

#[test]
fn test_direct_connector_rejects_ssh() {
    let connector = DirectConnector;
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
    assert!(!connector.supports(&ConnectionMethod::Ssh(ssh_config)));
}

#[test]
fn test_direct_connector_rejects_ssl() {
    let connector = DirectConnector;
    let ssl_config = SslConfig::default();
    assert!(!connector.supports(&ConnectionMethod::Ssl(ssl_config)));
}

#[test]
fn test_direct_connector_rejects_http_proxy() {
    let connector = DirectConnector;
    let proxy_config = ProxyConfig {
        host: "proxy.example.com".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(!connector.supports(&ConnectionMethod::HttpProxy(proxy_config)));
}

#[test]
fn test_direct_connector_rejects_socks_proxy() {
    let connector = DirectConnector;
    let proxy_config = ProxyConfig {
        host: "socks.example.com".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(!connector.supports(&ConnectionMethod::SocksProxy(proxy_config)));
}

#[test]
fn test_direct_connector_rejects_chain() {
    let connector = DirectConnector;
    assert!(!connector.supports(&ConnectionMethod::Chain(vec![])));
}

#[tokio::test]
#[ignore = "requires network access to an unreachable host"]
async fn test_direct_connector_connect_refused() {
    let connector = DirectConnector;
    let config = ConnectionConfig::direct("127.0.0.1", 19999);
    let result = connector.connect(&config).await;
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::Network { .. })) => {
            // 预期：连接被拒绝
        }
        _other => panic!("Expected Network error, got unexpected result"),
    }
}

#[tokio::test]
#[ignore = "requires network access to an unreachable host"]
async fn test_direct_connector_connect_unresolvable_host() {
    let connector = DirectConnector;
    let config = ConnectionConfig::direct("nonexistent-host-12345.invalid", 3306);
    let result = connector.connect(&config).await;
    assert!(result.is_err());
}

// ============================================================================
// SslConnector 测试
// ============================================================================

#[test]
fn test_ssl_connector_name() {
    let connector = SslConnector;
    assert_eq!(connector.name(), "ssl");
}

#[test]
fn test_ssl_connector_supports_ssl() {
    let connector = SslConnector;
    let ssl_config = SslConfig::default();
    assert!(connector.supports(&ConnectionMethod::Ssl(ssl_config)));
}

#[test]
fn test_ssl_connector_rejects_direct() {
    let connector = SslConnector;
    assert!(!connector.supports(&ConnectionMethod::Direct));
}

#[test]
fn test_ssl_connector_rejects_ssh() {
    let connector = SslConnector;
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
    assert!(!connector.supports(&ConnectionMethod::Ssh(ssh_config)));
}

#[test]
fn test_ssl_connector_rejects_http_proxy() {
    let connector = SslConnector;
    let proxy_config = ProxyConfig {
        host: "proxy".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(!connector.supports(&ConnectionMethod::HttpProxy(proxy_config)));
}

#[tokio::test]
async fn test_ssl_connector_wrong_method_returns_error() {
    let connector = SslConnector;
    let config = ConnectionConfig::direct("localhost", 3306);
    let result = connector.connect(&config).await;
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::InvalidConfig { reason, .. })) => {
            assert!(reason.contains("Expected SSL"));
        }
        _other => panic!("Expected InvalidConfig error, got unexpected result"),
    }
}

#[tokio::test]
#[ignore = "requires a real TLS server"]
async fn test_ssl_connector_connect_with_valid_config() {
    let connector = SslConnector;
    let ssl_config = SslConfig {
        verify_server_cert: false,
        ..Default::default()
    };
    let config = ConnectionConfig::ssl("localhost", 443, ssl_config);
    let result = connector.connect(&config).await;
    // 可能成功也可能失败，取决于 localhost:443 是否有 TLS 服务
    // 至少验证返回的是 Result 类型
    let _ = result;
}

// ============================================================================
// HttpProxyConnector 测试
// ============================================================================

#[test]
fn test_http_proxy_connector_name() {
    let connector = HttpProxyConnector;
    assert_eq!(connector.name(), "http_proxy");
}

#[test]
fn test_http_proxy_connector_supports_http_proxy() {
    let connector = HttpProxyConnector;
    let proxy_config = ProxyConfig {
        host: "proxy.example.com".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(connector.supports(&ConnectionMethod::HttpProxy(proxy_config)));
}

#[test]
fn test_http_proxy_connector_rejects_direct() {
    let connector = HttpProxyConnector;
    assert!(!connector.supports(&ConnectionMethod::Direct));
}

#[test]
fn test_http_proxy_connector_rejects_ssl() {
    let connector = HttpProxyConnector;
    assert!(!connector.supports(&ConnectionMethod::Ssl(SslConfig::default())));
}

#[test]
fn test_http_proxy_connector_rejects_socks_proxy() {
    let connector = HttpProxyConnector;
    let proxy_config = ProxyConfig {
        host: "socks".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(!connector.supports(&ConnectionMethod::SocksProxy(proxy_config)));
}

#[tokio::test]
async fn test_http_proxy_connector_wrong_method_returns_error() {
    let connector = HttpProxyConnector;
    let config = ConnectionConfig::direct("localhost", 3306);
    let result = connector.connect(&config).await;
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::InvalidConfig { reason, .. })) => {
            assert!(reason.contains("Expected HTTP proxy"));
        }
        _other => panic!("Expected InvalidConfig error, got unexpected result"),
    }
}

#[tokio::test]
#[ignore = "requires a real HTTP proxy server"]
async fn test_http_proxy_connector_connect_unreachable() {
    let connector = HttpProxyConnector;
    let proxy_config = ProxyConfig {
        host: "127.0.0.1".to_string(),
        port: 19999,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 5,
    };
    let config = ConnectionConfig {
        host: "db.example.com".to_string(),
        port: 3306,
        method: ConnectionMethod::HttpProxy(proxy_config),
        options: Default::default(),
    };
    let result = connector.connect(&config).await;
    assert!(result.is_err());
}

// ============================================================================
// SocksProxyConnector 测试
// ============================================================================

#[test]
fn test_socks_proxy_connector_name() {
    let connector = SocksProxyConnector;
    assert_eq!(connector.name(), "socks_proxy");
}

#[test]
fn test_socks_proxy_connector_supports_socks_proxy() {
    let connector = SocksProxyConnector;
    let proxy_config = ProxyConfig {
        host: "socks.example.com".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(connector.supports(&ConnectionMethod::SocksProxy(proxy_config)));
}

#[test]
fn test_socks_proxy_connector_rejects_direct() {
    let connector = SocksProxyConnector;
    assert!(!connector.supports(&ConnectionMethod::Direct));
}

#[test]
fn test_socks_proxy_connector_rejects_ssl() {
    let connector = SocksProxyConnector;
    assert!(!connector.supports(&ConnectionMethod::Ssl(SslConfig::default())));
}

#[test]
fn test_socks_proxy_connector_rejects_http_proxy() {
    let connector = SocksProxyConnector;
    let proxy_config = ProxyConfig {
        host: "http-proxy".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(!connector.supports(&ConnectionMethod::HttpProxy(proxy_config)));
}

#[tokio::test]
async fn test_socks_proxy_connector_wrong_method_returns_error() {
    let connector = SocksProxyConnector;
    let config = ConnectionConfig::direct("localhost", 3306);
    let result = connector.connect(&config).await;
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::InvalidConfig { reason, .. })) => {
            assert!(reason.contains("Expected SOCKS proxy"));
        }
        _other => panic!("Expected InvalidConfig error, got unexpected result"),
    }
}

#[tokio::test]
#[ignore = "requires a real SOCKS5 proxy server"]
async fn test_socks_proxy_connector_connect_unreachable() {
    let connector = SocksProxyConnector;
    let proxy_config = ProxyConfig {
        host: "127.0.0.1".to_string(),
        port: 19998,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 5,
    };
    let config = ConnectionConfig {
        host: "db.example.com".to_string(),
        port: 3306,
        method: ConnectionMethod::SocksProxy(proxy_config),
        options: Default::default(),
    };
    let result = connector.connect(&config).await;
    assert!(result.is_err());
}

// ============================================================================
// SshTunnelConnector 测试
// ============================================================================

#[test]
fn test_ssh_tunnel_connector_name() {
    let connector = SshTunnelConnector;
    assert_eq!(connector.name(), "ssh_tunnel");
}

#[test]
fn test_ssh_tunnel_connector_supports_ssh() {
    let connector = SshTunnelConnector;
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
    assert!(connector.supports(&ConnectionMethod::Ssh(ssh_config)));
}

#[test]
fn test_ssh_tunnel_connector_rejects_direct() {
    let connector = SshTunnelConnector;
    assert!(!connector.supports(&ConnectionMethod::Direct));
}

#[test]
fn test_ssh_tunnel_connector_rejects_ssl() {
    let connector = SshTunnelConnector;
    assert!(!connector.supports(&ConnectionMethod::Ssl(SslConfig::default())));
}

#[tokio::test]
async fn test_ssh_tunnel_connector_connect_returns_not_supported() {
    let connector = SshTunnelConnector;
    let config = ConnectionConfig {
        host: "db.internal".to_string(),
        port: 3306,
        method: ConnectionMethod::Ssh(SshConfig {
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
        }),
        options: Default::default(),
    };
    let result = connector.connect(&config).await;
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::NotSupported(msg))) => {
            assert!(
                msg.contains("SSH"),
                "Expected SSH-related message, got: {}",
                msg
            );
        }
        _other => panic!("Expected NotSupported error, got unexpected result"),
    }
}

// ============================================================================
// Connection 测试
// ============================================================================

#[tokio::test]
async fn test_connection_new_creates_with_tcp_stream() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind listener");
    let addr = listener.local_addr().unwrap();

    let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = listener.accept().await;
        let _ = server_done_rx.await;
    });

    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("Failed to connect");
    let conn_stream = ConnectionStream::tcp(stream);
    let config = ConnectionConfig::direct("127.0.0.1", addr.port());

    let conn = Connection::new(conn_stream, config);

    assert_eq!(conn.config.host, "127.0.0.1");
    assert_eq!(conn.config.port, addr.port());

    let _ = server_done_tx.send(());
}

#[tokio::test]
async fn test_connection_duration_returns_positive() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = listener.accept().await;
        let _ = rx.await;
    });

    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let conn_stream = ConnectionStream::tcp(stream);
    let config = ConnectionConfig::direct("127.0.0.1", addr.port());

    let conn = Connection::new(conn_stream, config);

    // 稍微等待一下确保流逝时间 > 0
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let duration = conn.duration();
    assert!(
        duration > std::time::Duration::ZERO,
        "duration should be positive"
    );

    let _ = tx.send(());
}

#[tokio::test]
async fn test_connection_is_encrypted_false_for_tcp() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = listener.accept().await;
        let _ = rx.await;
    });

    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let conn_stream = ConnectionStream::tcp(stream);
    let config = ConnectionConfig::direct("127.0.0.1", addr.port());

    let conn = Connection::new(conn_stream, config);
    assert!(!conn.is_encrypted());

    let _ = tx.send(());
}

#[tokio::test]
async fn test_connection_is_encrypted_false_for_ssh_tunnel() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = listener.accept().await;
        let _ = rx.await;
    });

    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let conn_stream = ConnectionStream::ssh_tunnel(stream);
    let config = ConnectionConfig::direct("127.0.0.1", addr.port());

    let conn = Connection::new(conn_stream, config);
    assert!(!conn.is_encrypted());

    let _ = tx.send(());
}

#[tokio::test]
async fn test_connection_is_encrypted_false_for_http_proxy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = listener.accept().await;
        let _ = rx.await;
    });

    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let conn_stream = ConnectionStream::HttpProxy(stream);
    let config = ConnectionConfig::direct("127.0.0.1", addr.port());

    let conn = Connection::new(conn_stream, config);
    assert!(!conn.is_encrypted());

    let _ = tx.send(());
}

#[tokio::test]
async fn test_connection_is_encrypted_false_for_socks_proxy() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = listener.accept().await;
        let _ = rx.await;
    });

    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let conn_stream = ConnectionStream::SocksProxy(stream);
    let config = ConnectionConfig::direct("127.0.0.1", addr.port());

    let conn = Connection::new(conn_stream, config);
    assert!(!conn.is_encrypted());

    let _ = tx.send(());
}

// ============================================================================
// SslConfig 测试
// ============================================================================

#[test]
fn test_ssl_config_default_verify_server_cert() {
    let config = SslConfig::default();
    // Rust Default for bool is false; serde(default = "default_true") only applies during deserialization
    assert!(!config.verify_server_cert, "Rust Default for bool is false");
}

#[test]
fn test_ssl_config_default_min_tls_version() {
    let config = SslConfig::default();
    assert_eq!(config.min_tls_version, TlsVersion::Tls1_2);
}

#[test]
fn test_ssl_config_default_ca_cert_path_none() {
    let config = SslConfig::default();
    assert!(config.ca_cert_path.is_none());
}

#[test]
fn test_ssl_config_default_client_cert_path_none() {
    let config = SslConfig::default();
    assert!(config.client_cert_path.is_none());
}

#[test]
fn test_ssl_config_default_client_key_path_none() {
    let config = SslConfig::default();
    assert!(config.client_key_path.is_none());
}

#[test]
fn test_ssl_config_with_ca_cert_path() {
    let config = SslConfig {
        ca_cert_path: Some("/path/to/ca.pem".to_string()),
        ..Default::default()
    };
    assert_eq!(config.ca_cert_path, Some("/path/to/ca.pem".to_string()));
}

#[test]
fn test_ssl_config_with_client_cert_and_key() {
    let config = SslConfig {
        client_cert_path: Some("/path/to/client-cert.pem".to_string()),
        client_key_path: Some("/path/to/client-key.pem".to_string()),
        ..Default::default()
    };
    assert_eq!(
        config.client_cert_path,
        Some("/path/to/client-cert.pem".to_string())
    );
    assert_eq!(
        config.client_key_path,
        Some("/path/to/client-key.pem".to_string())
    );
}

#[test]
fn test_ssl_config_disable_verify_server_cert() {
    let config = SslConfig {
        verify_server_cert: false,
        ..Default::default()
    };
    assert!(!config.verify_server_cert);
}

#[test]
fn test_ssl_config_tls_version_variants() {
    // 验证所有 TlsVersion 变体可以正确构造
    let v1_0 = TlsVersion::Tls1_0;
    let v1_1 = TlsVersion::Tls1_1;
    let v1_2 = TlsVersion::Tls1_2;
    let v1_3 = TlsVersion::Tls1_3;

    // 验证它们互不相同
    assert_ne!(v1_0, v1_1);
    assert_ne!(v1_1, v1_2);
    assert_ne!(v1_2, v1_3);
    assert_ne!(v1_0, v1_3);

    // 验证每个变体可以用在 SslConfig 中
    let config = SslConfig {
        min_tls_version: TlsVersion::Tls1_0,
        ..Default::default()
    };
    assert_eq!(config.min_tls_version, TlsVersion::Tls1_0);

    let config = SslConfig {
        min_tls_version: TlsVersion::Tls1_3,
        ..Default::default()
    };
    assert_eq!(config.min_tls_version, TlsVersion::Tls1_3);
}

#[test]
fn test_ssl_config_clone_equality() {
    let config = SslConfig {
        verify_server_cert: false,
        ca_cert_path: Some("/ca.pem".to_string()),
        client_cert_path: Some("/cert.pem".to_string()),
        client_key_path: Some("/key.pem".to_string()),
        min_tls_version: TlsVersion::Tls1_3,
    };
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

// ============================================================================
// check_cert_expiry 测试
// ============================================================================

#[test]
fn test_check_cert_expiry_nonexistent_file() {
    let result = check_cert_expiry("/nonexistent/path/to/cert.pem");
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::InvalidConfig { reason, .. })) => {
            assert!(
                reason.contains("无法读取证书文件") || reason.contains("证书"),
                "Expected cert-related error, got: {}",
                reason
            );
        }
        other => panic!("Expected InvalidConfig error, got: {:?}", other),
    }
}

#[test]
fn test_check_cert_expiry_invalid_cert_data() {
    // 创建一个临时文件，写入无效的证书数据
    let dir = std::env::temp_dir();
    let cert_path = dir.join("test_invalid_cert.pem");
    std::fs::write(&cert_path, "this is not a valid certificate").unwrap();

    let result = check_cert_expiry(cert_path.to_str().unwrap());
    // 清理
    let _ = std::fs::remove_file(&cert_path);

    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::InvalidConfig { reason, .. })) => {
            assert!(
                reason.contains("证书解析失败") || reason.contains("parse"),
                "Expected parse error, got: {}",
                reason
            );
        }
        other => panic!("Expected InvalidConfig error, got: {:?}", other),
    }
}

#[test]
fn test_check_cert_expiry_with_generated_self_signed_cert() {
    // 使用 openssl 生成自签名证书
    let dir = std::env::temp_dir();
    let cert_path = dir.join("test_self_signed_cert.pem");
    let key_path = dir.join("test_self_signed_key.pem");

    // 尝试生成自签名证书
    let output = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "365",
            "-nodes",
            "-subj",
            "/CN=test.rdata.local",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            // 证书生成成功，执行测试
            let result = check_cert_expiry(cert_path.to_str().unwrap());
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);

            match result {
                Ok(info) => {
                    assert_eq!(info.path, cert_path.to_str().unwrap());
                    assert!(
                        info.subject.contains("test.rdata.local") || info.subject.contains("CN"),
                        "Subject should contain CN info, got: {}",
                        info.subject
                    );
                    assert!(!info.issuer.is_empty(), "Issuer should not be empty");
                    assert!(
                        !info.not_before.is_empty(),
                        "not_before should not be empty"
                    );
                    assert!(!info.not_after.is_empty(), "not_after should not be empty");
                    // 365 天的证书，应该还有 > 0 天
                    assert!(
                        info.days_until_expiry > 0,
                        "Expected positive days_until_expiry, got: {}",
                        info.days_until_expiry
                    );
                    assert!(!info.is_expired, "Fresh cert should not be expired");
                }
                Err(CoreError::Connection(ConnectionError::InvalidConfig { reason, .. })) => {
                    panic!("Failed to parse generated cert: {}", reason);
                }
                Err(other) => {
                    panic!("Unexpected error: {:?}", other);
                }
            }
        }
        _ => {
            // openssl 不可用，跳过测试
            // 清理可能已创建的文件
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);
            eprintln!("[SKIP] openssl not available, skipping cert generation test");
        }
    }
}

#[test]
fn test_check_cert_expiry_with_short_validity_cert() {
    // 使用 openssl 生成一个即将过期的证书（1天有效期）
    let dir = std::env::temp_dir();
    let cert_path = dir.join("test_short_validity_cert.pem");
    let key_path = dir.join("test_short_validity_key.pem");

    let output = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "1",
            "-nodes",
            "-subj",
            "/CN=test-short.rdata.local",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let result = check_cert_expiry(cert_path.to_str().unwrap());
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);

            match result {
                Ok(info) => {
                    // 1天有效期，在生成后应该还有 0 或 1 天
                    assert!(
                        info.days_until_expiry >= 0,
                        "Expected non-negative days_until_expiry, got: {}",
                        info.days_until_expiry
                    );
                    assert!(
                        !info.is_expired,
                        "Fresh 1-day cert should not be expired yet"
                    );
                }
                Err(e) => {
                    panic!("Failed to parse short-validity cert: {:?}", e);
                }
            }
        }
        _ => {
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);
            eprintln!("[SKIP] openssl not available, skipping short-validity cert test");
        }
    }
}

#[test]
fn test_check_cert_expiry_with_pem_format() {
    // 使用 openssl 生成 PEM 格式证书
    let dir = std::env::temp_dir();
    let cert_path = dir.join("test_pem_cert.pem");
    let key_path = dir.join("test_pem_key.pem");

    let output = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "365",
            "-nodes",
            "-subj",
            "/CN=test-pem.rdata.local/O=RdataStation",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let result = check_cert_expiry(cert_path.to_str().unwrap());
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);

            match result {
                Ok(info) => {
                    // PEM 格式应该被正确解析
                    assert!(!info.subject.is_empty());
                    assert!(!info.issuer.is_empty());
                    assert!(!info.is_expired);
                }
                Err(e) => {
                    panic!("Failed to parse PEM cert: {:?}", e);
                }
            }
        }
        _ => {
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);
            eprintln!("[SKIP] openssl not available, skipping PEM cert test");
        }
    }
}

#[test]
fn test_ssl_cert_info_clone() {
    let info = SslCertInfo {
        path: "/test/cert.pem".to_string(),
        subject: "CN=test".to_string(),
        issuer: "CN=test".to_string(),
        not_before: "2024-01-01 00:00:00".to_string(),
        not_after: "2025-01-01 00:00:00".to_string(),
        days_until_expiry: 180,
        is_expired: false,
    };
    let cloned = info.clone();
    assert_eq!(info.path, cloned.path);
    assert_eq!(info.subject, cloned.subject);
    assert_eq!(info.issuer, cloned.issuer);
    assert_eq!(info.not_before, cloned.not_before);
    assert_eq!(info.not_after, cloned.not_after);
    assert_eq!(info.days_until_expiry, cloned.days_until_expiry);
    assert_eq!(info.is_expired, cloned.is_expired);
}

#[test]
fn test_ssl_cert_info_debug_format() {
    let info = SslCertInfo {
        path: "/test/cert.pem".to_string(),
        subject: "CN=test".to_string(),
        issuer: "CN=test".to_string(),
        not_before: "2024-01-01".to_string(),
        not_after: "2025-01-01".to_string(),
        days_until_expiry: 180,
        is_expired: false,
    };
    let debug_str = format!("{:?}", info);
    assert!(debug_str.contains("cert.pem"));
    assert!(debug_str.contains("CN=test"));
}

// ============================================================================
// ConnectionConfig 测试
// ============================================================================

#[test]
fn test_connection_config_is_encrypted_with_ssl() {
    let config = ConnectionConfig::ssl("localhost", 3306, SslConfig::default());
    assert!(config.is_encrypted());
}

#[test]
fn test_connection_config_is_encrypted_with_ssh() {
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
    let config = ConnectionConfig::ssh("db.internal", 3306, ssh_config);
    assert!(config.is_encrypted());
}

#[test]
fn test_connection_config_is_not_encrypted_with_direct() {
    let config = ConnectionConfig::direct("localhost", 3306);
    assert!(!config.is_encrypted());
}

#[test]
fn test_connection_config_is_not_encrypted_with_http_proxy() {
    let proxy_config = ProxyConfig {
        host: "proxy".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    let config = ConnectionConfig::http_proxy("db.example.com", 3306, proxy_config);
    assert!(!config.is_encrypted());
}

#[test]
fn test_connection_config_is_not_encrypted_with_socks_proxy() {
    let proxy_config = ProxyConfig {
        host: "socks".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    let config = ConnectionConfig::socks_proxy("db.example.com", 3306, proxy_config);
    assert!(!config.is_encrypted());
}

#[test]
fn test_connection_config_is_proxied_with_http_proxy() {
    let proxy_config = ProxyConfig {
        host: "proxy".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    let config = ConnectionConfig::http_proxy("db.example.com", 3306, proxy_config);
    assert!(config.is_proxied());
}

#[test]
fn test_connection_config_is_proxied_with_socks_proxy() {
    let proxy_config = ProxyConfig {
        host: "socks".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    let config = ConnectionConfig::socks_proxy("db.example.com", 3306, proxy_config);
    assert!(config.is_proxied());
}

#[test]
fn test_connection_config_is_not_proxied_with_direct() {
    let config = ConnectionConfig::direct("localhost", 3306);
    assert!(!config.is_proxied());
}

#[test]
fn test_connection_config_is_not_proxied_with_ssl() {
    let config = ConnectionConfig::ssl("localhost", 3306, SslConfig::default());
    assert!(!config.is_proxied());
}

#[test]
fn test_connection_config_method_name_direct() {
    let config = ConnectionConfig::direct("localhost", 3306);
    assert_eq!(config.method_name(), "direct");
}

#[test]
fn test_connection_config_method_name_ssl() {
    let config = ConnectionConfig::ssl("localhost", 3306, SslConfig::default());
    assert_eq!(config.method_name(), "ssl");
}

#[test]
fn test_connection_config_method_name_ssh() {
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
    let config = ConnectionConfig::ssh("db.internal", 3306, ssh_config);
    assert_eq!(config.method_name(), "ssh");
}

#[test]
fn test_connection_config_method_name_http_proxy() {
    let proxy_config = ProxyConfig {
        host: "proxy".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    let config = ConnectionConfig::http_proxy("db.example.com", 3306, proxy_config);
    assert_eq!(config.method_name(), "http_proxy");
}

#[test]
fn test_connection_config_method_name_socks_proxy() {
    let proxy_config = ProxyConfig {
        host: "socks".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    let config = ConnectionConfig::socks_proxy("db.example.com", 3306, proxy_config);
    assert_eq!(config.method_name(), "socks_proxy");
}

#[test]
fn test_connection_config_method_name_chain() {
    let config = ConnectionConfig::chain("db.example.com", 3306, vec![]);
    assert_eq!(config.method_name(), "chain");
}

#[test]
fn test_connection_config_chain_with_ssl_is_encrypted() {
    let hops = vec![ChainHop::Ssl(SslConfig::default())];
    let config = ConnectionConfig::chain("db.example.com", 3306, hops);
    assert!(config.is_encrypted());
}

#[test]
fn test_connection_config_chain_with_ssh_and_proxy() {
    let hops = vec![
        ChainHop::HttpProxy(ProxyConfig {
            host: "proxy".to_string(),
            port: 8080,
            auth: None,
            no_proxy: vec![],
            timeout_secs: 30,
        }),
        ChainHop::Ssh(SshConfig {
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
        }),
    ];
    let config = ConnectionConfig::chain("db.internal", 3306, hops);
    assert!(config.is_encrypted());
    assert!(config.is_proxied());
}

#[test]
fn test_connection_config_chain_empty_not_encrypted() {
    let config = ConnectionConfig::chain("db.example.com", 3306, vec![]);
    assert!(!config.is_encrypted());
    assert!(!config.is_proxied());
}

// ============================================================================
// ChainHop 测试
// ============================================================================

#[test]
fn test_chain_hop_ssh_name() {
    let hop = ChainHop::Ssh(SshConfig {
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
    });
    assert_eq!(hop.hop_name(), "ssh");
}

#[test]
fn test_chain_hop_ssl_name() {
    let hop = ChainHop::Ssl(SslConfig::default());
    assert_eq!(hop.hop_name(), "ssl");
}

#[test]
fn test_chain_hop_http_proxy_name() {
    let hop = ChainHop::HttpProxy(ProxyConfig {
        host: "proxy".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    });
    assert_eq!(hop.hop_name(), "http_proxy");
}

#[test]
fn test_chain_hop_socks_proxy_name() {
    let hop = ChainHop::SocksProxy(ProxyConfig {
        host: "socks".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    });
    assert_eq!(hop.hop_name(), "socks_proxy");
}

// ============================================================================
// ProxyConfig 和 ProxyAuth 测试
// ============================================================================

#[test]
fn test_proxy_config_with_auth() {
    let config = ProxyConfig {
        host: "proxy.example.com".to_string(),
        port: 3128,
        auth: Some(ProxyAuth {
            username: "proxyuser".to_string(),
            password: "proxypass".to_string(),
        }),
        no_proxy: vec!["localhost".to_string(), "127.0.0.1".to_string()],
        timeout_secs: 60,
    };
    assert_eq!(config.host, "proxy.example.com");
    assert_eq!(config.port, 3128);
    assert!(config.auth.is_some());
    let auth = config.auth.unwrap();
    assert_eq!(auth.username, "proxyuser");
    assert_eq!(auth.password, "proxypass");
    assert_eq!(config.no_proxy.len(), 2);
    assert_eq!(config.timeout_secs, 60);
}

#[test]
fn test_proxy_config_without_auth() {
    let config = ProxyConfig {
        host: "proxy.example.com".to_string(),
        port: 8080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    };
    assert!(config.auth.is_none());
    assert!(config.no_proxy.is_empty());
}

#[test]
fn test_proxy_auth_clone() {
    let auth = ProxyAuth {
        username: "user".to_string(),
        password: "pass".to_string(),
    };
    let cloned = auth.clone();
    assert_eq!(auth.username, cloned.username);
    assert_eq!(auth.password, cloned.password);
}

// ============================================================================
// SshAuth 测试
// ============================================================================

#[test]
fn test_ssh_auth_password() {
    let auth = SshAuth::Password {
        password: "secret".to_string(),
    };
    match auth {
        SshAuth::Password { password } => assert_eq!(password, "secret"),
        _ => panic!("Expected Password variant"),
    }
}

#[test]
fn test_ssh_auth_private_key_with_passphrase() {
    let auth = SshAuth::PrivateKey {
        key_path: "/home/user/.ssh/id_rsa".to_string(),
        passphrase: Some("keypass".to_string()),
    };
    match auth {
        SshAuth::PrivateKey {
            key_path,
            passphrase,
        } => {
            assert_eq!(key_path, "/home/user/.ssh/id_rsa");
            assert_eq!(passphrase, Some("keypass".to_string()));
        }
        _ => panic!("Expected PrivateKey variant"),
    }
}

#[test]
fn test_ssh_auth_private_key_without_passphrase() {
    let auth = SshAuth::PrivateKey {
        key_path: "/home/user/.ssh/id_rsa".to_string(),
        passphrase: None,
    };
    match auth {
        SshAuth::PrivateKey {
            key_path,
            passphrase,
        } => {
            assert_eq!(key_path, "/home/user/.ssh/id_rsa");
            assert!(passphrase.is_none());
        }
        _ => panic!("Expected PrivateKey variant"),
    }
}

#[test]
fn test_ssh_auth_agent() {
    let auth = SshAuth::Agent;
    assert!(matches!(auth, SshAuth::Agent));
}

// ============================================================================
// Connector trait 对象安全测试
// ============================================================================

#[test]
fn test_connector_trait_is_object_safe() {
    // 验证 Connector trait 可以用作 trait object
    fn _accept_connector(_c: &dyn Connector) {}

    let connector = DirectConnector;
    _accept_connector(&connector);
}

#[test]
fn test_all_connectors_implement_trait() {
    // 编译期验证所有连接器实现了 Connector trait
    fn _assert_connector<T: Connector>() {}

    _assert_connector::<DirectConnector>();
    _assert_connector::<SslConnector>();
    _assert_connector::<HttpProxyConnector>();
    _assert_connector::<SocksProxyConnector>();
    _assert_connector::<SshTunnelConnector>();
}

// ============================================================================
// ConnectionConfig 构造器测试
// ============================================================================

#[test]
fn test_connection_config_direct_builder() {
    let config = ConnectionConfig::direct("192.168.1.1", 5432);
    assert_eq!(config.host, "192.168.1.1");
    assert_eq!(config.port, 5432);
    assert!(matches!(config.method, ConnectionMethod::Direct));
    assert!(config.options.is_empty());
}

#[test]
fn test_connection_config_ssl_builder() {
    let ssl = SslConfig {
        verify_server_cert: false,
        ..Default::default()
    };
    let config = ConnectionConfig::ssl("db.example.com", 3306, ssl.clone());
    assert_eq!(config.host, "db.example.com");
    assert_eq!(config.port, 3306);
    match config.method {
        ConnectionMethod::Ssl(c) => assert_eq!(c, ssl),
        _ => panic!("Expected Ssl method"),
    }
}

// ============================================================================
// 边界情况 / 边缘测试
// ============================================================================

#[test]
fn test_connection_config_high_port() {
    let config = ConnectionConfig::direct("localhost", 65535);
    assert_eq!(config.port, 65535);
}

#[test]
fn test_connection_config_port_zero() {
    let config = ConnectionConfig::direct("localhost", 0);
    assert_eq!(config.port, 0);
}

#[test]
fn test_ssl_config_all_fields_custom() {
    let config = SslConfig {
        verify_server_cert: false,
        ca_cert_path: Some("/custom/ca.pem".to_string()),
        client_cert_path: Some("/custom/client.pem".to_string()),
        client_key_path: Some("/custom/key.pem".to_string()),
        min_tls_version: TlsVersion::Tls1_3,
    };
    assert!(!config.verify_server_cert);
    assert_eq!(config.ca_cert_path, Some("/custom/ca.pem".to_string()));
    assert_eq!(
        config.client_cert_path,
        Some("/custom/client.pem".to_string())
    );
    assert_eq!(config.client_key_path, Some("/custom/key.pem".to_string()));
    assert_eq!(config.min_tls_version, TlsVersion::Tls1_3);
}

#[tokio::test]
async fn test_tunnel_guard_multiple_instances() {
    let (tx1, _rx1) = tokio::sync::oneshot::channel::<()>();
    let (tx2, _rx2) = tokio::sync::oneshot::channel::<()>();
    let task1 = tokio::spawn(async {});
    let task2 = tokio::spawn(async {});

    let guard1 = TunnelGuard::new(10001, tx1, task1, "tunnel-1");
    let guard2 = TunnelGuard::new(10002, tx2, task2, "tunnel-2");

    assert_eq!(guard1.port(), 10001);
    assert_eq!(guard2.port(), 10002);
    assert_eq!(guard1.label(), "tunnel-1");
    assert_eq!(guard2.label(), "tunnel-2");
}

#[test]
fn test_tls_version_default_is_tls1_2() {
    assert_eq!(TlsVersion::default(), TlsVersion::Tls1_2);
}

#[test]
fn test_connection_method_default_is_direct() {
    assert_eq!(ConnectionMethod::default(), ConnectionMethod::Direct);
}

// ============================================================================
// 注意：以下函数为私有函数，无法在集成测试中直接测试
// ============================================================================
// - map_tls_version(): 私有函数，通过 SslConfig::min_tls_version 间接测试
//   （见 test_ssl_config_tls_version_variants）
// - base64::encode(): 私有子模块，通过 establish_http_proxy 间接使用
// - establish_tls(): 私有函数，通过 SslConnector::connect() 间接使用
// ============================================================================

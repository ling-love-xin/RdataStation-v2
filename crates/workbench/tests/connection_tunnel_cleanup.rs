//! 连接失败路径的隧道回收（集成测试，C3 第三批后的生命周期验证）。
//!
//! 场景：协议链（HTTP CONNECT 代理）隧道建立成功，但后续数据库握手失败。
//! 断言：失败返回后隧道守卫已被回收（`tunnel_count == 0`，本地端口与后台任务不残留）。
//!
//! 隔离原则：全部 127.0.0.1 临时端口 + `skip_persistence = true`，不落库、不触外网。

use std::sync::Arc;
use std::time::Duration;

use connection::config::{ConnectionMethod, ProxyConfig};
use engine::connection_manager::{ConnectionManager, ConnectionType};
use rds_workbench::services::connection_service::{ConnectRequest, ConnectionService};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 占位「数据库」服务：仅监听端口（握手由代理 mock 截断，连接不会真正到达）。
async fn spawn_dummy_target() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定占位数据库服务");
    let port = listener.local_addr().expect("读取占位数据库端口").port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            drop(stream);
        }
    });
    port
}

/// 最小 HTTP 代理：应答 CONNECT 200 后立即关闭（模拟上游不可用）。
async fn spawn_http_proxy_that_drops_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定 HTTP 代理");
    let port = listener.local_addr().expect("读取 HTTP 代理端口").port();
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                // 读 CONNECT 请求头（至 CRLFCRLF）
                let mut head = Vec::new();
                let mut chunk = [0u8; 256];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match client.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&chunk[..n]),
                    }
                    if head.len() > 8192 {
                        return;
                    }
                }
                let _ = client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await;
                let _ = client.shutdown().await;
            });
        }
    });
    port
}

#[tokio::test]
async fn failed_database_handshake_releases_tunnel() {
    let target_port = spawn_dummy_target().await;
    let proxy_port = spawn_http_proxy_that_drops_upstream().await;

    let service = ConnectionService::new(Arc::new(ConnectionManager::new()));
    let conn_id = "G_it_tunnel_cleanup".to_string();

    let attempt = service.connect_with_type(ConnectRequest {
        conn_id: Some(conn_id.clone()),
        db_type: "postgres".to_string(),
        url: format!("postgres://user:secret@127.0.0.1:{}/db", target_port),
        name: Some("it-tunnel-cleanup".to_string()),
        connection_type: ConnectionType::Global,
        project_path: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
        skip_persistence: Some(true),
        network_method: Some(ConnectionMethod::HttpProxy(ProxyConfig {
            host: "127.0.0.1".to_string(),
            port: proxy_port,
            auth: None,
            no_proxy: Vec::new(),
            timeout_secs: 5,
        })),
    });

    let result = tokio::time::timeout(Duration::from_secs(20), attempt)
        .await
        .expect("连接尝试超时（代理应答后应立即失败）");

    assert!(result.is_err(), "上游断开时数据库握手应失败");
    assert_eq!(
        service.tunnel_count(&conn_id).await,
        0,
        "连接失败后应回收隧道守卫（本地端口与后台任务不残留）"
    );
}

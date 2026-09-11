//! 隧道数据面集成测试（C3 第三批 `connection::chain` 下沉后的端到端验证）。
//!
//! 与 `chain.rs` 内单测互补：单测只验证 URL 改写与注册表语义；这里用本地回环
//! socket 与最小代理服务端（HTTP CONNECT / SOCKS5），验证「本地端口转发 →
//! 代理协商 → 目标服务」的完整数据往返，以及守卫释放后隧道关闭。
//!
//! 隔离原则：全部使用 127.0.0.1 临时端口与内存数据，不读写用户文件、不触外网。
//!
//! 命名说明：集成测试目标以 lib target 名导入本包（`rds_connection`），
//! 本包未用 `[lib] name` 覆盖，故不使用工作区内 `connection::` 依赖别名。

use std::time::Duration;

use rds_connection::chain::apply_network_method;
use rds_connection::config::{ChainHop, ConnectionMethod, ProxyConfig};
use rds_connection::url_params::parse_host_port_from_url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 单次网络往返的超时上限（防止测试挂死）。
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// 构造回环无认证代理配置。
fn loopback_proxy(port: u16) -> ProxyConfig {
    ProxyConfig {
        host: "127.0.0.1".to_string(),
        port,
        auth: None,
        no_proxy: Vec::new(),
        timeout_secs: 5,
    }
}

/// 双向转发两个已建立的流，直到任一方向结束。
async fn bridge(left: TcpStream, right: TcpStream) {
    let (mut lr, mut lw) = left.into_split();
    let (mut rr, mut rw) = right.into_split();
    let _ = tokio::join!(
        tokio::io::copy(&mut lr, &mut rw),
        tokio::io::copy(&mut rr, &mut lw),
    );
}

/// 本地回环 echo 服务（模拟数据库服务器）：收到什么回什么。
async fn spawn_echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定 echo 服务");
    let port = listener.local_addr().expect("读取 echo 服务端口").port();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 512];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    port
}

/// 最小 SOCKS5 服务端：仅支持无认证 + CONNECT（RFC1928），转发到请求目标。
async fn spawn_socks5_proxy() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定 SOCKS5 代理");
    let port = listener.local_addr().expect("读取 SOCKS5 代理端口").port();
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let _ = socks5_handle(client).await;
            });
        }
    });
    port
}

async fn socks5_handle(mut client: TcpStream) -> std::io::Result<()> {
    // 握手：VER NMETHODS METHODS...
    let mut head = [0u8; 2];
    client.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    client.read_exact(&mut methods).await?;
    // 测试只覆盖无认证（0x00）
    client.write_all(&[0x05, 0x00]).await?;

    // 请求：VER CMD RSV ATYP ADDR PORT
    let mut req = [0u8; 4];
    client.read_exact(&mut req).await?;
    let addr = match req[3] {
        0x01 => {
            let mut octets = [0u8; 4];
            client.read_exact(&mut octets).await?;
            std::net::Ipv4Addr::from(octets).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            client.read_exact(&mut len).await?;
            let mut domain = vec![0u8; len[0] as usize];
            client.read_exact(&mut domain).await?;
            String::from_utf8_lossy(&domain).into_owned()
        }
        0x04 => {
            let mut octets = [0u8; 16];
            client.read_exact(&mut octets).await?;
            std::net::Ipv6Addr::from(octets).to_string()
        }
        _ => return Ok(()),
    };
    let mut port_bytes = [0u8; 2];
    client.read_exact(&mut port_bytes).await?;
    let port = u16::from_be_bytes(port_bytes);

    let upstream = match TcpStream::connect((addr.as_str(), port)).await {
        Ok(stream) => stream,
        Err(_) => {
            // REP=0x05（connection refused）
            client
                .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await?;
            return Ok(());
        }
    };
    client
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    bridge(client, upstream).await;
    Ok(())
}

/// 最小 HTTP 代理服务端：仅支持 CONNECT 方法，转发到请求目标。
async fn spawn_http_connect_proxy() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定 HTTP 代理");
    let port = listener.local_addr().expect("读取 HTTP 代理端口").port();
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let _ = http_connect_handle(client).await;
            });
        }
    });
    port
}

async fn http_connect_handle(mut client: TcpStream) -> std::io::Result<()> {
    // 读取完整请求头（至 CRLFCRLF），解析 CONNECT 目标 host:port
    let mut head = Vec::new();
    let mut chunk = [0u8; 512];
    loop {
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&chunk[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() > 16 * 1024 {
            break;
        }
    }
    let head_text = String::from_utf8_lossy(&head).into_owned();
    let target = head_text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string();

    let upstream = TcpStream::connect(target.as_str()).await?;
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    bridge(client, upstream).await;
    Ok(())
}

/// 经隧道本地端口发送数据并读回等长回显（带超时）。
async fn roundtrip(local_port: u16, payload: &[u8]) -> Vec<u8> {
    let exchange = async {
        let mut stream = TcpStream::connect(("127.0.0.1", local_port))
            .await
            .expect("连接隧道本地端口");
        stream.write_all(payload).await.expect("写入隧道");
        let mut echoed = vec![0u8; payload.len()];
        stream.read_exact(&mut echoed).await.expect("读回隧道数据");
        echoed
    };
    tokio::time::timeout(IO_TIMEOUT, exchange)
        .await
        .expect("隧道数据往返超时")
}

/// 轮询等待本地端口关闭（守卫释放 → accept 循环退出 → listener drop）。
async fn assert_port_closed(port: u16) {
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("守卫释放后本地端口 {} 仍可连接", port);
}

#[tokio::test]
async fn socks_proxy_tunnel_relays_payload_and_closes_on_drop() {
    let echo_port = spawn_echo_server().await;
    let proxy_port = spawn_socks5_proxy().await;

    let url = format!("postgres://user:secret@127.0.0.1:{}/db", echo_port);
    let (effective_url, guards) = apply_network_method(
        &url,
        &Some(ConnectionMethod::SocksProxy(loopback_proxy(proxy_port))),
        "G_it_socks",
        "postgres",
    )
    .await
    .expect("建立 SOCKS5 隧道");

    assert_eq!(guards.len(), 1, "代理跳应产生 1 个隧道守卫");
    let (host, local_port) = parse_host_port_from_url(&effective_url).expect("解析改写后 URL");
    assert_eq!(host, "127.0.0.1", "URL 应改写为本地回环");
    assert_ne!(local_port, echo_port, "本地转发端口不应等于目标端口");

    let payload = b"rds-socks-roundtrip";
    assert_eq!(roundtrip(local_port, payload).await, payload, "数据应经代理隧道往返");
    // 凭据与库名应在改写后保留
    assert!(effective_url.contains("user:secret@"), "改写后 URL 应保留凭据");

    drop(guards);
    assert_port_closed(local_port).await;
}

#[tokio::test]
async fn http_connect_proxy_tunnel_relays_payload() {
    let echo_port = spawn_echo_server().await;
    let proxy_port = spawn_http_connect_proxy().await;

    let url = format!("mysql://root@127.0.0.1:{}/app", echo_port);
    let (effective_url, guards) = apply_network_method(
        &url,
        &Some(ConnectionMethod::HttpProxy(loopback_proxy(proxy_port))),
        "G_it_http",
        "mysql",
    )
    .await
    .expect("建立 HTTP CONNECT 隧道");

    assert_eq!(guards.len(), 1, "代理跳应产生 1 个隧道守卫");
    let (_, local_port) = parse_host_port_from_url(&effective_url).expect("解析改写后 URL");

    let payload = b"rds-http-roundtrip";
    assert_eq!(roundtrip(local_port, payload).await, payload, "数据应经 HTTP 代理隧道往返");
}

#[tokio::test]
async fn two_hop_proxy_chain_relays_payload() {
    let echo_port = spawn_echo_server().await;
    let proxy1_port = spawn_socks5_proxy().await;
    let proxy2_port = spawn_socks5_proxy().await;

    let url = format!("mysql://127.0.0.1:{}/db", echo_port);
    let hops = vec![
        ChainHop::SocksProxy(loopback_proxy(proxy1_port)),
        ChainHop::SocksProxy(loopback_proxy(proxy2_port)),
    ];
    let (effective_url, guards) = apply_network_method(
        &url,
        &Some(ConnectionMethod::Chain(hops)),
        "G_it_chain",
        "mysql",
    )
    .await
    .expect("建立两跳代理链");

    assert_eq!(guards.len(), 2, "两跳代理应各产生一个隧道守卫");
    let (_, local_port) = parse_host_port_from_url(&effective_url).expect("解析改写后 URL");

    let payload = b"rds-chain-roundtrip";
    assert_eq!(roundtrip(local_port, payload).await, payload, "数据应经两跳代理链往返");
}

#[tokio::test]
async fn no_proxy_rule_skips_proxy_tunnel() {
    let echo_port = spawn_echo_server().await;
    let proxy_port = spawn_socks5_proxy().await;

    let url = format!("postgres://127.0.0.1:{}/db", echo_port);
    let mut proxy = loopback_proxy(proxy_port);
    proxy.no_proxy = vec!["127.0.0.1".to_string()];

    let (effective_url, guards) = apply_network_method(
        &url,
        &Some(ConnectionMethod::SocksProxy(proxy)),
        "G_it_no_proxy",
        "postgres",
    )
    .await
    .expect("no_proxy 命中应直接返回");

    assert!(guards.is_empty(), "no_proxy 命中不应建立隧道");
    assert_eq!(effective_url, url, "URL 应保持原样");
}

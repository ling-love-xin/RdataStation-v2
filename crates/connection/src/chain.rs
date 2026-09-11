//! 协议链执行与隧道生命周期（C3 第三批收敛：自 workbench `connection_service` 下沉）。
//!
//! 遍历协议链（SSH / HTTP 代理 / SOCKS 代理 / SSL 跳），建立本地端口转发、
//! 改写连接 URL 并注入 SSL 参数；隧道守卫由 [`TunnelRegistry`] 按连接 ID 持有，
//! 断开连接时统一释放（drop 守卫即关闭隧道与后台任务）。
//!
//! 依赖仅 `connection` 自身（config / connector / url_params）与 `shared`；
//! 网络配置读取（network_store 等，依赖 engine）留在调用方（workbench）。

use std::collections::HashMap;
use std::sync::Arc;

use shared::error::{ConnectionError, CoreError};

use crate::config::{ChainHop, ConnectionConfig, ConnectionMethod, ProxyConfig, SslConfig, SshConfig};
use crate::connector::{self, TunnelGuard};
use crate::url_params;

/// 隧道注册表：按连接 ID 持有隧道守卫（克隆共享同一张表）。
///
/// 生命周期约定：`connect` 成功后 `insert`；`close_connection` 时 `take` 并 drop；
/// `close_all` 时 `clear`。守卫 drop 会发送关闭信号并取消后台 accept 循环。
#[derive(Clone, Default)]
pub struct TunnelRegistry {
    tunnels: Arc<tokio::sync::Mutex<HashMap<String, Vec<TunnelGuard>>>>,
}

impl TunnelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一条连接的隧道守卫（覆盖旧值并释放旧隧道）。
    pub async fn insert(&self, conn_id: &str, guards: Vec<TunnelGuard>) {
        let mut map = self.tunnels.lock().await;
        if let Some(old) = map.insert(conn_id.to_string(), guards) {
            tracing::warn!(conn_id, count = old.len(), "替换既有隧道守卫（旧隧道将关闭）");
            drop(old);
        }
    }

    /// 取出该连接的隧道守卫（调用方 drop 即关闭隧道）。
    pub async fn take(&self, conn_id: &str) -> Vec<TunnelGuard> {
        self.tunnels
            .lock()
            .await
            .remove(conn_id)
            .unwrap_or_default()
    }

    /// 该连接已登记的隧道数量。
    pub async fn count(&self, conn_id: &str) -> usize {
        self.tunnels
            .lock()
            .await
            .get(conn_id)
            .map(Vec::len)
            .unwrap_or(0)
    }

    /// 清空全部隧道（关闭所有隧道与后台任务）。
    pub async fn clear(&self) {
        self.tunnels.lock().await.clear();
    }
}

/// 应用单跳网络方式，返回（改写后的 URL, 隧道守卫）。
///
/// - `None` / `Direct`：原样返回；
/// - `Chain`：交由 [`process_chain`] 迭代处理；
/// - `Ssh`：建立本地端口转发并改写 URL；
/// - `Ssl`：仅注入 SSL 参数（由 sqlx 原生处理）；
/// - `HttpProxy` / `SocksProxy`：无代理隧道 + URL 改写；命中 `no_proxy` 时跳过。
pub async fn apply_network_method(
    url: &str,
    method: &Option<ConnectionMethod>,
    conn_id: &str,
    db_type: &str,
) -> Result<(String, Vec<TunnelGuard>), CoreError> {
    match method {
        None | Some(ConnectionMethod::Direct) => Ok((url.to_string(), vec![])),
        Some(ConnectionMethod::Chain(hops)) => process_chain(url, hops, conn_id, db_type).await,
        Some(ConnectionMethod::Ssh(ssh_config)) => {
            let guard = create_ssh_tunnel_port(ssh_config, None).await?;
            let local_port = guard.port();
            let rewritten = url_params::rewrite_url_host_port(url, "127.0.0.1", local_port)?;
            tracing::info!(
                conn_id = %conn_id,
                original = %crate::url::mask_password_in_url(url),
                tunnel = %rewritten,
                "SSH 隧道已建立，URL 已改写为本地端口"
            );
            Ok((rewritten, vec![guard]))
        }
        Some(ConnectionMethod::Ssl(ssl_config)) => {
            // SSL 参数由 sqlx 原生支持，通过 URL query 参数传递
            // 根据数据库类型自动映射 ssl_mode/sslmode 与证书路径
            let url_with_ssl = url_params::append_ssl_params(url, db_type, ssl_config)?;
            Ok((url_with_ssl, vec![]))
        }
        Some(ConnectionMethod::HttpProxy(_) | ConnectionMethod::SocksProxy(_)) => {
            let (target_host, target_port) = url_params::parse_host_port_from_url(url)?;
            let proxy_config = match method {
                Some(ConnectionMethod::HttpProxy(c)) => c,
                Some(ConnectionMethod::SocksProxy(c)) => c,
                _ => unreachable!(),
            };
            let is_socks = matches!(method, Some(ConnectionMethod::SocksProxy(_)));

            if url_params::matches_no_proxy(&target_host, &proxy_config.no_proxy) {
                tracing::info!(
                    conn_id = %conn_id,
                    host = %target_host,
                    rules = ?proxy_config.no_proxy,
                    "目标主机匹配 no_proxy 规则，跳过代理"
                );
                return Ok((url.to_string(), vec![]));
            }

            let guard = create_proxy_tunnel_port(
                proxy_config,
                &target_host,
                target_port,
                is_socks,
                None,
                None,
            )
            .await?;
            let local_port = guard.port();

            let rewritten = url_params::rewrite_url_host_port(url, "127.0.0.1", local_port)?;
            tracing::info!(
                conn_id = %conn_id,
                original = %crate::url::mask_password_in_url(url),
                proxy = %rewritten,
                "代理隧道已建立，URL 已改写为本地端口"
            );
            Ok((rewritten, vec![guard]))
        }
    }
}

/// 处理协议链路（外层 → 内层迭代）。
///
/// 每跳建立本地端口转发，将目标地址作为下一跳的连接入口：
/// - Proxy 跳的目标 = 下一跳的 host:port
/// - SSH 跳的 connect_to = 上一跳的 localhost 端口
/// - SSL 跳由 sqlx 原生处理
pub async fn process_chain(
    url: &str,
    hops: &[ChainHop],
    conn_id: &str,
    db_type: &str,
) -> Result<(String, Vec<TunnelGuard>), CoreError> {
    let (final_db_host, final_db_port) = url_params::parse_host_port_from_url(url)?;
    let mut tunnel_port: Option<u16> = None;
    let mut guards: Vec<TunnelGuard> = Vec::new();

    for (i, hop) in hops.iter().enumerate() {
        let next_hop = hops.get(i + 1);

        match hop {
            ChainHop::Ssh(ssh_config) => {
                let connect_override = tunnel_port.map(|p| ("127.0.0.1".to_string(), p));
                let guard = create_ssh_tunnel_port(ssh_config, connect_override).await?;
                let lp = guard.port();
                tunnel_port = Some(lp);
                tracing::info!(
                    conn_id = %conn_id,
                    hop = i,
                    port = lp,
                    "SSH 隧道跳已建立"
                );
                guards.push(guard);
            }
            ChainHop::HttpProxy(proxy) | ChainHop::SocksProxy(proxy) => {
                let (target_host, target_port) = if let Some(next) = next_hop {
                    match next {
                        ChainHop::Ssh(s) => (s.host.clone(), s.port),
                        ChainHop::HttpProxy(p) | ChainHop::SocksProxy(p) => (p.host.clone(), p.port),
                        ChainHop::Ssl(_) => (final_db_host.clone(), final_db_port),
                    }
                } else {
                    (final_db_host.clone(), final_db_port)
                };
                let is_socks = matches!(hop, ChainHop::SocksProxy(_));
                let connect_override = tunnel_port.map(|p| ("127.0.0.1".to_string(), p));

                if url_params::matches_no_proxy(&target_host, &proxy.no_proxy) {
                    tracing::info!(
                        conn_id = %conn_id,
                        hop = i,
                        host = %target_host,
                        rules = ?proxy.no_proxy,
                        "链路中目标主机匹配 no_proxy 规则，跳过此代理跳"
                    );
                    continue;
                }

                let wrap_ssl = match next_hop {
                    Some(ChainHop::Ssl(ssl_cfg)) => Some(ssl_cfg.clone()),
                    _ => None,
                };
                let guard = create_proxy_tunnel_port(
                    proxy,
                    &target_host,
                    target_port,
                    is_socks,
                    connect_override,
                    wrap_ssl,
                )
                .await?;
                let lp = guard.port();
                tunnel_port = Some(lp);
                guards.push(guard);
                tracing::info!(
                    conn_id = %conn_id,
                    hop = i,
                    port = lp,
                    target = %format!("{}:{}", target_host, target_port),
                    "代理跳已建立"
                );
            }
            ChainHop::Ssl(_) => {
                tracing::info!(
                    conn_id = %conn_id,
                    hop = i,
                    "SSL 跳（由 sqlx 原生处理）"
                );
            }
        }
    }

    match tunnel_port {
        Some(port) => {
            let rewritten = url_params::rewrite_url_host_port(url, "127.0.0.1", port)?;
            tracing::info!(
                conn_id = %conn_id,
                original = %crate::url::mask_password_in_url(url),
                tunnel = %rewritten,
                hops = hops.len(),
                "协议链已建立"
            );
            let url_with_ssl = url_params::inject_chain_ssl_params(&rewritten, hops, db_type)?;
            Ok((url_with_ssl, guards))
        }
        None => {
            let url_with_ssl = url_params::inject_chain_ssl_params(url, hops, db_type)?;
            Ok((url_with_ssl, guards))
        }
    }
}

/// 创建 SSH 隧道端口转发，返回隧道生命周期守卫。
///
/// `connect_override`：链式跳转时覆盖 SSH 连接目标（经由上一跳的 localhost 端口间接连接）。
async fn create_ssh_tunnel_port(
    ssh_config: &SshConfig,
    connect_override: Option<(String, u16)>,
) -> Result<TunnelGuard, CoreError> {
    let effective_config = if let Some((ref host, port)) = connect_override {
        let mut modified = ssh_config.clone();
        modified.host = host.clone();
        modified.port = port;
        modified
    } else {
        ssh_config.clone()
    };

    let dummy_config = ConnectionConfig::direct("127.0.0.1", 0);

    connector::establish_ssh_tunnel(&dummy_config, &effective_config).await
}

/// 创建代理隧道端口转发，返回隧道生命周期守卫。
///
/// 建立本地端口转发：绑定本地端口 → accept 循环 → 每个连接的桥接通过代理到目标。
///
/// `connect_override`：链式跳转时覆盖代理连接目标（经由上一跳的 localhost 端口间接连接代理服务器）；
/// `wrap_ssl`：代理 CONNECT 成功后对连接进行 TLS 封装（Proxy → SSL 嵌套）。
async fn create_proxy_tunnel_port(
    proxy_config: &ProxyConfig,
    target_host: &str,
    target_port: u16,
    is_socks: bool,
    connect_override: Option<(String, u16)>,
    wrap_ssl: Option<SslConfig>,
) -> Result<TunnelGuard, CoreError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: format!("{}:{}", target_host, target_port),
                reason: format!("绑定代理转发本地端口失败: {}", e),
            })
        })?;

    let local_port = listener
        .local_addr()
        .map_err(|e| {
            CoreError::connection(ConnectionError::Network {
                conn_id: format!("{}:{}", target_host, target_port),
                reason: format!("获取代理转发本地端口失败: {}", e),
            })
        })?
        .port();

    let pc = proxy_config.clone();
    let effective_pc = if let Some((ref host, port)) = connect_override {
        let mut modified = pc.clone();
        modified.host = host.clone();
        modified.port = port;
        modified
    } else {
        pc.clone()
    };
    let th = target_host.to_string();
    let ssl_config = wrap_ssl.clone();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let task = tokio::spawn(async move {
        tracing::info!(
            target: "proxy_tunnel",
            target = %format!("{}:{}", th, target_port),
            local_port,
            is_socks,
            has_tls = ssl_config.is_some(),
            "代理隧道后台任务启动 (accept 循环)"
        );
        let th_outer = th.clone();
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((local_stream, _)) => {
                            let th2 = th_outer.clone();
                            let dummy = ConnectionConfig::direct(&th2, target_port);
                            let epc = effective_pc.clone();
                            let ssl = ssl_config.clone();
                            tokio::spawn(async move {
                                let tunneled = if is_socks {
                                    connector::establish_socks_proxy(&dummy, &epc).await
                                } else {
                                    connector::establish_http_proxy(&dummy, &epc).await
                                };
                                match tunneled {
                                    Ok(proxy_stream) => {
                                        if let Some(ref ssl_cfg) = ssl {
                                            match connector::wrap_tls_stream(
                                                proxy_stream,
                                                ssl_cfg,
                                                &th2,
                                            )
                                            .await
                                            {
                                                Ok(tls_stream) => {
                                                    let (mut lr, mut lw) =
                                                        tokio::io::split(local_stream);
                                                    let (mut pr, mut pw) =
                                                        tokio::io::split(tls_stream);
                                                    let _ = tokio::join!(
                                                        tokio::io::copy(&mut lr, &mut pw),
                                                        tokio::io::copy(&mut pr, &mut lw),
                                                    );
                                                    tracing::debug!(target: "proxy_tunnel", "TLS 加密代理桥接结束");
                                                }
                                                Err(e) => {
                                                    tracing::warn!(target: "proxy_tunnel", host = %th2, "TLS 封装失败: {}", e);
                                                }
                                            }
                                        } else {
                                            let (mut lr, mut lw) =
                                                tokio::io::split(local_stream);
                                            let (mut pr, mut pw) =
                                                tokio::io::split(proxy_stream);
                                            let _ = tokio::join!(
                                                tokio::io::copy(&mut lr, &mut pw),
                                                tokio::io::copy(&mut pr, &mut lw),
                                            );
                                            tracing::debug!(target: "proxy_tunnel", "代理桥接结束");
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(target: "proxy_tunnel", host = %th2, port = %target_port, "代理隧道连接失败: {}", e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(target: "proxy_tunnel", "接受本地代理连接失败: {}", e);
                            break;
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    tracing::info!(target: "proxy_tunnel", local_port, "代理隧道收到关闭信号，退出 accept 循环");
                    break;
                }
            }
        }
        drop(listener);
        drop(effective_pc);
        tracing::info!(target: "proxy_tunnel", local_port, "代理隧道后台任务已退出");
    });

    tracing::info!(
        target: "proxy_tunnel",
        target = %format!("{}:{}", target_host, target_port),
        local_port,
        is_socks,
        "代理隧道已建立"
    );

    Ok(TunnelGuard::new(
        local_port,
        shutdown_tx,
        task,
        format!("proxy:{}", target_host),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TlsVersion;

    fn dummy_guard(label: &str) -> TunnelGuard {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = rx.await;
        });
        TunnelGuard::new(0, tx, task, label.to_string())
    }

    #[tokio::test]
    async fn tunnel_registry_lifecycle() {
        let registry = TunnelRegistry::new();
        registry
            .insert("G_conn_a", vec![dummy_guard("a1"), dummy_guard("a2")])
            .await;
        assert_eq!(registry.count("G_conn_a").await, 2);

        // 覆盖：旧守卫被替换（drop 后关闭），数量取新值
        registry.insert("G_conn_a", vec![dummy_guard("a3")]).await;
        assert_eq!(registry.count("G_conn_a").await, 1);

        let taken = registry.take("G_conn_a").await;
        assert_eq!(taken.len(), 1, "take 应取出全部守卫");
        assert_eq!(registry.count("G_conn_a").await, 0);

        registry.insert("G_conn_b", vec![dummy_guard("b1")]).await;
        registry.clear().await;
        assert_eq!(registry.count("G_conn_b").await, 0);
    }

    #[tokio::test]
    async fn apply_direct_keeps_url() {
        let (url, guards) = apply_network_method(
            "postgres://u:p@h:5432/db",
            &None,
            "G_conn_a",
            "postgres",
        )
        .await
        .expect("direct");
        assert_eq!(url, "postgres://u:p@h:5432/db");
        assert!(guards.is_empty());

        let (url, _) = apply_network_method(
            "postgres://h/db",
            &Some(ConnectionMethod::Direct),
            "G_conn_a",
            "postgres",
        )
        .await
        .expect("direct");
        assert_eq!(url, "postgres://h/db");
    }

    #[tokio::test]
    async fn apply_ssl_appends_params_without_tunnel() {
        let method = ConnectionMethod::Ssl(SslConfig {
            verify_server_cert: true,
            ca_cert_path: Some("/ca.pem".to_string()),
            client_cert_path: None,
            client_key_path: None,
            min_tls_version: TlsVersion::Tls1_2,
        });
        let (url, guards) = apply_network_method(
            "postgres://h:5432/db",
            &Some(method),
            "G_conn_a",
            "postgres",
        )
        .await
        .expect("ssl");
        assert_eq!(url, "postgres://h:5432/db?sslmode=verify-ca&sslrootcert=/ca.pem");
        assert!(guards.is_empty(), "SSL 跳不产生隧道守卫");
    }

    #[tokio::test]
    async fn process_chain_with_only_ssl_has_no_tunnel() {
        let hops = vec![ChainHop::Ssl(SslConfig {
            verify_server_cert: false,
            ..Default::default()
        })];
        let (url, guards) = process_chain("mysql://h:3306/db", &hops, "G_conn_a", "mysql")
            .await
            .expect("chain");
        assert_eq!(url, "mysql://h:3306/db?ssl-mode=REQUIRED");
        assert!(guards.is_empty());
    }
}

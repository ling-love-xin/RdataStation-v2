//! SSH 隧道连接器：`SshTunnelConnector` + russh 客户端（establish_ssh_tunnel）。

use std::sync::Arc;

use async_trait::async_trait;
// SSH Agent 身份（仅 unix 用得上：Windows 侧 Agent 走 `#[cfg(not(unix))]` 那条不支持分支）
#[cfg(unix)]
use russh::keys::agent::AgentIdentity;
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyBase64};

use crate::config::{ConnectionConfig, ConnectionMethod, SshAuth, SshConfig};
use crate::connector::TunnelGuard;
use crate::stream::ConnectionStream;
use crate::connector::Connector;
use shared::error::{ConnectionError, CoreError};

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
/// 处理 SSH 连接期间的服务端事件，集成 known_hosts 校验（决策 #37 TOFU）
struct SshClientHandler {
    host: String,
    port: u16,
    known_hosts: crate::known_hosts::KnownHosts,
    /// Host Key 校验策略（TOFU 默认 / Strict 严格 / AllowUnknown 内网退路）
    host_key_check: crate::known_hosts::HostKeyCheck,
}

// russh 0.63 起 Handler 改用原生 async trait（不再需要 #[async_trait]），
// 且入参由 PublicKey 改为 PublicKeyOrCertificate（支持证书形式的服务器密钥）
impl russh::client::Handler for SshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // 证书形式统一取其公钥，后续按裸公钥校验，逻辑保持一致
        let public_key = server_public_key.public_key();
        let fingerprint = public_key.fingerprint(russh::keys::HashAlg::Sha256);
        let key_b64 = public_key.public_key_base64();
        let key_type = key_b64.split_whitespace().next().unwrap_or("unknown");

        match self
            .known_hosts
            .verify_outcome(&self.host, self.port, &public_key)
        {
            crate::known_hosts::HostKeyOutcome::Matched => {
                tracing::info!(
                    target: "ssh_tunnel",
                    fingerprint = %fingerprint,
                    key_type = %key_type,
                    host = %self.host,
                    "Host Key 校验通过（known_hosts 匹配）"
                );
                Ok(true)
            }
            crate::known_hosts::HostKeyOutcome::Mismatched => {
                tracing::error!(
                    target: "ssh_tunnel",
                    fingerprint = %fingerprint,
                    key_type = %key_type,
                    host = %self.host,
                    "Host Key 不匹配 known_hosts 记录，可能存在中间人攻击（MITM），连接拒绝"
                );
                Ok(false)
            }
            crate::known_hosts::HostKeyOutcome::Unknown => {
                if !self.host_key_check.allows_unknown() {
                    tracing::warn!(
                        target: "ssh_tunnel",
                        fingerprint = %fingerprint,
                        key_type = %key_type,
                        host = %self.host,
                        policy = "strict",
                        "Host 不在 known_hosts 中，严格模式拒绝连接（可用 AllowUnknown 作为内网退路）"
                    );
                    return Ok(false);
                }

                // TOFU：首连自动信任并写入 known_hosts（落盘后二次连接同指纹匹配、异指纹拒绝）
                if self.host_key_check.should_append_unknown() {
                    if let Err(e) = self
                        .known_hosts
                        .append(&self.host, self.port, &public_key)
                    {
                        // 落盘失败不阻断连接（下次仍会提示/校验），仅记录
                        tracing::warn!(
                            target: "ssh_tunnel",
                            host = %self.host,
                            error = %e,
                            "Host Key 写入 known_hosts 失败（TOFU 落盘跳过）"
                        );
                    }
                    tracing::info!(
                        target: "ssh_tunnel",
                        fingerprint = %fingerprint,
                        key_type = %key_type,
                        host = %self.host,
                        policy = "tofu",
                        "首连信任：Host Key 已写入 known_hosts"
                    );
                } else {
                    tracing::info!(
                        target: "ssh_tunnel",
                        fingerprint = %fingerprint,
                        key_type = %key_type,
                        host = %self.host,
                        policy = "allow_unknown",
                        "Host 不在 known_hosts 中，AllowUnknown 策略放行（不落盘）"
                    );
                }
                Ok(true)
            }
        }
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
    let known_hosts = crate::known_hosts::create_known_hosts_checker(true);
    let handler = SshClientHandler {
        host: ssh_config.host.clone(),
        port: ssh_config.port,
        known_hosts,
        // 决策 #37：Host Key 策略由连接配置注入（TOFU 默认 / Strict / AllowUnknown）
        host_key_check: ssh_config.host_key_check,
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
            // russh 0.63 起 PrivateKeyWithHashAlg::new 不再返回 Result
            // （哈希算法不可用时由内部回退处理）
            let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key), None);
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
            for identity in &identities {
                // russh 0.63 的签名是 `(user, key|cert, hash_alg, signer) -> AuthResult`
                // （旧版是 `(user, identity, signer) -> bool`，本块是 unix-only，升级时漏改了）。
                // 公钥与 OpenSSH 证书是**两条 API**，按身份类型分派；`hash_alg = None` 表示
                // 算法按密钥类型内部选，与上面私钥那条（`PrivateKeyWithHashAlg::new(_, None)`）同口径。
                let attempt = match identity {
                    AgentIdentity::PublicKey { key, .. } => {
                        session
                            .lock()
                            .await
                            .authenticate_publickey_with(
                                &ssh_config.username,
                                key.clone(),
                                None,
                                &mut agent,
                            )
                            .await
                    }
                    AgentIdentity::Certificate { certificate, .. } => {
                        session
                            .lock()
                            .await
                            .authenticate_certificate_with(
                                &ssh_config.username,
                                certificate.clone(),
                                None,
                                &mut agent,
                            )
                            .await
                    }
                };
                match attempt {
                    // 只有 `Success` 算认证通过：`Failure { partial_success: true }` 表示
                    // 密钥被接受但服务器还要第二因子——对只建一条隧道的我们等同于没通过。
                    Ok(result) if result.success() => {
                        authenticated = true;
                        tracing::info!(
                            target: "ssh_tunnel",
                            username = %ssh_config.username,
                            "SSH Agent 认证成功"
                        );
                        break;
                    }
                    Ok(result) => {
                        tracing::debug!(
                            target: "ssh_tunnel",
                            "SSH Agent 身份未通过认证（{:?}），尝试下一个",
                            result
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
// 注意：本函数为 #[cfg(unix)]，在 Windows 上不参与编译（改动后需在 Linux/macOS 侧再验证）
#[cfg(unix)]
async fn connect_ssh_agent(
    ssh_addr: &str,
) -> Result<
    russh::keys::agent::client::AgentClient<
        Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>,
    >,
    CoreError,
> {
    russh::keys::agent::client::AgentClient::connect_env()
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

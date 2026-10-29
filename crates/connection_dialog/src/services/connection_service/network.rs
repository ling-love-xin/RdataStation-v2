//! 网络方式解析：网络档案（network_config_id）→ `ConnectionMethod` 协议链。
//!
//! 从 `connection_service.rs` 拆出（P1 结构拆分）：只负责「档案 → 连接方式」的
//! 解析与认证注入；连接生命周期（建连/关闭/探测/指纹）在 `runtime.rs`。
//! 依赖只向下：engine / connection / shared，不引用特性 crate。

use connection::config::ConnectionMethod;
use shared::error::CoreError;

pub async fn resolve_network_method_with_project(
    network_config_id: Option<&str>,
    project_path: Option<&str>,
) -> Result<Option<ConnectionMethod>, CoreError> {
    use std::path::Path;

    let Some(net_id) = network_config_id else {
        return Ok(None);
    };
    // 空串同 None：旧数据可能存空串而非 NULL（与 auth_config_id 同源问题）。
    if net_id.trim().is_empty() {
        return Ok(None);
    }

    // ===== GP_ 前缀：项目级全局快照配置 =====
    if net_id.starts_with("GP_") {
        if let Some(pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        project_path,
                    )
                    .await;
                }
            }
        }
        tracing::warn!(net_id = %net_id, "GP_ 前缀网络配置未找到（project_path={:?}）", project_path);
        return Ok(None);
    }

    // ===== P_ 前缀：项目级配置 =====
    if net_id.starts_with("P_") {
        if let Some(pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        project_path,
                    )
                    .await;
                }
            }
        }
        tracing::warn!(net_id = %net_id, "P_ 前缀网络配置未找到（project_path={:?}）", project_path);
        return Ok(None);
    }

    // ===== 全局配置（G_ 前缀或无前缀） =====
    if let Some(gdb) = engine::migration::get_global_db_manager() {
        if let Ok(nets) = gdb.list_network_configs(None).await {
            if let Some(net) = nets.iter().find(|n| n.id == net_id) {
                return parse_network_config_json(
                    &net.network_type,
                    &net.config,
                    net.auth_config_id.as_deref(),
                    None,
                )
                .await;
            }
        }
    }

    // ===== 向后兼容：无前缀时也查项目 DB =====
    if !net_id.starts_with("G_") {
        if let Some(pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        project_path,
                    )
                    .await;
                }
            }
        }
    }

    tracing::warn!(net_id = %net_id, "未找到网络配置");
    Ok(None)
}

/// 查询项目网络配置，同时返回 network_type、config 和 auth_config_id。
///
/// 这是**直查 SQL**（绕过 `network_store` 的读路径），所以必须自己解密 `config`：
/// 网络档案的 `password` / `passphrase` 落库时已加密（§14 #34），不解密会让隧道拿
/// `AES:…` 当密码用。
fn project_query_network_config_with_auth(
    db_path: &std::path::Path,
    net_id: &str,
) -> Result<(String, String, Option<String>), String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let (network_type, config, auth_config_id) = conn
        .query_row(
            "SELECT network_type, config, auth_config_id FROM network_configs WHERE id = ?1",
            rusqlite::params![net_id],
            |row| {
                let network_type: String = row.get(0)?;
                let config: String = row.get(1)?;
                let auth_config_id: Option<String> = row.get(2)?;
                Ok((network_type, config, auth_config_id))
            },
        )
        .map_err(|e| e.to_string())?;
    let config = engine::persistence::network_store::decrypt_network_config(&config)
        .map_err(|e| e.to_string())?;
    Ok((network_type, config, auth_config_id))
}

/// 根据 network_type 将 config JSON 解析为 ConnectionMethod
///
/// 公共函数，commands 层和 service 层共享
///
/// 如果提供了 auth_config_id，则从 auth_configs 表读取网络认证配置并注入
pub async fn parse_network_config_json(
    network_type: &str,
    config_json: &str,
    auth_config_id: Option<&str>,
    project_path: Option<&str>,
) -> Result<Option<ConnectionMethod>, CoreError> {
    // 类型键归一化：写入侧可能是 UI 标签（`SSH` / `Proxy`）或别名，统一小写后匹配。
    // 此前只匹配小写字面量，导致「档案存在、类型也写了，但整条链静默不生效」（审计 #20）。
    let kind = network_type.trim().to_ascii_lowercase();
    match kind.as_str() {
        "chain" => {
            let hops: Vec<connection::config::ChainHop> = serde_json::from_str(config_json)
                .map_err(|e| CoreError::from(format!("解析协议链配置 JSON 失败: {}", e)))?;
            if hops.is_empty() {
                return Ok(None);
            }
            Ok(Some(ConnectionMethod::Chain(hops)))
        }
        "ssh" | "ssh_tunnel" => {
            let mut ssh_config: connection::config::SshConfig =
                serde_json::from_str(config_json)
                    .map_err(|e| CoreError::from(format!("解析 SSH 隧道配置 JSON 失败: {}", e)))?;

            // 如果有 auth_config_id，从 auth_configs 读取网络认证并注入
            if let Some(auth_id) = auth_config_id {
                if let Some(auth_data) =
                    load_auth_data_from_db_for_network(auth_id, project_path).await?
                {
                    ssh_config = inject_ssh_auth_from_auth_data(ssh_config, &auth_data);
                }
            }

            Ok(Some(ConnectionMethod::Ssh(ssh_config)))
        }
        "ssl" | "tls" => {
            let ssl_config: connection::config::SslConfig = serde_json::from_str(config_json)
                .map_err(|e| CoreError::from(format!("解析 SSL 配置 JSON 失败: {}", e)))?;
            Ok(Some(ConnectionMethod::Ssl(ssl_config)))
        }
        "proxy" | "http_proxy" | "http" | "socks" | "socks5" | "socks_proxy" => {
            let mut proxy_config: connection::config::ProxyConfig =
                serde_json::from_str(config_json)
                    .map_err(|e| CoreError::from(format!("解析代理配置 JSON 失败: {}", e)))?;

            // 如果有 auth_config_id，从 auth_configs 读取网络认证并注入
            if let Some(auth_id) = auth_config_id {
                if let Some(auth_data) =
                    load_auth_data_from_db_for_network(auth_id, project_path).await?
                {
                    proxy_config = inject_proxy_auth_from_auth_data(proxy_config, &auth_data);
                }
            }

            if kind.starts_with("socks") {
                Ok(Some(ConnectionMethod::SocksProxy(proxy_config)))
            } else {
                Ok(Some(ConnectionMethod::HttpProxy(proxy_config)))
            }
        }
        other => {
            tracing::warn!(
                network_type = other,
                "未知的网络配置类型（档案已保存但不会生效）"
            );
            Ok(None)
        }
    }
}

/// 从 auth_configs 表加载网络认证配置（用于 SSH/Proxy 认证）
///
/// 此函数与 `load_auth_data_from_db` 不同，它用于加载网络认证，
/// 而 `load_auth_data_from_db` 用于加载数据库认证
async fn load_auth_data_from_db_for_network(
    auth_config_id: &str,
    project_path: Option<&str>,
) -> Result<Option<String>, CoreError> {
    // 优先从全局数据库读取
    if let Some(gdb) = engine::migration::get_global_db_manager() {
        if let Ok(Some(auth_config)) = gdb.get_auth_config(auth_config_id).await {
            let auth_data =
                engine::persistence::auth_store::decrypt_auth_data(&auth_config.auth_data)?;
            return Ok(Some(auth_data));
        }
    }

    // 全局未找到，尝试从项目数据库读取（项目级网络认证配置）
    if let Some(pp) = project_path {
        let db_path = std::path::Path::new(pp).join(".RSmeta").join("project.db");
        if db_path.exists() {
            if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                if let Ok(Some(auth_config)) =
                    engine::persistence::auth_store::get_auth_config(&conn, auth_config_id)
                {
                    let auth_data =
                        engine::persistence::auth_store::decrypt_auth_data(&auth_config.auth_data)?;
                    return Ok(Some(auth_data));
                }
            }
        }
    }

    Ok(None)
}

/// 从 auth_data JSON 注入 SSH 认证配置
fn inject_ssh_auth_from_auth_data(
    mut ssh_config: connection::config::SshConfig,
    auth_data_json: &str,
) -> connection::config::SshConfig {
    let auth_data: serde_json::Value = match serde_json::from_str(auth_data_json) {
        Ok(v) => v,
        Err(_) => return ssh_config,
    };

    let obj = match auth_data.as_object() {
        Some(o) => o,
        None => return ssh_config,
    };

    // 优先使用 auth_data 中的 username
    if let Some(username) = obj.get("username").and_then(|v| v.as_str()) {
        if !username.is_empty() {
            ssh_config.username = username.to_string();
        }
    }

    // 根据常见的 SSH 认证类型注入认证
    // 首先尝试私钥认证
    if let Some(key_path) = obj.get("keyPath").and_then(|v| v.as_str()) {
        if !key_path.is_empty() {
            let passphrase = obj
                .get("passphrase")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            ssh_config.auth = connection::config::SshAuth::PrivateKey {
                key_path: key_path.to_string(),
                passphrase,
            };
            return ssh_config;
        }
    }

    // 然后尝试密码认证
    if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
        if !password.is_empty() {
            ssh_config.auth = connection::config::SshAuth::Password {
                password: password.to_string(),
            };
            return ssh_config;
        }
    }

    ssh_config
}

/// 从 auth_data JSON 注入 Proxy 认证配置
fn inject_proxy_auth_from_auth_data(
    mut proxy_config: connection::config::ProxyConfig,
    auth_data_json: &str,
) -> connection::config::ProxyConfig {
    let auth_data: serde_json::Value = match serde_json::from_str(auth_data_json) {
        Ok(v) => v,
        Err(_) => return proxy_config,
    };

    let obj = match auth_data.as_object() {
        Some(o) => o,
        None => return proxy_config,
    };

    // 从 auth_data 读取 username 和 password
    let username = obj.get("username").and_then(|v| v.as_str());
    let password = obj.get("password").and_then(|v| v.as_str());

    if let (Some(u), Some(p)) = (username, password) {
        if !u.is_empty() && !p.is_empty() {
            proxy_config.auth = Some(connection::config::ProxyAuth {
                username: u.to_string(),
                password: p.to_string(),
            });
        }
    }

    proxy_config
}
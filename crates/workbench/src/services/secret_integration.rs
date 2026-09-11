//! DuckDB Secret 集成（M3 本地加速闭环）
//!
//! 连接生命周期 → Secret 注册：连接建立成功后，将源库凭据注册为 DuckDB Secret，
//! 使分析引擎可直接联邦查询源库（Postgres/MySQL），"一次注册、到处可用"。
//!
//! 依赖方向：workbench → connection（SecretManager）/ engine / shared。

use connection::secret::{DatabaseCredential, SecretError, SecretManager, SecretResult};
use percent_encoding::percent_decode_str;
use tracing::warn;

/// URL 解析结果（`scheme://user:pass@host:port/database`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlParts {
    pub scheme: String,
    pub user: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub database: String,
}

/// URL 组件百分号解码（`%40` → `@`），避免把编码形式当字面量注册进 Secret。
fn decode_component(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().to_string()
}

/// 解析数据源连接 URL 为 Secret 注册所需字段
///
/// 支持形式：
/// - `postgres://user:pass@host:5432/db`
/// - `postgresql://user:pass@host:5432/db`
/// - `mysql://user:pass@host:3306/db`
/// - `sqlite:///path/to.db`
/// - `duckdb:///path/to.duckdb`
pub fn parse_connection_url(url: &str) -> Option<UrlParts> {
    let rest = url.split("://").nth(1)?;
    let (authority, database) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i + 1..].trim_end_matches('/').to_string()),
        None => (rest, String::new()),
    };

    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (&authority[..i], &authority[i + 1..]),
        None => ("", authority),
    };

    let (user, password) = match userinfo.find(':') {
        Some(i) => (
            decode_component(&userinfo[..i]),
            decode_component(&userinfo[i + 1..]),
        ),
        None => (decode_component(userinfo), String::new()),
    };

    let (host, port) = match hostport.rfind(':') {
        Some(i) => (
            decode_component(&hostport[..i]),
            hostport[i + 1..].parse::<u16>().unwrap_or(0),
        ),
        None => (decode_component(hostport), 0),
    };

    let scheme = url.split("://").next()?.to_lowercase();
    let scheme = scheme.split('+').next().unwrap_or(&scheme).to_string();

    Some(UrlParts {
        scheme,
        user,
        password,
        host,
        port,
        database: decode_component(&database),
    })
}

/// 数据库类型 → DuckDB Secret 类型
///
/// 可用性（bundled DuckDB 实测）：POSTGRES / MYSQL / S3 / GCS / R2 / AZURE 内置；
/// SQLITE / DUCKDB 需扩展（返回类型但注册可能失败，由调用方按警告处理）。
pub fn db_type_to_secret_type(db_type: &str) -> Option<&'static str> {
    match db_type.to_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => Some("POSTGRES"),
        "mysql" => Some("MYSQL"),
        "sqlite" => Some("SQLITE"),
        "duckdb" => Some("DUCKDB"),
        "s3" => Some("S3"),
        _ => None,
    }
}

/// 将连接凭据注册为 DuckDB Secret（加速通道核心调用）
pub fn register_connection_secret(
    mgr: &SecretManager,
    conn_id: &str,
    db_type: &str,
    url: &str,
) -> SecretResult<()> {
    let Some(parts) = parse_connection_url(url) else {
        return Err(connection::secret::SecretError::Invalid(format!(
            "无法解析连接 URL: {url}"
        )));
    };
    let Some(secret_type) = db_type_to_secret_type(db_type) else {
        return Err(connection::secret::SecretError::Invalid(format!(
            "不支持的 Secret 类型: {db_type}"
        )));
    };
    let cred = DatabaseCredential {
        name: sanitize_secret_name(conn_id),
        secret_type: secret_type.to_string(),
        host: parts.host,
        port: parts.port,
        username: parts.user,
        password: parts.password,
        database: parts.database,
    };
    mgr.register(&cred)
}

/// Secret 名称净化（DuckDB 标识符：字母/数字/下划线，连字符等其他字符转下划线）
fn sanitize_secret_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "conn".to_string()
    } else {
        cleaned
    }
}

/// 分析库（DuckDB）路径：Secret 注册/注销的统一目标（`analytics.duckdb`）。
fn analysis_db_path() -> Option<std::path::PathBuf> {
    match engine::migration::get_global_duckdb_path() {
        Ok(path) => Some(path),
        Err(e) => {
            warn!("[secret] 无法定位分析库，跳过 Secret 联动: {e}");
            None
        }
    }
}

/// 会话级 Secret 注册（默认目标：全局分析库）。
pub fn ensure_secret_registered(conn_id: &str, db_type: &str, url: &str) {
    ensure_secret_registered_at(None, conn_id, db_type, url);
}

/// 会话级 Secret 注册（可指定目标库；`None` = 全局分析库）。
///
/// 注册目标为持久分析库（`analytics.duckdb`），跨会话可用；
/// 临时内存库注册随连接句柄销毁而丢失，不具备联邦加速能力。
/// `target` 供测试/多环境注入，避免触碰用户真实分析库。
pub fn ensure_secret_registered_at(
    target: Option<&std::path::Path>,
    conn_id: &str,
    db_type: &str,
    url: &str,
) {
    if db_type_to_secret_type(db_type).is_none() {
        return; // 非联邦目标类型不注册
    }
    let path = match target {
        Some(p) => p.to_path_buf(),
        None => match analysis_db_path() {
            Some(p) => p,
            None => return,
        },
    };
    match SecretManager::open(&path)
        .and_then(|mgr| register_connection_secret(&mgr, conn_id, db_type, url))
    {
        Ok(()) => tracing::info!(
            "[secret] 已为连接 {} 注册 DuckDB Secret（联邦加速可用）",
            conn_id
        ),
        Err(e) => warn!(
            "[secret] 连接 {} 的 Secret 注册跳过（不影响连接）: {}",
            conn_id, e
        ),
    }
}

/// 删除连接时注销 Secret；返回是否确实移除了已有 Secret（默认目标：全局分析库）。
pub fn remove_connection_secret(conn_id: &str) -> bool {
    remove_connection_secret_at(None, conn_id)
}

/// 删除连接时注销 Secret（可指定目标库；`None` = 全局分析库）。
///
/// 未注册过（未开启联邦加速）返回 false，不视为失败。
pub fn remove_connection_secret_at(target: Option<&std::path::Path>, conn_id: &str) -> bool {
    let path = match target {
        Some(p) => p.to_path_buf(),
        None => match analysis_db_path() {
            Some(p) => p,
            None => return false,
        },
    };
    let name = sanitize_secret_name(conn_id);
    match SecretManager::open(&path).and_then(|mgr| mgr.remove(&name)) {
        Ok(()) => {
            tracing::info!("[secret] 已移除连接 {} 的 DuckDB Secret", conn_id);
            true
        }
        Err(SecretError::NotFound(_)) => false,
        Err(e) => {
            warn!("[secret] 连接 {} 的 Secret 移除失败: {}", conn_id, e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_url() {
        let p = parse_connection_url("postgres://admin:p%40ss@db.example.com:5432/market")
            .expect("parse ok");
        assert_eq!(p.scheme, "postgres");
        assert_eq!(p.user, "admin");
        assert_eq!(p.password, "p@ss", "百分号编码应解码后再注册");
        assert_eq!(p.host, "db.example.com");
        assert_eq!(p.port, 5432);
        assert_eq!(p.database, "market");
    }

    #[test]
    fn test_parse_without_creds() {
        let p = parse_connection_url("mysql://10.0.0.1:3306/orders").expect("parse ok");
        assert_eq!(p.user, "");
        assert_eq!(p.password, "");
        assert_eq!(p.host, "10.0.0.1");
        assert_eq!(p.port, 3306);
        assert_eq!(p.database, "orders");
    }

    #[test]
    fn test_parse_file_db() {
        let p = parse_connection_url("duckdb:///data/analytics.duckdb").expect("parse ok");
        assert_eq!(p.scheme, "duckdb");
        assert_eq!(p.host, "");
        assert_eq!(p.database, "data/analytics.duckdb");
    }

    #[test]
    fn test_db_type_mapping() {
        assert_eq!(db_type_to_secret_type("postgres"), Some("POSTGRES"));
        assert_eq!(db_type_to_secret_type("PostgreSQL"), Some("POSTGRES"));
        assert_eq!(db_type_to_secret_type("mysql"), Some("MYSQL"));
        assert_eq!(db_type_to_secret_type("oracle"), None);
    }

    #[test]
    fn test_register_postgres_secret_roundtrip() {
        let mgr = SecretManager::in_memory().unwrap();
        register_connection_secret(
            &mgr,
            "conn-001",
            "postgres",
            "postgres://admin:secret@localhost:5432/market",
        )
        .unwrap();
        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].secret_type.to_uppercase(), "POSTGRES");
        // 名称净化：conn-001 → conn_001
        assert_eq!(list[0].name, "conn_001");
    }

    #[test]
    fn test_secret_name_sanitize() {
        assert_eq!(sanitize_secret_name("conn-001"), "conn_001");
        assert_eq!(sanitize_secret_name("连接 01"), "___01");
        assert_eq!(sanitize_secret_name(""), "conn");
    }
}

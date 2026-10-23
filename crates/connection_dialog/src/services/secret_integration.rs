//! DuckDB Secret 集成（M3 本地加速闭环）
//!
//! 连接生命周期 → Secret 注册：连接建立成功后，将源库凭据注册为 DuckDB Secret。
//!
//! ## 实测结论（别把 Secret 当成挂载凭据的唯一来源）
//!
//! **DuckDB 1.5.5 的 mysql / postgres 扫描器都不认 Secret**（会话级、持久化、带 scope、
//! `ATTACH ''` 各种写法都试过，见 `crates/engine/tests/federation_credentials_probe.rs`）：
//! 加速 / 联邦真正靠的是**运行时连接串里的凭据**，引擎侧再把离开它的文本脱敏
//! （`accel::scrub_credentials`）。保留这里的注册是为了凭据集中管理（以及将来可能支持的场景），
//! **不是**挂载路径的前提。
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

/// 数据库族 → DuckDB Secret 类型
///
/// Secret 只服务**远程扫描源**（bundled DuckDB 实测内置：POSTGRES / MYSQL / S3 /
/// GCS / R2 / AZURE）。`sqlite` / `duckdb` 是本地文件：靠 scanner 直接 ATTACH 文件，
/// 没有凭据、也没有 Secret 概念（DuckDB 不存在 SQLITE / DUCKDB secret type，强行注册
/// 会被引擎拒绝），因此映射 `None`，调用方不注册。
///
/// 只认**数据库族**（`data_source_types.id`）与同义写法（`pg` / `mariadb`）；
/// 驱动实现 id（`mysql_native`）请走 [`secret_type_of`]。
pub fn db_type_to_secret_type(db_type: &str) -> Option<&'static str> {
    match db_type.to_lowercase().as_str() {
        "postgres" | "postgresql" | "pg" => Some("POSTGRES"),
        "mysql" | "mariadb" => Some("MYSQL"),
        "s3" => Some("S3"),
        _ => None,
    }
}

/// 驱动 id / 数据库族 → DuckDB Secret 类型（**唯一入口**）。
///
/// 为什么要这一步：连接记录里的 `db_type` 存的是驱动 id（`mysql_native` /
/// `postgres_native`），而 Secret 类型按数据库**族**给。不归一就会出现
/// 「选了 Official 驱动 → `db_type_to_secret_type` 返回 None → Secret 静默不注册」。
/// 先按原名试一次（族 id 与旧数据占多数，免一次库读），未命中再查驱动目录
/// （[`engine::persistence::driver_catalog::type_id_of`]）；目录不在位时按原名结论返回。
pub fn secret_type_of(db_type: &str) -> Option<&'static str> {
    if let Some(t) = db_type_to_secret_type(db_type) {
        return Some(t);
    }
    let family = engine::persistence::driver_catalog::type_id_of(db_type)?;
    db_type_to_secret_type(&family)
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
    let Some(secret_type) = secret_type_of(db_type) else {
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

/// Secret 名称净化：按 DuckDB 标识符规则（字母/数字/下划线）转换，
/// 并统一转小写（未加引号的标识符会被 DuckDB 折叠为小写，避免注册名与回读名不一致）。
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
        cleaned.to_ascii_lowercase()
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

/// 解析 Secret 目标：`(数据库文件, Secret 落盘目录)`。
///
/// 默认（`None`）：全局分析库 + `{system}/secrets`（应用可控目录，不写用户主目录
/// `~/.duckdb`；DuckDB 会话的 `secret_directory` 也指向同一处，见
/// `DuckDBManager::configure_connection`）；`Some(db)`：以该库所在目录的 `secrets/` 子目录为
/// Secret 目录（测试/多环境隔离）。
fn resolve_target(
    target: Option<&std::path::Path>,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    match target {
        Some(db) => {
            let dir = db
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("secrets");
            Some((db.to_path_buf(), dir))
        }
        None => {
            let db = analysis_db_path()?;
            let dir = engine::migration::get_secrets_dir().ok()?;
            Some((db, dir))
        }
    }
}

/// 会话级 Secret 注册（默认目标：全局分析库 + `{system}/secrets`）。
pub fn ensure_secret_registered(conn_id: &str, db_type: &str, url: &str) {
    ensure_secret_registered_at(None, conn_id, db_type, url);
}

/// 该驱动类型是否有对应的 DuckDB 扩展（本地加速 / 联邦扫描可用）。
///
/// 门控依据（用户规则 2026-10-03）：Secret 注册**只按是否有对应扩展**放行——
/// 实现与唯一来源在 `engine::duckdb::accel::AccelKind::has_federable_extension`
/// （mysql / postgres / sqlite 有官方 scanner 扩展，oracle 走社区扩展；
/// **duckdb 文件源无扩展 → 默认不注册**；未知类型同样不注册）。
pub fn has_federable_extension(db_type: &str) -> bool {
    engine::duckdb::accel::AccelKind::has_federable_extension(db_type)
}

/// 连接作用域 → 本地加速实例（目标分析库）。
///
/// 用户规则（2026-10-03）：本地加速使用的实例是**全局或项目的分析库**——
/// 全局侧连接（`G_`）→ 全局分析库（`{system}/shared.duckdb` 系）；
/// 项目侧连接（`P_` / `GP_` 项目快照）→ 项目分析库（`{项目}/.RSmeta/analytics.duckdb`）。
/// 与身份指纹的域隔离同源（`GP_` 项目侧归项目域）。
pub fn analysis_db_for_scope(conn_id: &str, project_path: Option<&str>) -> Option<std::path::PathBuf> {
    if engine::persistence::id_prefix::uses_project_storage(conn_id) {
        project_path.map(|p| {
            engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(
                std::path::Path::new(p),
            )
        })
    } else {
        analysis_db_path()
    }
}

/// 门控注册（**统一入口**）：加速开 + 有对应扩展才注册，项目 / 全局同规则。
///
/// 规则（2026-10-03 用户拍板，替代 §14 #38 旧"按 use_duckdb_fed 单条件"）：
/// 1. **默认不注册**——`use_duckdb_fed` 未开 → 直接返回（无论项目还是全局）；
/// 2. **无对应 DuckDB 扩展 → 不注册**（duckdb 文件源 / 未知类型，即使加速开）；
/// 3. `target` 由调用方按作用域传入（[`analysis_db_for_scope`]），失败仅告警不影响连接。
pub fn ensure_secret_registered_gated(
    target: Option<&std::path::Path>,
    conn_id: &str,
    db_type: &str,
    url: &str,
    use_duckdb_fed: bool,
) {
    if !use_duckdb_fed {
        return; // 默认不注册（加速关）
    }
    if !has_federable_extension(db_type) {
        tracing::info!(
            "[secret] 连接 {}（{db_type}）无对应 DuckDB 扩展，跳过 Secret 注册",
            conn_id
        );
        return;
    }
    ensure_secret_registered_at(target, conn_id, db_type, url);
}

/// 会话级 Secret 注册（可指定目标库；`None` = 全局分析库）。
///
/// Secret 以 `PERSISTENT` 方式落盘到 Secret 目录，跨会话可用；
/// `target` 供测试/多环境注入，避免触碰用户真实分析库与默认 Secret 存储。
pub fn ensure_secret_registered_at(
    target: Option<&std::path::Path>,
    conn_id: &str,
    db_type: &str,
    url: &str,
) {
    if secret_type_of(db_type).is_none() {
        return; // 非联邦目标类型不注册
    }
    let Some((db, dir)) = resolve_target(target) else {
        return;
    };
    match SecretManager::open_with_dir(&db, Some(&dir))
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
    let Some((db, dir)) = resolve_target(target) else {
        return false;
    };
    let name = sanitize_secret_name(conn_id);
    match SecretManager::open_with_dir(&db, Some(&dir)).and_then(|mgr| mgr.remove(&name)) {
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
        // MariaDB 走 MySQL 协议：同一个 Secret 类型
        assert_eq!(db_type_to_secret_type("mariadb"), Some("MYSQL"));
        assert_eq!(db_type_to_secret_type("oracle"), None);
        // 本地文件源无 Secret 概念：不映射、不尝试注册（避免引擎报 Secret type 不存在）
        assert_eq!(db_type_to_secret_type("sqlite"), None);
        assert_eq!(db_type_to_secret_type("duckdb"), None);
    }

    /// 驱动 id → Secret 类型的归一：族 id 走快路径，未命中（驱动 id / 未知值）查驱动目录。
    /// 本单测环境没有全局库（目录不在位）→ 只有快路径生效，且**不 panic**。
    #[test]
    fn test_secret_type_of_falls_back_to_driver_catalog() {
        assert_eq!(secret_type_of("postgresql"), Some("POSTGRES"));
        assert_eq!(secret_type_of("MYSQL"), Some("MYSQL"));
        // 目录不可用：`mysql_native` 无法归一 → None（调用方不注册，且不会静默注册错类型）
        assert_eq!(secret_type_of("mysql_native"), None);
        assert_eq!(secret_type_of("oracle"), None);
    }

    #[test]
    fn test_register_postgres_secret_roundtrip() {
        // 隔离存储目录：不触碰 DuckDB 默认 Secret 存储（用户主目录）。
        let dir = std::env::temp_dir().join(format!("rds_secret_rt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let mgr = SecretManager::open_with_dir(
            dir.join("store.duckdb"),
            Some(&dir.join("secrets")),
        )
        .expect("open isolated store");
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
        drop(mgr);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_secret_name_sanitize() {
        assert_eq!(sanitize_secret_name("conn-001"), "conn_001");
        assert_eq!(sanitize_secret_name("连接 01"), "___01");
        assert_eq!(sanitize_secret_name(""), "conn");
        // DuckDB 未加引号的标识符折叠为小写，净化统一小写避免回读不一致。
        assert_eq!(sanitize_secret_name("G_conn_Demo"), "g_conn_demo");
    }

    // ===== 2026-10-03 门控规则（用户拍板：按扩展门控 + 作用域分析库 + 默认不注册）=====

    #[test]
    fn test_has_federable_extension_by_scanner_extensions() {
        // 有对应 DuckDB 扩展 → 可注册（本地加速 / 联邦扫描可用）
        assert!(has_federable_extension("mysql"));
        assert!(has_federable_extension("postgres"));
        assert!(has_federable_extension("sqlite"));
        assert!(has_federable_extension("postgres_native"), "驱动 id 也应归一");
        // duckdb 文件源：内核自带能力、无扩展 → 默认不注册
        assert!(!has_federable_extension("duckdb"));
        // 未知 / 无法联邦的类型 → 不注册
        assert!(!has_federable_extension("mongo"));
        assert!(!has_federable_extension(""));
    }

    #[test]
    fn test_analysis_db_for_scope_routes_by_domain() {
        // 全局侧（G_）→ 全局分析库（真实路径由环境决定，只验证 Some）
        assert!(analysis_db_for_scope("G_conn_prod", None).is_some());
        // 项目侧（P_ / GP_ 项目快照）→ 项目分析库 {项目}/.RSmeta/analytics.duckdb
        let p = analysis_db_for_scope("P_conn_a1b2c3d4", Some("C:/proj/rds-demo"))
            .expect("项目路径存在");
        assert!(p.ends_with(".RSmeta\\analytics.duckdb") || p.ends_with(".RSmeta/analytics.duckdb"));
        let gp = analysis_db_for_scope("GP_conn_prod_20261003", Some("C:/proj/rds-demo"))
            .expect("GP_ 项目侧应归项目域");
        assert_eq!(gp, p, "GP_ 项目快照侧与 P_ 同为项目分析库（域隔离同源）");
        // 项目侧但项目路径缺失 → None（调用方跳过，不静默写错位置）
        assert!(analysis_db_for_scope("P_conn_x", None).is_none());
        assert!(analysis_db_for_scope("GP_conn_x_20261003", None).is_none());
    }
}

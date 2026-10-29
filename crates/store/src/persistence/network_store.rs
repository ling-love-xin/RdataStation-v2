use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, StorageError};

/// 网络配置（SSH 隧道、HTTP 代理、SSL 证书等）
/// config 列保留完整配置信息（含 host/port/forwarding + auth 冗余）
/// auth_config_id 引用 auth_configs.id，指向独立存储的认证凭据
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NetworkConfig {
    pub id: String,
    pub name: Option<String>,
    pub network_type: String,
    pub config: String,
    pub auth_config_id: Option<String>,
    pub origin: Option<String>,
    pub source_id: Option<String>,
    pub snapshot_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "network_store".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 确保 network_configs 表存在（回退修复）
fn ensure_table(conn: &Connection) -> Result<(), CoreError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS network_configs (
            id            TEXT PRIMARY KEY,
            name          TEXT,
            network_type  TEXT NOT NULL,
            config        TEXT NOT NULL,
            auth_config_id TEXT,
            origin        TEXT,
            source_id     TEXT,
            snapshot_at   TEXT,
            created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| storage_err("ensure_table", e.to_string()))?;

    // 补充：确保迁移可能遗漏的列（ALTER TABLE ADD COLUMN 不支持 IF NOT EXISTS，忽略重复错误）
    let extra_cols = [
        "auth_config_id TEXT",
        "origin TEXT",
        "source_id TEXT",
        "snapshot_at TEXT",
    ];
    for col_def in &extra_cols {
        let _ = conn.execute(
            &format!("ALTER TABLE network_configs ADD COLUMN {}", col_def),
            [],
        );
    }
    Ok(())
}

/// 创建网络配置（项目库，含快照溯源字段）
pub fn create_network_config(conn: &Connection, nc: &NetworkConfig) -> Result<(), CoreError> {
    let _ = ensure_table(conn);
    // 敏感键（password / passphrase）先加密再落库：网络档案里可以直接填 SSH / 代理密码，
    // 与 `auth_data` 同等级别的凭据，不应明文留在磁盘上（§14 #34）。
    let config = encrypt_network_config(&nc.config)?;
    conn.execute(
        "INSERT OR REPLACE INTO network_configs (id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            nc.id,
            nc.name,
            nc.network_type,
            config,
            nc.auth_config_id,
            nc.origin,
            nc.source_id,
            nc.snapshot_at,
            nc.created_at,
            nc.updated_at
        ],
    )
    .map_err(|e| storage_err("create_network_config", e.to_string()))?;
    Ok(())
}

/// 列出网络配置，可按网络类型过滤（项目库，含快照溯源字段）
pub fn list_network_configs(
    conn: &Connection,
    network_type: Option<&str>,
) -> Result<Vec<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let (sql, param): (String, Option<String>) = if let Some(t) = network_type {
        (
            "SELECT id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at
             FROM network_configs WHERE network_type = ?1 ORDER BY name"
                .to_string(),
            Some(t.to_string()),
        )
    } else {
        (
            "SELECT id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at
             FROM network_configs ORDER BY network_type, name"
                .to_string(),
            None,
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_network_configs", e.to_string()))?;

    let mut items: Vec<NetworkConfig> = if let Some(ref p) = param {
        stmt.query_map(params![p], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: row.get(5)?,
                source_id: row.get(6)?,
                snapshot_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| storage_err("query_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: row.get(5)?,
                source_id: row.get(6)?,
                snapshot_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| storage_err("query_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    // 读路径统一解密：调用方（连接解析 / 管理器回填）拿到的是明文凭据。
    for it in &mut items {
        if let Ok(plain) = decrypt_network_config(&it.config) {
            it.config = plain;
        }
    }

    Ok(items)
}

/// 根据 ID 获取网络配置（项目库，含快照溯源字段）
pub fn get_network_config(conn: &Connection, id: &str) -> Result<Option<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at
             FROM network_configs WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_network_config", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(NetworkConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            network_type: row.get(2)?,
            config: row.get(3)?,
            auth_config_id: row.get(4)?,
            origin: row.get(5)?,
            source_id: row.get(6)?,
            snapshot_at: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_network_config", e.to_string()))
    .map(|found| found.map(|mut nc| {
        nc.config = decrypt_network_config(&nc.config).unwrap_or(nc.config);
        nc
    }))
}

/// 更新网络配置，若配置不存在则返回错误
pub fn update_network_config(conn: &Connection, nc: &NetworkConfig) -> Result<(), CoreError> {
    let config = encrypt_network_config(&nc.config)?;
    let rows = conn
        .execute(
            "UPDATE network_configs SET name = ?1, network_type = ?2, config = ?3, auth_config_id = ?4, updated_at = ?5 WHERE id = ?6",
            params![nc.name, nc.network_type, config, nc.auth_config_id, nc.updated_at, nc.id],
        )
        .map_err(|e| storage_err("update_network_config", e.to_string()))?;

    if rows == 0 {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "network_store".to_string(),
            operation: "update_network_config".to_string(),
            reason: format!("network config not found: {}", nc.id),
        }));
    }
    Ok(())
}

/// 删除网络配置
pub fn delete_network_config(conn: &Connection, id: &str) -> Result<(), CoreError> {
    conn.execute("DELETE FROM network_configs WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_network_config", e.to_string()))?;
    Ok(())
}

// ===========================================================================
// ======================== 全局库专用函数（无 origin/source_id/snapshot_at）==
// ===========================================================================
// 全局 network_configs 表不需要快照溯源字段（全局和项目物理隔离）
// 项目 network_configs 表（有 origin 列）使用上面的通用函数

/// 全局库：创建网络配置（不含快照溯源字段，含 auth_config_id）
pub fn create_global_network_config(
    conn: &Connection,
    nc: &NetworkConfig,
) -> Result<(), CoreError> {
    let _ = ensure_table(conn);
    let config = encrypt_network_config(&nc.config)?;
    conn.execute(
        "INSERT INTO network_configs (id, name, network_type, config, auth_config_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            nc.id,
            nc.name,
            nc.network_type,
            config,
            nc.auth_config_id,
            nc.created_at,
            nc.updated_at
        ],
    )
    .map_err(|e| storage_err("create_global_network_config", e.to_string()))?;
    Ok(())
}

/// 全局库：列出网络配置（不含快照溯源字段）
pub fn list_global_network_configs(
    conn: &Connection,
    network_type: Option<&str>,
) -> Result<Vec<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let (sql, param): (String, Option<String>) = if let Some(t) = network_type {
        (
            "SELECT id, name, network_type, config, auth_config_id, created_at, updated_at
             FROM network_configs WHERE network_type = ?1 ORDER BY name"
                .to_string(),
            Some(t.to_string()),
        )
    } else {
        (
            "SELECT id, name, network_type, config, auth_config_id, created_at, updated_at
             FROM network_configs ORDER BY network_type, name"
                .to_string(),
            None,
        )
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_global_network_configs", e.to_string()))?;

    let mut items: Vec<NetworkConfig> = if let Some(ref p) = param {
        stmt.query_map(params![p], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: Some("global".to_string()),
                source_id: None,
                snapshot_at: None,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| storage_err("query_global_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok(NetworkConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                network_type: row.get(2)?,
                config: row.get(3)?,
                auth_config_id: row.get(4)?,
                origin: Some("global".to_string()),
                source_id: None,
                snapshot_at: None,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|e| storage_err("query_global_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    // 读路径统一解密（与项目库同策略）。
    for it in &mut items {
        if let Ok(plain) = decrypt_network_config(&it.config) {
            it.config = plain;
        }
    }

    Ok(items)
}

/// 全局库：根据 ID 获取网络配置（不含快照溯源字段）
pub fn get_global_network_config(
    conn: &Connection,
    id: &str,
) -> Result<Option<NetworkConfig>, CoreError> {
    let _ = ensure_table(conn);
    let mut stmt = conn
        .prepare(
            "SELECT id, name, network_type, config, auth_config_id, created_at, updated_at
             FROM network_configs WHERE id = ?1",
        )
        .map_err(|e| storage_err("prepare_get_global_network_config", e.to_string()))?;

    stmt.query_row(params![id], |row| {
        Ok(NetworkConfig {
            id: row.get(0)?,
            name: row.get(1)?,
            network_type: row.get(2)?,
            config: row.get(3)?,
            auth_config_id: row.get(4)?,
            origin: Some("global".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    })
    .optional()
    .map_err(|e| storage_err("get_global_network_config", e.to_string()))
    .map(|found| found.map(|mut nc| {
        nc.config = decrypt_network_config(&nc.config).unwrap_or(nc.config);
        nc
    }))
}

// ===== 敏感字段加密（§14 #34：网络档案 config 内的密码不再明文入库）=====
//
// 背景：`network_configs.config` 原本明文落库，而 SSH / 代理字段表单允许直接填密码 /
// 口令——库文件被复制或备份即泄露跳板机与代理凭据。这里复用 `auth_data` 的同一套
// 加密（`shared::crypto`：AES-256-GCM + 随机 nonce，`AES:` 前缀）。

/// 需要加密的键名（任意层级；与 `connection::config` 的模型对应）：
/// - SSH：`password`（密码认证）/ `passphrase`（私钥口令）；
/// - 代理：`password`（`ProxyConfig.auth.password`，嵌套一层）；
/// - 协议链：数组里每个 hop 的同类字段（递归覆盖）。
const SECRET_KEYS: [&str; 2] = ["password", "passphrase"];

/// 写入前加密 `config` 里的敏感键（**幂等**：已带 `AES:` 前缀的值原样保留）。
///
/// 非法 JSON / 非对象非数组 → 原样返回（存储层不因格式问题拒写）。
pub fn encrypt_network_config(config: &str) -> Result<String, CoreError> {
    rewrite_secrets(config, true)
}

/// 读取后解密 `config` 里的敏感键（与 [`encrypt_network_config`] 对称）。
///
/// 明文旧值（未带 `AES:`）原样返回，兼容升级前写入的档案。
pub fn decrypt_network_config(config: &str) -> Result<String, CoreError> {
    rewrite_secrets(config, false)
}

/// 把库中仍为明文的网络档案重新加密（一次性迁移，**幂等**；返回改动数）。
///
/// `encrypt_network_config` 不会重复加密已加密的值，所以可安全重复调用；
/// 项目库由调用方按项目根自行调用。
pub fn reencrypt_all_network_configs(conn: &Connection) -> Result<usize, CoreError> {
    let _ = ensure_table(conn);
    let rows: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, config FROM network_configs")
            .map_err(|e| storage_err("prepare_reencrypt_network_configs", e.to_string()))?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| storage_err("query_reencrypt_network_configs", e.to_string()))?
            .filter_map(|r| r.ok())
            .collect()
    };

    let mut changed = 0usize;
    for (id, raw) in rows {
        let encrypted = encrypt_network_config(&raw)?;
        if encrypted != raw {
            conn.execute(
                "UPDATE network_configs SET config = ?1 WHERE id = ?2",
                params![encrypted, id],
            )
            .map_err(|e| storage_err("update_reencrypt_network_config", e.to_string()))?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 对**项目库**执行网络档案加密迁移（`{root}/.RSmeta/project.db`）。
///
/// 与全局库迁移同款，但：
/// - 项目库可能还不存在 / 没有 `network_configs` 表 → **直接跳过**（不建目录、不建表，
///   读路径无副作用，与全库的写/读分离约定一致）；
/// - 返回该库的改动数；调用方（启动迁移 / 项目打开）自行汇总。
pub fn reencrypt_project_network_configs(
    project_root: &std::path::Path,
) -> Result<usize, CoreError> {
    use rusqlite::OptionalExtension;

    let db_path = project_root
        .join(crate::persistence::connection_org_store::RS_META_DIR_NAME)
        .join("project.db");
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| storage_err("open_project_network_store", e.to_string()))?;
    // 不建表：老项目库可能根本没有 network_configs（`reencrypt_all_*` 内部会 ensure_table，
    // 所以必须先确认表存在再调）。
    let has_table: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'network_configs'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| storage_err("probe_project_network_store", e.to_string()))?;
    if has_table.is_none() {
        return Ok(0);
    }
    reencrypt_all_network_configs(&conn)
}

fn rewrite_secrets(config: &str, encrypt: bool) -> Result<String, CoreError> {
    if config.trim().is_empty() {
        return Ok(config.to_string());
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(config) else {
        return Ok(config.to_string());
    };
    if !value.is_object() && !value.is_array() {
        return Ok(config.to_string());
    }
    rewrite_node(&mut value, encrypt)?;
    Ok(value.to_string())
}

fn rewrite_node(node: &mut serde_json::Value, encrypt: bool) -> Result<(), CoreError> {
    match node {
        serde_json::Value::Object(map) => {
            for key in SECRET_KEYS {
                let Some(serde_json::Value::String(v)) = map.get_mut(key) else {
                    continue;
                };
                if encrypt {
                    if v.is_empty() || v.starts_with("AES:") {
                        continue;
                    }
                    *v = format!("AES:{}", shared::crypto::encrypt_password(v)?);
                } else if let Some(raw) = v.strip_prefix("AES:") {
                    // 解密失败（数据损坏 / 密钥变更）保留原值：不因单条档案让读路径整体报错。
                    if let Ok(plain) = shared::crypto::decrypt_password(raw) {
                        *v = plain;
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                rewrite_node(child, encrypt)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                rewrite_node(item, encrypt)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_of(json: &str) -> NetworkConfig {
        NetworkConfig {
            id: "G_net_test".into(),
            name: Some("t".into()),
            network_type: "ssh".into(),
            config: json.into(),
            auth_config_id: None,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "2026-09-12T00:00:00Z".into(),
            updated_at: "2026-09-12T00:00:00Z".into(),
        }
    }

    #[test]
    fn ssh_and_proxy_secrets_are_encrypted() {
        // SSH：根级 `password` / `passphrase`
        let ssh = r#"{"host":"jump","port":22,"username":"u","auth_type":"password","password":"p@ss","passphrase":"pp","remote_host":"db","remote_port":5432}"#;
        let enc = encrypt_network_config(ssh).expect("encrypt");
        assert!(!enc.contains("p@ss"), "明文密码不得留下：{enc}");
        assert!(!enc.contains("\"pp\""), "明文口令不得留下：{enc}");
        assert!(enc.contains("AES:"), "应带 AES 前缀：{enc}");
        // 幂等：已加密不再重复加密
        assert_eq!(encrypt_network_config(&enc).expect("idempotent"), enc);

        let dec = decrypt_network_config(&enc).expect("decrypt");
        let parsed: serde_json::Value = serde_json::from_str(&dec).expect("json");
        assert_eq!(parsed["password"], "p@ss");
        assert_eq!(parsed["passphrase"], "pp");
        assert_eq!(parsed["host"], "jump", "非敏感字段不变");

        // 代理：嵌套 `auth.password`
        let proxy = r#"{"host":"127.0.0.1","port":1080,"auth":{"username":"u","password":"secret"}}"#;
        let enc = encrypt_network_config(proxy).expect("encrypt proxy");
        assert!(!enc.contains("secret"), "{enc}");
        let dec = decrypt_network_config(&enc).expect("decrypt proxy");
        let parsed: serde_json::Value = serde_json::from_str(&dec).expect("json");
        assert_eq!(parsed["auth"]["password"], "secret");

        // 协议链：数组内每跳都要覆盖
        let chain = r#"[{"type":"ssh","host":"a","username":"u","auth_type":"password","password":"p1"},{"type":"proxy","host":"b","port":8080,"auth":{"password":"p2"}}]"#;
        let enc = encrypt_network_config(chain).expect("encrypt chain");
        // 逐字段断言，而不是在整串密文上做子串匹配：密文是 base64，随机密文可能恰好含
        // "p1" 这样的短明文（约 1~2% 概率），整串 contains 会变成随机失败的假阳性。
        let fields: serde_json::Value = serde_json::from_str(&enc).expect("json");
        let ssh_password = fields[0]["password"].as_str().expect("ssh password");
        let proxy_password = fields[1]["auth"]["password"].as_str().expect("proxy password");
        assert!(
            ssh_password.starts_with("AES:") && ssh_password != "p1",
            "SSH 密码不得以明文留下：{ssh_password}"
        );
        assert!(
            proxy_password.starts_with("AES:") && proxy_password != "p2",
            "代理密码不得以明文留下：{proxy_password}"
        );
        assert_eq!(fields[1]["host"], "b", "非敏感字段不变");
        let dec = decrypt_network_config(&enc).expect("decrypt chain");
        let parsed: serde_json::Value = serde_json::from_str(&dec).expect("json");
        assert_eq!(parsed[0]["password"], "p1");
        assert_eq!(parsed[1]["auth"]["password"], "p2");
    }

    #[test]
    fn rewrite_tolerates_non_json_and_plaintext() {
        assert_eq!(encrypt_network_config("").expect("empty"), "");
        assert_eq!(encrypt_network_config("  ").expect("blank"), "  ");
        assert_eq!(encrypt_network_config("not-json").expect("bad"), "not-json");
        assert_eq!(decrypt_network_config("not-json").expect("bad"), "not-json");
        assert_eq!(encrypt_network_config("42").expect("scalar"), "42");
        // 明文旧值（未带 AES:）在读取时原样返回
        let plain = r#"{"password":"plain"}"#;
        assert_eq!(decrypt_network_config(plain).expect("plain"), plain);
        // 无敏感键：内容不变（键序被 JSON 规范化，不含敏感键时不比较字符串）
        let no_secret = r#"{"host":"h","port":22}"#;
        let out = encrypt_network_config(no_secret).expect("no secret");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(parsed["host"], "h");
    }

    #[test]
    fn write_and_read_paths_encrypt_roundtrip() {
        let conn = Connection::open_in_memory().expect("memory db");
        let nc = config_of(
            r#"{"host":"jump","username":"u","auth_type":"password","password":"plain"}"#,
        );
        create_network_config(&conn, &nc).expect("create");

        // 库里必须是密文
        let raw: String = conn
            .query_row(
                "SELECT config FROM network_configs WHERE id = ?1",
                params![nc.id],
                |r| r.get(0),
            )
            .expect("read raw");
        assert!(!raw.contains("plain"), "落库不得为明文：{raw}");
        assert!(raw.contains("AES:"));

        // 读路径自动解密（连接解析与管理器回填都走它）
        let got = get_network_config(&conn, &nc.id).expect("get").expect("exists");
        let parsed: serde_json::Value = serde_json::from_str(&got.config).expect("json");
        assert_eq!(parsed["password"], "plain");

        // 一次性迁移：已是密文 → 改动数为 0（幂等）
        assert_eq!(reencrypt_all_network_configs(&conn).expect("migrate"), 0);
    }

    #[test]
    fn reencrypt_migrates_legacy_plaintext_rows() {
        let conn = Connection::open_in_memory().expect("memory db");
        let _ = ensure_table(&conn);
        // 模拟升级前的存量：直接写明文（绕过写路径加密）
        conn.execute(
            "INSERT INTO network_configs (id, name, network_type, config, auth_config_id, origin, source_id, snapshot_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, NULL, ?5, ?6)",
            params![
                "G_net_legacy",
                "legacy",
                "ssh",
                r#"{"host":"j","username":"u","auth_type":"password","password":"legacy-plain"}"#,
                "2026-09-12T00:00:00Z",
                "2026-09-12T00:00:00Z"
            ],
        )
        .expect("insert legacy");

        assert_eq!(
            reencrypt_all_network_configs(&conn).expect("migrate"),
            1,
            "应迁移 1 条明文档案"
        );
        let raw: String = conn
            .query_row(
                "SELECT config FROM network_configs WHERE id = ?1",
                params!["G_net_legacy"],
                |r| r.get(0),
            )
            .expect("read raw");
        assert!(!raw.contains("legacy-plain") && raw.contains("AES:"), "{raw}");

        // 幂等：再跑一次无改动
        assert_eq!(reencrypt_all_network_configs(&conn).expect("migrate again"), 0);
    }

    #[test]
    fn project_migration_skips_missing_db_and_table() {
        let base = std::env::temp_dir().join(format!("rds_netproj_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        // 1) 项目 / 库文件都不存在 → 0，且**不创建任何东西**（读路径无副作用）
        assert_eq!(reencrypt_project_network_configs(&base).expect("missing db"), 0);
        assert!(!base.exists(), "迁移不得创建项目目录");

        // 2) 有库但没有 network_configs 表 → 0，且不建表
        let meta = base.join(crate::persistence::connection_org_store::RS_META_DIR_NAME);
        std::fs::create_dir_all(&meta).expect("mkdir .RSmeta");
        let db_path = meta.join("project.db");
        {
            let conn = Connection::open(&db_path).expect("open db");
            conn.execute_batch("CREATE TABLE connections (id TEXT PRIMARY KEY)")
                .expect("create connections");
        }
        assert_eq!(reencrypt_project_network_configs(&base).expect("no table"), 0);
        {
            let conn = Connection::open(&db_path).expect("reopen");
            let has: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='network_configs'",
                    [],
                    |r| r.get(0),
                )
                .optional()
                .expect("probe");
            assert!(has.is_none(), "不得凭空创建 network_configs 表");
        }

        // 3) 有表 + 明文行 → 迁移 1 条（幂等）
        {
            let conn = Connection::open(&db_path).expect("reopen");
            conn.execute_batch(
                "CREATE TABLE network_configs (id TEXT PRIMARY KEY, name TEXT, network_type TEXT, config TEXT, auth_config_id TEXT);\
                 INSERT INTO network_configs (id, name, network_type, config, auth_config_id) \
                 VALUES ('P_net_1','t','ssh','{\"host\":\"j\",\"password\":\"plain\"}',NULL);",
            )
            .expect("seed plaintext");
        }
        assert_eq!(reencrypt_project_network_configs(&base).expect("migrate"), 1);
        assert_eq!(
            reencrypt_project_network_configs(&base).expect("migrate again"),
            0,
            "幂等：已加密不再重复"
        );
        {
            let conn = Connection::open(&db_path).expect("reopen");
            let raw: String = conn
                .query_row(
                    "SELECT config FROM network_configs WHERE id = 'P_net_1'",
                    [],
                    |r| r.get(0),
                )
                .expect("read raw");
            assert!(!raw.contains("plain") && raw.contains("AES:"), "{raw}");
        }

        let _ = std::fs::remove_dir_all(&base);
    }
}

//! 密钥来源切档编排：旧 key 解密全量 → 新 key 重加密（事务内，失败回滚）。
//!
//! 为什么必须在**事务**里：切档是一次跨三张表的全量改写（认证 / 网络 / 网络资源）。
//! 中途失败若留下「一半新钥、一半旧钥」的库，用户下次启动看到的就是随机几条解不开的
//! 密码。事务回滚保证：要么全换，要么一条都没换（旧密文原样保留，旧 key 仍能解）。
//!
//! 密文格式不变（仍是 base64(nonce‖ct)），只是换了加密它的 key——所以切档**不需要迁移
//! 表结构**，也不需要运行时改解析；解密侧本来就多钥尝试（见 `shared::crypto`）。

use rusqlite::{Connection, params};

use shared::crypto;
use shared::error::{CoreError, StorageError};

use crate::persistence::network_store::SECRET_KEYS;

/// 切档结果（各表改写条数，供 UI 报告「N 条认证 / M 条网络」）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReencryptReport {
    pub auth: usize,
    pub network: usize,
    pub resources: usize,
}

impl ReencryptReport {
    pub fn total(self) -> usize {
        self.auth + self.network + self.resources
    }
}

fn store_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "vault_switch".to_string(),
        operation: op.to_string(),
        reason,
    })
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![name],
        |_| Ok(()),
    )
    .is_ok()
}

/// 递归改写 JSON 里敏感键的密文（`old` 解开 → `new` 重新加密）。
///
/// - 未带 `AES:` 的明文值：**先加密**（与 `network_store` 的迁移口径一致）；
/// - 旧钥解不开的值：返回错误，让整次切档回滚——静默跳过等于制造「看着像成功」的假象。
fn rewrite_node(
    node: &mut serde_json::Value,
    old: &[[u8; 32]],
    new: &[u8; 32],
) -> Result<(), CoreError> {
    match node {
        serde_json::Value::Object(map) => {
            for key in SECRET_KEYS {
                let Some(serde_json::Value::String(v)) = map.get_mut(key) else {
                    continue;
                };
                if v.is_empty() {
                    continue;
                }
                let plain = match v.strip_prefix("AES:") {
                    Some(raw) => crypto::decrypt_password_with_keys(old, raw).map_err(|_| {
                        store_err(
                            "reencrypt_secret",
                            format!("旧密钥解不开字段 `{key}`（密文可能来自已丢失的档位）"),
                        )
                    })?,
                    None => v.clone(),
                };
                *v = format!("AES:{}", crypto::encrypt_password_with_key(new, &plain)?);
            }
            for (_, child) in map.iter_mut() {
                rewrite_node(child, old, new)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                rewrite_node(item, old, new)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn rewrite_json(raw: &str, old: &[[u8; 32]], new: &[u8; 32]) -> Result<String, CoreError> {
    if raw.trim().is_empty() {
        return Ok(raw.to_string());
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        // 坏 JSON：原样保留（存储层不因格式问题拒写，也不会因它让整次切档失败）
        return Ok(raw.to_string());
    };
    if !value.is_object() && !value.is_array() {
        return Ok(raw.to_string());
    }
    rewrite_node(&mut value, old, new)?;
    Ok(value.to_string())
}

/// 单表全量重加密（表不存在 → 计 0）。
fn reencrypt_table(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    old: &[[u8; 32]],
    new: &[u8; 32],
) -> Result<usize, CoreError> {
    if !table_exists(tx, table) {
        return Ok(0);
    }
    let sql = format!("SELECT id, {column} FROM {table}");
    let rows: Vec<(String, String)> = {
        let mut stmt = tx
            .prepare(&sql)
            .map_err(|e| store_err("prepare_reencrypt", e.to_string()))?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| store_err("query_reencrypt", e.to_string()))?
            .filter_map(|r| r.ok())
            .collect()
    };
    let update = format!("UPDATE {table} SET {column} = ?1 WHERE id = ?2");
    let mut changed = 0usize;
    for (id, raw) in rows {
        let next = rewrite_json(&raw, old, new)?;
        if next != raw {
            tx.execute(&update, params![next, id])
                .map_err(|e| store_err("update_reencrypt", e.to_string()))?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 全局库切档重加密：认证 / 网络档案 / 网络资源三张表，**单事务**。
pub fn reencrypt_global(
    conn: &mut Connection,
    new_key: &[u8; 32],
) -> Result<ReencryptReport, CoreError> {
    // 旧钥集合必须在**任何写之前**固定：切档过程中进程内缓存可能已变。
    let old = crypto_old_keys();
    reencrypt_global_with_keys(conn, &old, new_key)
}

/// 显式旧钥版（切档入口用 [`reencrypt_global`]；测试与「已知旧钥」场景用本函数）。
pub fn reencrypt_global_with_keys(
    conn: &mut Connection,
    old: &[[u8; 32]],
    new_key: &[u8; 32],
) -> Result<ReencryptReport, CoreError> {
    let tx = conn
        .transaction()
        .map_err(|e| store_err("begin_switch", e.to_string()))?;
    let report = ReencryptReport {
        auth: reencrypt_table(&tx, "auth_configs", "auth_data", old, new_key)?,
        network: reencrypt_table(&tx, "network_configs", "config", old, new_key)?,
        resources: reencrypt_table(&tx, "net_resources", "props", old, new_key)?,
    };
    tx.commit()
        .map_err(|e| store_err("commit_switch", e.to_string()))?;
    Ok(report)
}

/// 项目库切档重加密（表可能不存在；不存在即跳过，不建库不建表）。
pub fn reencrypt_project(
    conn: &mut Connection,
    new_key: &[u8; 32],
) -> Result<ReencryptReport, CoreError> {
    let old = crypto_old_keys();
    let tx = conn
        .transaction()
        .map_err(|e| store_err("begin_switch_project", e.to_string()))?;
    let report = ReencryptReport {
        auth: reencrypt_table(&tx, "auth_configs", "auth_data", &old, new_key)?,
        network: reencrypt_table(&tx, "network_configs", "config", &old, new_key)?,
        resources: reencrypt_table(&tx, "net_resources", "props", &old, new_key)?,
    };
    tx.commit()
        .map_err(|e| store_err("commit_switch_project", e.to_string()))?;
    Ok(report)
}

/// 旧钥集合 = 现行三档的可用钥（机器档 + 系统档 + 主密码已解锁档）。
///
/// v1 的固定盐兼容钥只在 `shared::crypto::decrypt_password` 内部追加，本模块拿不到——
/// 如果库里真有 v1 遗留密文，切档会在该行报「旧密钥解不开」并整体回滚（宁可失败，
/// 也不留半新半旧的库）。这是刻意的：切档前先用「连接测试」验证能解才是正路。
fn crypto_old_keys() -> Vec<[u8; 32]> {
    shared::vault_source::candidates()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE auth_configs (id TEXT PRIMARY KEY, auth_data TEXT NOT NULL);
             CREATE TABLE network_configs (id TEXT PRIMARY KEY, config TEXT NOT NULL);
             CREATE TABLE net_resources (id TEXT PRIMARY KEY, props TEXT NOT NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn reencrypt_moves_all_three_tables_to_new_key() -> Result<(), CoreError> {
        let old_key = [1u8; 32];
        let new_key = [2u8; 32];
        let mut conn = setup();
        let secret = format!(
            "AES:{}",
            crypto::encrypt_password_with_key(&old_key, "s3cret")?
        );
        let auth = format!(
            r#"{{"username":"u","password":"{secret}"}}"#
        );
        let net = format!(
            r#"{{"hops":[{{"type":"ssh","host":"h","password":"{secret}"}}]}}"#
        );
        let res = format!(r#"{{"kind":"ssh","password":"{secret}"}}"#);
        conn.execute(
            "INSERT INTO auth_configs VALUES ('a1', ?1)",
            params![auth],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO network_configs VALUES ('n1', ?1)",
            params![net],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO net_resources VALUES ('r1', ?1)",
            params![res],
        )
        .unwrap();

        let report = reencrypt_global_with_keys(&mut conn, &[old_key], &new_key)?;
        assert_eq!(report, ReencryptReport { auth: 1, network: 1, resources: 1 });

        let raw: String = conn
            .query_row("SELECT auth_data FROM auth_configs WHERE id='a1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let enc = value["password"].as_str().unwrap();
        let raw_ct = enc.strip_prefix("AES:").unwrap();
        assert_eq!(
            crypto::decrypt_password_with_keys(&[new_key], raw_ct).unwrap(),
            "s3cret",
            "新钥必须能解开"
        );
        assert!(
            crypto::decrypt_password_with_keys(&[old_key], raw_ct).is_err(),
            "旧钥不得再能解开（否则等于没换钥）"
        );
        Ok(())
    }

    #[test]
    fn missing_tables_are_skipped_not_created() -> Result<(), CoreError> {
        let mut conn = Connection::open_in_memory().unwrap();
        let report = reencrypt_global(&mut conn, &[3u8; 32])?;
        assert_eq!(report, ReencryptReport::default());
        assert!(!table_exists(&conn, "auth_configs"), "不得顺手建表");
        Ok(())
    }

    #[test]
    fn undecryptable_secret_fails_the_whole_switch() {
        let mut conn = setup();
        let foreign = format!("AES:{}", crypto::encrypt_password_with_key(&[9u8; 32], "x").unwrap());
        let auth = format!(r#"{{"password":"{foreign}"}}"#);
        conn.execute("INSERT INTO auth_configs VALUES ('a1', ?1)", params![auth])
            .unwrap();
        let err = reencrypt_global_with_keys(&mut conn, &[[1u8; 32]], &[2u8; 32]).expect_err("未知旧钥必须报错");
        assert!(err.to_string().contains("旧密钥解不开"), "{err}");
        // 事务回滚：行保持原样
        let raw: String = conn
            .query_row("SELECT auth_data FROM auth_configs WHERE id='a1'", [], |r| r.get(0))
            .unwrap();
        assert!(raw.contains(&foreign), "回滚后旧密文必须原样保留");
    }

    #[test]
    fn plaintext_values_are_encrypted_on_switch() -> Result<(), CoreError> {
        let mut conn = setup();
        conn.execute(
            "INSERT INTO network_configs VALUES ('n1', '{\"password\":\"legacy-plain\"}')",
            [],
        )
        .unwrap();
        let new_key = [4u8; 32];
        let report = reencrypt_global_with_keys(&mut conn, &[], &new_key)?;
        assert_eq!(report.network, 1);
        let raw: String = conn
            .query_row("SELECT config FROM network_configs WHERE id='n1'", [], |r| r.get(0))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let ct = v["password"].as_str().unwrap().strip_prefix("AES:").unwrap();
        assert_eq!(crypto::decrypt_password_with_keys(&[new_key], ct).unwrap(), "legacy-plain");
        Ok(())
    }
}

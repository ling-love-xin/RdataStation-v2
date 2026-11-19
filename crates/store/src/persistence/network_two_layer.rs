//! 网络两层模型：`net_resources`（最小单位）+ `chains`（资源引用序列）。
//!
//! 设计依据：`docs/architecture/connection/prototypes/manager-network.html`（Tab5 表模型）
//! 与架构决策 #122。三个口径要点：
//!
//! 1. **归一化视图，不是新执行语义**：`props` 里存的就是「单跳原生配置 JSON」，与旧
//!    `network_configs.config` 同形；[`expand_chain_config`] 把链按 hop 顺序拼回旧格式，
//!    运行时（`parse_network_config_json`）拿到的数组与改造前逐字等价。
//! 2. **凭据只在 `auth_configs`**：资源只留 `auth_config_id`；存量里内嵌的 `password`
//!    / `passphrase` 仍留在 props 内（加密落库），由运行时按旧路径注入，不做静默丢失。
//! 3. **存量迁移幂等**：资源 / 链沿用旧档案的 id，连接上的 `network_config_id` 不需要
//!    改写；重复执行只会补缺，不会重复建记录。
//!
//! 旧表 `network_configs` 在过渡期内保留只读：读路径先查两表，命中即用；未命中回落旧表
//! （见 `network_store`），验证通过后再删旧表。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, StorageError};

use super::network_store::{decrypt_network_config, encrypt_network_config};

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "network_two_layer".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 网络资源（最小单位）。`kind` ∈ `ssh | http | socks | ssl`。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NetResource {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub host: String,
    pub port: i64,
    pub auth_config_id: Option<String>,
    /// 单跳原生配置 JSON（与旧 `network_configs.config` 同形；写入前加密敏感键）。
    pub props: String,
    pub origin: Option<String>,
    pub source_id: Option<String>,
    pub snapshot_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 协议链（组合）。`hops` 是**有序资源 id 数组**的 JSON。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NetChain {
    pub id: String,
    pub name: String,
    pub hops: String,
    pub timeout_s: Option<i64>,
    pub origin: Option<String>,
    pub source_id: Option<String>,
    pub snapshot_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 网络节点类型（连接上 `network_config_id` 指向的到底是资源还是链）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetNodeKind {
    Resource,
    Chain,
}

/// 确保两表存在（回退修复；与迁移 037 / 项目库 033 同构）。
fn ensure_tables(conn: &Connection) -> Result<(), CoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS net_resources (
            id             TEXT PRIMARY KEY,
            name           TEXT NOT NULL,
            kind           TEXT NOT NULL,
            host           TEXT NOT NULL DEFAULT '',
            port           INTEGER NOT NULL DEFAULT 0,
            auth_config_id TEXT,
            props          TEXT NOT NULL DEFAULT '{}',
            origin         TEXT,
            source_id      TEXT,
            snapshot_at    TEXT,
            created_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at     TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS chains (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            hops       TEXT NOT NULL DEFAULT '[]',
            timeout_s  INTEGER,
            origin     TEXT,
            source_id  TEXT,
            snapshot_at TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .map_err(|e| storage_err("ensure_tables", e.to_string()))
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// 旧 `network_type` → 新 `kind`（`proxy` 归一到 `http`，链不是资源）。
pub fn legacy_kind_of(network_type: &str) -> Option<&'static str> {
    match network_type.trim().to_ascii_lowercase().as_str() {
        "ssh" | "ssh_tunnel" => Some("ssh"),
        "proxy" | "http" | "http_proxy" | "http_proxy_tunnel" => Some("http"),
        "socks" | "socks5" | "socks_proxy" => Some("socks"),
        "ssl" | "tls" => Some("ssl"),
        _ => None,
    }
}

/// `kind` → 链 hop 的 serde tag（`ChainHop` 的 `type` 字段）。
pub fn hop_type_of(kind: &str) -> Option<&'static str> {
    match kind {
        "ssh" => Some("ssh"),
        "ssl" => Some("ssl"),
        "http" => Some("http_proxy"),
        "socks" => Some("socks_proxy"),
        _ => None,
    }
}

// ===== 资源 CRUD =====

pub fn create_resource(conn: &Connection, r: &NetResource) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    let props = encrypt_network_config(&r.props)?;
    conn.execute(
        "INSERT INTO net_resources (id, name, kind, host, port, auth_config_id, props, origin, source_id, snapshot_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            r.id,
            r.name,
            r.kind,
            r.host,
            r.port,
            r.auth_config_id,
            props,
            r.origin,
            r.source_id,
            r.snapshot_at,
            r.created_at,
            r.updated_at
        ],
    )
    .map_err(|e| storage_err("create_resource", e.to_string()))?;
    Ok(())
}

pub fn list_resources(
    conn: &Connection,
    kind: Option<&str>,
) -> Result<Vec<NetResource>, CoreError> {
    ensure_tables(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, kind, host, port, auth_config_id, props, origin, source_id, snapshot_at, created_at, updated_at
             FROM net_resources ORDER BY kind, name",
        )
        .map_err(|e| storage_err("prepare_list_resources", e.to_string()))?;
    let mut items: Vec<NetResource> = stmt
        .query_map([], |row| {
            Ok(NetResource {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                host: row.get(3)?,
                port: row.get(4)?,
                auth_config_id: row.get(5)?,
                props: row.get(6)?,
                origin: row.get(7)?,
                source_id: row.get(8)?,
                snapshot_at: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|e| storage_err("query_resources", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    for it in &mut items {
        if let Ok(plain) = decrypt_network_config(&it.props) {
            it.props = plain;
        }
    }
    if let Some(k) = kind {
        let k = k.trim().to_ascii_lowercase();
        items.retain(|it| it.kind.eq_ignore_ascii_case(&k));
    }
    Ok(items)
}

pub fn get_resource(conn: &Connection, id: &str) -> Result<Option<NetResource>, CoreError> {
    ensure_tables(conn)?;
    let found = conn
        .query_row(
            "SELECT id, name, kind, host, port, auth_config_id, props, origin, source_id, snapshot_at, created_at, updated_at
             FROM net_resources WHERE id = ?1",
            params![id],
            |row| {
                Ok(NetResource {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    host: row.get(3)?,
                    port: row.get(4)?,
                    auth_config_id: row.get(5)?,
                    props: row.get(6)?,
                    origin: row.get(7)?,
                    source_id: row.get(8)?,
                    snapshot_at: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|e| storage_err("get_resource", e.to_string()))?;
    Ok(found.map(|mut r| {
        if let Ok(plain) = decrypt_network_config(&r.props) {
            r.props = plain;
        }
        r
    }))
}

pub fn update_resource(conn: &Connection, r: &NetResource) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    let props = encrypt_network_config(&r.props)?;
    let rows = conn
        .execute(
            "UPDATE net_resources SET name = ?1, kind = ?2, host = ?3, port = ?4, auth_config_id = ?5, props = ?6, updated_at = ?7 WHERE id = ?8",
            params![r.name, r.kind, r.host, r.port, r.auth_config_id, props, r.updated_at, r.id],
        )
        .map_err(|e| storage_err("update_resource", e.to_string()))?;
    if rows == 0 {
        return Err(storage_err(
            "update_resource",
            format!("net resource not found: {}", r.id),
        ));
    }
    Ok(())
}

pub fn delete_resource(conn: &Connection, id: &str) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    conn.execute("DELETE FROM net_resources WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_resource", e.to_string()))?;
    Ok(())
}

// ===== 链 CRUD =====

pub fn create_chain(conn: &Connection, c: &NetChain) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    conn.execute(
        "INSERT INTO chains (id, name, hops, timeout_s, origin, source_id, snapshot_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            c.id,
            c.name,
            c.hops,
            c.timeout_s,
            c.origin,
            c.source_id,
            c.snapshot_at,
            c.created_at,
            c.updated_at
        ],
    )
    .map_err(|e| storage_err("create_chain", e.to_string()))?;
    Ok(())
}

fn row_to_chain(row: &rusqlite::Row<'_>) -> rusqlite::Result<NetChain> {
    Ok(NetChain {
        id: row.get(0)?,
        name: row.get(1)?,
        hops: row.get(2)?,
        timeout_s: row.get(3)?,
        origin: row.get(4)?,
        source_id: row.get(5)?,
        snapshot_at: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn list_chains(conn: &Connection) -> Result<Vec<NetChain>, CoreError> {
    ensure_tables(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, hops, timeout_s, origin, source_id, snapshot_at, created_at, updated_at
             FROM chains ORDER BY name",
        )
        .map_err(|e| storage_err("prepare_list_chains", e.to_string()))?;
    let rows = stmt
        .query_map([], row_to_chain)
        .map_err(|e| storage_err("query_chains", e.to_string()))?;
    let mut items = Vec::new();
    for row in rows {
        if let Ok(c) = row {
            items.push(c);
        }
    }
    Ok(items)
}

pub fn get_chain(conn: &Connection, id: &str) -> Result<Option<NetChain>, CoreError> {
    ensure_tables(conn)?;
    conn.query_row(
        "SELECT id, name, hops, timeout_s, origin, source_id, snapshot_at, created_at, updated_at
         FROM chains WHERE id = ?1",
        params![id],
        row_to_chain,
    )
    .optional()
    .map_err(|e| storage_err("get_chain", e.to_string()))
}

pub fn update_chain(conn: &Connection, c: &NetChain) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    let rows = conn
        .execute(
            "UPDATE chains SET name = ?1, hops = ?2, timeout_s = ?3, updated_at = ?4 WHERE id = ?5",
            params![c.name, c.hops, c.timeout_s, c.updated_at, c.id],
        )
        .map_err(|e| storage_err("update_chain", e.to_string()))?;
    if rows == 0 {
        return Err(storage_err(
            "update_chain",
            format!("chain not found: {}", c.id),
        ));
    }
    Ok(())
}

pub fn delete_chain(conn: &Connection, id: &str) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    conn.execute("DELETE FROM chains WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete_chain", e.to_string()))?;
    Ok(())
}

/// 解析 `hops` 里的资源 id 列表（坏 JSON → 空列表，不阻断读路径）。
pub fn chain_hop_ids(c: &NetChain) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&c.hops).unwrap_or_default()
}

/// 某个 id 是资源、链，还是两表都没有。
pub fn node_kind(conn: &Connection, id: &str) -> Result<Option<NetNodeKind>, CoreError> {
    ensure_tables(conn)?;
    if get_resource(conn, id)?.is_some() {
        return Ok(Some(NetNodeKind::Resource));
    }
    if get_chain(conn, id)?.is_some() {
        return Ok(Some(NetNodeKind::Chain));
    }
    Ok(None)
}

/// 链 → 旧格式 hop 数组 JSON（每跳 = `{type, …props}`；敏感键按需经 auth 档案注入）。
///
/// 任何一跳解析失败 → 返回 `None`：调用方按「链不可用」处理（与旧实现解析失败同路径），
/// 不返回半条链——半截代理链比明确失败更难排查。
pub fn expand_chain_config(conn: &Connection, chain_id: &str) -> Result<Option<String>, CoreError> {
    ensure_tables(conn)?;
    let Some(chain) = get_chain(conn, chain_id)? else {
        return Ok(None);
    };
    let ids = chain_hop_ids(&chain);
    if ids.is_empty() {
        return Ok(None);
    }
    let mut hops = Vec::with_capacity(ids.len());
    for rid in ids {
        let Some(res) = get_resource(conn, &rid)? else {
            return Ok(None);
        };
        let Some(tag) = hop_type_of(&res.kind) else {
            return Ok(None);
        };
        let mut value: serde_json::Value = serde_json::from_str(&res.props)
            .map_err(|e| storage_err("expand_chain_config", e.to_string()))?;
        let Some(obj) = value.as_object_mut() else {
            return Ok(None);
        };
        obj.insert(
            "type".to_string(),
            serde_json::Value::String(tag.to_string()),
        );
        hops.push(value);
    }
    let json = serde_json::Value::Array(hops).to_string();
    Ok(Some(json))
}

// ===== 存量迁移 =====

/// 把旧 `network_configs` 存量归一化到两表（**幂等**；返回新建记录数）。
///
/// - 单跳档案 → 资源 1:1（**沿用旧 id**，连接上的引用无需改写）；
/// - `chain` 档案 → 逐跳按 `(kind, host, port)` 复用既有资源、未命中建资源，
///   链记录沿用旧 id、`hops` 写资源 id 序列。
pub fn migrate_legacy_network_configs(conn: &Connection) -> Result<usize, CoreError> {
    ensure_tables(conn)?;
    let legacy: Vec<(String, Option<String>, String, String, Option<String>)> = {
        let mut stmt = conn
            .prepare("SELECT id, name, network_type, config, auth_config_id FROM network_configs")
            .map_err(|e| storage_err("prepare_legacy_scan", e.to_string()))?;
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| storage_err("scan_legacy_network_configs", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    let mut created = 0usize;
    let now = now_iso();
    for (id, name, net_type, config_raw, auth_id) in legacy {
        let config = decrypt_network_config(&config_raw).unwrap_or(config_raw);
        if net_type.eq_ignore_ascii_case("chain") {
            if get_chain(conn, &id)?.is_some() {
                continue;
            }
            let hops: Vec<serde_json::Value> = serde_json::from_str(&config).unwrap_or_default();
            let mut hop_ids = Vec::with_capacity(hops.len());
            for (idx, hop) in hops.iter().enumerate() {
                let tag = hop.get("type").and_then(|v| v.as_str()).unwrap_or("");
                let Some(kind) = kind_of_hop_tag(tag) else {
                    continue;
                };
                let host = hop.get("host").and_then(|v| v.as_str()).unwrap_or("");
                let port = hop.get("port").and_then(|v| v.as_i64()).unwrap_or(0);
                if let Some(existing) = find_resource(conn, kind, host, port)? {
                    hop_ids.push(existing);
                    continue;
                }
                let rid = format!("NR_{}_{}", id, idx);
                let r = NetResource {
                    id: rid.clone(),
                    name: format!("{}:{}", host, port),
                    kind: kind.to_string(),
                    host: host.to_string(),
                    port,
                    auth_config_id: None,
                    props: strip_type(hop).to_string(),
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                };
                create_resource(conn, &r)?;
                created += 1;
                hop_ids.push(rid);
            }
            create_chain(
                conn,
                &NetChain {
                    id,
                    name: name.unwrap_or_else(|| "chain".to_string()),
                    hops: serde_json::to_string(&hop_ids)
                        .map_err(|e| storage_err("migrate_chain_hops", e.to_string()))?,
                    timeout_s: None,
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                },
            )?;
            created += 1;
            continue;
        }
        let Some(kind) = legacy_kind_of(&net_type) else {
            continue;
        };
        if get_resource(conn, &id)?.is_some() {
            continue;
        }
        let host = serde_json::from_str::<serde_json::Value>(&config)
            .ok()
            .and_then(|v| v.get("host").and_then(|h| h.as_str()).map(String::from))
            .unwrap_or_default();
        let port = serde_json::from_str::<serde_json::Value>(&config)
            .ok()
            .and_then(|v| v.get("port").and_then(|p| p.as_i64()))
            .unwrap_or(0);
        create_resource(
            conn,
            &NetResource {
                id,
                name: name.unwrap_or_else(|| format!("{}:{}", host, port)),
                kind: kind.to_string(),
                host,
                port,
                auth_config_id: auth_id,
                props: config,
                origin: None,
                source_id: None,
                snapshot_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        )?;
        created += 1;
    }
    Ok(created)
}

fn kind_of_hop_tag(tag: &str) -> Option<&'static str> {
    match tag {
        "ssh" => Some("ssh"),
        "ssl" => Some("ssl"),
        "http_proxy" | "http" | "proxy" => Some("http"),
        "socks_proxy" | "socks" => Some("socks"),
        _ => None,
    }
}

fn find_resource(
    conn: &Connection,
    kind: &str,
    host: &str,
    port: i64,
) -> Result<Option<String>, CoreError> {
    conn.query_row(
        "SELECT id FROM net_resources WHERE kind = ?1 AND host = ?2 AND port = ?3 LIMIT 1",
        params![kind, host, port],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| storage_err("find_resource", e.to_string()))
}

fn strip_type(hop: &serde_json::Value) -> String {
    let mut v = hop.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("type");
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("memory db");
        c.execute_batch(
            "CREATE TABLE network_configs (
                id TEXT PRIMARY KEY, name TEXT, network_type TEXT NOT NULL,
                config TEXT NOT NULL, auth_config_id TEXT, origin TEXT, source_id TEXT,
                snapshot_at TEXT, created_at TEXT, updated_at TEXT
            );",
        )
        .expect("legacy table");
        c
    }

    fn legacy(conn: &Connection, id: &str, name: &str, t: &str, config: &str) {
        conn.execute(
            "INSERT INTO network_configs (id, name, network_type, config, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'x', 'x')",
            params![id, name, t, config],
        )
        .expect("insert legacy");
    }

    #[test]
    fn legacy_single_hop_becomes_resource_keeping_id() {
        let c = conn();
        legacy(
            &c,
            "G_net_ssh1",
            "jump-a",
            "ssh",
            r#"{"host":"10.0.0.1","port":22,"username":"deploy","password":"pw"}"#,
        );
        let created = migrate_legacy_network_configs(&c).expect("migrate");
        assert_eq!(created, 1);
        let r = get_resource(&c, "G_net_ssh1").expect("get").expect("exists");
        assert_eq!(r.kind, "ssh");
        assert_eq!(r.host, "10.0.0.1");
        assert_eq!(r.port, 22);
        // 幂等
        assert_eq!(migrate_legacy_network_configs(&c).expect("again"), 0);
    }

    #[test]
    fn legacy_chain_dedupes_shared_resources_and_expands_back() {
        let c = conn();
        // 单跳资源先存在（同一跳板机）
        legacy(
            &c,
            "G_net_bastion",
            "bastion",
            "ssh",
            r#"{"type":"ssh","host":"jump","port":22,"username":"u","password":"p1"}"#,
        );
        // 两条链都经过同一跳板机
        let chain_a = r#"[{"type":"ssh","host":"jump","port":22,"username":"u","password":"p1"},{"type":"http_proxy","host":"proxy","port":8080,"auth":{"username":"pu","password":"pp"}}]"#;
        legacy(&c, "G_net_chainA", "chainA", "chain", chain_a);
        let chain_b = r#"[{"type":"ssh","host":"jump","port":22,"username":"u","password":"p1"}]"#;
        legacy(&c, "G_net_chainB", "chainB", "chain", chain_b);

        let created = migrate_legacy_network_configs(&c).expect("migrate");
        // bastion 资源 + chainA 的新 proxy 资源 + chainA 链 + chainB 链 = 4
        assert_eq!(created, 4, "应建 2 条资源 + 2 条链，其中 bastion 复用");

        let res = list_resources(&c, None).expect("list");
        assert_eq!(res.len(), 2, "同一跳板机不得重复建资源: {res:?}");
        let chains = list_chains(&c).expect("chains");
        assert_eq!(chains.len(), 2);
        let a = get_chain(&c, "G_net_chainA").expect("get").expect("exists");
        assert_eq!(chain_hop_ids(&a).len(), 2);

        // 展开回旧格式：与原始 JSON 语义等价（逐跳 type / host / 密码）
        let expanded = expand_chain_config(&c, "G_net_chainA")
            .expect("expand")
            .expect("json");
        let v: serde_json::Value = serde_json::from_str(&expanded).expect("json");
        assert_eq!(v[0]["type"], "ssh");
        assert_eq!(v[0]["host"], "jump");
        assert_eq!(v[1]["type"], "http_proxy");
        assert_eq!(v[1]["host"], "proxy");
        assert_eq!(v[1]["auth"]["password"], "pp", "密码应保留可解密");

        // 幂等：再迁不新增
        assert_eq!(migrate_legacy_network_configs(&c).expect("again"), 0);
    }

    #[test]
    fn node_kind_distinguishes_resource_and_chain() {
        let c = conn();
        let r = NetResource {
            id: "NR_1".into(),
            name: "r".into(),
            kind: "http".into(),
            host: "p".into(),
            port: 8080,
            auth_config_id: None,
            props: "{}".into(),
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "x".into(),
            updated_at: "x".into(),
        };
        create_resource(&c, &r).expect("create");
        let ch = NetChain {
            id: "C_1".into(),
            name: "c".into(),
            hops: r#"["NR_1"]"#.into(),
            timeout_s: Some(30),
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "x".into(),
            updated_at: "x".into(),
        };
        create_chain(&c, &ch).expect("chain");
        assert_eq!(node_kind(&c, "NR_1").expect("k"), Some(NetNodeKind::Resource));
        assert_eq!(node_kind(&c, "C_1").expect("k"), Some(NetNodeKind::Chain));
        assert_eq!(node_kind(&c, "nope").expect("k"), None);
        // 展开结果按 JSON 值比较（键序不属于语义）
        let expanded = expand_chain_config(&c, "C_1").expect("expand").expect("json");
        let v: serde_json::Value = serde_json::from_str(&expanded).expect("json");
        assert_eq!(v[0]["type"], "http_proxy");
    }

    #[test]
    fn resource_props_secrets_are_encrypted_at_rest() {
        let c = conn();
        let r = NetResource {
            id: "NR_s".into(),
            name: "s".into(),
            kind: "ssh".into(),
            host: "h".into(),
            port: 22,
            auth_config_id: None,
            props: r#"{"host":"h","username":"u","password":"plain"}"#.into(),
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "x".into(),
            updated_at: "x".into(),
        };
        create_resource(&c, &r).expect("create");
        let raw: String = c
            .query_row(
                "SELECT props FROM net_resources WHERE id = 'NR_s'",
                [],
                |row| row.get(0),
            )
            .expect("raw");
        assert!(!raw.contains("plain") && raw.contains("AES:"), "{raw}");
        let back = get_resource(&c, "NR_s").expect("get").expect("exists");
        assert!(back.props.contains("plain"), "读路径应解密");
    }

    #[test]
    fn expand_returns_none_for_missing_hop() {
        let c = conn();
        let ch = NetChain {
            id: "C_bad".into(),
            name: "bad".into(),
            hops: r#"["NR_missing"]"#.into(),
            timeout_s: None,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "x".into(),
            updated_at: "x".into(),
        };
        create_chain(&c, &ch).expect("chain");
        assert_eq!(
            expand_chain_config(&c, "C_bad").expect("expand"),
            None,
            "缺跳 → 整链不可用（不返回半截链）"
        );
    }
}

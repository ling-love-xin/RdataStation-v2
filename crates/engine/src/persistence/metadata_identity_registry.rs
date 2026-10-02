//! 连接 ID → 身份指纹 进程内注册表（**接线桥**）。
//!
//! 背景：L2 元数据缓存按连接 ID 分文件（`conn_{id}.sqlite`），指向同一物理库的
//! 多条连接会各自重建缓存。`metadata_identity` 提供纯函数（规范化串 + 指纹），
//! 但缓存访问路径（`database::NavCache` / `engine::MetadataCacheManager`）只有
//! `conn_id`，拿不到地址与主体——本模块在**有连接记录的路径**（保存 / 连接建立 /
//! 删除）登记 `conn_id → 指纹`，缓存路径只查表即可决定文件名。
//!
//! 域隔离（用户约束，2026-10-02）：**全局与项目不互通**——指纹只在作用域域内共享，
//! 全局连接（`G_` / `GP_` 的全局侧）在全局域，项目连接（`P_` / `GP_` 的项目侧）
//! 在项目域。域由缓存**存储目录**保证（`MetadataCacheManager::build_metadata_path`：
//! 全局 → `{system}/global_metadata/`，项目 → `{project}/meta/connection_metadata/`），
//! 注册表只记 `conn_id → 指纹`，不跨域合并——同一物理库在全局域与项目域各有一份
//! 缓存文件，互不相通；`GP_` 连接走项目域（`NavSource::from_conn_id` 判定）。
//!
//! 指纹相同的两条连接（改名 / 改密码 / 加 SSL 参数）共享同一 `meta_{fp}.sqlite`，
//! 正是“同一物理库只扫一份元数据”的目的。不可共享（内存库 / 地址空）时
//! `fingerprint()` 为 `None`——注册表不登记，缓存路径回退 `conn_{safe}.sqlite`
//! （每连接独立，不共享）。
//!
//! 生命周期：连接保存 / 建立 → `register_identity`；连接删除 → `unregister_identity`。
//! 注册表**不落盘**（索引表 `metadata_cache_index` 的持久化由「缓存管理」轮落地）。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use super::metadata_identity::ConnectionIdentity;

/// 单条注册记录：指纹与可读规范化串（`None` 表示不可共享，不参与共享）。
#[derive(Debug, Clone)]
struct Registered {
    fingerprint: Option<String>,
    canonical: Option<String>,
}

/// 进程级注册表（`conn_id → Registered`）。
struct Registry {
    entries: RwLock<HashMap<String, Registered>>,
}

impl Registry {
    fn instance() -> &'static Self {
        static REGISTRY: OnceLock<Registry> = OnceLock::new();
        REGISTRY.get_or_init(|| Registry {
            entries: RwLock::new(HashMap::new()),
        })
    }

    fn register(&self, conn_id: &str, identity: &ConnectionIdentity) {
        let entry = Registered {
            fingerprint: identity.fingerprint(),
            canonical: identity.canonical_string(),
        };
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(conn_id.to_string(), entry);
        }
    }

    /// 指纹查询：未注册 / 不可共享 → `None`（缓存路径回退 conn_id 分文件）。
    fn fingerprint(&self, conn_id: &str) -> Option<String> {
        let entries = self.entries.read().ok()?;
        entries.get(conn_id).and_then(|e| e.fingerprint.clone())
    }

    fn canonical(&self, conn_id: &str) -> Option<String> {
        let entries = self.entries.read().ok()?;
        entries.get(conn_id).and_then(|e| e.canonical.clone())
    }

    fn unregister(&self, conn_id: &str) {
        if let Ok(mut entries) = self.entries.write() {
            entries.remove(conn_id);
        }
    }

    fn len(&self) -> usize {
        self.entries.read().map(|e| e.len()).unwrap_or(0)
    }
}

/// 登记 `conn_id → 指纹`（连接保存 / 连接建立成功后调用；幂等，覆盖旧值）。
pub fn register_identity(conn_id: &str, identity: &ConnectionIdentity) {
    Registry::instance().register(conn_id, identity);
}

/// 注销某连接的指纹登记（连接删除时调用；不影响已落盘的缓存文件）。
pub fn unregister_identity(conn_id: &str) {
    Registry::instance().unregister(conn_id);
}

/// 查询某连接的指纹（缓存路径用；未登记 / 不可共享 → `None`）。
pub fn identity_fingerprint(conn_id: &str) -> Option<String> {
    Registry::instance().fingerprint(conn_id)
}

/// 查询某连接的可读规范化串（缓存管理 / 诊断展示用）。
pub fn identity_canonical(conn_id: &str) -> Option<String> {
    Registry::instance().canonical(conn_id)
}

/// 当前登记条数（测试 / 诊断）。
pub fn registered_identity_count() -> usize {
    Registry::instance().len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::metadata_identity::{CacheAuthority, NetworkAuthority};

    fn mysql_identity(host: &str, database: &str, principal: &str) -> ConnectionIdentity {
        ConnectionIdentity::new(
            "mysql",
            CacheAuthority::Network(NetworkAuthority {
                host: host.to_string(),
                port: Some(3306),
                default_port: None,
                database: Some(database.to_string()),
                schema: None,
            }),
            Some(principal),
        )
    }

    #[test]
    fn register_then_fingerprint_roundtrip() {
        let id = mysql_identity("db.internal", "orders", "app_ro");
        let fp = id.fingerprint().expect("可共享");

        register_identity("G_conn_1", &id);
        assert_eq!(identity_fingerprint("G_conn_1"), Some(fp.clone()));
        assert_eq!(
            identity_canonical("G_conn_1"),
            Some("v1|mysql|net:db.internal:3306/orders/-|user:app_ro".to_string())
        );

        unregister_identity("G_conn_1");
        assert_eq!(identity_fingerprint("G_conn_1"), None);
    }

    #[test]
    fn same_physical_db_connections_share_fingerprint() {
        // 改名 / 改密码 / 加参数都不改变身份 → 同一指纹（共享缓存文件）
        let a = mysql_identity("db.internal", "orders", "app_ro");
        let b = ConnectionIdentity::new(
            "mysql",
            CacheAuthority::Network(NetworkAuthority {
                host: "DB.Internal".to_string(), // 主机大小写折叠
                port: None,                      // 回落默认端口
                default_port: Some(3306),
                database: Some("orders".to_string()),
                schema: None,
            }),
            Some("app_ro"),
        );
        assert_eq!(a.fingerprint(), b.fingerprint());

        register_identity("G_conn_a", &a);
        register_identity("G_conn_b", &b);
        assert_eq!(
            identity_fingerprint("G_conn_a"),
            identity_fingerprint("G_conn_b")
        );

        unregister_identity("G_conn_a");
        unregister_identity("G_conn_b");
        assert_eq!(identity_fingerprint("G_conn_a"), None);
    }

    #[test]
    fn unshareable_identity_is_not_registered() {
        let mem = ConnectionIdentity::new(
            "sqlite",
            CacheAuthority::File { path: ":memory:".to_string() },
            None,
        );
        assert!(mem.fingerprint().is_none());

        register_identity("P_conn_mem", &mem);
        // 不可共享 → 指纹 None → 缓存路径回退 conn_id 分文件
        assert_eq!(identity_fingerprint("P_conn_mem"), None);
        unregister_identity("P_conn_mem");
    }

    #[test]
    fn re_register_overwrites_previous() {
        let first = mysql_identity("db1", "orders", "app");
        let second = mysql_identity("db2", "orders", "app");
        register_identity("G_conn_x", &first);
        let fp1 = identity_fingerprint("G_conn_x").unwrap();
        register_identity("G_conn_x", &second);
        let fp2 = identity_fingerprint("G_conn_x").unwrap();
        assert_ne!(fp1, fp2);
        unregister_identity("G_conn_x");
        assert_eq!(identity_fingerprint("G_conn_x"), None);
    }
}

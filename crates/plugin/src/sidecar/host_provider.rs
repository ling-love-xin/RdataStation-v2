//! 反向 Host API 的宿主**默认实现**（协议面在 `host.rs`，这里是"接线"的那一半）。
//!
//! 原则：**宿主没有对应设施就不假装有**（可读的 `Unsupported` 优于谜一样的超时）。
//! 本轮真正接线的能力：
//!
//! - `storage.get / storage.set`：真落盘 —— 每个插件的 `paths::plugin_data_dir(id)`
//!   下 `host-storage/<namespace>/<key>.json`（key 与 namespace 都过字符集校验，
//!   不拼路径就不会穿越）；值 = 原样 JSON，`ttl_secs` 存过期时间，读时过期即删。
//!   每个 key 独立文件 + 临时文件 rename = 原子写，插件内多实例并发不串值。
//! - `log`：走 `tracing`（`module` 与 `fields` 原样带上）。
//!
//! 保持 `Unsupported` 的能力（`HostApiProvider` trait 默认已是）：
//! `request_credential` / `secret.resolve`（宿主还没有通用 secret store——
//! 只有 DuckDB Secret 目录与 `shared::crypto` 的密码加密，缺"按引用取凭据"的服务）、
//! `notify`（通知按钮要 UI 回调通道，P4 接界面时再实现）、`ssh.open_tunnel`
//! （宿主还没有 SSH 连接引擎）。
//!
//! 并发口径：`host.storage` 的读写是**同步 fs**（文件小、事件泵低频，可接受）；
//! 多实例并发写同一 key 时以最后一次 rename 为准（原子，不产生半截文件）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sidecar::host::{
    HostApiProvider, HostApiError, LogLevel, LogParams, StorageGetParams, StorageGetResult,
    StorageSetParams,
};

/// 反向 Host API 的宿主默认实现。构造时带插件 id（storage 的落盘根从它来）。
#[derive(Debug)]
pub struct DefaultHostProvider {
    plugin_id: String,
    store: PluginKvStore,
}

impl DefaultHostProvider {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        let plugin_id = plugin_id.into();
        let base = paths::plugin_data_dir(&plugin_id).join("host-storage");
        Self {
            plugin_id,
            store: PluginKvStore { base },
        }
    }
}

impl HostApiProvider for DefaultHostProvider {
    fn storage_get(&mut self, params: &StorageGetParams) -> Result<StorageGetResult, HostApiError> {
        self.store
            .get(&params.key, params.namespace.as_deref().unwrap_or(&self.plugin_id))
    }

    fn storage_set(&mut self, params: &StorageSetParams) -> Result<(), HostApiError> {
        self.store.set(
            &params.key,
            &params.value,
            params.namespace.as_deref().unwrap_or(&self.plugin_id),
            params.ttl_secs,
        )
    }

    fn log(&mut self, params: &LogParams) -> Result<(), HostApiError> {
        // module 作结构化字段、fields 原样带上，与宿主自己的日志同一条线
        // （`app_logs` 表 / tracing）。tracing 的 `target:` 只接受常量字符串，
        // 运行时 module 只能走字段。
        let module = params.module.as_deref().unwrap_or("plugin");
        match params.level {
            LogLevel::Trace => tracing::trace!(module, fields = %params.fields, "{}", params.message),
            LogLevel::Debug => tracing::debug!(module, fields = %params.fields, "{}", params.message),
            LogLevel::Info => tracing::info!(module, fields = %params.fields, "{}", params.message),
            LogLevel::Warn => tracing::warn!(module, fields = %params.fields, "{}", params.message),
            LogLevel::Error => tracing::error!(module, fields = %params.fields, "{}", params.message),
        }
        Ok(())
    }
}

// ============================================================================
// 插件 KV 存储（每个 key 一个文件 + 原子 rename）
// ============================================================================

/// 单条记录的落盘形状：`value` 原样，`expires_at`（unix 秒）缺省 = 永久。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct KvEntry {
    value: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
}

/// 插件级 KV：`<plugin_data_dir>/host-storage/<namespace>/<key>.json`。
#[derive(Debug)]
pub struct PluginKvStore {
    base: PathBuf,
}

/// key / namespace 只允许安全字符：不拼进路径的文件名不能带分隔符与点段。
fn is_safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl PluginKvStore {
    fn file_for(&self, key: &str, namespace: &str) -> Result<PathBuf, HostApiError> {
        if !is_safe_segment(key) || !is_safe_segment(namespace) {
            return Err(HostApiError::Rejected {
                detail: format!("storage key/namespace 含不安全字符：key={key:?} ns={namespace:?}"),
            });
        }
        Ok(self.base.join(namespace).join(format!("{key}.json")))
    }

    /// `now` 由调用方传入（provider 用系统时间；测试可以钉在过去 / 未来验证过期）。
    pub fn get_at(
        &self,
        key: &str,
        namespace: &str,
        now: u64,
    ) -> Result<StorageGetResult, HostApiError> {
        let path = self.file_for(key, namespace)?;
        let Ok(raw) = std::fs::read(&path) else {
            // 文件不存在 = 键不存在（读路径不做 create_dir，避免 get 产生副作用）
            return Ok(StorageGetResult::default());
        };
        let entry: KvEntry = serde_json::from_slice(&raw).map_err(|e| HostApiError::Internal {
            detail: format!("storage 记录损坏（{}）：{e}", path.display()),
        })?;
        // 过期：删掉再当不存在（懒清理，不留死数据）
        if let Some(at) = entry.expires_at {
            if now >= at {
                let _ = std::fs::remove_file(&path);
                return Ok(StorageGetResult::default());
            }
        }
        Ok(StorageGetResult {
            value: Some(entry.value),
        })
    }

    pub fn get(&self, key: &str, namespace: &str) -> Result<StorageGetResult, HostApiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.get_at(key, namespace, now)
    }

    pub fn set(
        &self,
        key: &str,
        value: &serde_json::Value,
        namespace: &str,
        ttl_secs: Option<u32>,
    ) -> Result<(), HostApiError> {
        let path = self.file_for(key, namespace)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| HostApiError::Internal {
                detail: format!("storage 目录创建失败（{}）：{e}", dir.display()),
            })?;
        }
        let expires_at = ttl_secs.map(|ttl| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() + u64::from(ttl))
                .unwrap_or(0)
        });
        let entry = KvEntry {
            value: value.clone(),
            expires_at,
        };
        let body = serde_json::to_vec(&entry).map_err(|e| HostApiError::Internal {
            detail: format!("storage 序列化失败：{e}"),
        })?;
        // 原子写：临时文件 + rename（写一半断电也不会留下半截 JSON）
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &body).map_err(|e| HostApiError::Internal {
            detail: format!("storage 写入失败（{}）：{e}", tmp.display()),
        })?;
        std::fs::rename(&tmp, &path).map_err(|e| HostApiError::Internal {
            detail: format!("storage 落盘失败（{}）：{e}", path.display()),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::host::NotifyParams;
    use serde_json::json;

    fn store_in(tmp: &std::path::Path) -> PluginKvStore {
        PluginKvStore {
            base: tmp.to_path_buf(),
        }
    }

    /// 每个测试独立临时目录（并发跑时共用同一目录会互相 remove_dir_all）。
    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("rds-host-store-{}-{name}", std::process::id()))
    }

    // ---- storage 语义 ----

    #[test]
    fn storage_set_get_round_trips_any_json() {
        let dir = tmp_dir("set_get");
        let _ = std::fs::remove_dir_all(&dir);
        let store = store_in(&dir);

        let value = serde_json::json!({"a": [1, 2, 3], "ok": true, "s": "文本"});
        store.set("conn.1", &value, "com.example.ext", None).unwrap();
        let got = store.get("conn.1", "com.example.ext").unwrap();
        assert_eq!(got.value, Some(value));

        // 默认 namespace（协议：默认扩展 id）由 provider 层补，store 层不猜
        assert!(store.get("conn.1", "other").unwrap().value.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_namespaces_are_isolated() {
        let dir = tmp_dir("namespaces");
        let _ = std::fs::remove_dir_all(&dir);
        let store = store_in(&dir);

        store.set("k", &json!("a"), "ns1", None).unwrap();
        store.set("k", &json!("b"), "ns2", None).unwrap();
        assert_eq!(store.get("k", "ns1").unwrap().value, Some(json!("a")));
        assert_eq!(store.get("k", "ns2").unwrap().value, Some(json!("b")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_ttl_expires_and_cleans() {
        let dir = tmp_dir("ttl");
        let _ = std::fs::remove_dir_all(&dir);
        let store = store_in(&dir);

        store.set("short", &json!("v"), "ns", Some(60)).unwrap();
        // expires_at 是**绝对 unix 秒**（set 时 = now + ttl），get_at 也要传绝对时间
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 未过期：now 在 expires_at 之前
        assert_eq!(store.get_at("short", "ns", now).unwrap().value, Some(json!("v")));
        // 已过期：返回 None 且文件被清掉
        assert!(store.get_at("short", "ns", now + 61).unwrap().value.is_none());
        assert!(!store.base.join("ns").join("short.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_rejects_unsafe_key_and_namespace() {
        let dir = tmp_dir("unsafe");
        let _ = std::fs::remove_dir_all(&dir);
        let store = store_in(&dir);

        assert!(matches!(
            store.set("../evil", &json!(1), "ns", None),
            Err(HostApiError::Rejected { .. })
        ));
        assert!(matches!(
            store.set("k", &json!(1), "a/b", None),
            Err(HostApiError::Rejected { .. })
        ));
        assert!(matches!(
            store.get("..", "ns").unwrap_err(),
            HostApiError::Rejected { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- provider 层 ----

    #[test]
    fn provider_defaults_namespace_to_plugin_id() {
        let dir = tmp_dir("provider_default_ns");
        let _ = std::fs::remove_dir_all(&dir);
        // 把 base 指到临时目录：DefaultHostProvider 默认走 paths::plugin_data_dir，
        // 测试里手工构造同构 store 验证默认 namespace 的语义。
        let mut provider = DefaultHostProvider {
            plugin_id: "com.example.ext".into(),
            store: store_in(&dir),
        };
        provider
            .storage_set(&StorageSetParams {
                key: "k".into(),
                value: json!({"n": 1}),
                namespace: None,
                ttl_secs: None,
            })
            .unwrap();
        let got = provider
            .storage_get(&StorageGetParams {
                key: "k".into(),
                namespace: None,
            })
            .unwrap();
        assert_eq!(got.value, Some(json!({"n": 1})));
        // 落点确实在默认 namespace（= plugin_id）下
        assert!(dir.join("com.example.ext").join("k.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_log_accepts_all_levels() {
        let mut p = DefaultHostProvider::new("com.example.ext");
        for (level, message) in [
            (LogLevel::Trace, "t"),
            (LogLevel::Debug, "d"),
            (LogLevel::Info, "i"),
            (LogLevel::Warn, "w"),
            (LogLevel::Error, "e"),
        ] {
            assert!(p
                .log(&LogParams {
                    level,
                    message: message.into(),
                    module: Some("m".into()),
                    fields: json!({"f": 1}),
                })
                .is_ok());
        }
    }

    #[test]
    fn provider_unsupported_methods_stay_unsupported() {
        // notify / credential / ssh 未接线：trait 默认返回 Unsupported，宿主不假装有
        let mut p = DefaultHostProvider::new("com.example.ext");
        assert!(matches!(
            p.notify(&NotifyParams {
                level: crate::sidecar::host::NotifyLevel::Info,
                title: "hi".into(),
                body: String::new(),
                duration_ms: None,
                actions: vec![],
            }),
            Err(HostApiError::Unsupported { .. })
        ));
    }
}

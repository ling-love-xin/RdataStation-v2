//! 反向 Host API（`host.*` 面）。
//!
//! 「反向」指调用方向与驱动协议相反——请求由**扩展进程**发起，经
//! [`super::router::RouterEvent::InboundRequest`] → [`super::conn::ConnEvent::InboundRequest`]
//! 到达宿主侧，宿主处理完用 [`super::conn::SidecarConn::reply`] /
//! [`super::conn::SidecarConn::reply_error`] 回帧。method 统一 `host.` 前缀（点分，
//! 与 `method.rs` 的正向协议同风格；navop 用 `host/` 斜杠，语义一致）。
//!
//! ## 与参考实现的对照
//!
//! 直接蓝本是 navop `extension-protocol/src/host.rs`：`host/request_credential`、
//! `host/secret/resolve`、`host/notify`、`host/storage/*`、`host/log`、
//! `host/ssh/open_tunnel` 六个方向，本模块全部对齐（含 SecretRef 引用制、
//! base64 一次性值、通知 action 回调）。差异只有两点：
//!
//! 1. 分隔符：`host.` vs `host/`（沿本项目线格式）。
//! 2. 权限键：navop 用 `secrets:read:<glob>` 冒号式，本项目对齐 `manifest.rs`
//!    的四轨权限命名，用点分 glob（`secrets.read.*` / `host.ssh_tunnel`）。
//!
//! ## 权限（方法级 glob，安装期声明的展示/门控轨之外的第二道闸）
//!
//! ```text
//! 方法                          | 所需权限
//! ─────────────────────────────────────────────────────
//! host.request_credential       | secrets.read（save_as 时宿主另验 secrets.write）
//! host.secret.resolve           | secrets.read
//! host.notify                   | notifications.show
//! host.storage.get              | storage.read
//! host.storage.set              | storage.write
//! host.log                      | logs.write
//! host.ssh.open_tunnel          | host.ssh_tunnel
//! ```
//!
//! [`HostApiHandler`] 构造时接收扩展声明的权限列表（宿主从清单 `permissions` 取出），
//! 每个请求先过 [`ensure_permission`](HostApiHandler::ensure_permission)（glob 匹配），
//! 不过就回 [`HostApiError::PermissionDenied`]。**默认拒绝**：未声明即无能力。
//!
//! ## 凭据口径（SecretRef 引用制）
//!
//! `host.request_credential` 只返回**引用**（`SecretRef`），永不返回明文；扩展后续
//! 需要值时再调 `host.secret.resolve` 换取**一次性** base64 字节。宿主必须保证：
//! 值不进 UI 状态、不进日志（navop `host.rs` 原话语义）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// 方法常量
// ============================================================================

/// 反向 Host API 的统一前缀。
pub const HOST_PREFIX: &str = "host.";

pub const HOST_REQUEST_CREDENTIAL: &str = "host.request_credential";
pub const HOST_SECRET_RESOLVE: &str = "host.secret.resolve";
pub const HOST_NOTIFY: &str = "host.notify";
pub const HOST_STORAGE_GET: &str = "host.storage.get";
pub const HOST_STORAGE_SET: &str = "host.storage.set";
pub const HOST_LOG: &str = "host.log";
pub const HOST_SSH_OPEN_TUNNEL: &str = "host.ssh.open_tunnel";

/// 全部已定义的反向方法（协议面；宿主实现与否以 [`HostApiProvider`] 为准）。
pub const ALL_HOST_METHODS: &[&str] = &[
    HOST_REQUEST_CREDENTIAL,
    HOST_SECRET_RESOLVE,
    HOST_NOTIFY,
    HOST_STORAGE_GET,
    HOST_STORAGE_SET,
    HOST_LOG,
    HOST_SSH_OPEN_TUNNEL,
];

/// 判断一个 method 是否属于反向 Host API（`host.` 前缀）。
pub fn is_host_method(method: &str) -> bool {
    method.starts_with(HOST_PREFIX)
}

/// 方法 → 所需权限键（见模块头权限表）。
///
/// `None` = 不是已知的 host 方法（调用方按 [`HostApiError::MethodNotFound`] 处理）。
pub fn permission_for(method: &str) -> Option<&'static str> {
    match method {
        HOST_REQUEST_CREDENTIAL => Some("secrets.read"),
        HOST_SECRET_RESOLVE => Some("secrets.read"),
        HOST_NOTIFY => Some("notifications.show"),
        HOST_STORAGE_GET => Some("storage.read"),
        HOST_STORAGE_SET => Some("storage.write"),
        HOST_LOG => Some("logs.write"),
        HOST_SSH_OPEN_TUNNEL => Some("host.ssh_tunnel"),
        _ => None,
    }
}

// ============================================================================
// 权限 glob 校验
// ============================================================================

/// 方法级权限的 glob 匹配（`*` 匹配任意字符序列，**含空**）。
///
/// 语义对齐 navop 的 `secrets:read:<glob>`。本项目的权限键没有目标维度（就是
/// `secrets.read` / `host.ssh_tunnel` 这类单段），所以 `prefix.*` 的点号交给 `*`
/// 吸收：声明 `secrets.read.*` 同时覆盖 `secrets.read` 本体与 `secrets.read.<任意目标>`，
/// 是最直觉的「域通配」写法。
pub fn glob_match(pattern: &str, value: &str) -> bool {
    // `prefix.*` → `prefix*`（`.` 并入通配；否则 `secrets.read.*` 匹配不到 `secrets.read`）
    let pattern = pattern.replace(".*", "*");
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut rest = value;

    // 第一段必须钉在 value 开头
    if !parts[0].is_empty() {
        if !rest.starts_with(parts[0]) {
            return false;
        }
        rest = &rest[parts[0].len()..];
    }

    // 其余段按顺序出现；最后一段必须钉在 value 结尾
    for (i, part) in parts.iter().enumerate().skip(1) {
        if part.is_empty() {
            // `*` 产生的空段（开头/结尾的通配）不约束位置
            continue;
        }
        match rest.find(part) {
            Some(pos) => {
                let end = pos + part.len();
                if i == parts.len() - 1 {
                    if end != rest.len() {
                        return false;
                    }
                } else {
                    rest = &rest[end..];
                }
            }
            None => return false,
        }
    }
    true
}

// ============================================================================
// SecretRef（引用制，不承载明文）
// ============================================================================

/// 已存凭据的**引用**（如 `kss://ext/ssh/password`）。扩展永远只持有引用，
/// 明文值通过 [`ResolveSecretParams`] 一次性换取，宿主保证不外泄。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRef {
    pub secret_ref: String,
}

impl SecretRef {
    pub fn new(secret_ref: impl Into<String>) -> Self {
        Self {
            secret_ref: secret_ref.into(),
        }
    }
}

// ============================================================================
// host.request_credential
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCredentialParams {
    /// `password` / `keyfile` / `token` / `oauth_code` / ...。
    pub kind: String,
    /// 给用户看的提示文案。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// 如果填了，宿主拿到值后会写入 secret store 并把同一 ref 返回。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_as: Option<String>,
    /// 用户是否能勾「记住此凭据」。
    #[serde(default)]
    pub remember_option: bool,
    /// 失败可重试次数（0 表示一次性）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCredentialResult {
    /// 引用，不返回明文值——扩展继续用它换取一次性值或回填到连接参数。
    pub secret_ref: SecretRef,
    /// 用户是否选了「记住」（只在 `remember_option` 为 true 时有意义）。
    #[serde(default)]
    pub remembered: bool,
}

// ============================================================================
// host.secret.resolve
// ============================================================================

/// 把已存引用解析成**一次性**值。宿主必须按 `secrets.read` 权限校验后才返回；
/// 值不得进 UI 状态、不得记日志。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveSecretParams {
    pub secret_ref: SecretRef,
}

/// 值以 base64 字符串承载（JSON 只能表达文本；token / 密码 / 证书 / 密钥共用同一契约）。
/// 宿主负责编码、扩展负责解码——线格式上约定死的，两侧不得再猜编码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveSecretResult {
    #[serde(with = "serde_bytes_base64")]
    pub value: Vec<u8>,
}

mod serde_bytes_base64 {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        STANDARD.encode(value).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

// ============================================================================
// host.notify
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    Info,
    Warning,
    Error,
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyParams {
    pub level: NotifyLevel,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    /// 持续多少 ms；`None` 走宿主默认值；0 表示常驻。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u32>,
    /// 通知里附带的按钮（宿主渲染，扩展不知用户是否点击）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<NotifyAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyAction {
    /// 用户点击后扩展应收到的回调命令 id。
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub primary: bool,
}

/// notify 无返回数据，但仍是请求（可能阻塞通知队列）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyResult {
    /// 用户点击的 action id（若有）；超时关闭则为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clicked: Option<String>,
}

// ============================================================================
// host.storage.get / host.storage.set
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageGetParams {
    pub key: String,
    /// 命名空间；默认是扩展 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageGetResult {
    /// 不存在则为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSetParams {
    pub key: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// 过期时间（秒），`None` 永久。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_secs: Option<u32>,
}

// ============================================================================
// host.log
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogParams {
    pub level: LogLevel,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// 结构化字段（`tracing` 风格）。
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub fields: Value,
}

// ============================================================================
// host.ssh.open_tunnel
// ============================================================================

/// 让扩展在**宿主已有的 SSH 连接**上开一条隧道（navop 同款：SSH 引擎归宿主，
/// 扩展只申请隧道；`host:ssh_tunnel` 权限门控）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshOpenTunnelParams {
    /// 宿主已有的 SSH 连接（配置 id）。
    pub ssh_connection_id: String,
    /// 隧道目标主机，默认 `127.0.0.1`。
    #[serde(default = "default_tunnel_host")]
    pub remote_host: String,
    pub remote_port: u16,
    /// 本地映射端口，`None` 表示自动分配。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    /// 隧道空闲超时（秒）；0 表示常驻直到扩展关闭。
    #[serde(default = "default_tunnel_idle_secs")]
    pub idle_secs: u32,
}

fn default_tunnel_host() -> String {
    "127.0.0.1".into()
}

fn default_tunnel_idle_secs() -> u32 {
    600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshOpenTunnelResult {
    /// 宿主侧登记的隧道会话引用（扩展关闭隧道时用它）。
    pub session_ref: String,
    /// 实际本地端口（可能与请求不同）。
    pub local_port: u16,
}

// ============================================================================
// 错误
// ============================================================================

/// 反向请求处理失败。错误码映射（JSON-RPC 约定）：
/// -32601 方法不存在 / -32602 参数不合法 / -32603 宿主内部错误 /
/// -32001 权限不足 / -32002 宿主未实现 / -32003 宿主实现层拒绝。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostApiError {
    MethodNotFound { method: String },
    InvalidParams { method: String, detail: String },
    PermissionDenied { method: String, required: String },
    Unsupported { method: String },
    Rejected { detail: String },
    Internal { detail: String },
}

impl HostApiError {
    /// 转成 JSON-RPC 错误响应的 `(code, message)`。
    pub fn to_rpc_error(&self) -> (i32, String) {
        match self {
            Self::MethodNotFound { method } => (-32601, format!("方法不存在：{method}")),
            Self::InvalidParams { method, detail } => {
                (-32602, format!("{method} 参数不合法：{detail}"))
            }
            Self::PermissionDenied { method, required } => {
                (-32001, format!("{method} 需要权限 {required}，插件未声明"))
            }
            Self::Unsupported { method } => (-32002, format!("宿主未实现 {method}")),
            Self::Rejected { detail } => (-32003, detail.clone()),
            Self::Internal { detail } => (-32603, format!("宿主内部错误：{detail}")),
        }
    }
}

// ============================================================================
// Provider（宿主的实现面）与 Handler（分发 + 权限闸）
// ============================================================================

/// 宿主对反向 Host API 的实现面。默认全部返回 [`HostApiError::Unsupported`]：
/// **协议面就绪 ≠ 宿主已接线**，未实现的方法对扩展来说是清晰可读的「宿主没接」，
/// 而不是谜一样的超时。
pub trait HostApiProvider {
    fn request_credential(
        &mut self,
        params: &RequestCredentialParams,
    ) -> Result<RequestCredentialResult, HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_REQUEST_CREDENTIAL.into(),
        })
    }

    fn resolve_secret(&mut self, params: &ResolveSecretParams) -> Result<ResolveSecretResult, HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_SECRET_RESOLVE.into(),
        })
    }

    fn notify(&mut self, params: &NotifyParams) -> Result<NotifyResult, HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_NOTIFY.into(),
        })
    }

    fn storage_get(&mut self, params: &StorageGetParams) -> Result<StorageGetResult, HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_STORAGE_GET.into(),
        })
    }

    fn storage_set(&mut self, params: &StorageSetParams) -> Result<(), HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_STORAGE_SET.into(),
        })
    }

    fn log(&mut self, params: &LogParams) -> Result<(), HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_LOG.into(),
        })
    }

    fn ssh_open_tunnel(
        &mut self,
        params: &SshOpenTunnelParams,
    ) -> Result<SshOpenTunnelResult, HostApiError> {
        let _ = params;
        Err(HostApiError::Unsupported {
            method: HOST_SSH_OPEN_TUNNEL.into(),
        })
    }
}

/// 分发器：方法路由 + 权限闸（方法级 glob）。
pub struct HostApiHandler<P> {
    provider: P,
    /// 扩展声明过的权限 glob 列表（宿主从清单 `permissions` 取出后传入）。
    allowed: Vec<String>,
}

impl<P: HostApiProvider> HostApiHandler<P> {
    pub fn new(provider: P, allowed: impl IntoIterator<Item = String>) -> Self {
        Self {
            provider,
            allowed: allowed.into_iter().collect(),
        }
    }

    /// 权限闸：方法必须是已知 host 方法，且所需权限被声明的 glob 覆盖。
    fn ensure_permission(&self, method: &str) -> Result<(), HostApiError> {
        let Some(required) = permission_for(method) else {
            return Err(HostApiError::MethodNotFound {
                method: method.to_string(),
            });
        };
        if self.allowed.iter().any(|p| glob_match(p, required)) {
            Ok(())
        } else {
            Err(HostApiError::PermissionDenied {
                method: method.to_string(),
                required: required.to_string(),
            })
        }
    }

    /// 处理一个反向请求，返回成功结果（`Value`）或错误。
    ///
    /// 宿主侧拿到 [`super::conn::ConnEvent::InboundRequest`] 后调用本方法，再按
    /// 结果用 [`super::conn::SidecarConn::reply`] / `reply_error` 回帧。
    pub fn handle(&mut self, method: &str, params: Value) -> Result<Value, HostApiError> {
        self.ensure_permission(method)?;
        let out = match method {
            HOST_REQUEST_CREDENTIAL => {
                let p = parse_params::<RequestCredentialParams>(method, params)?;
                to_value(self.provider.request_credential(&p)?, method)?
            }
            HOST_SECRET_RESOLVE => {
                let p = parse_params::<ResolveSecretParams>(method, params)?;
                to_value(self.provider.resolve_secret(&p)?, method)?
            }
            HOST_NOTIFY => {
                let p = parse_params::<NotifyParams>(method, params)?;
                to_value(self.provider.notify(&p)?, method)?
            }
            HOST_STORAGE_GET => {
                let p = parse_params::<StorageGetParams>(method, params)?;
                to_value(self.provider.storage_get(&p)?, method)?
            }
            HOST_STORAGE_SET => {
                let p = parse_params::<StorageSetParams>(method, params)?;
                self.provider.storage_set(&p)?;
                Value::Null
            }
            HOST_LOG => {
                let p = parse_params::<LogParams>(method, params)?;
                self.provider.log(&p)?;
                Value::Null
            }
            HOST_SSH_OPEN_TUNNEL => {
                let p = parse_params::<SshOpenTunnelParams>(method, params)?;
                to_value(self.provider.ssh_open_tunnel(&p)?, method)?
            }
            // 权限闸已挡掉未知方法，这里只是兜底（保持 match 穷尽）
            _ => {
                return Err(HostApiError::MethodNotFound {
                    method: method.to_string(),
                })
            }
        };
        Ok(out)
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(
    method: &str,
    params: Value,
) -> Result<T, HostApiError> {
    serde_json::from_value(params).map_err(|e| HostApiError::InvalidParams {
        method: method.to_string(),
        detail: e.to_string(),
    })
}

fn to_value<T: Serialize>(v: T, method: &str) -> Result<Value, HostApiError> {
    serde_json::to_value(v).map_err(|e| HostApiError::Internal {
        detail: format!("{method} 结果序列化失败：{e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- SecretRef / 类型 serde ----

    #[test]
    fn secret_ref_round_trip() {
        let r = SecretRef::new("kss://ext/ssh/password");
        let j = serde_json::to_string(&r).unwrap();
        assert_eq!(j, r#"{"secret_ref":"kss://ext/ssh/password"}"#);
        let parsed: SecretRef = serde_json::from_str(&j).unwrap();
        assert_eq!(parsed, r);
    }

    /// resolve 的值在线格式上必须是 base64（明文不进 JSON）。
    #[test]
    fn resolve_secret_is_base64_on_the_wire() {
        let result = ResolveSecretResult {
            value: b"token-value".to_vec(),
        };
        let j = serde_json::to_string(&result).unwrap();
        assert_eq!(r#"{"value":"dG9rZW4tdmFsdWU="}"#, j);
        let parsed: ResolveSecretResult =
            serde_json::from_str(r#"{"value":"dG9rZW4tdmFsdWU="}"#).unwrap();
        assert_eq!(b"token-value".to_vec(), parsed.value);
    }

    #[test]
    fn ssh_open_tunnel_params_defaults() {
        let p: SshOpenTunnelParams =
            serde_json::from_str(r#"{"ssh_connection_id":"s1","remote_port":3306}"#).unwrap();
        assert_eq!(p.remote_host, "127.0.0.1");
        assert_eq!(p.idle_secs, 600);
        assert!(p.local_port.is_none());
    }

    #[test]
    fn notify_params_skip_empty_body_and_actions() {
        let p = NotifyParams {
            level: NotifyLevel::Success,
            title: "ok".into(),
            body: String::new(),
            duration_ms: None,
            actions: vec![],
        };
        let j = serde_json::to_string(&p).unwrap();
        assert!(!j.contains("body"));
        assert!(!j.contains("actions"));
        assert!(!j.contains("duration_ms"));
    }

    // ---- 常量 / 映射 ----

    #[test]
    fn is_host_method_matches_prefix() {
        assert!(is_host_method("host.log"));
        assert!(is_host_method("host.ssh.open_tunnel"));
        assert!(!is_host_method("log"));
        assert!(!is_host_method("driver.describe"));
    }

    #[test]
    fn permission_table_covers_every_host_method() {
        for m in ALL_HOST_METHODS {
            assert!(
                permission_for(m).is_some(),
                "{m} 缺权限映射（模块头权限表要同步）"
            );
        }
        assert_eq!(permission_for("host.notify"), Some("notifications.show"));
        assert_eq!(permission_for("host.ssh.open_tunnel"), Some("host.ssh_tunnel"));
        assert_eq!(permission_for("driver.describe"), None);
    }

    // ---- glob ----

    #[test]
    fn glob_exact_match() {
        assert!(glob_match("host.ssh_tunnel", "host.ssh_tunnel"));
        assert!(!glob_match("host.ssh_tunnel", "host.ssh"));
    }

    #[test]
    fn glob_wildcard_covers_sub_scope_and_exact() {
        assert!(glob_match("secrets.read.*", "secrets.read"));
        assert!(glob_match("secrets.read.*", "secrets.read:kss://x/y"));
        assert!(glob_match("storage.*", "storage.read"));
        assert!(glob_match("storage.*", "storage.write"));
        assert!(!glob_match("storage.*", "secrets.read"));
    }

    #[test]
    fn glob_wildcard_in_the_middle() {
        assert!(glob_match("secrets.*.write", "secrets.read.write"));
        assert!(glob_match("secrets.*.write", "secrets.global.write"));
        assert!(!glob_match("secrets.*.write", "secrets.read"));
    }

    // ---- Handler ----

    /// 记录调用的假宿主（并支持"按方法定制失败"）。
    struct MockProvider {
        calls: Vec<String>,
        fail: Option<(&'static str, HostApiError)>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                calls: Vec::new(),
                fail: None,
            }
        }
    }

    impl HostApiProvider for MockProvider {
        fn notify(&mut self, params: &NotifyParams) -> Result<NotifyResult, HostApiError> {
            self.calls.push(format!("notify:{}", params.title));
            if let Some((m, e)) = &self.fail {
                if *m == HOST_NOTIFY {
                    return Err(e.clone());
                }
            }
            Ok(NotifyResult {
                clicked: Some("open_folder".into()),
            })
        }

        fn log(&mut self, params: &LogParams) -> Result<(), HostApiError> {
            self.calls.push(format!("log:{}", params.message));
            Ok(())
        }

        fn ssh_open_tunnel(
            &mut self,
            params: &SshOpenTunnelParams,
        ) -> Result<SshOpenTunnelResult, HostApiError> {
            self.calls.push(format!("tunnel:{}", params.remote_port));
            Ok(SshOpenTunnelResult {
                session_ref: "sht://rds/ssh/1".into(),
                local_port: 13306,
            })
        }
    }

    #[test]
    fn handler_dispatch_calls_provider_with_permission() {
        let mut h = HostApiHandler::new(
            MockProvider::new(),
            ["notifications.show".to_string(), "host.ssh_tunnel".to_string()],
        );
        let out = h
            .handle(
                HOST_NOTIFY,
                json!({"level": "info", "title": "备份完成", "body": "ok"}),
            )
            .unwrap();
        assert_eq!(out["clicked"], "open_folder");

        let out = h
            .handle(
                HOST_SSH_OPEN_TUNNEL,
                json!({"ssh_connection_id": "s1", "remote_port": 3306}),
            )
            .unwrap();
        assert_eq!(out["local_port"], 13306);
        assert_eq!(out["session_ref"], "sht://rds/ssh/1");
    }

    #[test]
    fn handler_denies_without_declared_permission() {
        let mut h = HostApiHandler::new(MockProvider::new(), Vec::<String>::new());
        let err = h
            .handle(HOST_NOTIFY, json!({"level": "info", "title": "x"}))
            .unwrap_err();
        assert_eq!(
            err,
            HostApiError::PermissionDenied {
                method: HOST_NOTIFY.into(),
                required: "notifications.show".into()
            }
        );
    }

    #[test]
    fn handler_rejects_unknown_method() {
        let mut h = HostApiHandler::new(MockProvider::new(), Vec::<String>::new());
        let err = h.handle("host.frobnicate", json!({})).unwrap_err();
        assert!(matches!(err, HostApiError::MethodNotFound { .. }), "{err:?}");

        // 正向协议的方法不属于 host 面，同样拒绝
        let err = h.handle("driver.describe", json!({})).unwrap_err();
        assert!(matches!(err, HostApiError::MethodNotFound { .. }), "{err:?}");
    }

    #[test]
    fn handler_rejects_bad_params() {
        let mut h = HostApiHandler::new(
            MockProvider::new(),
            ["notifications.show".to_string()],
        );
        let err = h
            .handle(HOST_NOTIFY, json!({"level": "info"}))
            .unwrap_err();
        assert!(matches!(err, HostApiError::InvalidParams { .. }), "{err:?}");
    }

    #[test]
    fn handler_defaults_to_unsupported_for_unimplemented_methods() {
        // MockProvider 没实现 request_credential → trait 默认返回 Unsupported
        let mut h = HostApiHandler::new(
            MockProvider::new(),
            ["secrets.read".to_string()],
        );
        let err = h
            .handle(HOST_REQUEST_CREDENTIAL, json!({"kind": "password"}))
            .unwrap_err();
        assert_eq!(
            err,
            HostApiError::Unsupported {
                method: HOST_REQUEST_CREDENTIAL.into()
            }
        );
        assert_eq!(err.to_rpc_error().0, -32002);
    }

    #[test]
    fn error_to_rpc_code_mapping() {
        let cases: Vec<(HostApiError, i32)> = vec![
            (
                HostApiError::MethodNotFound {
                    method: "host.x".into(),
                },
                -32601,
            ),
            (
                HostApiError::InvalidParams {
                    method: "host.x".into(),
                    detail: "d".into(),
                },
                -32602,
            ),
            (
                HostApiError::PermissionDenied {
                    method: "host.x".into(),
                    required: "secrets.read".into(),
                },
                -32001,
            ),
            (
                HostApiError::Unsupported {
                    method: "host.x".into(),
                },
                -32002,
            ),
            (
                HostApiError::Rejected {
                    detail: "宿主拒绝".into(),
                },
                -32003,
            ),
            (
                HostApiError::Internal {
                    detail: "i".into(),
                },
                -32603,
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.to_rpc_error().0, code, "{err:?}");
        }
    }
}

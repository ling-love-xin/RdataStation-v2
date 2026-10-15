//! 协议 method 常量表（借鉴 navop `extension-protocol/method.rs` 的集中式做法）。
//!
//! 这层不带任何逻辑，只是把 wire protocol 里出现过的 `method` 字符串集中起来，
//! 避免业务代码硬编码字面量 —— 打错字在编译期就暴露，而不是跑起来才发现
//! 「这个方法没人认」（navop 那层的注释原话：拼写错误不能悄悄变成“不支持”）。
//!
//! 命名约定：`<namespace>.<action>`，namespace 全小写，action 用 snake_case；
//! 协议元方法以 `$/` 开头（借鉴 LSP 的命名，辨识度高）。
//!
//! ## 与参考实现的分隔符差异
//!
//! navop 用 `/`（`conn/open` / `schema/columns`），本协议沿用参考实现
//! `Rdata-Sidecar` 的**点分**（`driver.describe` / `query.execute`）—— 线格式已经
//! 定死，这里只集中常量、不重命名（重命名会让已存在的驱动全部失配）。
//! 扩展的**私有能力**同理用 `x.` 前缀（navop 是 `x/`），保留给尚未进入公共
//! 协议的业务 namespace。
//!
//! ## 谁用它
//!
//! - [`super::driver`] / [`super::conn`] / [`super::supervisor`] 的调用点（换掉字面量）
//! - `PluginManifest::validate`（`crate::manifest`）：sidecar 插件声明的 `methods`
//!   必须过 [`is_allowed_declaration`]，否则清单在安装期就拒绝 —— 声明一个
//!   拼错的方法名，等于对宿主承诺一个它不会履行的能力（dev-plan §3.5 的教训）。

// -- 协议元方法（宿主 ↔ 对端，`$/` 前缀） --
/// 取消任意 in-flight 请求，按 request id。**预留**：线格式上还没有实现，
/// 也不允许驱动声明它（元方法不是插件能力，不进 [`ALL_METHODS`]）。
pub const CANCEL_REQUEST: &str = "$/cancelRequest";

// -- 生命周期 --
pub const INIT: &str = "initialize";
pub const SHUTDOWN: &str = "shutdown";
/// 进程级探活（dev-plan §4.2.2 的 `ping()`；会话级探活是 [`SESSION_PING`]）。
pub const PING: &str = "ping";

// -- 会话管理 --
pub const SESSION_OPEN: &str = "session.open";
pub const SESSION_CLOSE: &str = "session.close";
pub const SESSION_PING: &str = "session.ping";

// -- 驱动自述 --
pub const DRIVER_DESCRIBE: &str = "driver.describe";

// -- Schema 内省（meta.*） --
pub const META_CATALOGS: &str = "meta.catalogs";
pub const META_SCHEMAS: &str = "meta.schemas";
pub const META_OBJECTS: &str = "meta.objects";
pub const META_OBJECT_DETAIL: &str = "meta.object_detail";
pub const META_ROUTINE_SOURCE: &str = "meta.routine_source";

// -- 查询执行 --
pub const QUERY_EXECUTE: &str = "query.execute";
pub const QUERY_FETCH: &str = "query.fetch";
pub const QUERY_CANCEL: &str = "query.cancel";

// -- 事务（协议方法表 §4.2.2 已定义；驱动未声明事务能力时宿主明确报不支持） --
pub const TX_BEGIN: &str = "tx.begin";
pub const TX_COMMIT: &str = "tx.commit";
pub const TX_ROLLBACK: &str = "tx.rollback";

// -- 事件通知（对端 → 宿主，无 id 的通知） --
pub const EVENT_PROGRESS: &str = "progress";
pub const EVENT_LOG: &str = "log";

/// 判断一个 method 是否属于协议元方法（以 `$/` 开头）。
pub fn is_meta_method(method: &str) -> bool {
    method.starts_with("$/")
}

/// 协议里定义过的全部 method 名（用于校验驱动声明的 `methods` 是否拼写正确）。
/// 与 `plugin-dev-plan.md` §4.2.2 的 RPC 方法表一一对应（含未接线的事务三件套：
/// 协议已知 ≠ 驱动支持，支持与否以 `driver.describe` 的能力声明为准）。
pub const ALL_METHODS: &[&str] = &[
    PING,
    INIT,
    SHUTDOWN,
    SESSION_OPEN,
    SESSION_CLOSE,
    SESSION_PING,
    DRIVER_DESCRIBE,
    META_CATALOGS,
    META_SCHEMAS,
    META_OBJECTS,
    META_OBJECT_DETAIL,
    META_ROUTINE_SOURCE,
    QUERY_EXECUTE,
    QUERY_FETCH,
    QUERY_CANCEL,
    TX_BEGIN,
    TX_COMMIT,
    TX_ROLLBACK,
    EVENT_PROGRESS,
    EVENT_LOG,
];

/// 该 method 名是否是协议已知方法。驱动声明 `methods` 时用它过滤拼写错误。
pub fn is_known(method: &str) -> bool {
    ALL_METHODS.contains(&method)
}

/// 判断 driver / extension 的 method 声明是否可接受。
///
/// 协议标准 method 必须在 [`ALL_METHODS`] 中，避免 `query.execut` 这类拼写错误
/// 悄悄变成「不支持」。扩展私有能力必须显式放到 `x.` 命名空间，避免污染协议命名空间。
pub fn is_allowed_declaration(method: &str) -> bool {
    is_known(method)
        || method
            .strip_prefix("x.")
            .is_some_and(|private| !private.trim().is_empty())
}

/// 取 method 的 namespace 部分（`.` 之前）。返回空表示无 namespace 或元方法。
pub fn namespace(method: &str) -> &str {
    if let Some(meta) = method.strip_prefix("$/") {
        return meta.split('.').next().unwrap_or("");
    }
    method.split('.').next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_methods_has_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for m in ALL_METHODS {
            assert!(seen.insert(*m), "ALL_METHODS 里有重复：{m}");
        }
    }

    #[test]
    fn is_meta_method_detects_dollar_prefix() {
        assert!(is_meta_method(CANCEL_REQUEST));
        assert!(!is_meta_method(PING));
        assert!(!is_meta_method(INIT));
        assert!(!is_meta_method(QUERY_EXECUTE));
        assert!(!is_meta_method(""));
    }

    #[test]
    fn is_known_accepts_defined_methods_rejects_typos() {
        assert!(is_known(DRIVER_DESCRIBE));
        assert!(is_known(QUERY_EXECUTE));
        assert!(is_known(SESSION_PING));
        assert!(is_known(META_OBJECT_DETAIL));
        assert!(is_known(TX_BEGIN));
        assert!(is_known(TX_COMMIT));
        assert!(is_known(TX_ROLLBACK));
        assert!(is_known(PING));
        // 元方法不是插件能力：即使协议已知也不允许驱动声明
        assert!(!is_known(CANCEL_REQUEST));
        assert!(!is_known("query.execut"));
        assert!(!is_known("driver.describ"));
        assert!(!is_known(""));
    }

    /// 声明校验：协议已知 method 放行；`x.` 私有能力放行；其余拒绝。
    #[test]
    fn declaration_allows_known_protocol_and_private_methods() {
        assert!(is_allowed_declaration(DRIVER_DESCRIBE));
        assert!(is_allowed_declaration(QUERY_EXECUTE));
        assert!(is_allowed_declaration("x.demo.profile"));
        assert!(!is_allowed_declaration("query.execut"));
        assert!(!is_allowed_declaration("x."));
        assert!(!is_allowed_declaration(""));
    }

    #[test]
    fn namespace_extracts_prefix() {
        assert_eq!(namespace(DRIVER_DESCRIBE), "driver");
        assert_eq!(namespace(QUERY_EXECUTE), "query");
        assert_eq!(namespace(SESSION_PING), "session");
        assert_eq!(namespace(META_OBJECTS), "meta");
    }

    #[test]
    fn namespace_for_meta_and_unprefixed_methods() {
        // `$/cancelRequest` 没有 namespace，返回整段 meta name（与 navop 同口径）
        assert_eq!(namespace(CANCEL_REQUEST), "cancelRequest");
        assert_eq!(namespace("init"), "init");
        assert_eq!(namespace(""), "");
    }
}

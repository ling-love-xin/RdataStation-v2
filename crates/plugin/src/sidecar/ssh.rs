//! 正向 SSH 协议方法面（`x.ssh.*`）。
//!
//! 「正向」指调用方向与驱动协议一致——**SSH 插件（sidecar）自己是 SSH 引擎**，
//! 宿主通过驱动桥调它开会话 / 执行命令 / 建隧道。与反向
//! [`super::host::HOST_SSH_OPEN_TUNNEL`]（`host.ssh.open_tunnel`，宿主是引擎、
//! 扩展申请隧道）是两个方向，不冲突：SSH 插件可以两者都提供。
//!
//! ## 为什么是 `x.` 私有命名空间
//!
//! `x.ssh.*` 走 [`super::method::is_allowed_declaration`] 的私有前缀规则（`x.`
//! 之后非空即合法），所以 SSH 插件在自己的 `manifest.methods` 里声明
//! `["x.ssh.connect", "x.ssh.exec", …]` 安装期即可通过，**不污染协议命名空间**；
//! 协议已知方法（`query.*` / `meta.*` 等）仍是宿主专用的。
//!
//! ## 第一刀范围（本轮落地）
//!
//! 全部方法**一次一答**（JSON-RPC 请求/响应，与 `conn.rs` 的 `call` 天然契合）：
//!
//! | 方法 | 语义 | 结果 |
//! | --- | --- | --- |
//! | `x.ssh.connect` | 建 SSH 会话（凭据走 SecretRef 引用，插件侧自行 resolve） | session_ref |
//! | `x.ssh.exec` | 一次命令执行（小输出一次一答） | exit_code + stdout/stderr |
//! | `x.ssh.shell` | 交互 shell / PTY | channel_id（**持续字节流 = 第二刀**） |
//! | `x.ssh.forward` | 端口转发（本地映射到远程，对齐 navop local_forward） | channel_id + bound_local_port（**字节流 = 第二刀**） |
//! | `x.ssh.close` | 关闭会话 / 通道 | closed |
//!
//! `shell` / `forward` 的**持续双向字节流**需要新的流式帧（`proto.rs` 加 kind +
//! `conn.rs` 流通道 + `router.rs` 流事件转发），作为第二刀；本轮先把方法面与
//! 参数契约钉住（类型即文档，插件照此实现就能与宿主桥对接）。
//!
//! 能力面对齐参考：navop `crates/ssh/`（session_manager / registry /
//! socks5 / dynamic_socks / remote_forwarding），本模块取其方法面骨架，
//! 传输仍走本项目 stdio 分帧协议。

use serde::{Deserialize, Serialize};

// ============================================================================
// 方法常量
// ============================================================================

/// 正向 SSH 方法面的统一前缀。
pub const SSH_PREFIX: &str = "x.ssh.";

pub const SSH_CONNECT: &str = "x.ssh.connect";
pub const SSH_EXEC: &str = "x.ssh.exec";
pub const SSH_SHELL: &str = "x.ssh.shell";
pub const SSH_FORWARD: &str = "x.ssh.forward";
pub const SSH_CLOSE: &str = "x.ssh.close";

/// 全部已定义的正向 SSH 方法（协议面；引擎实现与否以插件侧为准）。
pub const ALL_SSH_METHODS: &[&str] = &[
    SSH_CONNECT,
    SSH_EXEC,
    SSH_SHELL,
    SSH_FORWARD,
    SSH_CLOSE,
];

/// 判断一个 method 是否属于正向 SSH 面（`x.ssh.` 前缀）。
pub fn is_ssh_method(method: &str) -> bool {
    method.starts_with(SSH_PREFIX)
}

// ============================================================================
// x.ssh.connect
// ============================================================================

/// 建 SSH 会话。
///
/// 凭据口径：只传 `secret_ref`（引用），明文值由插件经反向
/// `host.secret.resolve` 一次性换取 —— 与 [`super::host`] 的 SecretRef 制一致，
/// 防止连接配置在插件目录里留下明文。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConnectParams {
    /// 目标主机（IP / 域名 / ssh 配置别名）。
    pub host: String,
    /// 目标端口，默认 22。
    #[serde(default = "default_port")]
    pub port: u16,
    /// 登录用户名。
    pub username: String,
    /// 已存凭据的引用（如 `kss://ext/ssh/password`）；`None` = 插件侧自行决定
    /// （agent / 交互式 / 直接内联 —— 第一刀不强制）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<String>,
    /// 连接超时（秒），默认 10。
    #[serde(default = "default_connect_timeout_secs")]
    pub timeout_secs: u32,
}

fn default_port() -> u16 {
    22
}

fn default_connect_timeout_secs() -> u32 {
    10
}

/// 建会话成功：宿主 / 插件侧登记一个会话引用，后续 exec / shell / forward 用它。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConnectResult {
    /// 会话引用（`x.ssh.close` 时用它）。
    pub session_ref: String,
    /// 服务端 banner（诊断 / 展示用，可为空）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub banner: String,
}

// ============================================================================
// x.ssh.exec
// ============================================================================

/// 一次命令执行（一次一答；大输出 / 交互走 `x.ssh.shell` 的字节流第二刀）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshExecParams {
    /// 会话引用（来自 `x.ssh.connect`）。
    pub session_ref: String,
    /// 要执行的命令（原样交给远端 shell）。
    pub command: String,
    /// 命令超时（秒）；`None` = 插件侧默认（第一刀约定 30s）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u32>,
}

/// 执行结果（一次一答；exit_code + stdout/stderr 分离）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshExecResult {
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stdout: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stderr: String,
}

// ============================================================================
// x.ssh.shell（交互 shell / PTY；字节流承载在第二刀）
// ============================================================================

/// 开一个交互 shell（PTY）。持续双向字节流（stdin/stdout）由第二刀的流式帧承载；
/// 本轮先钉住通道建立契约。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshShellParams {
    /// 会话引用（来自 `x.ssh.connect`）。
    pub session_ref: String,
    /// 终端类型，默认 `xterm`。
    #[serde(default = "default_term")]
    pub term: String,
    /// PTY 列数，默认 80。
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// PTY 行数，默认 24。
    #[serde(default = "default_rows")]
    pub rows: u16,
}

fn default_term() -> String {
    "xterm".into()
}

fn default_cols() -> u16 {
    80
}

fn default_rows() -> u16 {
    24
}

/// 通道建立成功：宿主持 `channel_id` 收流 / 写流（字节流帧第二刀消费它）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshShellResult {
    pub channel_id: String,
}

// ============================================================================
// x.ssh.forward（端口转发；字节流承载在第二刀）
// ============================================================================

/// 本地端口映射到远程（对齐 navop `local_forward`：本地 listen → 隧道 → 远端目标）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshForwardParams {
    /// 会话引用（来自 `x.ssh.connect`）。
    pub session_ref: String,
    /// 远端目标主机，默认 `127.0.0.1`。
    #[serde(default = "default_target_host")]
    pub remote_host: String,
    /// 远端目标端口（必填）。
    pub remote_port: u16,
    /// 本地监听端口；`None` = 插件侧自动分配。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
}

fn default_target_host() -> String {
    "127.0.0.1".into()
}

/// 转发建立成功：宿主实际监听的本地端口（可能与请求不同）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshForwardResult {
    /// 通道引用（`x.ssh.close` 时用它关闭转发）。
    pub channel_id: String,
    /// 实际本地端口。
    pub bound_local_port: u16,
}

// ============================================================================
// x.ssh.close
// ============================================================================

/// 关闭会话（`channel_id = None`）或单个通道（shell / forward）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshCloseParams {
    /// 会话引用（必填；关闭会话会连带关闭其下全部通道）。
    pub session_ref: String,
    /// 只关单个通道时给它的 id；`None` = 关整个会话。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

/// 关闭结果（`closed = false` 表示引用不存在或已关闭 —— 幂等）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshCloseResult {
    pub closed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ssh_methods_have_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for m in ALL_SSH_METHODS {
            assert!(seen.insert(*m), "ALL_SSH_METHODS 里有重复：{m}");
        }
    }

    #[test]
    fn ssh_methods_carry_the_private_prefix() {
        for m in ALL_SSH_METHODS {
            assert!(is_ssh_method(m), "{m} 应属于 x.ssh. 前缀");
            // `x.` 私有前缀：安装期声明必须能过（不能污染协议命名空间）
            assert!(
                crate::sidecar::method::is_allowed_declaration(m),
                "{m} 应能通过安装期声明校验"
            );
        }
    }

    #[test]
    fn non_ssh_methods_are_not_matched() {
        assert!(!is_ssh_method("x.redis.get"));
        assert!(!is_ssh_method("query.execute"));
        assert!(!is_ssh_method(""));
        assert!(!is_ssh_method("host.ssh.open_tunnel"), "反向面不是正向轨");
    }

    #[test]
    fn connect_params_defaults() {
        let p: SshConnectParams =
            serde_json::from_str(r#"{"host":"db.internal","username":"root"}"#).unwrap();
        assert_eq!(p.port, 22);
        assert_eq!(p.timeout_secs, 10);
        assert!(p.secret_ref.is_none());
    }

    #[test]
    fn connect_params_round_trip_with_secret_ref() {
        let p = SshConnectParams {
            host: "db.internal".into(),
            port: 2222,
            username: "root".into(),
            secret_ref: Some("kss://ext/ssh/password".into()),
            timeout_secs: 30,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: SshConnectParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back.port, 2222);
        assert_eq!(back.secret_ref.as_deref(), Some("kss://ext/ssh/password"));
    }

    #[test]
    fn connect_requires_host_and_username() {
        let err = serde_json::from_str::<SshConnectParams>(r#"{"host":"h"}"#).unwrap_err();
        assert!(err.to_string().contains("username"), "{err}");
        let err = serde_json::from_str::<SshConnectParams>(r#"{"username":"u"}"#).unwrap_err();
        assert!(err.to_string().contains("host"), "{err}");
    }

    #[test]
    fn exec_defaults_and_round_trip() {
        let p: SshExecParams =
            serde_json::from_str(r#"{"session_ref":"s1","command":"uptime"}"#).unwrap();
        assert!(p.timeout_secs.is_none());

        let result: SshExecResult = serde_json::from_str(
            r#"{"exit_code":0,"stdout":" 1 user","stderr":""}"#,
        )
        .unwrap();
        assert_eq!(result.exit_code, 0);
        let round: SshExecResult = serde_json::from_str(&serde_json::to_string(&result).unwrap())
            .unwrap();
        assert_eq!(round.stdout, " 1 user");
    }

    #[test]
    fn shell_defaults_and_round_trip() {
        let p: SshShellParams =
            serde_json::from_str(r#"{"session_ref":"s1"}"#).unwrap();
        assert_eq!(p.term, "xterm");
        assert_eq!(p.cols, 80);
        assert_eq!(p.rows, 24);

        let r: SshShellResult = serde_json::from_str(r#"{"channel_id":"c1"}"#).unwrap();
        assert_eq!(r.channel_id, "c1");
    }

    #[test]
    fn forward_defaults_and_round_trip() {
        let p: SshForwardParams =
            serde_json::from_str(r#"{"session_ref":"s1","remote_port":3306}"#).unwrap();
        assert_eq!(p.remote_host, "127.0.0.1");
        assert!(p.local_port.is_none());

        let r: SshForwardResult =
            serde_json::from_str(r#"{"channel_id":"c1","bound_local_port":49152}"#).unwrap();
        assert_eq!(r.bound_local_port, 49152);
    }

    #[test]
    fn close_round_trip() {
        let p: SshCloseParams = serde_json::from_str(r#"{"session_ref":"s1"}"#).unwrap();
        assert!(p.channel_id.is_none());
        let p: SshCloseParams =
            serde_json::from_str(r#"{"session_ref":"s1","channel_id":"c1"}"#).unwrap();
        assert_eq!(p.channel_id.as_deref(), Some("c1"));
    }
}

//! LSP 客户端（JSON-RPC 2.0 over stdio）——宿主侧 provider bridge 的数据源
//!
//! ## 一句话
//!
//! gpui-kit 编辑器自带 LSP provider 接口（六个 trait + `lsp-types` 数据模型）但**不含 client**；
//! 本模块补上"数据源"：一个走 LSP 标准 `Content-Length` 分帧的最小 client
//! （`frame` 编解码 + `client` 请求/通知分发），供宿主把语言服务接进编辑器。
//!
//! ## 与本仓插件系统的边界（别混）
//!
//! - 插件 D5 分帧（`crates/plugin`）= 4B 整帧长 + 1B 种类，面向数据库驱动；
//! - LSP 分帧（本模块）= `Content-Length` 头帧 + JSON-RPC，面向语言服务。
//!   两者协议不同、服务对象不同，不强行统一（`plugin-prototype-design.md` §14.2）。
//!
//! ## 落点与后续
//!
//! 本模块做 client 核心（进程 I/O + 请求/响应关联 + 通知分发）+ provider bridge
//! （[`bridge`]：把请求转发给语言服务的 completion / hover / definition / showDocument，
//! 挂在 `EditorState::lsp_mut()` 的对应槽位上），诊断翻译是纯函数——宿主轮询 [`LspEvent`]
//! 把 `publishDiagnostics` 合并进内核诊断（`DiagnosticSet` 已有
//! `From<lsp_types::Diagnostic>`，可直接 extend）。
//!
//! 槽位现状（都在用，别再当"暂未实现"）：completion / hover / definition /
//! showDocument 经 [`bridge`] 转发；hover 是**单槽**，语言服务可用走
//! [`bridge::LspHoverProvider`]、SQL 模式走自研 [`sql_hover`]，二者互斥（二选一）。
//! `document_color`、`sql_code_action`（槽位可挂多个）、`semantic_tokens` 是 SQL 自研
//! 实现，不经 LSP 转发。

pub mod bridge;
pub mod client;
pub mod document_color;
pub mod frame;
pub mod host;
pub mod language_config;
pub mod sql_code_action;
pub mod sql_definition;
pub mod sql_hover;
pub mod sql_syntax_context;

pub use client::{
    CallError, LspClient, LspEvent, LspHandle, LspNotification, LspReader, LspWriter, ProcessIo,
};
pub use frame::{FrameError, LspMessage, MAX_BODY_BYTES};
pub use host::{CompletionChoice, LspPort, LspPortHandle, LspSession};

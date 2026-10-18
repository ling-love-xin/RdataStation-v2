//! # rds-kernel：Jupyter 内核协议客户端
//!
//! 分析模式的 Python / R / Julia 单元执行走**标准 Jupyter 内核协议**（ZeroMQ 四通道）：
//! 本项目不内嵌浏览器（GPUI 无 DOM），所以不走 webview，直接与 ipykernel / IRkernel /
//! IJulia 等内核进程对话，拿到的富输出（mime bundle）由 editor 侧按 GPUI 能力分发渲染。
//!
//! 职责边界：
//! - [`KernelSession`]：启动内核（kernelspec / 直连）、解析 connection file、维护生命周期
//! - 执行与事件：execute / interrupt / shutdown，事件流（stream / display_data /
//!   execute_result / error / status）经 mpsc 回传给调用方
//! - **不渲染**：mime bundle 的展示由上层（editor 渲染分发器）负责
//!
//! 依赖方向（硬约束）：editor → kernel →（jupyter-zmq-client / jupyter-protocol / tokio）。
//! kernel 不依赖 gpui / editor，可独立测试。

pub mod connection;
pub mod registry;
pub mod session;

pub use registry::{detect_kernels, KernelLanguage, KernelSpec};
pub use session::{KernelEvent, KernelSession, KernelStatus};

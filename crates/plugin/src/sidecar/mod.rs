//! Sidecar 进程管理模块
//!
//! P1 之后的形状（设计与进度见 `docs/architecture/plugin/plugin-dev-plan.md`）：
//!
//! | 模块 | 管什么 |
//! | --- | --- |
//! | [`proto`] | 帧格式（4B 大端整帧长 + 1B 种类）与流读写、版本闸、内联阈值、错误码 |
//! | [`router`] | 附件语义：响应与 Arrow 分片的装配（两个方向互为逆运算） |
//! | [`conn`] | 一条连接上的 JSON-RPC：在飞表、超时放弃、断线交还 |
//! | [`process`] | 起收真实进程：stdio 接线、`current_dir`、stderr 日志、EOF 回收 |
//! | [`lifecycle`] | 三层对象模型的决策内核（sans-io，零 I/O） |
//! | [`supervisor`] | 把内核的动作落到进程上，把崩溃 / 判死 / 通知如实报给宿主 |
//! | [`driver`] | 会话之上的驱动调用：`driver.describe` / `query.execute` / `query.cancel` … |
//!
//! # HTTP / 端口那条路已经删掉
//!
//! 原先的 `client.rs` 与 `manager.rs` 走的是 `reqwest` 打 `http://localhost:<port>`：
//! 零鉴权、装不下 Arrow，与 D5（stdio + 二进制分帧）相反。两个文件在 P1 一并删除，
//! 同时去掉了 `rds-plugin` 对 `reqwest` 的依赖 —— 旧的那份 JSON-RPC 信封也随之下线
//! （信封只该有一份，见 §3.5「契约重复两份」的教训）。
//!
//! [`health_checker`] 与 [`hot_reload_manager`] 仍是空文件：进程探活的基本盘已经由
//! [`supervisor`] 的 `ping_all` 承担；热重载属于后面的事（P3/P5）。

pub mod conn;
pub mod driver;
pub mod factory;
pub mod health_checker;
pub mod hot_reload_manager;
pub mod lifecycle;
pub mod process;
pub mod proto;
pub mod router;
pub mod supervisor;

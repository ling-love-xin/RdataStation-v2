//! 执行（A14）：**执行什么 / 谁来跑 / 跑完放哪**
//!
//! 三件事各自独立，好让其中最易错的那件可以直接断言：
//!
//! | 问题 | 落点 | 形态 |
//! | --- | --- | --- |
//! | 执行什么 | [`resolve_target`] | **纯函数**（选区 > 光标所在语句 > 全部，语句定位走 `engine::sql::split`） |
//! | 谁来跑 | [`QueryRunner`] | 端口（宿主注入：连接池 / 全局单例 / 假实现都由宿主决定） |
//! | 跑完放哪 | [`ExecQueue`] 的结果队列 | 工作线程 → 主线程（GPUI 轮询泵取回） |
//!
//! ## 线程模型
//!
//! 照 `workbench::services::nav_jobs` 的做法：**一条专用工作线程 + mpsc + 主线程轮询**。
//! 界面不等 I/O，也不需要在 UI 线程上 `block_on`（那会冻住窗口）。
//!
//! 这里**不引 tokio**：editor 的依赖表里没有它，而宿主的 [`QueryRunner`] 实现自己负责把
//! 异步驱动跑起来（workbench 的实现内部用 tokio runtime `block_on`）。于是"后台任务"
//! 这件事在本 crate 里就是普普通通的线程 + 队列，单测能直接跑真线程。
//!
//! ## 并发策略（1a → 阶段②）
//!
//! **按连接分组并发**：每个连接一条专属工作线程 + mpsc（懒建），不同连接可同时执行；
//! 同一连接内**排队串行**（单通道天然 FIFO，批量语句在同一 worker 上顺序跑完，不会被别的
//! 提交插队）。未绑定作业（`None`）统一归 `""` 队列——它们最终都落到同一活动连接，天然串行。
//! 忙态按连接计数（[`ExecQueue::is_busy_for`]），取消按连接（[`ExecQueue::cancel`]）。
//! 事务 / 会话亲和（架构 §12 #2）属 1b：那需要 per-session 独占连接，不是这里加锁能解决的。



// 【下移】ResultPlacement / Pushdown 已随结果存储搬到 rds-result-panel（editor 经
// `crate::store` 重导出）：执行层与结果存储共用同一份定义，不再各写一份。
// `pub use` 保持 `crate::execution::ResultPlacement` 旧路径对调用方仍公开。
pub use crate::store::{Pushdown, ResultPlacement};

// ==== 拆分兼容层（2026-10-29）：execution.rs 已拆为 exec/{target,runner,notes,queue}.rs ====
// `crate::exec::execution::*`（即 `crate::execution::*` 旧路径）经以下 re-export 保持不变。
pub use super::notes::{
    CommitNote, CommitReport, DuckDbExportRequest, ExecOutcome, ExportNote, SourceAction, SourceNote,
    SubmitError, TxNote,
};
pub use super::queue::ExecQueue;
pub use super::runner::{QueryData, QueryRunner};
pub use super::target::{
    ExecMenuKind, ExecTarget, RunOptions, SEGMENT_ROWS, TxAction, TxSnapshot, all_target,
    batch_target, resolve_target, statement_target, target_for_menu,
};

#[cfg(test)]
mod tests;

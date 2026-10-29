//! 编辑器宿主面板 —— 执行域（bound_connection/channel/on_execute_sql/run_all/execute/explain_current/tx_action/interrupt/submit_scan/ensure_exec_pump/drain_lsp_events/drain_exec_results）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）


use super::*;
// ==== Step 10 拆分（2026-10-29）：执行域按能力域迁入子模块 ====
mod connection;
mod edit_cmds;
mod row_ops;
mod execute;
mod tx;
mod scan;
mod pump;

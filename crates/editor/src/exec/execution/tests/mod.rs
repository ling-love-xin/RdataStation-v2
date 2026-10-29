    // 安全模式：**不通配导入**
    pub(crate) use super::{
        CommitReport, ExecMenuKind, ExecQueue, ExecTarget, QueryData, QueryRunner, RunOptions,
        SEGMENT_ROWS, SourceAction, CommitNote, ExecOutcome, SourceNote, SubmitError, TxAction, TxNote, TxSnapshot,
        all_target, batch_target, resolve_target, statement_target, target_for_menu,
    };
    pub(crate) use crate::channel::ExecChannel;
    pub(crate) use crate::model::DocumentId;
    // 【下移】ResultPlacement / Pushdown 在 rds-result-panel，经 crate::store 重导出
    pub(crate) use crate::store::ResultPlacement;
    pub(crate) use std::sync::Arc;
    pub(crate) use std::sync::Mutex;
    pub(crate) use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    pub(crate) use std::time::{Duration, Instant};
// ==== Step 6 拆分（2026-10-29）：按主题拆分为子模块 ====
pub(crate) use runner::*;
mod target;
mod runner;
mod queue;
mod tx;

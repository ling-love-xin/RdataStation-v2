//! 编辑器共享状态 —— 执行端口（attach_runner/submit/request_source_action/duckdb 导出/drain_exec）
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use std::sync::Arc;

use crate::execution::{ExecQueue, QueryRunner};

use super::*;

impl EditorShared {

    /// 注入执行器（**宿主调用一次**；重复注入会换掉旧通道，旧通道的工作线程随之退出）
    pub fn attach_runner(&self, runner: Arc<dyn QueryRunner>) {
        *self.exec.borrow_mut() = Some(ExecQueue::new(runner));
    }

    /// 是否接了执行（未接时执行动作要明确说明，而不是静默）
    pub fn has_runner(&self) -> bool {
        self.exec.borrow().is_some()
    }

    /// 提交一次执行（借用边界内完成：不在 `Ref` 存活期间回调调用方）
    ///
    /// 文档绑定的连接（B1）在这里取出并随请求交给执行器：调用方（面板 / 菜单）不需要知道
    /// 连接从哪来，只要知道“执行这份文档”。`placement` 决定结果落到当前结果集还是新结果集（B2）。
    ///
    /// 对 `Option<ExecQueue>` 的借用刻意收在这里：`ExecQueue` 内部是
    /// `Sender + Arc`，调 `submit` 期间不会回到调用方，因此不构成重入风险。
    pub fn submit(
        &self,
        document: crate::model::DocumentId,
        target: &crate::execution::ExecTarget,
        placement: crate::store::ResultPlacement,
        options: crate::execution::RunOptions,
    ) -> Result<(), crate::execution::SubmitError> {
        // 先把文档属性拷出来（不把服务层的 `Ref` 带到下面的借用里）：B1 的连接 + T2c-1 的 schema + B13 的通道
        let (connection, schema, channel) = {
            let service = self.service.borrow();
            (
                service.connection_for(&document),
                service.schema_for(&document),
                service.channel_for(&document),
            )
        };
        let guard = self.exec.borrow();
        let Some(channel_queue) = guard.as_ref() else {
            return Err(crate::execution::SubmitError::NoRunner);
        };
        channel_queue.submit(
            document,
            target,
            connection,
            schema,
            placement,
            options,
            channel,
        )
    }

    /// 【B13】请一个源动作（重挂 / 换主源）
    ///
    /// 与 [`Self::submit`] 同一口径：连接从**服务层**取（调用方不需要知道绑定在哪）。
    pub fn request_source_action(
        &self,
        document: crate::model::DocumentId,
        connection: Option<String>,
        channel: crate::channel::ExecChannel,
        action: crate::execution::SourceAction,
    ) -> Result<(), String> {
        let guard = self.exec.borrow();
        let Some(queue) = guard.as_ref() else {
            return Err("当前未接入执行".to_string());
        };
        queue.request_source_action(document, connection, channel, action)
    }

    /// 【B13】源动作的回执（面板轮询取走）
    pub fn drain_source_notes(&self) -> Vec<crate::execution::SourceNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|queue| queue.drain_source_notes())
            .unwrap_or_default()
    }

    /// 【B7 切片二】请一次 DuckDB 导出（Parquet / XLSX）
    ///
    /// 与 [`Self::request_source_action`] 同一口径：没有执行器就直说（不静默失败）。
    pub fn request_duckdb_export(
        &self,
        request: crate::execution::DuckDbExportRequest,
    ) -> Result<(), String> {
        let guard = self.exec.borrow();
        let Some(queue) = guard.as_ref() else {
            return Err("当前未接入执行".to_string());
        };
        queue.request_duckdb_export(request)
    }

    /// 【B7 切片二】DuckDB 导出的回执（面板轮询取走）
    pub fn drain_export_notes(&self) -> Vec<crate::execution::ExportNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|queue| queue.drain_export_notes())
            .unwrap_or_default()
    }

    /// 结果队列里已完成但尚未取走的执行（轮询泵调用）
    ///
    /// **顺带留一份回执**（[`ExecReceipt`]）：结果归编辑区，回执给宿主。
    pub fn drain_exec(&self) -> Vec<crate::execution::ExecOutcome> {
        let drained = {
            let guard = self.exec.borrow();
            guard
                .as_ref()
                .map(|channel| channel.drain())
                .unwrap_or_default()
        };
        if !drained.is_empty() {
            let mut receipts = self.receipts.borrow_mut();
            for outcome in &drained {
                receipts.push(ExecReceipt {
                    document: outcome.document.clone(),
                    connection: outcome.connection.clone(),
                    succeeded: outcome.result.is_ok(),
                });
            }
            let overflow = receipts.len().saturating_sub(MAX_EXEC_RECEIPTS);
            if overflow > 0 {
                receipts.drain(0..overflow);
            }
        }
        drained
    }

    /// 只取走**本文档**已完成的执行（面板轮询泵调用；多面板共享队列，按文档隔离互不抢回执）。
    ///
    /// 与 [`Self::drain_exec`] 同一回执留档口径：结果归编辑区，回执给宿主；别的文档回执留在
    /// 队列里，等归属面板自己的泵取走。
    pub fn drain_exec_for(
        &self,
        document: &crate::model::DocumentId,
    ) -> Vec<crate::execution::ExecOutcome> {
        let drained = {
            let guard = self.exec.borrow();
            guard
                .as_ref()
                .map(|channel| channel.drain_for(document))
                .unwrap_or_default()
        };
        if !drained.is_empty() {
            let mut receipts = self.receipts.borrow_mut();
            for outcome in &drained {
                receipts.push(ExecReceipt {
                    document: outcome.document.clone(),
                    connection: outcome.connection.clone(),
                    succeeded: outcome.result.is_ok(),
                });
            }
            let overflow = receipts.len().saturating_sub(MAX_EXEC_RECEIPTS);
            if overflow > 0 {
                receipts.drain(0..overflow);
            }
        }
        drained
    }

    /// 取走执行回执（宿主轮询调用；每个回执只交付一次）
    pub fn drain_exec_receipts(&self) -> Vec<ExecReceipt> {
        std::mem::take(&mut self.receipts.borrow_mut())
    }

    /// 【2b】脚本执行的结论（宿主轮询取走；脚本结果不是网格形状，走独立通道）
    pub fn drain_script_notes(&self) -> Vec<crate::env::ScriptNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.drain_script_notes())
            .unwrap_or_default()
    }

    // ===== 【2d】分析笔记：会话级单元序列（每文档一份） =====
}

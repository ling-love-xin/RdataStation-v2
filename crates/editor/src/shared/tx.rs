//! 编辑器共享状态 —— 事务端口（request_transaction/request_commit_changes/is_executing）
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use super::*;

impl EditorShared {

    /// 请一个事务动作（B4）：开始 / 提交 / 回滚；没在跑才接受（理由可读）
    pub fn request_transaction(
        &self,
        document: crate::model::DocumentId,
        action: crate::execution::TxAction,
    ) -> Result<(), String> {
        let connection = self.service.borrow().connection_for(&document);
        let guard = self.exec.borrow();
        match guard.as_ref() {
            Some(channel) => channel.request_transaction(document, action, connection),
            None => Err("当前未接入执行".to_string()),
        }
    }

    /// 事务动作的结论（主线程轮询）
    pub fn drain_tx_notes(&self) -> Vec<crate::execution::TxNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.drain_tx_notes())
            .unwrap_or_default()
    }

    /// 【B4】执行器支不支持事务（界面据此决定摆不摆 TX 区）
    pub fn has_transactions(&self) -> bool {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.supports_transactions())
            .unwrap_or(false)
    }

    /// 【T2a】请一次批量提交（把变更集写回源库）
    ///
    /// 与 [`Self::request_transaction`] 同一口径：连接从**服务层**取（调用方不需要知道绑定在哪），
    /// 没有执行器就直说（不静默失败）。`updates` 是编辑器侧生成好的 UPDATE 文本
    /// （`changes::commit_updates_rows`），`sql` 是原查询（血缘 / 历史），`table` / `key` 供诊断。
    pub fn request_commit_changes(
        &self,
        document: crate::model::DocumentId,
        table: String,
        key: crate::export::RowKey,
        sql: String,
        updates: String,
    ) -> Result<(), String> {
        let (connection, schema, channel) = {
            let service = self.service.borrow();
            (
                service.connection_for(&document),
                service.schema_for(&document),
                service.channel_for(&document),
            )
        };
        let guard = self.exec.borrow();
        match guard.as_ref() {
            Some(queue) => queue.request_commit_changes(
                document,
                connection,
                schema,
                channel,
                table,
                key,
                sql,
                updates,
            ),
            None => Err("当前未接入执行".to_string()),
        }
    }

    /// 【T2a】批量提交的回执（面板轮询取走）
    pub fn drain_commit_notes(&self) -> Vec<crate::execution::CommitNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|queue| queue.drain_commit_notes())
            .unwrap_or_default()
    }

    /// 是否有执行在跑
    pub fn is_executing(&self) -> bool {
        let guard = self.exec.borrow();
        guard.as_ref().is_some_and(|channel| channel.is_busy())
    }
}

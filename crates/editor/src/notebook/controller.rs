//! 【R1】分析模式编排账本 —— 「全部运行」串行队列 + 一次内核执行的归属记录
//!
//! 自 `view/host/notebook.rs` 的控制流抽出（2026-11-23）：这里**只记账与判定**，
//! 不持 `Context`、不发通知、不碰 `EditorShared`。视图按返回值决定 UI 动作
//! （`set_message` / 回写 cells / 起泵 / `cx.notify`）。
//!
//! 两条纪律：
//! - 「全部运行」是**串行队列**：执行队列（`ExecQueue`）一次只跑一个，跑完一个再由
//!   [`NotebookController::next_to_run`] 取下一下个。
//! - 同一时间最多一个内核单元执行（[`KernelRun`]），与 `NotebookState::has_running`
//!   的防并发口径一致；两个收口点（SQL 回执 / 内核收口）都经同一个守卫推进。

use crate::cell::Cell;

/// 一次进行中的内核单元执行（原 `view/host/mod.rs::KernelRun`，字段仍为四个）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelRun {
    /// 目标内核（`ExecutionSource::Kernel` 指定，否则语言默认 id）
    pub kernel_id: String,
    /// 目标单元 id（事件回执按它回填输出）
    pub cell_id: String,
    /// 目标会话 id（切换会话后回执到了不落）
    pub session_id: String,
    /// 【R8】本次 `execute_request` 的 msg_id：事件归属对账，只认这个 id 的输出与收口
    pub msg_id: String,
}

/// 每个编辑器面板一份的编排账本（`Default` 即空闲）
#[derive(Default, Debug, Clone)]
pub struct NotebookController {
    /// 「全部运行」待跑单元 id（队首下一个跑；空 = 没排队）
    queue: Vec<String>,
    /// 正在跑的内核单元（同一时间最多一个）
    kernel_run: Option<KernelRun>,
}

impl NotebookController {
    /// 全部运行入队：可执行语言且正文非空才算（Markdown 备注 / 空单元不入队），
    /// 覆盖旧队列；返回入队数（`0` = 没有可跑单元，调用方据此提示）
    pub fn enqueue_all(&mut self, cells: &[Cell]) -> usize {
        self.queue = cells
            .iter()
            .filter(|cell| cell.language.is_executable() && !cell.body.trim().is_empty())
            .map(|cell| cell.id.clone())
            .collect();
        self.queue.len()
    }

    /// 队列推进：队列空 / 已有单元在执行 → `None`（守卫，防两个收口点重复推进）
    ///
    /// 两个真值来源都查：`has_running` 覆盖 **SQL 执行**（单元在跑，但 controller 没有内核账本），
    /// [`Self::kernel_run`] 覆盖 **内核执行**（账本在手，即便调用方漏传也必须挡住）。
    pub fn next_to_run(&mut self, has_running: bool) -> Option<String> {
        if has_running || self.kernel_run.is_some() || self.queue.is_empty() {
            return None;
        }
        Some(self.queue.remove(0))
    }

    /// 待跑队列（只读；「全部运行」入口判空用）
    pub fn pending(&self) -> &[String] {
        &self.queue
    }

    /// 队列已清空且没有内核执行在跑（可重新开始「全部运行」的判据之一）
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.kernel_run.is_none()
    }

    /// 作废整个队列（重启内核：当前单元被停，后续不该自动跑）
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// 登记一次内核执行开始
    pub fn begin(&mut self, run: KernelRun) {
        self.kernel_run = Some(run);
    }

    /// 内核执行收口（`ExecuteReply` / `Failed`）：账本清空
    pub fn finish(&mut self) {
        self.kernel_run = None;
    }

    /// 当前进行中的内核执行（状态栏回显 / 事件泵判定用）
    pub fn current(&self) -> Option<&KernelRun> {
        self.kernel_run.as_ref()
    }

    /// 会话已切换：作废本次执行并返回它（调用方负责把单元复位 / 清输出）
    pub fn abort_on_session_switch(&mut self) -> Option<KernelRun> {
        self.kernel_run.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Cell, CellLanguage};

    fn run(cell_id: &str) -> KernelRun {
        KernelRun {
            kernel_id: "python".to_string(),
            cell_id: cell_id.to_string(),
            session_id: "s1".to_string(),
            msg_id: "rds-exec-1".to_string(),
        }
    }

    #[test]
    fn enqueue_all_skips_markdown_and_empty() {
        let cells = vec![
            Cell::new("a", CellLanguage::Sql, "select 1"),
            Cell::new("b", CellLanguage::Markdown, "# 说明"),
            Cell::new("c", CellLanguage::Python, "   "),
            Cell::new("d", CellLanguage::Python, "print(1)"),
        ];
        let mut ctrl = NotebookController::default();
        assert_eq!(ctrl.enqueue_all(&cells), 2);
        assert_eq!(ctrl.pending(), &["a".to_string(), "d".to_string()]);
    }

    #[test]
    fn next_to_run_returns_none_while_running() {
        // 区分性证明：去掉 has_running 守卫，这里会返回 Some("a") 造成 SQL 单元并发执行
        let mut ctrl = NotebookController::default();
        ctrl.enqueue_all(&[Cell::new("a", CellLanguage::Sql, "select 1")]);
        assert!(ctrl.next_to_run(true).is_none());
        assert_eq!(ctrl.pending(), &["a".to_string()], "被守卫挡住时不得出队");
    }

    #[test]
    fn next_to_run_returns_none_while_kernel_accounting_says_running() {
        // 区分性证明：只信调用方传的 has_running 时，这一条必红（内核执行中仍会出队）
        let mut ctrl = NotebookController::default();
        ctrl.enqueue_all(&[Cell::new("a", CellLanguage::Python, "print(1)")]);
        ctrl.begin(run("a"));
        assert!(ctrl.next_to_run(false).is_none(), "账本在手时不得推进");
        ctrl.finish();
        assert_eq!(ctrl.next_to_run(false).as_deref(), Some("a"));
    }

    #[test]
    fn next_to_run_pops_in_order_then_none() {
        let mut ctrl = NotebookController::default();
        ctrl.enqueue_all(&[
            Cell::new("a", CellLanguage::Sql, "select 1"),
            Cell::new("b", CellLanguage::Sql, "select 2"),
        ]);
        assert_eq!(ctrl.next_to_run(false).as_deref(), Some("a"));
        assert_eq!(ctrl.next_to_run(false).as_deref(), Some("b"));
        assert!(ctrl.next_to_run(false).is_none());
        assert!(ctrl.pending().is_empty());
    }

    #[test]
    fn finish_clears_kernel_run_and_leads_to_next() {
        let mut ctrl = NotebookController::default();
        ctrl.enqueue_all(&[Cell::new("a", CellLanguage::Python, "print(1)")]);
        ctrl.begin(run("a"));
        assert!(ctrl.current().is_some());
        // 执行中：账本守卫挡住队列推进（即便这里 has_running 传 false）
        assert!(ctrl.next_to_run(false).is_none());
        ctrl.finish();
        assert!(ctrl.current().is_none());
        assert_eq!(ctrl.next_to_run(false).as_deref(), Some("a"));
    }

    #[test]
    fn abort_on_session_switch_returns_previous_run() {
        let mut ctrl = NotebookController::default();
        ctrl.begin(run("a"));
        let aborted = ctrl.abort_on_session_switch().expect("应返回被作废的执行");
        assert_eq!(aborted.cell_id, "a");
        assert!(ctrl.current().is_none());
        assert!(ctrl.abort_on_session_switch().is_none());
    }

    #[test]
    fn clear_drops_queue_but_keeps_running_flag() {
        let mut ctrl = NotebookController::default();
        ctrl.enqueue_all(&[Cell::new("a", CellLanguage::Sql, "select 1")]);
        ctrl.begin(run("a"));
        ctrl.clear();
        assert!(ctrl.pending().is_empty());
        // 清队列不等于停执行：调用方还要 finish()（与视图「重启内核」两步一致）
        assert!(ctrl.current().is_some());
        assert!(!ctrl.is_idle());
        ctrl.finish();
        assert!(ctrl.is_idle());
    }
}

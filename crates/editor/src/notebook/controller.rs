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

use crate::cell::{Cell, CellLanguage, ExecutionSource};
use crate::model::DocumentId;
use rds_kernel::KernelSpec;

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

    /// 【切片 B】跑一个内核单元：空检查 → 解析内核 → 启动（复用）→ 标记 Running →
    /// 提交代码（拿 msg_id）→ 登记账本 → 回写「上次用过的内核」。
    ///
    /// 外部依赖全部经 [`KernelHost`] 注入（视图用 `EditorShared` 实现），这里不碰
    /// `Context` / 不写 cells 表 / 不弹提示。视图只按 [`KernelStart`] 做 UI 收尾：
    ///
    /// - `Started`：把单元状态回写（`mark_running` 已做过）+ `set_message(None)` + 起泵
    /// - `Rejected { reset_cell: false }`：单元**未被改动**，只需提示
    /// - `Rejected { reset_cell: true }`：`execute` 失败前已 `mark_running`，需复位 Idle + 清输出
    pub fn start_kernel_cell(
        &mut self,
        host: &dyn KernelHost,
        document: &DocumentId,
        cell: &mut Cell,
        session_id: &str,
    ) -> KernelStart {
        if cell.body.trim().is_empty() {
            return KernelStart::Rejected {
                reason: format!(
                    "这个 {} 单元是空的，没有可执行的代码",
                    cell.language.label()
                ),
                reset_cell: false,
            };
        }
        let source = cell.execution_source.clone();
        let language = cell.language;
        let spec = match host.resolve_kernel(document, source.as_ref(), language) {
            Ok(spec) => spec,
            Err(reason) => {
                return KernelStart::Rejected {
                    reason,
                    reset_cell: false,
                };
            }
        };
        let kernel_id = spec.id.clone();
        if let Err(reason) = host.ensure_kernel(&spec) {
            return KernelStart::Rejected {
                reason: format!("内核 {} 启动失败：{reason}", spec.display_name),
                reset_cell: false,
            };
        }
        let code = cell.body.clone();
        cell.mark_running();
        // 【R8】先提交、拿到 msg_id 再登记账本：提交失败就不登记（避免空等一个不存在的执行）
        let msg_id = match host.execute(&kernel_id, &code) {
            Ok(id) => id,
            Err(reason) => {
                return KernelStart::Rejected {
                    reason,
                    reset_cell: true,
                };
            }
        };
        let run = KernelRun {
            kernel_id: kernel_id.clone(),
            cell_id: cell.id.clone(),
            session_id: session_id.to_string(),
            msg_id,
        };
        self.begin(run.clone());
        // 执行提交成功 → 回写会话默认（“上次用过的内核”）
        host.set_default_kernel(document, Some(kernel_id));
        KernelStart::Started(run)
    }
}

/// 【切片 B】内核启动序列的外部依赖（由视图用 `EditorShared` 实现，controller 只声明契约）
pub trait KernelHost {
    /// 解析实际内核：显式指定 > 会话默认（上次用过的）> 注册表第一个可用
    fn resolve_kernel(
        &self,
        document: &DocumentId,
        source: Option<&ExecutionSource>,
        language: CellLanguage,
    ) -> Result<KernelSpec, String>;

    /// 启动（或复用）该内核
    fn ensure_kernel(&self, spec: &KernelSpec) -> Result<(), String>;

    /// 提交代码，返回本次执行的 `msg_id`（事件归属对账靠它）
    fn execute(&self, kernel_id: &str, code: &str) -> Result<String, String>;

    /// 回写会话默认内核
    fn set_default_kernel(&self, document: &DocumentId, kernel_id: Option<String>);
}

/// 【切片 B】启动序列的结果（视图按它决定提示 / 回写 / 起泵）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelStart {
    /// 已提交：账本已登记，默认内核已回写
    Started(KernelRun),
    /// 被拒：`reset_cell` 为真表示单元已被标记 Running，调用方必须复位
    Rejected { reason: String, reset_cell: bool },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::CellState;

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

    // ==== 切片 B：内核启动序列（假 host，不碰 shared / 不碰 GPUI） ====

    fn spec() -> KernelSpec {
        KernelSpec {
            id: "python".to_string(),
            display_name: "Python 3".to_string(),
            language: rds_kernel::KernelLanguage::Python,
            command: "python".to_string(),
            ready: true,
            note: None,
            launch: rds_kernel::KernelLaunch::ipykernel("python"),
        }
    }

    struct FakeHost {
        resolved: Result<KernelSpec, String>,
        ensured: Result<(), String>,
        executed: Result<String, String>,
        executes: std::cell::RefCell<Vec<(String, String)>>,
        defaults: std::cell::RefCell<Vec<Option<String>>>,
    }

    impl FakeHost {
        fn ok() -> Self {
            Self {
                resolved: Ok(spec()),
                ensured: Ok(()),
                executed: Ok("rds-exec-1".to_string()),
                executes: std::cell::RefCell::new(Vec::new()),
                defaults: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl KernelHost for FakeHost {
        fn resolve_kernel(
            &self,
            _document: &DocumentId,
            _source: Option<&ExecutionSource>,
            _language: CellLanguage,
        ) -> Result<KernelSpec, String> {
            self.resolved.clone()
        }

        fn ensure_kernel(&self, _spec: &KernelSpec) -> Result<(), String> {
            self.ensured.clone()
        }

        fn execute(&self, kernel_id: &str, code: &str) -> Result<String, String> {
            self.executes
                .borrow_mut()
                .push((kernel_id.to_string(), code.to_string()));
            self.executed.clone()
        }

        fn set_default_kernel(&self, _document: &DocumentId, kernel_id: Option<String>) {
            self.defaults.borrow_mut().push(kernel_id);
        }
    }

    #[test]
    fn empty_body_rejects_without_reset() {
        let host = FakeHost::ok();
        let doc = DocumentId::new("doc");
        let mut cell = Cell::new("a", CellLanguage::Python, "   ");
        let mut ctrl = NotebookController::default();
        match ctrl.start_kernel_cell(&host, &doc, &mut cell, "s1") {
            KernelStart::Rejected { reason, reset_cell } => {
                assert!(!reset_cell, "空单元没被 mark_running，不该要求复位");
                assert!(reason.contains("是空的"), "提示应说明为空：{reason}");
            }
            KernelStart::Started(_) => panic!("空单元不该启动"),
        }
        assert!(ctrl.current().is_none());
        assert_eq!(cell.state, CellState::Idle);
        assert!(host.executes.borrow().is_empty(), "不该提交任何代码");
    }

    #[test]
    fn resolve_failure_rejects_without_reset() {
        let mut host = FakeHost::ok();
        host.resolved = Err("没有可用的 Python 内核".to_string());
        let doc = DocumentId::new("doc");
        let mut cell = Cell::new("a", CellLanguage::Python, "print(1)");
        let mut ctrl = NotebookController::default();
        match ctrl.start_kernel_cell(&host, &doc, &mut cell, "s1") {
            KernelStart::Rejected { reason, reset_cell } => {
                assert!(!reset_cell);
                assert_eq!(reason, "没有可用的 Python 内核", "解析失败原因原样上报");
            }
            KernelStart::Started(_) => panic!("解析失败不该启动"),
        }
        assert_eq!(cell.state, CellState::Idle);
        assert!(host.executes.borrow().is_empty());
        assert!(host.defaults.borrow().is_empty());
    }

    #[test]
    fn execute_failure_asks_for_reset_and_writes_no_accounting() {
        let mut host = FakeHost::ok();
        host.executed = Err("内核 python 未启动".to_string());
        let doc = DocumentId::new("doc");
        let mut cell = Cell::new("a", CellLanguage::Python, "print(1)");
        let mut ctrl = NotebookController::default();
        match ctrl.start_kernel_cell(&host, &doc, &mut cell, "s1") {
            KernelStart::Rejected { reason, reset_cell } => {
                // 区分性证明：execute 前已 mark_running，若不要求复位，单元会永远停「执行中」
                assert!(reset_cell, "提交失败必须让调用方复位");
                assert!(reason.contains("未启动"), "{reason}");
            }
            KernelStart::Started(_) => panic!("提交失败不该启动"),
        }
        assert!(ctrl.current().is_none(), "提交失败不得登记账本");
        assert_eq!(
            cell.state,
            CellState::Running,
            "controller 不代劳复位：复位是调用方按 reset_cell 做的"
        );
        assert!(host.defaults.borrow().is_empty(), "失败不得回写默认内核");
    }

    #[test]
    fn started_registers_run_and_writes_default_kernel() {
        let host = FakeHost::ok();
        let doc = DocumentId::new("doc");
        let mut cell = Cell::new("a", CellLanguage::Python, "print(1)");
        let mut ctrl = NotebookController::default();
        match ctrl.start_kernel_cell(&host, &doc, &mut cell, "s1") {
            KernelStart::Started(run) => {
                assert_eq!(run.kernel_id, "python");
                assert_eq!(run.cell_id, "a");
                assert_eq!(run.session_id, "s1");
                assert_eq!(run.msg_id, "rds-exec-1");
            }
            KernelStart::Rejected { reason, .. } => panic!("不该被拒：{reason}"),
        }
        assert!(ctrl.current().is_some(), "启动成功必须登记账本");
        assert_eq!(cell.state, CellState::Running, "controller 负责 mark_running");
        assert_eq!(
            host.executes.borrow().as_slice(),
            &[("python".to_string(), "print(1)".to_string())]
        );
        assert_eq!(
            host.defaults.borrow().as_slice(),
            &[Some("python".to_string())],
            "成功后回写「上次用过的内核」"
        );
    }
}

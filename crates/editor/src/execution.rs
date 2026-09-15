//! 执行（A14）：**执行什么 / 谁来跑 / 跑完放哪**
//!
//! 三件事各自独立，好让其中最易错的那件可以直接断言：
//!
//! | 问题 | 落点 | 形态 |
//! | --- | --- | --- |
//! | 执行什么 | [`resolve_target`] | **纯函数**（选区 > 光标所在语句 > 全部，语句定位走 `engine::sql::split`） |
//! | 谁来跑 | [`QueryRunner`] | 端口（宿主注入：连接池 / 全局单例 / 假实现都由宿主决定） |
//! | 跑完放哪 | [`ExecChannel`] 的结果队列 | 工作线程 → 主线程（GPUI 轮询泵取回） |
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
//! ## 并发策略（1a）
//!
//! 同一通道**同时只跑一次执行**（[`ExecChannel::submit`] 忙时返回 [`SubmitError::Busy`]）。
//! 事务 / 会话亲和（架构 §12 #2）属 1b：那需要 per-session 独占连接，不是这里加锁能解决的。

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use crate::model::DocumentId;

// ===== 执行什么 =====

/// 一次执行的目标
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecTarget {
    /// 没有可执行的内容（空文档 / 只有注释与空白）
    Empty,
    /// 选区（有非空选区时优先）
    Selection(String),
    /// 光标所在语句（词法级切分定位）
    Statement(String),
    /// 全文（显式的"执行全部"）
    All(String),
}

impl ExecTarget {
    /// 要发给驱动的 SQL（`Empty` 为 `None`）
    pub fn sql(&self) -> Option<&str> {
        match self {
            Self::Empty => None,
            Self::Selection(sql) | Self::Statement(sql) | Self::All(sql) => Some(sql),
        }
    }

    /// 目标标签（状态栏 / 结果区标题用；与原型 §5.1 用词一致）
    pub fn label(&self) -> &'static str {
        match self {
            Self::Empty => "无可执行内容",
            Self::Selection(_) => "选区",
            Self::Statement(_) => "当前语句",
            Self::All(_) => "全部",
        }
    }
}

/// 解析 `Ctrl+Enter` 的执行目标：**选区优先，否则光标所在语句**
///
/// 语句定位是**词法级**的（`engine::sql::split`），不是 `split(';')`——字符串里的分号、
/// 注释里的分号都不会切错（`split.rs` 的既有保证）。
///
/// 光标落在语句之间的空白 / 注释里时，取**其后最近的一条**（"在空行上按执行"跑下面那条，
/// 与主流客户端一致）；其后没有才回退到之前最近的一条。
pub fn resolve_target(text: &str, selection: Range<usize>) -> ExecTarget {
    let selected = slice(text, &selection);
    if let Some(sql) = trimmed(selected) {
        return ExecTarget::Selection(sql.to_string());
    }

    let cursor = selection.start.min(text.len());
    let statements = engine::sql::split_statements(text);
    let statement = statements
        .iter()
        .find(|stmt| stmt.start <= cursor && cursor <= stmt.end)
        .or_else(|| statements.iter().find(|stmt| stmt.start >= cursor))
        .or_else(|| statements.last());

    match statement {
        Some(stmt) => match trimmed(stmt.text(text)) {
            Some(sql) => ExecTarget::Statement(sql.to_string()),
            None => ExecTarget::Empty,
        },
        None => ExecTarget::Empty,
    }
}

/// 执行全部（执行族里的显式入口）
///
/// 与 [`resolve_target`] 同一个判据：**词法级切分出来一条语句都没有**（空文档 / 只有注释与空白）
/// 就不算可执行内容——否则“执行全部”会拿一段注释去 `prepare`，报一个与用户意图无关的错。
pub fn all_target(text: &str) -> ExecTarget {
    if engine::sql::split_statements(text).is_empty() {
        return ExecTarget::Empty;
    }
    match trimmed(text) {
        Some(sql) => ExecTarget::All(sql.to_string()),
        None => ExecTarget::Empty,
    }
}

/// 按字节区间取原文（区间越界或多字节边界不齐时向前收敛，不 panic）
fn slice<'a>(text: &'a str, range: &Range<usize>) -> &'a str {
    let start = floor_char_boundary(text, range.start);
    let end = floor_char_boundary(text, range.end).max(start);
    &text[start..end]
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn trimmed(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

// ===== 谁来跑 =====

/// 一次执行的产物（结果集）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueryData {
    pub columns: Vec<String>,
    /// 行数据（已字符串化；`NULL` 显示为 `NULL`）
    pub rows: Vec<Vec<String>>,
    /// 执行耗时（毫秒，驱动给出的真实值）
    pub elapsed_ms: u64,
    /// 结果是否被截断（超出驱动的行数上限）
    pub truncated: bool,
}

/// 执行端口：**宿主提供"怎么把 SQL 跑出结果集"**
///
/// editor 不依赖任何具体连接来源——全是连接池、全局单例、还是假数据，都归宿主决定
/// （workbench 注入 `EngineQueryRunner`；单测注入假实现）。
///
/// 实现会被**工作线程**调用，因此必须 `Send + Sync`，且**允许阻塞**（异步驱动在这里
/// `block_on`）。取消（`cancel_query`）与超时属 1b，不在这条最小路径上。
pub trait QueryRunner: Send + Sync + 'static {
    fn run(&self, sql: &str) -> Result<QueryData, String>;
}

// ===== 跑完放哪 =====

/// 一次执行的请求（工作线程需要知道"这是哪份文档的哪句话"）
#[derive(Debug, Clone)]
struct ExecRequest {
    document: DocumentId,
    sql: String,
}

/// 一次执行的结论（回到主线程）
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub document: DocumentId,
    /// 实际执行的 SQL（结果区标题与历史用）
    pub sql: String,
    pub result: Result<QueryData, String>,
}

/// 提交被拒的原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    /// 宿主没注入执行器（能力缺失，不是用户错误：要说清楚而不是静默）
    NoRunner,
    /// 已有一次执行在跑（1a 的并发策略）
    Busy,
    /// 没有可执行的内容
    Empty,
}

impl SubmitError {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoRunner => "当前未接入执行",
            Self::Busy => "已有执行在进行中",
            Self::Empty => "没有可执行的内容",
        }
    }
}

/// 执行通道：一条工作线程 + 一个结果队列
///
/// 生命周期：随宿主（面板 / 工作台）存活；`Drop` 时把工作线程放掉（发送端关闭 → 线程退出）。
pub struct ExecChannel {
    tx: Option<Sender<ExecRequest>>,
    done: Arc<Mutex<VecDeque<ExecOutcome>>>,
    busy: Arc<AtomicBool>,
}

impl ExecChannel {
    /// 用一个执行器起通道
    pub fn new(runner: Arc<dyn QueryRunner>) -> Self {
        let (tx, rx) = mpsc::channel::<ExecRequest>();
        let done: Arc<Mutex<VecDeque<ExecOutcome>>> = Arc::new(Mutex::new(VecDeque::new()));
        let busy = Arc::new(AtomicBool::new(false));

        let worker_done = done.clone();
        let worker_busy = busy.clone();
        std::thread::Builder::new()
            .name("rds-editor-exec".to_string())
            // 驱动解析在 debug 下递归较深（与 nav 工作线程同一考虑）
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                while let Ok(request) = rx.recv() {
                    worker_busy.store(true, Ordering::SeqCst);
                    let result = runner.run(&request.sql);
                    if let Ok(mut queue) = worker_done.lock() {
                        queue.push_back(ExecOutcome {
                            document: request.document,
                            sql: request.sql,
                            result,
                        });
                    }
                    worker_busy.store(false, Ordering::SeqCst);
                }
            })
            .expect("failed to spawn editor exec worker");

        Self {
            tx: Some(tx),
            done,
            busy,
        }
    }

    /// 提交一次执行（忙 / 空目标会被拒，**不排队**：排队会让"再按一次"变成隐藏的批量执行）
    pub fn submit(
        &self,
        document: DocumentId,
        target: &ExecTarget,
    ) -> Result<(), SubmitError> {
        let Some(sql) = target.sql() else {
            return Err(SubmitError::Empty);
        };
        if self.is_busy() {
            return Err(SubmitError::Busy);
        }
        let Some(tx) = self.tx.as_ref() else {
            return Err(SubmitError::Busy);
        };
        self.busy.store(true, Ordering::SeqCst);
        if tx
            .send(ExecRequest {
                document,
                sql: sql.to_string(),
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(SubmitError::Busy);
        }
        Ok(())
    }

    /// 取回已完成的执行（主线程轮询；取走即清空）
    pub fn drain(&self) -> Vec<ExecOutcome> {
        let mut queue = match self.done.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 是否有执行在跑（状态栏 / 按钮禁用用）
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
}

impl Drop for ExecChannel {
    fn drop(&mut self) {
        // 关掉发送端 → 工作线程 `recv` 返回 Err → 线程自然退出
        self.tx = None;
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        ExecChannel, ExecTarget, QueryData, QueryRunner, SubmitError, all_target, resolve_target,
    };
    use crate::model::DocumentId;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    // ===== 执行目标解析 =====

    #[test]
    fn selection_wins_over_the_statement_under_the_cursor() {
        let text = "select 1;\nselect 2;";
        // 光标在第一句里，但有选区（第二句的一部分）
        let target = resolve_target(text, 10..18);
        assert_eq!(target, ExecTarget::Selection("select 2".to_string()));
        assert_eq!(target.label(), "选区");
    }

    #[test]
    fn blank_selection_falls_back_to_the_statement_under_the_cursor() {
        let text = "select 1;\nselect 2;";
        // 光标在第二句里，选区是空白
        let target = resolve_target(text, 12..12);
        assert_eq!(target, ExecTarget::Statement("select 2".to_string()));
        assert_eq!(target.label(), "当前语句");
    }

    #[test]
    fn cursor_inside_the_first_statement_picks_it() {
        let text = "select 1;\nselect 2;";
        let target = resolve_target(text, 3..3);
        assert_eq!(target, ExecTarget::Statement("select 1".to_string()));
    }

    #[test]
    fn cursor_in_the_gap_picks_the_next_statement() {
        // 光标停在第 1、2 句之间的空行上 → 跑下面那条（主流客户端行为）
        let text = "select 1;\n\nselect 2;";
        let target = resolve_target(text, 10..10);
        assert_eq!(target, ExecTarget::Statement("select 2".to_string()));
    }

    #[test]
    fn cursor_after_the_last_statement_keeps_it() {
        let text = "select 1;\nselect 2;";
        let target = resolve_target(text, text.len()..text.len());
        assert_eq!(target, ExecTarget::Statement("select 2".to_string()));
    }

    #[test]
    fn semicolons_inside_strings_do_not_split_the_statement() {
        let text = "select ';' as a;\nselect 2";
        let target = resolve_target(text, 5..5);
        assert_eq!(target, ExecTarget::Statement("select ';' as a".to_string()));
    }

    #[test]
    fn comments_only_document_has_nothing_to_run() {
        let text = "-- 只是注释\n\n";
        assert_eq!(resolve_target(text, 5..5), ExecTarget::Empty);
        assert!(resolve_target(text, 5..5).sql().is_none());
        assert_eq!(all_target(text), ExecTarget::Empty);
    }

    #[test]
    fn empty_document_has_nothing_to_run() {
        assert_eq!(resolve_target("", 0..0), ExecTarget::Empty);
    }

    #[test]
    fn all_target_keeps_the_whole_script() {
        let text = "select 1;\nselect 2;";
        assert_eq!(all_target(text), ExecTarget::All(text.to_string()));
        assert_eq!(all_target(text).sql(), Some(text));
        assert_eq!(all_target(text).label(), "全部");
    }

    #[test]
    fn selection_out_of_range_does_not_panic() {
        let text = "select 1";
        // 越界 + 落在多字节字符中间的选区都要能收敛
        let target = resolve_target("select '中文'", 7..99);
        assert_eq!(target, ExecTarget::Selection("'中文'".to_string()));
        assert_eq!(resolve_target(text, 100..200), ExecTarget::Statement(text.to_string()));
    }

    // ===== 执行通道 =====

    /// 假执行器：记录调用次数，返回固定结果（按 SQL 决定成败）
    struct FakeRunner {
        calls: Arc<AtomicUsize>,
    }

    impl QueryRunner for FakeRunner {
        fn run(&self, sql: &str) -> Result<QueryData, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if sql.contains("boom") {
                return Err("驱动报错：boom".to_string());
            }
            Ok(QueryData {
                columns: vec!["n".to_string()],
                rows: vec![vec!["1".to_string()]],
                elapsed_ms: 7,
                truncated: false,
            })
        }
    }

    fn channel() -> (ExecChannel, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let channel = ExecChannel::new(Arc::new(FakeRunner {
            calls: calls.clone(),
        }));
        (channel, calls)
    }

    /// 轮询等待结果（带超时：通道坏掉时测试要失败而不是挂住）
    fn wait(channel: &ExecChannel) -> Vec<super::ExecOutcome> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let done = channel.drain();
            if !done.is_empty() {
                return done;
            }
            assert!(Instant::now() < deadline, "后台执行迟迟没有结果");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn submitted_sql_comes_back_with_its_outcome() {
        let (channel, calls) = channel();
        let document = DocumentId::new("doc-test");

        let target = ExecTarget::Statement("select 1".to_string());
        channel.submit(document.clone(), &target).expect("提交");

        let done = wait(&channel);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "执行器应被调用一次");
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].document, document, "结果要认领回原文档");
        assert_eq!(done[0].sql, "select 1");
        let data = done[0].result.as_ref().expect("应当成功");
        assert_eq!(data.columns, vec!["n".to_string()]);
        assert_eq!(data.rows, vec![vec!["1".to_string()]]);
        assert_eq!(data.elapsed_ms, 7, "耗时用驱动给的真实值");
        assert!(!channel.is_busy(), "结束后必须回到空闲");
    }

    #[test]
    fn driver_errors_come_back_as_errors_not_panics() {
        let (channel, _calls) = channel();
        let target = ExecTarget::Statement("select boom".to_string());
        channel.submit(DocumentId::new("doc-err"), &target).expect("提交");

        let done = wait(&channel);
        let error = done[0].result.as_ref().expect_err("应当失败");
        assert!(error.contains("boom"), "{error}");
        assert!(!channel.is_busy());
    }

    #[test]
    fn empty_targets_are_rejected_before_touching_the_runner() {
        let (channel, calls) = channel();
        let error = channel
            .submit(DocumentId::new("doc-empty"), &ExecTarget::Empty)
            .expect_err("空目标应被拒");
        assert_eq!(error, SubmitError::Empty);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "不该打扰执行器");
        assert!(!channel.is_busy());
    }

    #[test]
    fn a_second_submit_is_refused_while_one_is_running() {
        // 慢执行器：卡住第一次调用，确保第二次提交撞上忙状态
        struct SlowRunner {
            release: Arc<Mutex<bool>>,
        }
        impl QueryRunner for SlowRunner {
            fn run(&self, _sql: &str) -> Result<QueryData, String> {
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if *self.release.lock().unwrap() {
                        return Ok(QueryData::default());
                    }
                    assert!(Instant::now() < deadline, "假执行器没被放行");
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }

        let release = Arc::new(Mutex::new(false));
        let channel = ExecChannel::new(Arc::new(SlowRunner {
            release: release.clone(),
        }));
        let document = DocumentId::new("doc-slow");
        let target = ExecTarget::Statement("select 1".to_string());

        channel.submit(document.clone(), &target).expect("首次提交");
        // 等它真的进到忙状态
        let deadline = Instant::now() + Duration::from_secs(5);
        while !channel.is_busy() {
            assert!(Instant::now() < deadline, "执行没开始");
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(
            channel.submit(document, &target).expect_err("忙时应被拒"),
            SubmitError::Busy
        );

        *release.lock().unwrap() = true;
        let done = wait(&channel);
        assert_eq!(done.len(), 1, "被拒的那次不该产生结果");
        assert_eq!(SubmitError::Busy.message(), "已有执行在进行中");
    }

    #[test]
    fn dropping_the_channel_lets_the_worker_exit() {
        let calls = Arc::new(AtomicUsize::new(0));
        let channel = ExecChannel::new(Arc::new(FakeRunner {
            calls: calls.clone(),
        }));
        drop(channel);
        // 线程退出是异步的：这里只要求不 panic、不阻塞进程退出
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

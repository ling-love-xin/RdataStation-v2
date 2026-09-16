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
//! 同一通道**同时只跑一个作业**（[`ExecChannel::submit`] 忙时返回 [`SubmitError::Busy`]）。
//! 一个作业可以带多条语句（[`ExecTarget::Batch`]）：它们在**同一条工作线程上顺序**跑完，
//! 每条语句各自回填一份结果——因此"批量"既不是驱动能力的猜测，也不会中途被别的提交插队。
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
    /// 批量（B2）：**逐条独立**的语句列表，一条一次执行、失败不中断
    ///
    /// 与 `All` 的差别不是"内容多少"而是**语义**：`All` 是整篇一次性发给驱动（驱动自己决定
    /// 怎么处理多语句），`Batch` 是在客户端切成独立请求逐条跑——所以它对驱动没有额外要求，
    /// 也因此能给出"每句一个结果集"。
    Batch(Vec<String>),
}

impl ExecTarget {
    /// 要发给驱动的 SQL（`Empty` 与 `Batch` 为 `None`；后者看 [`Self::statements`]）
    pub fn sql(&self) -> Option<&str> {
        match self {
            Self::Empty | Self::Batch(_) => None,
            Self::Selection(sql) | Self::Statement(sql) | Self::All(sql) => Some(sql),
        }
    }

    /// 这次执行要跑的语句（**逐条独立**；`Batch` 之外都是一条）
    pub fn statements(&self) -> Vec<String> {
        match self {
            Self::Empty => Vec::new(),
            Self::Selection(sql) | Self::Statement(sql) | Self::All(sql) => vec![sql.clone()],
            Self::Batch(list) => list.clone(),
        }
    }

    /// 目标标签（状态栏 / 结果区标题用；与原型 §5.1 用词一致）
    pub fn label(&self) -> &'static str {
        match self {
            Self::Empty => "无可执行内容",
            Self::Selection(_) => "选区",
            Self::Statement(_) => "当前语句",
            Self::All(_) => "全部",
            Self::Batch(_) => "批量",
        }
    }
}

/// 结果落到哪里（B2）：执行族里「批量执行」与「在新结果标签中执行」的差别只在这一维
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultPlacement {
    /// 替换当前选中的结果集（1a 口径：一次执行一份结果）
    Replace,
    /// 追加一个新结果集，**原结果集仍保持选中**（原型 §4.4：V1 常驻 `＋` 按钮的语义——
    /// “别动我正在看的东西，新的那份放旁边”）
    NewSet,
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

    statement_target(text, selection.start.min(text.len()))
}

/// 只看光标所在语句（**忽略选区**）：执行族的「执行当前语句」项用它
///
/// 与 [`resolve_target`] 的差别只在“有选区时”：菜单里选“执行当前语句”是明确意图，
/// 不该被顺手选中的一段文字改掉（那是“执行选区”的语义）。
pub fn statement_target(text: &str, cursor: usize) -> ExecTarget {
    let cursor = cursor.min(text.len());
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

/// 执行族菜单项（原型 §5.1 的五项）
///
/// 后两项（批量 / 新结果标签）都是 `target × placement` 的组合：前者换目标（切成一堆独立请求），
/// 后者换落位（结果进新结果集）。执行计划属 B10——没实现就不放进菜单
/// （“只宣传不实现”是原型 §2.2 的明确排除项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMenuKind {
    /// 光标所在语句（忽略选区）
    CurrentStatement,
    /// 只执行选区（没选区时置灰）
    Selection,
    /// 整篇脚本
    All,
    /// 批量：逐条独立执行（语句不足两条时置灰——没有“批”可言）
    Batch,
    /// 在新结果标签中执行（目标同主按钮：选区优先 → 当前语句）
    NewSet,
}

impl ExecMenuKind {
    /// 菜单顺序（原型 §5.1 的次序；渲染与断言共用一份）
    pub const ALL: [Self; 5] = [
        Self::CurrentStatement,
        Self::Selection,
        Self::All,
        Self::Batch,
        Self::NewSet,
    ];

    /// 菜单文案（与原型 §5.1 用词一致）
    pub fn label(self) -> &'static str {
        match self {
            Self::CurrentStatement => "执行当前语句",
            Self::Selection => "执行选区",
            Self::All => "执行全部",
            Self::Batch => "批量执行（逐条独立）",
            Self::NewSet => "在新结果标签中执行",
        }
    }

    /// 这一项要不要落到新结果集
    pub fn placement(self) -> ResultPlacement {
        match self {
            // 批量每句一个结果集：都往新结果集里放（原结果集留着回看）
            Self::Batch | Self::NewSet => ResultPlacement::NewSet,
            Self::CurrentStatement | Self::Selection | Self::All => ResultPlacement::Replace,
        }
    }

    /// 当前光标 / 选区 / 语句数下这个菜单项能不能用（置灰比“点了没反应”好）
    pub fn is_available(self, selection: &Range<usize>, statements: usize) -> bool {
        match self {
            Self::Selection => selection.end > selection.start,
            Self::Batch => statements >= 2,
            Self::CurrentStatement | Self::All | Self::NewSet => true,
        }
    }
}

/// 按菜单项解析目标（**显式目标**：不做“选区优先”推断）
///
/// 与 [`resolve_target`] 的分工：快捷键路径按“选区优先”猜用户意图，菜单路径是用户
/// 已经说清楚了要执行什么——两者都收敛到 [`ExecTarget`]，执行通道只有一条。
///
/// 例外是 `NewSet`：它只换落位，**目标是主按钮那一套**（选区优先），因此直接复用
/// [`resolve_target`]。
pub fn target_for_menu(kind: ExecMenuKind, text: &str, selection: Range<usize>) -> ExecTarget {
    match kind {
        ExecMenuKind::CurrentStatement => statement_target(text, selection.start),
        ExecMenuKind::Selection => match trimmed(slice(text, &selection)) {
            Some(sql) => ExecTarget::Selection(sql.to_string()),
            None => ExecTarget::Empty,
        },
        ExecMenuKind::All => all_target(text),
        ExecMenuKind::Batch => batch_target(text),
        ExecMenuKind::NewSet => resolve_target(text, selection),
    }
}

/// 批量目标：把脚本按语句切开，每句一条独立请求（**失败不中断**由执行通道保证）
///
/// 切分用 [`engine::sql::split_statements`]（词法级，与“执行当前语句”同一套判据），
/// 空白 / 注释段被丢掉：它们不是可执行内容，拿去 `prepare` 只会得到与用户意图无关的错。
/// 语句**前面的**注释跟着它一起走（`split` 的既有口径）——注释是合法 SQL，
/// 且这样批量执行与“执行当前语句”看到的是同一份文本。
pub fn batch_target(text: &str) -> ExecTarget {
    let statements: Vec<String> = engine::sql::split_statements(text)
        .iter()
        .filter_map(|statement| trimmed(statement.text(text)).map(str::to_string))
        .collect();
    if statements.is_empty() {
        return ExecTarget::Empty;
    }
    ExecTarget::Batch(statements)
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
    /// `connection` = 本文档绑定的连接 id（B1）；`None` = 未绑定，由实现决定回退口径
    /// （workbench 的实现回退到“当前活动连接”，与 1a 一致）
    fn run(&self, connection: Option<&str>, sql: &str) -> Result<QueryData, String>;
}

// ===== 跑完放哪 =====

/// 一次执行的请求（工作线程需要知道"这是哪份文档在哪个连接上的哪几句话"）
///
/// 一条 job 可以带多条语句（批量）：工作线程**顺序**跑完它们，逐条回填结论——这样忙标记
/// 覆盖整批，中途不会被误判成"跑完了"。
#[derive(Debug, Clone)]
struct ExecJob {
    document: DocumentId,
    /// 文档绑定的连接（B1；`None` = 未绑定）
    connection: Option<String>,
    /// 要跑的语句（逐条独立）
    statements: Vec<String>,
    /// 结果落到哪里（B2）
    placement: ResultPlacement,
}

/// 一次执行的结论（回到主线程）：**一条语句一条结论**
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub document: DocumentId,
    /// 实际使用的连接（B1）；`None` = 未绑定（跟随当前活动连接）
    pub connection: Option<String>,
    /// 实际执行的 SQL（结果区标题与历史用）
    pub sql: String,
    /// 这份结果怎么落位（B2）：批量 / 新标签执行都给 `NewSet`
    pub placement: ResultPlacement,
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
    tx: Option<Sender<ExecJob>>,
    done: Arc<Mutex<VecDeque<ExecOutcome>>>,
    busy: Arc<AtomicBool>,
}

impl ExecChannel {
    /// 用一个执行器起通道
    pub fn new(runner: Arc<dyn QueryRunner>) -> Self {
        let (tx, rx) = mpsc::channel::<ExecJob>();
        let done: Arc<Mutex<VecDeque<ExecOutcome>>> = Arc::new(Mutex::new(VecDeque::new()));
        let busy = Arc::new(AtomicBool::new(false));

        let worker_done = done.clone();
        let worker_busy = busy.clone();
        std::thread::Builder::new()
            .name("rds-editor-exec".to_string())
            // 驱动解析在 debug 下递归较深（与 nav 工作线程同一考虑）
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    // 忙标记覆盖**整批**：批量的语句之间不是“空闲”，否则面板会在中途收起
                    // 「执行中」并允许第二次提交（那就成了隐藏的并行执行）。
                    worker_busy.store(true, Ordering::SeqCst);
                    for sql in job.statements {
                        // 连接透传给执行器（B1）：`None` = 未绑定 → 执行器自己决定回退口径
                        let result = runner.run(job.connection.as_deref(), &sql);
                        if let Ok(mut queue) = worker_done.lock() {
                            queue.push_back(ExecOutcome {
                                document: job.document.clone(),
                                connection: job.connection.clone(),
                                sql,
                                placement: job.placement,
                                result,
                            });
                        }
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
    ///
    /// `connection` 是**文档绑定的连接**（B1）；`None` = 未绑定，执行器回退到“当前活动连接”。
    /// `placement` 决定结果落到当前结果集还是新结果集（B2）。
    pub fn submit(
        &self,
        document: DocumentId,
        target: &ExecTarget,
        connection: Option<String>,
        placement: ResultPlacement,
    ) -> Result<(), SubmitError> {
        let statements = target.statements();
        if statements.is_empty() {
            return Err(SubmitError::Empty);
        }
        if self.is_busy() {
            return Err(SubmitError::Busy);
        }
        let Some(tx) = self.tx.as_ref() else {
            return Err(SubmitError::Busy);
        };
        self.busy.store(true, Ordering::SeqCst);
        if tx
            .send(ExecJob {
                document,
                connection,
                statements,
                placement,
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
        ExecChannel, ExecMenuKind, ExecTarget, QueryData, QueryRunner, ResultPlacement, SubmitError,
        all_target, batch_target, resolve_target, statement_target, target_for_menu,
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

    /// 菜单里选“执行当前语句”是明确意图：**有选区也不改目标**（那是“执行选区”的语义）
    #[test]
    fn statement_target_ignores_an_active_selection() {
        let text = "select 1;\nselect 2;";
        // 光标在第一句里，但同时有选区（第二句）——两条路径必须给出不同目标
        assert_eq!(
            resolve_target(text, 10..18),
            ExecTarget::Selection("select 2".to_string())
        );
        assert_eq!(
            statement_target(text, 3),
            ExecTarget::Statement("select 1".to_string())
        );
        // 越界光标不 panic，收敛到末句（与 resolve_target 同一口径）
        assert_eq!(
            statement_target(text, 9999),
            ExecTarget::Statement("select 2".to_string())
        );
        assert_eq!(statement_target("-- 只有注释\n", 3), ExecTarget::Empty);
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

    /// 菜单是**显式目标**：各项各自独立，不互相推断；“执行选区”没选区就置灰
    #[test]
    fn menu_items_resolve_their_own_targets() {
        let text = "select 1;\nselect 2;";
        // 只选中第二句的一部分：菜单里的“当前语句”应当给**整句**，
        // “执行选区”才给那段片段（后者会报语法错，但那正是用户当下选的东西）
        let partial = 12..18;
        let statements = batch_target(text).statements().len();

        assert_eq!(
            target_for_menu(ExecMenuKind::CurrentStatement, text, partial.clone()),
            ExecTarget::Statement("select 2".to_string()),
            "菜单选“当前语句”时不得去执行半截选区"
        );
        assert_eq!(
            target_for_menu(ExecMenuKind::Selection, text, partial.clone()),
            ExecTarget::Selection("lect 2".to_string())
        );
        assert_eq!(
            target_for_menu(ExecMenuKind::All, text, partial.clone()),
            ExecTarget::All(text.to_string())
        );

        assert!(ExecMenuKind::Selection.is_available(&partial, statements));
        assert!(!ExecMenuKind::Selection.is_available(&(4..4), statements));
        assert!(ExecMenuKind::CurrentStatement.is_available(&(4..4), statements));
        assert!(ExecMenuKind::All.is_available(&(4..4), statements));

        // 五项文案彼此不同（菜单上看不出区别就是没实现），顺序也固定
        let labels: Vec<&str> = ExecMenuKind::ALL.iter().map(|kind| kind.label()).collect();
        assert_eq!(labels.len(), 5);
        assert!(labels.iter().all(|label| !label.is_empty()));
        let unique: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len(), "菜单项文案不得重复：{labels:?}");

        // 空文档 / 只有注释：各项都得是 Empty，不得拿注释去 prepare
        let comments = "-- 只是注释\n";
        for kind in ExecMenuKind::ALL {
            assert_eq!(
                target_for_menu(kind, comments, 3..3),
                ExecTarget::Empty,
                "{kind:?} 对纯注释文档应无可执行内容"
            );
        }
    }

    /// 批量：按语句切开、**逐条独立**（空白与纯注释段不是可执行内容，不进列表）
    #[test]
    fn batch_target_splits_the_script_into_independent_statements() {
        let text = "select 1;\n\nselect 2;\nselect 3";
        let target = batch_target(text);
        assert_eq!(
            target.statements(),
            vec![
                "select 1".to_string(),
                "select 2".to_string(),
                "select 3".to_string()
            ]
        );
        assert_eq!(target.label(), "批量");
        assert!(target.sql().is_none(), "批量不走单条 SQL 那条路");

        // 语句**前面**的注释跟着它一起走（与“执行当前语句”拿到的是同一份文本）
        let commented = batch_target("select 1;\n-- 说明\nselect 2;");
        assert_eq!(commented.statements().len(), 2);
        assert!(
            commented.statements()[1].ends_with("select 2"),
            "注释属于它后面那句：{:?}",
            commented.statements()
        );

        // 字符串里的分号不算切分点（与“执行当前语句”同一套词法级判据）
        assert_eq!(
            batch_target("select ';' as a; select 2").statements().len(),
            2
        );

        // 不足两条语句 / 纯注释：拿不到“批”
        assert_eq!(batch_target("select 1").statements().len(), 1);
        assert!(
            !ExecMenuKind::Batch.is_available(&(4..4), 1),
            "只有一句时“批量执行”该置灰"
        );
        assert!(ExecMenuKind::Batch.is_available(&(4..4), 2));
        assert_eq!(batch_target("-- 只是注释"), ExecTarget::Empty);
    }

    /// 落位：批量与新标签执行都往新结果集放；其余三项换掉当前那份
    #[test]
    fn batch_and_new_set_land_in_new_result_sets() {
        assert_eq!(ExecMenuKind::Batch.placement(), ResultPlacement::NewSet);
        assert_eq!(ExecMenuKind::NewSet.placement(), ResultPlacement::NewSet);
        for kind in [
            ExecMenuKind::CurrentStatement,
            ExecMenuKind::Selection,
            ExecMenuKind::All,
        ] {
            assert_eq!(kind.placement(), ResultPlacement::Replace, "{kind:?}");
        }

        // “在新结果标签中执行”只换落位：目标就是主按钮那一套（选区优先）
        let text = "select 1;\nselect 2;";
        assert_eq!(
            target_for_menu(ExecMenuKind::NewSet, text, 10..18),
            resolve_target(text, 10..18)
        );
        assert_eq!(
            target_for_menu(ExecMenuKind::NewSet, text, 3..3),
            ExecTarget::Statement("select 1".to_string())
        );
        assert!(
            ExecMenuKind::NewSet.is_available(&(4..4), 0),
            "没选区也能新开一份结果"
        );
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

    /// 假执行器看到的连接序列（B1 断言用）
    type SeenConnections = Arc<Mutex<Vec<Option<String>>>>;

    /// 假执行器：记录调用次数与收到的连接，返回固定结果（按 SQL 决定成败）
    struct FakeRunner {
        calls: Arc<AtomicUsize>,
        seen_connections: SeenConnections,
    }

    impl QueryRunner for FakeRunner {
        fn run(&self, connection: Option<&str>, sql: &str) -> Result<QueryData, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen_connections
                .lock()
                .expect("锁")
                .push(connection.map(str::to_string));
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

    fn channel() -> (ExecChannel, Arc<AtomicUsize>, SeenConnections) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen_connections = Arc::new(Mutex::new(Vec::new()));
        let channel = ExecChannel::new(Arc::new(FakeRunner {
            calls: calls.clone(),
            seen_connections: seen_connections.clone(),
        }));
        (channel, calls, seen_connections)
    }

    /// 轮询等待结果（带超时：通道坏掉时测试要失败而不是挂住）
    fn wait(channel: &ExecChannel) -> Vec<super::ExecOutcome> {
        wait_for(channel, 1)
    }

    /// 轮询等待 N 条结果（批量：语句逐条回来，只等第一条会把后面的漏掉）
    fn wait_for(channel: &ExecChannel, count: usize) -> Vec<super::ExecOutcome> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut collected = Vec::new();
        while collected.len() < count {
            collected.extend(channel.drain());
            if collected.len() >= count {
                return collected;
            }
            assert!(
                Instant::now() < deadline,
                "只等到 {} 条结果（期望 {count} 条）",
                collected.len()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        collected
    }

    /// 等通道回到空闲（忙标记在工作线程上清，与回填不同步，不能立刻断言）
    fn wait_until_idle(channel: &ExecChannel) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while channel.is_busy() {
            assert!(Instant::now() < deadline, "通道迟迟没回到空闲");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn submitted_sql_comes_back_with_its_outcome() {
        let (channel, calls, seen) = channel();
        let document = DocumentId::new("doc-test");

        // 绑定连接（B1）：通道要把它原样交给执行器
        let target = ExecTarget::Statement("select 1".to_string());
        channel
            .submit(
                document.clone(),
                &target,
                Some("P_orders".to_string()),
                ResultPlacement::Replace,
            )
            .expect("提交");

        let done = wait(&channel);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "执行器应被调用一次");
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].document, document, "结果要认领回原文档");
        assert_eq!(done[0].sql, "select 1");
        assert_eq!(done[0].connection.as_deref(), Some("P_orders"));
        assert_eq!(
            seen.lock().expect("锁").as_slice(),
            [Some("P_orders".to_string())],
            "执行器必须收到文档绑定的连接（而不是“当前活动连接”）"
        );
        let data = done[0].result.as_ref().expect("应当成功");
        assert_eq!(data.columns, vec!["n".to_string()]);
        assert_eq!(data.rows, vec![vec!["1".to_string()]]);
        assert_eq!(data.elapsed_ms, 7, "耗时用驱动给的真实值");
        assert!(!channel.is_busy(), "结束后必须回到空闲");
    }

    #[test]
    fn driver_errors_come_back_as_errors_not_panics() {
        let (channel, _calls, _seen) = channel();
        let target = ExecTarget::Statement("select boom".to_string());
        channel
            .submit(
                DocumentId::new("doc-err"),
                &target,
                None,
                ResultPlacement::Replace,
            )
            .expect("提交");

        let done = wait(&channel);
        let error = done[0].result.as_ref().expect_err("应当失败");
        assert!(error.contains("boom"), "{error}");
        wait_until_idle(&channel);
    }

    /// 批量：三条语句 → 三条结论，顺序不变；**中间那条失败不中断**后面的
    #[test]
    fn a_batch_runs_every_statement_and_a_failure_does_not_stop_the_rest() {
        let (channel, calls, _seen) = channel();
        let document = DocumentId::new("doc-batch");
        let target = batch_target("select 1;\nselect boom;\nselect 3;");

        channel
            .submit(
                document.clone(),
                &target,
                Some("P_orders".to_string()),
                ResultPlacement::NewSet,
            )
            .expect("提交批量");

        let done = wait_for(&channel, 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3, "三条语句各跑一次");
        let sqls: Vec<&str> = done.iter().map(|outcome| outcome.sql.as_str()).collect();
        assert_eq!(sqls, ["select 1", "select boom", "select 3"], "顺序按脚本");
        assert!(done[0].result.is_ok());
        assert!(done[1].result.is_err(), "中间那条要如实报错");
        assert!(done[2].result.is_ok(), "失败不该中断后面的语句");
        assert!(
            done.iter()
                .all(|outcome| outcome.placement == ResultPlacement::NewSet
                    && outcome.document == document),
            "每条各自一个结果集，且都认领回原文档"
        );
        wait_until_idle(&channel);
    }

    /// 批量的忙标记覆盖**整批**：语句之间不是空闲（否则第二条提交能插进来，变成隐藏的并行执行）
    #[test]
    fn a_batch_stays_busy_between_its_statements() {
        /// 每句慢 60ms：足够在第一条回填之后、整批跑完之前观察到忙状态
        struct SlowStatementRunner;
        impl QueryRunner for SlowStatementRunner {
            fn run(&self, _connection: Option<&str>, _sql: &str) -> Result<QueryData, String> {
                std::thread::sleep(Duration::from_millis(60));
                Ok(QueryData::default())
            }
        }

        let channel = ExecChannel::new(Arc::new(SlowStatementRunner));
        let target = batch_target("select 1; select 2; select 3;");
        channel
            .submit(
                DocumentId::new("doc-busy-batch"),
                &target,
                None,
                ResultPlacement::NewSet,
            )
            .expect("提交");

        assert_eq!(wait_for(&channel, 1).len(), 1, "第一条回来了");
        assert!(
            channel.is_busy(),
            "整批没跑完就不算空闲（否则面板会收起“执行中”并允许第二次提交）"
        );
        assert_eq!(wait_for(&channel, 2).len(), 2, "剩下的两条接着回来");
        wait_until_idle(&channel);
    }

    #[test]
    fn empty_targets_are_rejected_before_touching_the_runner() {
        let (channel, calls, _seen) = channel();
        let error = channel
            .submit(
                DocumentId::new("doc-empty"),
                &ExecTarget::Empty,
                None,
                ResultPlacement::Replace,
            )
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
            fn run(&self, _connection: Option<&str>, _sql: &str) -> Result<QueryData, String> {
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

        channel
            .submit(
                document.clone(),
                &target,
                None,
                ResultPlacement::Replace,
            )
            .expect("首次提交");
        // 等它真的进到忙状态
        let deadline = Instant::now() + Duration::from_secs(5);
        while !channel.is_busy() {
            assert!(Instant::now() < deadline, "执行没开始");
            std::thread::sleep(Duration::from_millis(2));
        }

        assert_eq!(
            channel
                .submit(document, &target, None, ResultPlacement::Replace)
                .expect_err("忙时应被拒"),
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
            seen_connections: Arc::new(Mutex::new(Vec::new())),
        }));
        drop(channel);
        // 线程退出是异步的：这里只要求不 panic、不阻塞进程退出
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

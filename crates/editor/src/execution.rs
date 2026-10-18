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
//! ## 并发策略（1a）
//!
//! 同一通道**同时只跑一个作业**（[`ExecQueue::submit`] 忙时返回 [`SubmitError::Busy`]）。
//! 一个作业可以带多条语句（[`ExecTarget::Batch`]）：它们在**同一条工作线程上顺序**跑完，
//! 每条语句各自回填一份结果——因此"批量"既不是驱动能力的猜测，也不会中途被别的提交插队。
//! 事务 / 会话亲和（架构 §12 #2）属 1b：那需要 per-session 独占连接，不是这里加锁能解决的。

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use crate::channel::ExecChannel;
use crate::export::RowKey;
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
    /// 【B5b】取下一段：对**同一条原 SQL** 取 `offset` 之后的 `limit` 行
    ///
    /// 目标与 `Statement` 的区别不在“内容多少”而在**取数方式**：它不动当前结果集的位置，
    /// 新抓到的行**接在**选中那份的后面（[`ResultPlacement::Append`]）。
    Segment {
        sql: String,
        offset: usize,
        limit: usize,
    },
    /// 【B14】下发源库：把筛选词翻成一条 **WHERE 重查**（结果落**新结果集**）
    ///
    /// 与 `Segment` 一样，编辑器只表达意图：**怎么拼 WHERE 是执行器的事**——方言差异
    /// （CAST 目标类型）、去不去 LIMIT、ORDER BY 怎么摆，都得知道连接才能定。
    /// 这里带的是拼条件需要的原料：原 SQL、筛选词、结果集的列名。
    Filtered {
        sql: String,
        filter: String,
        columns: Vec<String>,
    },
    /// 【B14】排序下发：按用户点的那一列重查（结果落**新结果集**）
    ///
    /// 与筛选下发同构：编辑器只发意图，改写与方言差异都在执行器那边。
    /// `column` 用的是**结果集的列名**（原查询的输出列），不是列下标。
    SortedDown {
        sql: String,
        column: String,
        descending: bool,
    },
    /// 【B15】本地 DuckDB 分析：对**选中那份结果的已抓行**跑一条聚合 SQL（落**新结果集**）
    ///
    /// 与另两个下发都不同：它**不碰源库**（数据是桥接过来的行），所以通道闸 / 项目只读闸
    /// 对它一律放行（那两道管的是“写源库对象”）。
    Analysis(crate::analysis::AnalysisRequest),
    /// 【2b】通用模式的脚本执行：把代码交给**外部解释器**（环境 id + 代码；执行器负责 spawn）
    ///
    /// 与 `Analysis` 同款“编辑器只表达意图”口径：**进程怎么跑、超时怎么杀，都是执行器的事**
    /// （`QueryRunner::run_script`），编辑器不猜解释器命令与平台差异（Windows 的 `taskkill /T`）。
    /// 结果不是网格形状（stdout 文本），走独立的 `ScriptNote` 通道回主线程。
    Script(crate::env::ScriptRequest),
    /// 【2d】分析模式的单元执行：跑一个 **SQL 单元**（会话 id + 单元 id + SQL 正文）
    ///
    /// 与 `Script` 同款“编辑器只表达意图”口径：**怎么跑、连接怎么回退、通道怎么分流**都是执行器
    /// 的事（`QueryRunner::run_cell`，宿主复用 1b 执行链）。只有 SQL 单元会提交执行（T2d-4）；
    /// 结果不是停靠面板的形状（内联输出），走独立的 `CellNote` 通道回主线程。
    Cell(crate::cell::CellRequest),
    /// 【T2a】批量提交：把结果集变更集写回源库（每行一条 UPDATE，**单事务**）
    ///
    /// `updates` 是编辑器侧生成好的 UPDATE 文本（`changes::commit_updates_rows`），
    /// `sql` 是产生这份结果的**原查询**（血缘 / 历史用；提交本身不重跑它）；
    /// `table` / `key` 留给执行器做诊断与事务文案。
    ///
    /// 与 `Filtered` / `SortedDown` 一样只发意图与原料：**执行与事务是执行器的事**
    /// （`QueryRunner::commit_changes`），编辑器不猜方言与连接池。
    Commit {
        /// 原查询 SQL（血缘 / 历史）
        sql: String,
        /// 写入的目标表（`export::default_table_name` 的同一份猜测，展示用）
        table: String,
        /// 行定位依据（键列；`updates` 里的 `WHERE` 就是按它拼的）
        key: RowKey,
        /// 生成的 UPDATE 语句文本（逐条执行，同一事务）
        updates: String,
    },
}

impl ExecTarget {
    /// 【B15】分析目标带的载荷（其他目标为 `None`）
    pub fn analysis(&self) -> Option<&crate::analysis::AnalysisRequest> {
        match self {
            Self::Analysis(request) => Some(request),
            _ => None,
        }
    }

    /// 【2b】脚本目标带的载荷（其他目标为 `None`）
    pub fn script(&self) -> Option<&crate::env::ScriptRequest> {
        match self {
            Self::Script(request) => Some(request),
            _ => None,
        }
    }

    /// 【2d】单元目标带的载荷（其他目标为 `None`）
    pub fn cell(&self) -> Option<&crate::cell::CellRequest> {
        match self {
            Self::Cell(request) => Some(request),
            _ => None,
        }
    }
}

/// 分段抓取的一段多少行（B5b；计划口径：固定 1000 行/段）
pub const SEGMENT_ROWS: usize = 1000;

impl ExecTarget {
    /// 要发给驱动的 SQL（`Empty` 与 `Batch` 为 `None`；后者看 [`Self::statements`]）
    pub fn sql(&self) -> Option<&str> {
        match self {
            Self::Empty | Self::Batch(_) => None,
            Self::Selection(sql) | Self::Statement(sql) | Self::All(sql) => Some(sql),
            Self::Segment { sql, .. }
            | Self::Filtered { sql, .. }
            | Self::SortedDown { sql, .. } => Some(sql),
            // 分析 SQL 不发给源库驱动（它在本地 DuckDB 上跑，见 `analysis`）
            Self::Analysis(_) => None,
            // 脚本没有 SQL（走 `run_script`）
            Self::Script(_) => None,
            // 单元走 `run_cell`（SQL 正文在 `CellRequest` 里，不走 `sql`）
            Self::Cell(_) => None,
            // 提交不走 `run`（UPDATE 文本走 `commit_changes`），原查询只作血缘记录
            Self::Commit { .. } => None,
        }
    }

    /// 这次执行要跑的语句（**逐条独立**；`Batch` 之外都是一条）
    pub fn statements(&self) -> Vec<String> {
        match self {
            Self::Empty => Vec::new(),
            Self::Selection(sql) | Self::Statement(sql) | Self::All(sql) => vec![sql.clone()],
            Self::Batch(list) => list.clone(),
            Self::Segment { sql, .. }
            | Self::Filtered { sql, .. }
            | Self::SortedDown { sql, .. } => {
                vec![sql.clone()]
            }
            // 分析 SQL 走同一条“一句一条结论”的路（它不经过通道闸，见 `analysis`）
            Self::Analysis(request) => vec![request.sql.clone()],
            // 脚本不归 `statements`（它走 `run_script`，不数语句）
            Self::Script(_) => Vec::new(),
            // 单元不归 `statements`（它走 `run_cell`，SQL 正文在 `CellRequest` 里）
            Self::Cell(_) => Vec::new(),
            // 提交的语句由执行器按 `updates` 逐条跑，不归 `statements`（它会进 `run` 通道）
            Self::Commit { .. } => Vec::new(),
        }
    }

    /// 取下一段要的 `(offset, limit)`（其他目标为 `None`）
    pub fn segment(&self) -> Option<(usize, usize)> {
        match self {
            Self::Segment { offset, limit, .. } => Some((*offset, *limit)),
            _ => None,
        }
    }

    /// 【B14】下发源库要的原料：`(原 SQL, 筛选词, 结果集的列)`
    pub fn filtered(&self) -> Option<(&str, &str, &[String])> {
        match self {
            Self::Filtered {
                sql,
                filter,
                columns,
            } => Some((sql, filter, columns)),
            _ => None,
        }
    }

    /// 【B14】排序下发要的原料：`(原 SQL, 列名, 是否降序)`
    pub fn sorted_down(&self) -> Option<(&str, &str, bool)> {
        match self {
            Self::SortedDown {
                sql,
                column,
                descending,
            } => Some((sql, column, *descending)),
            _ => None,
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
            Self::Segment { .. } => "取下一段",
            Self::Filtered { .. } => "下发筛选",
            Self::SortedDown { .. } => "排序下发",
            Self::Analysis(_) => "本地分析",
            Self::Script(_) => "脚本执行",
            Self::Cell(_) => "单元执行",
            Self::Commit { .. } => "批量提交",
        }
    }

    /// 【B15】血缘摘要：这份结果是**怎么来的**（结果工具栏的来源段；原型 §2.4）
    ///
    /// 与 [`Self::label`] 分开：标签说的是“用户点的是哪个动作”（当前语句 / 选区 / 批量），
    /// 血缘说的是“这份数据由什么产生”——四个下发动作各自有名，其余都归「原查询」。
    /// 有自定义标题的结果（执行计划 / 分析）在界面上用标题当来源，不在这里猜。
    pub fn lineage(&self) -> &'static str {
        match self {
            Self::Segment { .. } => "取下一段",
            Self::Filtered { .. } => "下发筛选",
            Self::SortedDown { .. } => "排序下发",
            Self::Analysis(_) => "本地分析",
            Self::Script(_) => "脚本执行",
            Self::Cell(_) => "单元执行",
            // 提交是写回动作，血缘说清“这份是改回源库的结果”——与下发类同源口径
            Self::Commit { .. } => "本地写回",
            // 普通执行（含选区 / 全部 / 批量）：结果就是那句查询的原始输出
            Self::Empty
            | Self::Selection(_)
            | Self::Statement(_)
            | Self::All(_)
            | Self::Batch(_) => "原查询",
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
    /// 【B5b】把这一段**接在选中那份后面**（取下一段：不新开、不替换）
    ///
    /// 接的前提是列形状一致（同一份结果的下一段）；形状变了就当一份新结果落下。
    Append,
}

/// 一次执行的附加选项（B4）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunOptions {
    /// 本次执行要不要（在没有事务时）**自动开一个事务**——「自动提交」关掉时为真
    pub use_transaction: bool,
}

/// 会话的事务状态快照（B4）
///
/// 跟着**每次执行结果**与**每个事务动作**一起回来：界面不必再单独问一句（问就要阻塞等待）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TxSnapshot {
    /// 这个连接上有没有活动事务
    pub in_transaction: bool,
}

/// 事务动作（B4）：三种都要走**驱动**的事务接口，而不是拼 `BEGIN` / `COMMIT` 文本
///
/// 理由（P0.2 实测）：MySQL 的显式 `BEGIN` 会被 prepared 协议拒绝（1295），
/// 且池里每次取到的未必是同一条物理连接——拼文本的“事务”只是看着像。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxAction {
    Begin,
    Commit,
    Rollback,
}

impl TxAction {
    /// 动作文案（状态栏菜单 / 消息用）
    pub fn label(self) -> &'static str {
        match self {
            Self::Begin => "开启事务",
            Self::Commit => "提交",
            Self::Rollback => "回滚",
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
    /// 【Q6】每列的**驱动原始类型名**（`bigint` / `numeric(38,10)` / `timestamp with time zone` …）
    ///
    /// **空 vec = 驱动报不出类型**（不是“没有列”）：此时结果集按`无类型档`渲染——
    /// 左对齐、默认字体、表头不挂类型标签，与列类型化之前的行为一字不差。
    pub column_types: Vec<String>,
    /// 行数据（已字符串化；`NULL` 显示为 `NULL`）
    pub rows: Vec<Vec<String>>,
    /// 执行耗时（毫秒，驱动给出的真实值）
    pub elapsed_ms: u64,
    /// 结果是否被截断（超出驱动的行数上限）
    pub truncated: bool,
    /// 写语句的真实影响行数（B5；只有驱动报了这个值才是 `Some`）
    pub affected_rows: Option<u32>,
    /// 【B5b】这一段是不是**拿满了**（拿满 = 可能还有下一段；"还有没有"只靠这个判）
    pub has_more: bool,
    /// 【B14】这次执行附带的一句话说明（如“已去掉原查询的 LIMIT”）；没有就是 `None`
    pub notice: Option<String>,
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
    ///
    /// 【T2c-1】`schema` = 本文档绑定的 schema（`None` = 未选 → 用连接默认 schema）。
    /// 必须**逐执行**传递而不是靠连接级状态：同一连接可被多个文档绑定到不同 schema，
    /// 隐式状态会串（架构 §12 #2 的“会话亲和”教训）。实现须在发语句前把它应用为
    /// 会话上下文（引擎 `SqlExecuteOptions.schema`）。
    ///
    /// 【B13】`channel` = 本文档的**执行通道**（源库 / 本地加速 / 联邦）：回答“这条语句
    /// 到底发到哪儿”。实现必须**按它分流**——源库档走驱动，加速档走 DuckDB 上的附加源；
    /// 不分流就等于把用户的“执行位置”当成了装饰。
    ///
    /// `options.use_transaction`（B4）= 本次执行先自动开一个事务（「自动提交」关闭时）。
    fn run(
        &self,
        connection: Option<&str>,
        schema: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        options: RunOptions,
    ) -> Result<QueryData, String>;

    /// 这个连接上的事务状态（B4）
    ///
    /// 在**工作线程**上调用（可以阻塞），每次执行与每个事务动作之后各问一次。
    /// 默认实现 = 不知道 → 报“不在事务里”（宿主没接事务能力时的真实状态）。
    fn transaction_snapshot(&self, _connection: Option<&str>) -> TxSnapshot {
        TxSnapshot::default()
    }

    /// 事务动作（B4）：走驱动的事务接口。默认实现 = 不支持事务。
    fn transaction(&self, _connection: Option<&str>, _action: TxAction) -> Result<(), String> {
        Err("当前执行器不支持事务".to_string())
    }

    /// 这个执行器支不支持事务（B4）：界面据此决定**要不要摆 TX 区**
    ///
    /// 与全仓口径一致：能力没有就不摆控件，而不是摆一个点了没用的（原型 §2.2 排除项）。
    fn supports_transactions(&self) -> bool {
        false
    }

    /// 中断这个连接上正在跑的查询（B3）
    ///
    /// 语义分三种，都要能给人看：
    /// - `Ok(true)`：确实送到了取消（有的驱动是发 `KILL QUERY` 这类往返）；
    /// - `Ok(false)`：没有可中断的查询（已经在两句之间了）——不是错误，但要说出来；
    /// - `Err(reason)`：中断失败。
    ///
    /// 与 [`Self::run`] 一样在**工作线程**上调用（实现里可以阻塞）：通道会起一条一次性线程
    /// 来调它，UI 线程不等。默认实现 = 不支持中断（宿主没接能力的真实状态）。
    fn cancel(&self, _connection: Option<&str>) -> Result<bool, String> {
        Err("当前执行器不支持中断".to_string())
    }

    /// 【M8】文档**未绑定连接**时，执行会落到哪条连接上（`None` = 也解析不出）
    ///
    /// 为什么放在这里：这层语义**已经属于执行器**（`resolve_conn_id`：绑定优先 →
    /// 回退当前活动连接）。洞察入口要「产生这份结果的那条连接」时不能自己再猜一份，
    /// 否则会出现“执行走 A、洞察取样走 B”。
    ///
    /// **同步**返回是刻意的：入口要在 UI 回调里立刻决定“能不能给”（菜单项得有/没有），
    /// 而实现手里通常有一个现成的 runtime（真实现见 workbench 的 `EngineQueryRunner`）。
    /// 默认实现 = 解析不出（宿主没接能力的真实状态）。
    fn active_connection(&self) -> Option<String> {
        None
    }

    /// 【B14】下发源库：把筛选词拼成 `WHERE` 重查（**方言差异由实现负责**）
    ///
    /// `columns` 是结果集的列名（筛选命中的就是这些输出列）。默认实现 = 不支持下发
    /// （宿主没接能力的真实状态，不是静默失败）。
    fn run_filtered(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _filter: &str,
        _columns: &[String],
    ) -> Result<QueryData, String> {
        Err("当前执行器不支持下发筛选".to_string())
    }

    /// 【B14】排序下发：按某一列（结果集的列名）重查
    ///
    /// 默认实现与 [`Self::run_filtered`] 同口径：不支持就说出来。
    fn run_sorted_down(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _column: &str,
        _descending: bool,
    ) -> Result<QueryData, String> {
        Err("当前执行器不支持排序下发".to_string())
    }

    /// 【B5b】取下一段：`sql` 是**原 SQL**（不是上一段套了窗口的那句），
    /// `offset` = 已经拿到的行数
    ///
    /// 与 [`Self::run`] 一样在**工作线程**上调用。默认实现 = 不支持（宿主没接能力的真实状态，
    /// 与 `cancel` 同一口径：不假装成功）。
    fn fetch_next(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _offset: usize,
        _limit: usize,
    ) -> Result<QueryData, String> {
        Err("当前执行器不支持分段抓取".to_string())
    }

    /// 【T2a】批量提交：把编辑器生成的 UPDATE 文本**逐条执行在同一个事务里**
    ///
    /// 与 [`Self::run`] 同口径：在**工作线程**上调用（可阻塞）。单事务由实现负责
    /// （事务接口在 [`Self::transaction`]：逐条跑、全部成功 `COMMIT`、失败 `ROLLBACK`）。
    /// 每条 UPDATE 的影响行数 0 = **冲突**（源库里的旧值已被别人改过）——收集进
    /// [`CommitReport::conflicts`] 而不是当成错误：界面要区分"改到了"与"没改到"。
    ///
    /// `updates` 是编辑器侧生成好的语句文本（`changes::commit_updates_rows`）；
    /// 实现**不得改写**它（键列 / 值字面量口径在编辑器，执行器只负责跑与收影响行数）。
    /// 默认实现 = 不支持批量提交（宿主没接能力的真实状态，如实说出来）。
    fn commit_changes(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _table: &str,
        _key: &RowKey,
        _updates: &str,
    ) -> Result<CommitReport, String> {
        Err("当前执行器不支持批量提交".to_string())
    }

    /// 【B13】重新挂载源（**表清单刷新**）：源库新建的表要重挂才看得见
    ///
    /// - 加速档：`alias` 忽略（就那一条源），重挂它；
    /// - 联邦档：`alias` 给了就重挂那一条（源清单里的行级动作），`None` 就逐源全挂。
    ///
    /// 与 [`Self::run`] 一样在**旁路线程**上调用（`ATTACH` 是 I/O）。
    /// `Ok(说明)` 的那句话会显示在状态栏——它要说清“挂了哪个源 / 几张表”。
    /// 默认实现 = 不支持（没接本地加速的执行器）。
    fn refresh_sources(
        &self,
        _connection: Option<&str>,
        _channel: crate::channel::ExecChannel,
        _alias: Option<&str>,
    ) -> Result<String, String> {
        Err("当前执行器未接入本地加速".to_string())
    }

    /// 【联邦】切换主源（未限定名从此在它里面解析）
    ///
    /// 源清单里的「设为主源」走它；与重挂一样在旁路线程上调用。
    /// 默认实现 = 不支持（没接联邦的执行器）。
    fn set_federated_primary(
        &self,
        _connection: Option<&str>,
        _alias: &str,
    ) -> Result<String, String> {
        Err("当前执行器未接入联邦查询".to_string())
    }

    /// 【B15】本地 DuckDB 分析：对桥接过来的行跑一条聚合 SQL（**不碰源库**）
    ///
    /// 默认实现 = 不支持（宿主没接分析能力的真实状态，不是静默失败）。
    fn analyze(&self, _request: &crate::analysis::AnalysisRequest) -> Result<QueryData, String> {
        Err("当前执行器不支持本地分析".to_string())
    }

    /// 【2b】通用模式的脚本执行：把代码交给外部解释器（spawn + stdin + 超时 + 杀进程树）
    ///
    /// 默认实现 = 不支持（宿主没接脚本执行能力的真实状态，不是静默失败）。
    /// 与 [`Self::run`] 一样在**工作线程**上调用（实现里可以阻塞）。
    fn run_script(&self, _request: &crate::env::ScriptRequest) -> Result<crate::env::ScriptOutcome, String> {
        Err("当前执行器不支持脚本执行".to_string())
    }

    /// 【2d】分析模式的单元执行：跑一个 **SQL 单元**（复用 1b 执行链，宿主实现）
    ///
    /// 默认实现 = 不支持（宿主没接单元执行能力的真实状态，不是静默失败）。
    /// 与 [`Self::run_script`] 一样在**工作线程**上调用（实现里可以阻塞）。
    fn run_cell(&self, _request: &crate::cell::CellRequest) -> Result<crate::cell::CellOutcome, String> {
        Err("当前执行器不支持单元执行".to_string())
    }

    /// 【B7 切片二】经 DuckDB 把行落成 Parquet / XLSX（`COPY … TO …`）
    ///
    /// 返回写出去的行数。默认实现 = 不支持（宿主没接 DuckDB 导出的真实状态）。
    /// 这里**允许阻塞**：它在一次性线程上跑（XLSX 首次装扩展要联网几秒），UI 不等。
    fn export_via_duckdb(&self, _request: &DuckDbExportRequest) -> Result<usize, String> {
        Err("当前执行器不支持导出 Parquet / XLSX".to_string())
    }
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
    /// 【T2c-1】文档绑定的 schema（`None` = 未选 → 连接默认）；提交时从服务层拷出
    schema: Option<String>,
    /// 要跑的语句（逐条独立）
    statements: Vec<String>,
    /// 结果落到哪里（B2）
    placement: ResultPlacement,
    /// 执行选项（B4：自动提交关 → 本次执行进事务）
    options: RunOptions,
    /// 【B13】这次执行走哪个通道（回填结论时原样带回去）
    channel: crate::channel::ExecChannel,
    /// 【B5b】取下一段的 `(offset, limit)`；`Some` 时走 `fetch_next` 而不是 `run`
    segment: Option<(usize, usize)>,
    /// 【B14】下发源库的筛选词与列名；`Some` 时走 `run_filtered`
    filtered: Option<(String, Vec<String>)>,
    /// 【B14】排序下发的列名与方向；`Some` 时走 `run_sorted_down`
    sorted_down: Option<(String, bool)>,
    /// 【B15】本地分析的载荷；`Some` 时走 `analyze`（且**只一条**）
    analysis: Option<crate::analysis::AnalysisRequest>,
    /// 【2b】脚本执行的载荷；`Some` 时走 `run_script`（且**只一条**，不走 statements 循环）
    script: Option<crate::env::ScriptRequest>,
    /// 【2d】单元执行的载荷；`Some` 时走 `run_cell`（且**只一条**，不走 statements 循环）
    cell: Option<crate::cell::CellRequest>,
    /// 【B15】血缘摘要（回填时写到结果条目上；工具栏的来源段）
    lineage: &'static str,
}

/// 【B13】源清单上的一个动作（旁路线程执行；不产结果集）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceAction {
    /// 重挂**全部**源（加速档就是那一条；联邦档是逐源重挂）
    RefreshAll,
    /// 重挂某一个源（表清单刷新）
    RefreshSource { alias: String },
    /// 换主源（未限定名从此在它里面解析）
    SetPrimary { alias: String },
}

impl SourceAction {
    /// 动作名（状态栏文案 / 测试断言共用）
    pub fn label(&self) -> String {
        match self {
            Self::RefreshAll => "重新挂载源".to_string(),
            Self::RefreshSource { alias } => format!("重新挂载源 {alias}"),
            Self::SetPrimary { alias } => format!("把 {alias} 设为主源"),
        }
    }
}

/// 【B13】一次源动作的回执
///
/// 它**不产结果集**（是挂载维护，不是查询），所以走与事务动作同一条“一次性线程 + 回执”
/// 的旁路，而不是执行队列。
#[derive(Debug, Clone)]
pub struct SourceNote {
    pub document: DocumentId,
    pub action: SourceAction,
    /// `Ok` = 那句可读的“做到了什么”（如“已重挂 mysql_src（42 张表）”）；`Err` = 失败原因
    pub result: Result<String, String>,
}

/// 【B7 切片二】经 DuckDB 落盘的一次导出（Parquet / XLSX）
///
/// 为什么不与 CSV / JSON / INSERT 一样在事件路径上同步写完：这条路要 DuckDB 的 `COPY`，
/// 而 XLSX 首次要 `INSTALL excel`（联网一次；探针实测本机 3–11 秒）——在 UI 线程上等它
/// 就是界面假死。所以走**一次性线程 + 回执队列**（与源动作同一套），**不占执行位**
/// （它不产结果集，也不该把“执行中”灯点亮）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuckDbExportRequest {
    pub document: DocumentId,
    /// 结果集序号（1 基；回执用「结果 N」指代，与标签同口径）
    pub set: usize,
    pub format: crate::export::ExportFormat,
    pub path: std::path::PathBuf,
    pub columns: Vec<String>,
    /// 行（已字符串化，与网格一致：`NULL` 是字面量；桥接成 JSON 在执行器里做）
    pub rows: Vec<Vec<String>>,
    /// 这次导的是**筛后的行集**（回执按 B7 的口径要明示）
    pub filtered: bool,
}

/// 【B7 切片二】一次 DuckDB 导出的回执
#[derive(Debug, Clone)]
pub struct ExportNote {
    pub document: DocumentId,
    pub format: crate::export::ExportFormat,
    pub path: std::path::PathBuf,
    /// 这次导的是筛后的行集（回执要按 B7 的口径明示）
    pub filtered: bool,
    /// 成功时是写出去的行数，失败时是原因
    pub result: Result<usize, String>,
}

/// 一次执行的结论（回到主线程）：**一条语句一条结论**
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub document: DocumentId,
    /// 实际使用的连接（B1）；`None` = 未绑定（跟随当前活动连接）
    pub connection: Option<String>,
    /// 【B13】这次执行走的是哪个通道（结果集要记住它：界面据此打徽标、切通道后标灰）
    pub channel: crate::channel::ExecChannel,
    /// 实际执行的 SQL（结果区标题与历史用）
    pub sql: String,
    /// 这份结果怎么落位（B2）：批量 / 新标签执行都给 `NewSet`
    pub placement: ResultPlacement,
    /// 执行之后这个连接上的事务状态（B4）
    pub transaction: TxSnapshot,
    /// 【B15】这份结论是不是本地分析的产物（结果集要据此不摆“刷新”）
    pub analysis: bool,
    /// 【B15】血缘摘要（“原查询” / “下发筛选” / “排序下发” / “取下一段” / “本地分析”）
    pub lineage: &'static str,
    pub result: Result<QueryData, String>,
}

/// 一次事务动作的结论（回到主线程）
#[derive(Debug, Clone)]
pub struct TxNote {
    pub document: DocumentId,
    pub action: TxAction,
    /// 动作结果（`Err` = 失败原因）+ 动作之后的状态；界面两个都要
    pub result: Result<TxSnapshot, String>,
}

/// 【T2a】一次批量提交的结论（回到主线程）
///
/// 与 [`TxNote`] 同套：请求在一次性线程上做（端口允许阻塞），回执经
/// [`ExecQueue::drain_commit_notes`] 取回。`sql` / `table` / `key` 是面板回执时
/// 用来定位「这份提交属于哪个结果集」的原料。
#[derive(Debug, Clone)]
pub struct CommitNote {
    pub document: DocumentId,
    pub connection: Option<String>,
    pub channel: crate::channel::ExecChannel,
    /// 原查询 SQL（结果集定位 + 历史）
    pub sql: String,
    /// 写入的目标表（展示用）
    pub table: String,
    /// 行定位依据（键列；回执要按它标冲突格）
    pub key: RowKey,
    /// 提交结果（`Err` = 整体失败原因；`Ok` = 明细回执）
    pub result: Result<CommitReport, String>,
}

/// 【T2a】批量提交的回执（回到主线程）
///
/// 与 [`ExecOutcome`] 分开：提交不改结果集内容（成功的那几行由面板**本地面应用**，
/// 重查会丢排序/筛选/选中），所以它带的是"改了几行 / 哪些冲突"这种**提交专用**结论。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReport {
    /// 成功落库的 UPDATE 条数（影响行数 ≥ 1 的）
    pub committed: usize,
    /// 键值 NULL 而跳过的行数（生成时已报告过，回执再合一次，界面不用自己数）
    pub skipped: usize,
    /// 逐条影响行数（**按 `updates` 语句顺序**；执行器只报数，不猜行/列）
    ///
    /// 为什么这个字段在这里：执行器拿到的是拼接好的 UPDATE 文本，不知道每条对应结果集
    /// 的哪一行哪一列（那个映射在编辑器生成 `commit_updates_rows` 时才有）；`conflicts`
    /// 的行/列由面板拿 `per_statement` + 生成时的映射组装（T2a-6），执行器不编造。
    pub per_statement: Vec<usize>,
    /// 影响行数 0 的冲突：`(数据行号, 变更列, 说明)`——界面要标出这一格"没改到"
    ///
    /// 本字段由面板在组装回执时填（见 [`Self::per_statement`]）；执行器返回的这份里
    /// 保持空——0 行语句的信息都在 `per_statement` 里，没有丢。
    pub conflicts: Vec<(usize, usize, String)>,
}

impl CommitReport {
    /// 有没有任何一条 UPDATE 真正生效（提交失败的判断依据）
    pub fn nothing_committed(&self) -> bool {
        self.committed == 0
    }
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

/// 执行队列：一条工作线程 + 一个结果队列
///
/// 生命周期：随宿主（面板 / 工作台）存活；`Drop` 时把工作线程放掉（发送端关闭 → 线程退出）。
///
/// **与「执行通道」（[`crate::channel::ExecChannel`]：源库 / 加速 / 联邦）不是一回事**：那个回答
/// “在哪儿跑”，这个负责“把作业交给线程并收结论”。名字分开是为了不让两件事读起来像同一件。
pub struct ExecQueue {
    tx: Option<Sender<ExecJob>>,
    done: Arc<Mutex<VecDeque<ExecOutcome>>>,
    busy: Arc<AtomicBool>,
    /// 正在跑的作业的连接（B3）：中断要知道该取消哪个连接（`busy` 为真时才有意义）
    running_connection: Arc<Mutex<Option<String>>>,
    /// 已请求中断（B3）：批量里**剩余语句**据此标"已取消"，而不是接着往下跑
    cancel_requested: Arc<AtomicBool>,
    /// 中断尝试的结果文案（主线程轮询取走：中断失败 / 没在跑 都要留痕）
    cancel_notes: Arc<Mutex<VecDeque<String>>>,
    /// 事务动作的结论（主线程轮询取走：B4）
    tx_notes: Arc<Mutex<VecDeque<TxNote>>>,
    /// 【B13】重新挂载加速源的回执（主线程轮询取走）
    source_notes: Arc<Mutex<VecDeque<SourceNote>>>,
    /// 【B7 切片二】DuckDB 导出（Parquet / XLSX）的回执（主线程轮询取走）
    export_notes: Arc<Mutex<VecDeque<ExportNote>>>,
    /// 【T2a】批量提交的回执（主线程轮询取走）
    commit_notes: Arc<Mutex<VecDeque<CommitNote>>>,
    /// 【2b】脚本执行的结论（主线程轮询取走；与 `ExecOutcome` 分开——脚本结果不是网格形状）
    script_notes: Arc<Mutex<VecDeque<crate::env::ScriptNote>>>,
    /// 【2d】单元执行的结论（主线程轮询取走；与 `ExecOutcome` 分开——结果走内联输出）
    cell_notes: Arc<Mutex<VecDeque<crate::cell::CellNote>>>,
    /// 执行器句柄：`run` 在工作线程上、`cancel` 在一次性线程上，两处都要拿它
    runner: Arc<dyn QueryRunner>,
}

impl ExecQueue {
    /// 执行器句柄（M8：洞察入口要问它“未绑定时执行会落到哪条连接上”，口径只此一处）
    pub fn runner(&self) -> Arc<dyn QueryRunner> {
        self.runner.clone()
    }

    /// 用一个执行器起队列
    pub fn new(runner: Arc<dyn QueryRunner>) -> Self {
        let (tx, rx) = mpsc::channel::<ExecJob>();
        let done: Arc<Mutex<VecDeque<ExecOutcome>>> = Arc::new(Mutex::new(VecDeque::new()));
        let busy = Arc::new(AtomicBool::new(false));
        let running_connection: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let cancel_notes: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        let tx_notes: Arc<Mutex<VecDeque<TxNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let source_notes: Arc<Mutex<VecDeque<SourceNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let export_notes: Arc<Mutex<VecDeque<ExportNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let commit_notes: Arc<Mutex<VecDeque<CommitNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let script_notes: Arc<Mutex<VecDeque<crate::env::ScriptNote>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        let cell_notes: Arc<Mutex<VecDeque<crate::cell::CellNote>>> =
            Arc::new(Mutex::new(VecDeque::new()));

        let worker_done = done.clone();
        let worker_busy = busy.clone();
        let worker_running = running_connection.clone();
        let worker_cancelled = cancel_requested.clone();
        let worker_runner = runner.clone();
        let worker_script_done = script_notes.clone();
        let worker_cell_done = cell_notes.clone();
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
                        // 中断之后**不再往下跑**（架构 §3.4：剩余语句各自回一条“已取消”），
                        // 否则用户看到的“中断”只是打断了当前那一句。
                        let result = if worker_cancelled.load(Ordering::SeqCst) {
                            Err(CANCELED.to_string())
                        } else if let Some((offset, limit)) = job.segment {
                            // 【B5b】取下一段：同一条原 SQL 的后一段（不新开结果集）
                            worker_runner.fetch_next(
                                job.connection.as_deref(),
                                job.schema.as_deref(),
                                job.channel,
                                &sql,
                                offset,
                                limit,
                            )
                        } else if let Some((filter, columns)) = job.filtered.as_ref() {
                            // 【B14】下发源库：拼 WHERE 的活交给执行器（它才知道方言）
                            worker_runner.run_filtered(
                                job.connection.as_deref(),
                                job.schema.as_deref(),
                                job.channel,
                                &sql,
                                filter,
                                columns,
                            )
                        } else if let Some((column, descending)) = job.sorted_down.as_ref() {
                            // 【B14】排序下发：同上，改写与方言差异都在执行器
                            worker_runner.run_sorted_down(
                                job.connection.as_deref(),
                                job.schema.as_deref(),
                                job.channel,
                                &sql,
                                column,
                                *descending,
                            )
                        } else if let Some(request) = job.analysis.as_ref() {
                            // 【B15】本地分析：桥接的行 + 分析 SQL 一起交给执行器（不碰源库）
                            worker_runner.analyze(request)
                        } else {
                            // 连接透传给执行器（B1）：`None` = 未绑定 → 执行器自己决定回退口径
                            worker_runner.run(
                                job.connection.as_deref(),
                                job.schema.as_deref(),
                                job.channel,
                                &sql,
                                job.options,
                            )
                        };
                        // B4：事务状态跟着结论一起回去（界面不必再单独问一句）
                        let transaction =
                            worker_runner.transaction_snapshot(job.connection.as_deref());
                        if let Ok(mut queue) = worker_done.lock() {
                            queue.push_back(ExecOutcome {
                                document: job.document.clone(),
                                connection: job.connection.clone(),
                                sql,
                                placement: job.placement,
                                channel: job.channel,
                                transaction,
                                analysis: job.analysis.is_some(),
                                lineage: job.lineage,
                                result,
                            });
                        }
                    }
                    // 【2b】脚本执行：不走上面的 statements 循环（Script 的 `statements` 恒为空），
                    // 独立跑一次 `run_script`，结论进 `script_notes`（不是网格形状的 `ExecOutcome`）。
                    if let Some(request) = job.script.as_ref() {
                        let result = if worker_cancelled.load(Ordering::SeqCst) {
                            Err(CANCELED.to_string())
                        } else {
                            worker_runner.run_script(request)
                        };
                        if let Ok(mut queue) = worker_script_done.lock() {
                            queue.push_back(crate::env::ScriptNote {
                                document: job.document.clone(),
                                env_id: request.env_id.clone(),
                                result,
                            });
                        }
                    }
                    // 【2d】单元执行：同脚本，独立跑一次 `run_cell`，结论进 `cell_notes`
                    // （回执带会话 + 单元 id，主线程据此回填内联输出）。
                    if let Some(request) = job.cell.as_ref() {
                        let result = if worker_cancelled.load(Ordering::SeqCst) {
                            Err(CANCELED.to_string())
                        } else {
                            worker_runner.run_cell(request)
                        };
                        if let Ok(mut queue) = worker_cell_done.lock() {
                            queue.push_back(crate::cell::CellNote {
                                document: job.document.clone(),
                                session_id: request.session_id.clone(),
                                cell_id: request.cell_id.clone(),
                                result,
                            });
                        }
                    }
                    *lock(&worker_running) = None;
                    worker_busy.store(false, Ordering::SeqCst);
                }
            })
            .expect("failed to spawn editor exec worker");

        Self {
            tx: Some(tx),
            done,
            busy,
            running_connection,
            cancel_requested,
            cancel_notes,
            tx_notes,
            source_notes,
            export_notes,
            commit_notes,
            script_notes,
            cell_notes,
            runner,
        }
    }

    /// 【2b】脚本执行的结论（主线程轮询取走；与 `ExecOutcome` 分开）
    pub fn drain_script_notes(&self) -> Vec<crate::env::ScriptNote> {
        let mut queue = match self.script_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 【2d】单元执行的结论（主线程轮询取走；与 `ExecOutcome` 分开）
    pub fn drain_cell_notes(&self) -> Vec<crate::cell::CellNote> {
        let mut queue = match self.cell_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 提交一次执行（忙 / 空目标会被拒，**不排队**：排队会让"再按一次"变成隐藏的批量执行）
    ///
    /// `connection` 是**文档绑定的连接**（B1）；`None` = 未绑定，执行器回退到“当前活动连接”。
    /// `placement` 决定结果落到当前结果集还是新结果集（B2）；`options` 携带
    /// 「自动提交关闭 → 本次执行进事务」（B4）；`channel` 是**执行通道**（B13，原样回到结论里）。
    pub fn submit(
        &self,
        document: DocumentId,
        target: &ExecTarget,
        connection: Option<String>,
        schema: Option<String>,
        placement: ResultPlacement,
        options: RunOptions,
        channel: crate::channel::ExecChannel,
    ) -> Result<(), SubmitError> {
        let statements = target.statements();
        if statements.is_empty() && target.script().is_none() && target.cell().is_none() {
            return Err(SubmitError::Empty);
        }
        if self.is_busy() {
            return Err(SubmitError::Busy);
        }
        let Some(tx) = self.tx.as_ref() else {
            return Err(SubmitError::Busy);
        };
        // 起跑的记号在**提交线程**上同步写（不能等工作线程收到 job 再写）：
        // 用户可能刚按执行就按中断，那时工作线程还没开始——按“作业里的连接”去取消
        // 会取到空值，取消就发给了错误的连接。
        *lock(&self.running_connection) = connection.clone();
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.busy.store(true, Ordering::SeqCst);
        if tx
            .send(ExecJob {
                document,
                connection,
                schema,
                statements,
                placement,
                options,
                channel,
                // 【B5b】取下一段走另一条取数路径（`fetch_next`），其余字段都一样
                segment: target.segment(),
                // 【B14】下发筛选走 `run_filtered`（筛词与列名在这儿带上）
                filtered: target
                    .filtered()
                    .map(|(_, filter, columns)| (filter.to_string(), columns.to_vec())),
                // 【B14】排序下发同此
                sorted_down: target
                    .sorted_down()
                    .map(|(_, column, descending)| (column.to_string(), descending)),
                // 【B15】本地分析：载荷原样带过去（工作线程据此走 `analyze`）
                analysis: target.analysis().cloned(),
                // 【2b】脚本执行：载荷原样带过去（工作线程据此走 `run_script`）
                script: target.script().cloned(),
                // 【2d】单元执行：载荷原样带过去（工作线程据此走 `run_cell`）
                cell: target.cell().cloned(),
                // 【B15】血缘：结果回填时贴到条目上（工具栏显示“这份是怎么来的”）
                lineage: target.lineage(),
            })
            .is_err()
        {
            self.busy.store(false, Ordering::SeqCst);
            return Err(SubmitError::Busy);
        }
        Ok(())
    }

    /// 请一个事务动作（B4）：开始 / 提交 / 回滚
    ///
    /// 同步只做「能不能做」的判断（有执行在跑就回绝——事务动作插在执行中间会让
    /// “提交了什么”说不清）；动作本身在一次性线程上做（端口允许阻塞，UI 不等）。
    pub fn request_transaction(
        &self,
        document: DocumentId,
        action: TxAction,
        connection: Option<String>,
    ) -> Result<(), String> {
        if self.is_busy() {
            return Err("有执行在跑，先等它结束再操作事务".to_string());
        }
        let runner = self.runner.clone();
        let notes = self.tx_notes.clone();
        std::thread::Builder::new()
            .name("rds-editor-tx".to_string())
            .spawn(move || {
                let result = runner
                    .transaction(connection.as_deref(), action)
                    .map(|()| runner.transaction_snapshot(connection.as_deref()));
                if let Ok(mut queue) = notes.lock() {
                    queue.push_back(TxNote {
                        document,
                        action,
                        result,
                    });
                }
            })
            .expect("failed to spawn editor tx worker");
        Ok(())
    }

    /// 事务动作的结论（主线程轮询；取走即清空）
    pub fn drain_tx_notes(&self) -> Vec<TxNote> {
        let mut queue = match self.tx_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 【B13】请一个源动作（重挂 / 换主源；源清单与「重新挂载源库」菜单都走它）
    ///
    /// 与 [`Self::request_transaction`] 同一套：同步只做“能不能做”的判断，动作在一次性
    /// 线程上做（`ATTACH` 是 I/O，UI 不等），回执经 [`Self::drain_source_notes`] 取回。
    /// **不占执行位**：它不产结果集，也不该把“执行中”灯点亮。
    pub fn request_source_action(
        &self,
        document: DocumentId,
        connection: Option<String>,
        channel: crate::channel::ExecChannel,
        action: SourceAction,
    ) -> Result<(), String> {
        let runner = self.runner.clone();
        let notes = self.source_notes.clone();
        std::thread::Builder::new()
            .name("rds-editor-source-action".to_string())
            .spawn(move || {
                let conn = connection.as_deref();
                let result = match &action {
                    SourceAction::RefreshAll => runner.refresh_sources(conn, channel, None),
                    SourceAction::RefreshSource { alias } => {
                        runner.refresh_sources(conn, channel, Some(alias))
                    }
                    SourceAction::SetPrimary { alias } => runner.set_federated_primary(conn, alias),
                };
                if let Ok(mut queue) = notes.lock() {
                    queue.push_back(SourceNote {
                        document,
                        action,
                        result,
                    });
                }
            })
            .expect("failed to spawn editor source action worker");
        Ok(())
    }

    /// 【B13】源动作的回执（主线程轮询；取走即清空）
    pub fn drain_source_notes(&self) -> Vec<SourceNote> {
        let mut queue = match self.source_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 【B7 切片二】请一次 DuckDB 导出（Parquet / XLSX）
    ///
    /// 与 [`Self::request_source_action`] 同一套：请求在一次性线程上做（可能要联网装扩展），
    /// 回执经 [`Self::drain_export_notes`] 取回；**不占执行位**。
    pub fn request_duckdb_export(&self, request: DuckDbExportRequest) -> Result<(), String> {
        let runner = self.runner.clone();
        let notes = self.export_notes.clone();
        std::thread::Builder::new()
            .name("rds-editor-export".to_string())
            .spawn(move || {
                let result = runner.export_via_duckdb(&request);
                if let Ok(mut queue) = notes.lock() {
                    queue.push_back(ExportNote {
                        document: request.document.clone(),
                        format: request.format,
                        path: request.path.clone(),
                        filtered: request.filtered,
                        result,
                    });
                }
            })
            .map_err(|e| format!("起导出线程失败：{e}"))?;
        Ok(())
    }

    /// 【B7 切片二】DuckDB 导出的回执（主线程轮询；取走即清空）
    pub fn drain_export_notes(&self) -> Vec<ExportNote> {
        let mut queue = match self.export_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 【T2a】请一次批量提交（把变更集写回源库）
    ///
    /// 与 [`Self::request_transaction`] 同一套：同步只做「能不能做」的判断
    /// （有执行在跑就回绝——提交插在执行中间会让"提交了什么"说不清），
    /// 提交本身在**一次性线程**上做（端口允许阻塞，UI 不等），回执经
    /// [`Self::drain_commit_notes`] 取回。**不占执行位**：它不产结果集。
    pub fn request_commit_changes(
        &self,
        document: DocumentId,
        connection: Option<String>,
        schema: Option<String>,
        channel: crate::channel::ExecChannel,
        table: String,
        key: RowKey,
        sql: String,
        updates: String,
    ) -> Result<(), String> {
        if self.is_busy() {
            return Err("有执行在跑，先等它结束再提交".to_string());
        }
        let runner = self.runner.clone();
        let notes = self.commit_notes.clone();
        std::thread::Builder::new()
            .name("rds-editor-commit".to_string())
            .spawn(move || {
                let result = runner.commit_changes(
                    connection.as_deref(),
                    schema.as_deref(),
                    channel,
                    &table,
                    &key,
                    &updates,
                );
                if let Ok(mut queue) = notes.lock() {
                    queue.push_back(CommitNote {
                        document,
                        connection,
                        channel,
                        sql,
                        table,
                        key,
                        result,
                    });
                }
            })
            .map_err(|e| format!("起提交线程失败：{e}"))?;
        Ok(())
    }

    /// 【T2a】批量提交的回执（主线程轮询；取走即清空）
    pub fn drain_commit_notes(&self) -> Vec<CommitNote> {
        let mut queue = match self.commit_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 取回已完成的执行（主线程轮询；取走即清空）
    pub fn drain(&self) -> Vec<ExecOutcome> {
        let mut queue = match self.done.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 中断当前作业（B3）
    ///
    /// 同步只做"能不能中断"的判断（没在跑就直接回绝，理由可读）；真正的中断在一条一次性
    /// 线程上做——端口实现允许阻塞（有的驱动要发 `KILL QUERY` 这样的往返），而 UI 线程不能等。
    ///
    /// 先置中断意图再叫取消：批量里**剩余语句**据此标“已取消”。
    pub fn cancel(&self) -> Result<(), String> {
        if !self.is_busy() {
            return Err("当前没有执行在运行".to_string());
        }
        self.cancel_requested.store(true, Ordering::SeqCst);
        let connection = lock(&self.running_connection).clone();
        let runner = self.runner.clone();
        let notes = self.cancel_notes.clone();
        std::thread::Builder::new()
            .name("rds-editor-cancel".to_string())
            .spawn(move || {
                let note = match runner.cancel(connection.as_deref()) {
                    Ok(true) => None,
                    Ok(false) => Some("没有正在执行的查询".to_string()),
                    Err(error) => Some(format!("中断失败：{error}")),
                };
                if let Some(note) = note
                    && let Ok(mut queue) = notes.lock()
                {
                    queue.push_back(note);
                }
            })
            .expect("failed to spawn editor cancel worker");
        Ok(())
    }

    /// 中断尝试的结果文案（主线程轮询；成功的中断不需要回执——状态栏已经说了“已请求中断”）
    pub fn drain_cancel_notes(&self) -> Vec<String> {
        let mut queue = match self.cancel_notes.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        queue.drain(..).collect()
    }

    /// 是否有执行在跑（状态栏 / 按钮禁用用）
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// 执行器支不支持事务（B4）：界面据此决定摆不摆 TX 区
    pub fn supports_transactions(&self) -> bool {
        self.runner.supports_transactions()
    }
}

/// 中断之后剩余语句的结果文案（架构 §3.4：中断后不再往下跑）
const CANCELED: &str = "已取消";

/// 取锁（中毒时取回内部值：一条队列中毒不该把整个执行通道拖死）
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Drop for ExecQueue {
    fn drop(&mut self) {
        // 关掉发送端 → 工作线程 `recv` 返回 Err → 线程自然退出
        self.tx = None;
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        CommitReport, ExecMenuKind, ExecQueue, ExecTarget, QueryData, QueryRunner, ResultPlacement,
        RunOptions, SEGMENT_ROWS, SourceAction, SubmitError, TxAction, TxNote, TxSnapshot,
        all_target, batch_target, resolve_target, statement_target, target_for_menu,
    };
    use crate::channel::ExecChannel;
    use crate::model::DocumentId;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
        assert_eq!(
            resolve_target(text, 100..200),
            ExecTarget::Statement(text.to_string())
        );
    }

    /// 【B5b】取下一段是个**显式目标**：带 (offset, limit)，标签可读，SQL 就是原 SQL
    #[test]
    fn a_segment_target_carries_its_offset_and_limit() {
        let target = ExecTarget::Segment {
            sql: "select n from t".to_string(),
            offset: 1_000,
            limit: SEGMENT_ROWS,
        };
        assert_eq!(target.segment(), Some((1_000, SEGMENT_ROWS)));
        assert_eq!(target.statements(), ["select n from t".to_string()]);
        assert_eq!(target.sql(), Some("select n from t"));
        assert_eq!(target.label(), "取下一段");
        assert_eq!(target.lineage(), "取下一段");

        // 其它目标没有分段语义（不会误走 fetch_next）
        assert_eq!(all_target("select 1").segment(), None);
    }

    /// 【B15】血缘摘要：四个下发动作各自有名，普通执行归「原查询」
    ///
    /// 与 `label()` 分开的理由写在这里：标签说的是“用户点了哪个动作”（当前语句 / 选区 / 批量），
    /// 血缘说的是“这份数据由什么产生”——两者在普通执行上恰好不同。
    #[test]
    fn lineage_names_where_a_result_came_from() {
        let plain = |target: ExecTarget| target.lineage();
        assert_eq!(plain(all_target("select 1")), "原查询", "全部也是原查询");
        assert_eq!(plain(statement_target("select 1", 0)), "原查询");
        assert_eq!(plain(batch_target("select 1;select 2")), "原查询");

        assert_eq!(
            ExecTarget::Filtered {
                sql: "select 1".to_string(),
                filter: "a".to_string(),
                columns: vec!["c".to_string()],
            }
            .lineage(),
            "下发筛选"
        );
        assert_eq!(
            ExecTarget::SortedDown {
                sql: "select 1".to_string(),
                column: "c".to_string(),
                descending: false,
            }
            .lineage(),
            "排序下发"
        );
        assert_eq!(
            ExecTarget::Segment {
                sql: "select 1".to_string(),
                offset: 0,
                limit: SEGMENT_ROWS,
            }
            .lineage(),
            "取下一段"
        );
        assert_eq!(
            ExecTarget::Analysis(crate::analysis::AnalysisRequest {
                sql: "select count(*) from {table}".to_string(),
                columns: vec!["c".to_string()],
                rows: vec![vec!["1".to_string()]],
                dropped_rows: 0,
            })
            .lineage(),
            "本地分析"
        );
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
        fn run(
            &self,
            connection: Option<&str>,
            _schema: Option<&str>,
            _channel: ExecChannel,
            sql: &str,
            _options: RunOptions,
        ) -> Result<QueryData, String> {
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
                column_types: Vec::new(),
                elapsed_ms: 7,
                truncated: false,
                affected_rows: None,
                has_more: false,
                notice: None,
            })
        }

        /// 【B5b】分段假执行器：记下每次要的 (offset, limit)
        fn fetch_next(
            &self,
            connection: Option<&str>,
            _schema: Option<&str>,
            _channel: ExecChannel,
            _sql: &str,
            offset: usize,
            limit: usize,
        ) -> Result<QueryData, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen_connections
                .lock()
                .expect("锁")
                .push(connection.map(str::to_string));
            Ok(QueryData {
                columns: vec!["n".to_string()],
                rows: (offset..offset + limit)
                    .map(|n| vec![n.to_string()])
                    .collect(),
                column_types: Vec::new(),
                elapsed_ms: 3,
                truncated: false,
                affected_rows: None,
                has_more: false,
                notice: None,
            })
        }
    }

    fn channel() -> (ExecQueue, Arc<AtomicUsize>, SeenConnections) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen_connections = Arc::new(Mutex::new(Vec::new()));
        let channel = ExecQueue::new(Arc::new(FakeRunner {
            calls: calls.clone(),
            seen_connections: seen_connections.clone(),
        }));
        (channel, calls, seen_connections)
    }

    /// 轮询等待结果（带超时：通道坏掉时测试要失败而不是挂住）
    fn wait(channel: &ExecQueue) -> Vec<super::ExecOutcome> {
        wait_for(channel, 1)
    }

    /// 轮询等待 N 条结果（批量：语句逐条回来，只等第一条会把后面的漏掉）
    fn wait_for(channel: &ExecQueue, count: usize) -> Vec<super::ExecOutcome> {
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
    fn wait_until_idle(channel: &ExecQueue) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while channel.is_busy() {
            assert!(Instant::now() < deadline, "通道迟迟没回到空闲");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 等一次取消真的送到了执行器（取消在一次性线程上做，得等一小下）
    fn wait_until_cancelled(seen: &SeenConnections) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.lock().expect("锁").is_empty() {
            assert!(Instant::now() < deadline, "取消没送到执行器");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 等工作线程真的把某句发出去了（要中断的是“跑着的查询”，不是“刚提交的”那个瞬间）
    fn wait_until_started(calls: &Arc<AtomicUsize>, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while calls.load(Ordering::SeqCst) < expected {
            assert!(Instant::now() < deadline, "语句迟迟没发出去");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // ===== B3：中断 =====

    /// 可取消的假执行器：慢查询在 `run` 里自旋等取消（模拟驱动被取消令牌打断）
    struct CancellableRunner {
        stop: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
        seen_cancels: SeenConnections,
    }

    impl CancellableRunner {
        fn new() -> (
            Arc<Self>,
            Arc<AtomicBool>,
            Arc<AtomicUsize>,
            SeenConnections,
        ) {
            let stop = Arc::new(AtomicBool::new(false));
            let calls = Arc::new(AtomicUsize::new(0));
            let seen_cancels: SeenConnections = Arc::new(Mutex::new(Vec::new()));
            let runner = Arc::new(Self {
                stop: stop.clone(),
                calls: calls.clone(),
                seen_cancels: seen_cancels.clone(),
            });
            (runner, stop, calls, seen_cancels)
        }
    }

    impl QueryRunner for CancellableRunner {
        fn run(
            &self,
            _connection: Option<&str>,
            _schema: Option<&str>,
            _channel: ExecChannel,
            sql: &str,
            _options: RunOptions,
        ) -> Result<QueryData, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if sql.contains("slow") {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !self.stop.load(Ordering::SeqCst) {
                    assert!(Instant::now() < deadline, "假执行器没等到取消");
                    std::thread::sleep(Duration::from_millis(2));
                }
                return Err("Query cancelled".to_string());
            }
            Ok(QueryData {
                columns: vec!["n".to_string()],
                rows: vec![vec!["1".to_string()]],
                column_types: Vec::new(),
                elapsed_ms: 1,
                truncated: false,
                affected_rows: None,
                has_more: false,
                notice: None,
            })
        }

        fn cancel(&self, connection: Option<&str>) -> Result<bool, String> {
            self.seen_cancels
                .lock()
                .expect("锁")
                .push(connection.map(str::to_string));
            self.stop.store(true, Ordering::SeqCst);
            Ok(true)
        }
    }

    /// 中断要知道取消**哪个连接**：运行中作业的绑定必须原样传下去
    #[test]
    fn cancel_reaches_the_runner_with_the_running_connection() {
        let (runner, _stop, calls, seen_cancels) = CancellableRunner::new();
        let channel = ExecQueue::new(runner);
        let target = ExecTarget::Statement("select slow".to_string());
        channel
            .submit(
                DocumentId::new("doc-cancel"),
                &target,
                Some("P_orders".to_string()), None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");

        wait_until_started(&calls, 1);
        channel.cancel().expect("中断应当被接受");
        wait_until_cancelled(&seen_cancels);
        assert_eq!(
            seen_cancels.lock().expect("锁").as_slice(),
            [Some("P_orders".to_string())],
            "取消的是文档绑定的那个连接"
        );

        let done = wait(&channel);
        let error = done[0].result.as_ref().expect_err("被中断的语句要如实报错");
        assert!(error.contains("cancel"), "{error}");
        wait_until_idle(&channel);
    }

    /// 刚提交就中断（工作线程还没拿到 job）：取消仍要发给**这个作业的**连接，
    /// 而不是拿不到连接就取消活动连接
    #[test]
    fn cancel_right_after_submit_still_targets_the_job_connection() {
        let (runner, _stop, calls, seen_cancels) = CancellableRunner::new();
        let channel = ExecQueue::new(runner);
        let target = ExecTarget::Statement("select slow".to_string());
        channel
            .submit(
                DocumentId::new("doc-cancel-early"),
                &target,
                Some("P_orders".to_string()), None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");

        // 不等工作线程：这一刻正是“工作线程还没开始”的窗口
        channel.cancel().expect("中断应当被接受");
        wait_until_cancelled(&seen_cancels);
        assert_eq!(
            seen_cancels.lock().expect("锁").as_slice(),
            [Some("P_orders".to_string())]
        );

        let done = wait(&channel);
        assert!(done[0].result.is_err(), "这一句没有真的跑完");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "中断在它开跑前就到了");
        wait_until_idle(&channel);
    }

    /// 没在跑就回绝（理由可读）：中断不该是一个“点了没反应”的按钮
    #[test]
    fn cancel_without_a_running_job_is_refused() {
        let (channel, _calls, _seen) = channel();
        let error = channel.cancel().expect_err("没在跑就该回绝");
        assert!(error.contains("没有执行"), "{error}");
    }

    /// 中断之后**剩余语句不再发给驱动**，各自回一条“已取消”（架构 §3.4）
    #[test]
    fn cancel_stops_the_rest_of_a_batch() {
        let (runner, _stop, calls, _seen_cancels) = CancellableRunner::new();
        let channel = ExecQueue::new(runner);
        let target = batch_target("select slow;\nselect 2;\nselect 3;");
        channel
            .submit(
                DocumentId::new("doc-batch-cancel"),
                &target,
                None, None,
                ResultPlacement::NewSet,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交批量");

        wait_until_started(&calls, 1);
        channel.cancel().expect("中断应当被接受");
        let done = wait_for(&channel, 3);

        let first = done[0].result.as_ref().expect_err("第一句被中断");
        assert!(first.contains("cancel"), "{first}");
        for rest in &done[1..] {
            assert_eq!(
                rest.result.as_ref().expect_err("剩余语句也要有结果"),
                "已取消",
                "剩余语句要标已取消：{:?}",
                rest.sql
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "剩余语句不得再发给驱动（“中断”应当真的不再往下跑）"
        );
        wait_until_idle(&channel);
    }

    /// 取消时发现“没在跑的查询”（已在两句之间）要留一句可读的回执，不能当成功
    #[test]
    fn a_cancel_that_finds_nothing_running_leaves_a_note() {
        /// 取消总是“没找到在跑的查询”；`run` 卡住好让通道处于忙态
        struct NothingToCancel {
            stop: Arc<AtomicBool>,
        }
        impl QueryRunner for NothingToCancel {
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !self.stop.load(Ordering::SeqCst) {
                    assert!(Instant::now() < deadline, "假执行器没被放行");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Ok(QueryData::default())
            }
            fn cancel(&self, _connection: Option<&str>) -> Result<bool, String> {
                Ok(false)
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let channel = ExecQueue::new(Arc::new(NothingToCancel { stop: stop.clone() }));
        channel
            .submit(
                DocumentId::new("doc-nothing"),
                &ExecTarget::Statement("select 1".to_string()),
                None, None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");
        channel.cancel().expect("中断应当被接受");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut notes = Vec::new();
        while notes.is_empty() {
            notes = channel.drain_cancel_notes();
            assert!(Instant::now() < deadline, "取消回执迟迟没回来");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(notes[0].contains("没有正在执行的查询"), "{}", notes[0]);
        assert!(channel.drain_cancel_notes().is_empty(), "取走即清空");

        stop.store(true, Ordering::SeqCst);
        wait(&channel);
        wait_until_idle(&channel);
    }

    /// 断了中断能力的执行器要说实话（默认实现 = 不支持，而不是假装成功）
    #[test]
    fn the_port_says_the_truth_when_it_cannot_cancel() {
        let runner = FakeRunner {
            calls: Arc::new(AtomicUsize::new(0)),
            seen_connections: Arc::new(Mutex::new(Vec::new())),
        };
        let error = runner.cancel(None).expect_err("默认实现不支持中断");
        assert!(error.contains("不支持中断"), "{error}");
    }

    // ===== B4：事务 =====

    /// 假执行器（B4）：支持事务，记录执行选项与事务动作
    struct TxRunner {
        actions: Arc<Mutex<Vec<(TxAction, Option<String>)>>>,
        options: Arc<Mutex<Vec<RunOptions>>>,
        open: Arc<AtomicBool>,
    }

    impl TxRunner {
        #[allow(clippy::type_complexity)]
        fn new() -> (
            Arc<Self>,
            Arc<Mutex<Vec<(TxAction, Option<String>)>>>,
            Arc<Mutex<Vec<RunOptions>>>,
            Arc<AtomicBool>,
        ) {
            let actions = Arc::new(Mutex::new(Vec::new()));
            let options = Arc::new(Mutex::new(Vec::new()));
            let open = Arc::new(AtomicBool::new(false));
            let runner = Arc::new(Self {
                actions: actions.clone(),
                options: options.clone(),
                open: open.clone(),
            });
            (runner, actions, options, open)
        }
    }

    impl QueryRunner for TxRunner {
        fn run(
            &self,
            _connection: Option<&str>,
            _schema: Option<&str>,
            _channel: ExecChannel,
            _sql: &str,
            options: RunOptions,
        ) -> Result<QueryData, String> {
            self.options.lock().expect("锁").push(options);
            // 模拟引擎：自动提交关掉时，执行会先开一个事务
            if options.use_transaction {
                self.open.store(true, Ordering::SeqCst);
            }
            Ok(QueryData::default())
        }

        fn transaction_snapshot(&self, _connection: Option<&str>) -> TxSnapshot {
            TxSnapshot {
                in_transaction: self.open.load(Ordering::SeqCst),
            }
        }

        fn transaction(&self, connection: Option<&str>, action: TxAction) -> Result<(), String> {
            self.actions
                .lock()
                .expect("锁")
                .push((action, connection.map(str::to_string)));
            match action {
                TxAction::Begin => self.open.store(true, Ordering::SeqCst),
                TxAction::Commit | TxAction::Rollback => self.open.store(false, Ordering::SeqCst),
            }
            Ok(())
        }

        fn supports_transactions(&self) -> bool {
            true
        }
    }

    /// 等一次事务动作的回执（它不在“执行忙”里，得单独等）
    fn wait_for_tx_notes(channel: &ExecQueue, count: usize) -> Vec<TxNote> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut notes = Vec::new();
        while notes.len() < count {
            notes.extend(channel.drain_tx_notes());
            if notes.len() >= count {
                return notes;
            }
            assert!(Instant::now() < deadline, "事务动作的回执迟迟没回来");
            std::thread::sleep(Duration::from_millis(5));
        }
        notes
    }

    /// 截掉“这一轮”里最先到达的重新挂载回执（真线程，带超时）
    fn wait_for_source_note(channel: &ExecQueue) -> super::SourceNote {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(note) = channel.drain_source_notes().into_iter().next() {
                return note;
            }
            assert!(Instant::now() < deadline, "重新挂载的回执迟迟没回来");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 【B13】重新挂载加速源：走旁路线程，回执从队列取（**不占执行位**）
    #[test]
    fn a_source_refresh_runs_off_the_execution_queue() {
        /// 假执行器：记录被要求重新挂载的连接（附带别名），并给一个结果
        struct RefreshRunner {
            seen: Arc<Mutex<Vec<Option<(String, Option<String>)>>>>,
            fail_with: Option<String>,
        }

        impl QueryRunner for RefreshRunner {
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                Ok(QueryData::default())
            }

            fn refresh_sources(
                &self,
                connection: Option<&str>,
                channel: ExecChannel,
                alias: Option<&str>,
            ) -> Result<String, String> {
                self.seen.lock().expect("锁").push(
                    connection
                        .map(str::to_string)
                        .map(|conn| (conn, alias.map(str::to_string))),
                );
                let _ = channel;
                match &self.fail_with {
                    Some(reason) => Err(reason.clone()),
                    None => Ok("已重新挂载源库".to_string()),
                }
            }
        }

        let seen: Arc<Mutex<Vec<Option<(String, Option<String>)>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let channel = ExecQueue::new(Arc::new(RefreshRunner {
            seen: seen.clone(),
            fail_with: None,
        }));
        channel
            .request_source_action(
                DocumentId::new("doc-refresh"),
                Some("P_orders".to_string()),
                ExecChannel::Accelerated,
                SourceAction::RefreshAll,
            )
            .expect("请一次重新挂载");
        assert!(!channel.is_busy(), "重新挂载不该把“执行中”灯点亮");
        let note = wait_for_source_note(&channel);
        assert!(note.result.is_ok(), "假执行器应当成功");
        assert_eq!(note.document.as_str(), "doc-refresh");
        assert_eq!(
            seen.lock().expect("锁").as_slice(),
            [Some(("P_orders".to_string(), None))],
            "绑定/活动连接要原样传下去（全挂时 alias 为空）"
        );

        // 失败也要如实回一条（不能只有成功才留痕）
        let channel = ExecQueue::new(Arc::new(RefreshRunner {
            seen: Arc::new(Mutex::new(Vec::new())),
            fail_with: Some("挂不上".to_string()),
        }));
        channel
            .request_source_action(
                DocumentId::new("doc-refresh-2"),
                None,
                ExecChannel::Federated,
                SourceAction::RefreshSource {
                    alias: "mysql_src".to_string(),
                },
            )
            .expect("请一次");
        let note = wait_for_source_note(&channel);
        assert_eq!(note.action.label(), "重新挂载源 mysql_src");
        assert_eq!(note.result.expect_err("应当失败"), "挂不上");
    }

    /// 【T1.6】换主源：动作原样送到执行器（别名带着走），回执带回那句话
    #[test]
    fn setting_a_federated_primary_reaches_the_runner() {
        let seen: Arc<Mutex<Vec<Option<(String, Option<String>)>>>> =
            Arc::new(Mutex::new(Vec::new()));

        struct PrimaryRunner {
            seen: Arc<Mutex<Vec<Option<(String, Option<String>)>>>>,
        }

        impl QueryRunner for PrimaryRunner {
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                Ok(QueryData::default())
            }

            fn set_federated_primary(
                &self,
                connection: Option<&str>,
                alias: &str,
            ) -> Result<String, String> {
                self.seen
                    .lock()
                    .expect("锁")
                    .push(connection.map(|c| (c.to_string(), Some(alias.to_string()))));
                Ok(format!("主源已切到 {alias}"))
            }
        }

        let channel = ExecQueue::new(Arc::new(PrimaryRunner { seen: seen.clone() }));
        channel
            .request_source_action(
                DocumentId::new("doc-primary"),
                Some("P_fed".to_string()),
                ExecChannel::Federated,
                SourceAction::SetPrimary {
                    alias: "pg_warehouse".to_string(),
                },
            )
            .expect("请一次换主源");
        let note = wait_for_source_note(&channel);
        assert_eq!(note.action.label(), "把 pg_warehouse 设为主源");
        assert_eq!(note.result.expect("该成功"), "主源已切到 pg_warehouse");
        assert_eq!(
            seen.lock().expect("锁").as_slice(),
            [Some((
                "P_fed".to_string(),
                Some("pg_warehouse".to_string())
            ))],
            "连接与别名都要原样送到执行器"
        );
    }

    /// 事务动作：真的送到执行器（带着作业的连接），回执带回新状态
    #[test]
    fn a_transaction_action_reaches_the_runner_and_reports_the_new_state() {
        let (runner, actions, _options, _open) = TxRunner::new();
        let channel = ExecQueue::new(runner);
        let document = DocumentId::new("doc-tx");

        assert!(channel.supports_transactions(), "这个执行器支持事务");
        channel
            .request_transaction(
                document.clone(),
                TxAction::Begin,
                Some("P_orders".to_string()),
            )
            .expect("开启事务应当被接受");
        let notes = wait_for_tx_notes(&channel, 1);
        assert_eq!(notes[0].action, TxAction::Begin);
        assert_eq!(
            notes[0]
                .result
                .as_ref()
                .expect("动作应当成功")
                .in_transaction,
            true
        );
        assert_eq!(
            actions.lock().expect("锁").as_slice(),
            [(TxAction::Begin, Some("P_orders".to_string()))],
            "事务动作要落在文档绑定的那个连接上"
        );
    }

    /// 提交：执行器收到 Commit，回执说明已不在事务里
    #[test]
    fn committing_reports_that_the_transaction_is_closed() {
        let (runner, actions, _options, open) = TxRunner::new();
        open.store(true, Ordering::SeqCst);
        let channel = ExecQueue::new(runner);

        channel
            .request_transaction(DocumentId::new("doc-tx"), TxAction::Commit, None)
            .expect("提交应当被接受");
        let notes = wait_for_tx_notes(&channel, 1);
        assert_eq!(notes[0].action, TxAction::Commit);
        assert!(
            !notes[0]
                .result
                .as_ref()
                .expect("动作应当成功")
                .in_transaction
        );
        assert_eq!(actions.lock().expect("锁").len(), 1);
    }

    /// 有执行在跑时事务动作被回绝（理由可读）——插在执行中间会让“提交了什么”说不清
    #[test]
    fn transaction_actions_are_refused_while_a_statement_is_running() {
        let (runner, _stop, calls, _seen) = CancellableRunner::new();
        let channel = ExecQueue::new(runner);
        channel
            .submit(
                DocumentId::new("doc-tx-busy"),
                &ExecTarget::Statement("select slow".to_string()),
                None, None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");
        wait_until_started(&calls, 1);

        let error = channel
            .request_transaction(DocumentId::new("doc-tx-busy"), TxAction::Commit, None)
            .expect_err("忙着时不该接事务动作");
        assert!(error.contains("有执行在跑"), "{error}");

        channel.cancel().expect("收尾：中断掉那句慢查询");
        wait_for(&channel, 1);
        wait_until_idle(&channel);
    }

    /// 执行选项（自动提交关）真的到了执行器，且**执行后的事务状态跟着结论回来**
    #[test]
    fn run_options_reach_the_runner_and_the_snapshot_rides_along() {
        let (runner, _actions, options, _open) = TxRunner::new();
        let channel = ExecQueue::new(runner);
        channel
            .submit(
                DocumentId::new("doc-tx-run"),
                &ExecTarget::Statement("insert into t values (1)".to_string()),
                None, None,
                ResultPlacement::Replace,
                RunOptions {
                    use_transaction: true,
                },
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");

        let done = wait(&channel);
        assert_eq!(
            options.lock().expect("锁").as_slice(),
            [RunOptions {
                use_transaction: true
            }],
            "自动提交关掉时，执行要带上“进事务”的选项"
        );
        assert!(
            done[0].transaction.in_transaction,
            "结论里要带执行后的事务状态（界面不必再问一次）"
        );
        wait_until_idle(&channel);
    }

    /// 【B5b】取下一段的作业走 `fetch_next`（不是 `run`），并把收到的段接在选中那份后面
    #[test]
    fn a_segment_job_fetches_the_next_page_instead_of_running_the_sql() {
        let (channel, calls, _seen) = channel();
        let document = DocumentId::new("doc-test");

        channel
            .submit(
                document.clone(),
                &ExecTarget::Segment {
                    sql: "select n from t".to_string(),
                    offset: 1_000,
                    limit: 2,
                },
                None, None,
                ResultPlacement::Append,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");

        let done = wait(&channel);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "取下一段也是一次执行");
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].sql, "select n from t", "传下去的是原 SQL");
        assert_eq!(
            done[0].placement,
            ResultPlacement::Append,
            "取下一段是追加，不是替换"
        );
        let data = done[0].result.as_ref().expect("应当成功");
        assert_eq!(
            data.rows,
            vec![vec!["1000".to_string()], vec!["1001".to_string()]],
            "拿到的正是 offset 之后的那一段"
        );
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
                Some("P_orders".to_string()), None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
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

    // ===== T2c-1：schema 随执行传递 =====

    /// 记录每次 `run` 收到的 schema（断言“文档绑定的 schema 原样到执行器”用）
    struct SchemaRunner {
        seen_schemas: Arc<Mutex<Vec<Option<String>>>>,
    }

    impl QueryRunner for SchemaRunner {
        fn run(
            &self,
            _connection: Option<&str>,
            schema: Option<&str>,
            _channel: ExecChannel,
            _sql: &str,
            _options: RunOptions,
        ) -> Result<QueryData, String> {
            self.seen_schemas
                .lock()
                .expect("锁")
                .push(schema.map(str::to_string));
            Ok(QueryData::default())
        }
    }

    /// 【T2c-1】切 schema 真影响执行的第一环：文档绑定的 schema 要从 `submit`
    /// 一路流到执行器（而不是丢在提交线程）。这条盯住流水，引擎侧的应用在 engine。
    #[test]
    fn the_document_schema_flows_from_submit_to_the_runner() {
        let seen_schemas: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let channel = ExecQueue::new(Arc::new(SchemaRunner {
            seen_schemas: seen_schemas.clone(),
        }));

        let target = ExecTarget::Statement("select * from orders".to_string());
        channel
            .submit(
                DocumentId::new("doc-schema"),
                &target,
                Some("P_orders".to_string()),
                Some("sales".to_string()),
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::Source,
            )
            .expect("提交");

        wait(&channel);
        assert_eq!(
            seen_schemas.lock().expect("锁").as_slice(),
            [Some("sales".to_string())],
            "文档绑定的 schema 必须原样交给执行器（逐执行传递，不能靠连接级状态）"
        );
        wait_until_idle(&channel);
    }

    /// 【T2c-1】未选 schema 时执行器收到 `None`（用连接默认，不注入会话上下文）
    #[test]
    fn unset_schema_reaches_the_runner_as_none() {
        let seen_schemas: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let channel = ExecQueue::new(Arc::new(SchemaRunner {
            seen_schemas: seen_schemas.clone(),
        }));

        let target = ExecTarget::Statement("select 1".to_string());
        channel
            .submit(
                DocumentId::new("doc-no-schema"),
                &target,
                Some("P_orders".to_string()),
                None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::Source,
            )
            .expect("提交");

        wait(&channel);
        assert_eq!(
            seen_schemas.lock().expect("锁").as_slice(),
            [None],
            "未选 schema = None（执行器按连接默认处理）"
        );
        wait_until_idle(&channel);
    }

    #[test]
    fn driver_errors_come_back_as_errors_not_panics() {
        let (channel, _calls, _seen) = channel();
        let target = ExecTarget::Statement("select boom".to_string());
        channel
            .submit(
                DocumentId::new("doc-err"),
                &target,
                None, None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
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
                Some("P_orders".to_string()), None,
                ResultPlacement::NewSet,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
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
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                std::thread::sleep(Duration::from_millis(60));
                Ok(QueryData::default())
            }
        }

        let channel = ExecQueue::new(Arc::new(SlowStatementRunner));
        let target = batch_target("select 1; select 2; select 3;");
        channel
            .submit(
                DocumentId::new("doc-busy-batch"),
                &target,
                None, None,
                ResultPlacement::NewSet,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
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
                None, None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
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
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
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
        let channel = ExecQueue::new(Arc::new(SlowRunner {
            release: release.clone(),
        }));
        let document = DocumentId::new("doc-slow");
        let target = ExecTarget::Statement("select 1".to_string());

        channel
            .submit(
                document.clone(),
                &target,
                None, None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
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
                .submit(
                    document,
                    &target,
                    None, None,
                    ResultPlacement::Replace,
                    RunOptions::default(),
                    crate::channel::ExecChannel::default(),
                )
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
        let channel = ExecQueue::new(Arc::new(FakeRunner {
            calls: calls.clone(),
            seen_connections: Arc::new(Mutex::new(Vec::new())),
        }));
        drop(channel);
        // 线程退出是异步的：这里只要求不 panic、不阻塞进程退出
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    // ===== 批量提交（T2a-5）=====

    /// 假提交执行器：记录收到的参数（连接 / 通道 / 表 / 键 / UPDATE 文本），返回固定回执
    ///
    /// 提交的**执行与事务**在执行器侧（真机在 workbench 的 `EngineQueryRunner::commit_changes`，
    /// 依赖真实连接跑不了离线单测）；这里钉的是编辑器侧的回执链路：请求参数原样送达、
    /// `drain_commit_notes` 把回执完整搬回。
    struct CommitRunner {
        report: CommitReport,
        seen: Arc<Mutex<Vec<(Option<String>, ExecChannel, String, String, String)>>>,
    }

    impl QueryRunner for CommitRunner {
        fn run(
            &self,
            _connection: Option<&str>,
            _schema: Option<&str>,
            _channel: ExecChannel,
            _sql: &str,
            _options: RunOptions,
        ) -> Result<QueryData, String> {
            Err("这个执行器只服务提交".to_string())
        }

        fn commit_changes(
            &self,
            connection: Option<&str>,
            _schema: Option<&str>,
            channel: ExecChannel,
            table: &str,
            key: &crate::export::RowKey,
            updates: &str,
        ) -> Result<CommitReport, String> {
            self.seen
                .lock()
                .expect("锁")
                .push((
                    connection.map(str::to_string),
                    channel,
                    table.to_string(),
                    key.columns().join(","),
                    updates.to_string(),
                ));
            Ok(self.report.clone())
        }
    }

    /// 轮询等待提交回执（与 [`wait`] 同一套超时口径）
    fn wait_commit(queue: &ExecQueue) -> Vec<super::CommitNote> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let notes = queue.drain_commit_notes();
            if !notes.is_empty() {
                return notes;
            }
            assert!(Instant::now() < deadline, "提交回执迟迟没回来");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 请求参数（连接 / 通道 / 表 / 键 / 原查询 / UPDATE 文本）原样送达执行器，回执完整搬回
    #[test]
    fn commit_requests_carry_the_full_payload_and_return_the_receipt() {
        let report = CommitReport {
            committed: 2,
            skipped: 1,
            per_statement: vec![1, 0, 1],
            conflicts: Vec::new(),
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let queue = ExecQueue::new(Arc::new(CommitRunner {
            report,
            seen: seen.clone(),
        }));
        let updates = "UPDATE orders SET name = 'a' WHERE id = 1;\nUPDATE orders SET name = 'b' WHERE id = 2;\n";

        queue
            .request_commit_changes(
                DocumentId::new("doc1"),
                Some("P_orders".to_string()), None,
                ExecChannel::Source,
                "orders".to_string(),
                crate::export::RowKey::single("id"),
                "select * from orders".to_string(),
                updates.to_string(),
            )
            .expect("提交请求该受理");

        let notes = wait_commit(&queue);
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.document, DocumentId::new("doc1"));
        assert_eq!(note.connection.as_deref(), Some("P_orders"));
        assert_eq!(note.channel, ExecChannel::Source);
        assert_eq!(note.sql, "select * from orders");
        assert_eq!(note.table, "orders");
        assert_eq!(note.key.columns(), &["id"]);
        // 回执明细完整搬回（逐条影响行数 = 面板组装冲突格的原料）
        let receipt = note.result.clone().expect("提交该成功");
        assert_eq!(receipt.committed, 2);
        assert_eq!(receipt.skipped, 1);
        assert_eq!(receipt.per_statement, vec![1, 0, 1]);

        // 执行器收到的参数原样
        let seen = seen.lock().expect("锁");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0.as_deref(), Some("P_orders"));
        assert_eq!(seen[0].1, ExecChannel::Source);
        assert_eq!(seen[0].2, "orders");
        assert_eq!(seen[0].3, "id");
        assert_eq!(seen[0].4, updates);
    }

    /// 有执行在跑时提交被拒（提交插在执行中间会让"提交了什么"说不清）
    #[test]
    fn commit_is_refused_while_an_execution_is_running() {
        /// 每句慢 60ms：足够在作业跑完之前观察到忙状态
        struct SlowStatementRunner;
        impl QueryRunner for SlowStatementRunner {
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                std::thread::sleep(Duration::from_millis(60));
                Ok(QueryData::default())
            }
        }

        let queue = ExecQueue::new(Arc::new(SlowStatementRunner));
        queue
            .submit(
                DocumentId::new("doc-busy"),
                &ExecTarget::Statement("select 1".to_string()),
                None, None,
                ResultPlacement::NewSet,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交");

        let error = queue
            .request_commit_changes(
                DocumentId::new("doc-busy"),
                Some("P_orders".to_string()), None,
                ExecChannel::Source,
                "orders".to_string(),
                crate::export::RowKey::single("id"),
                "select * from orders".to_string(),
                "UPDATE orders SET name = 'x' WHERE id = 1;".to_string(),
            )
            .expect_err("有执行在跑该被拒");
        assert!(error.contains("有执行在跑"), "{error}");
        wait_until_idle(&queue);
    }

    /// 【2d】`run_cell` 默认实现如实拒绝（宿主没接单元执行能力的真实状态）
    #[test]
    fn run_cell_defaults_to_honest_rejection() {
        struct NoopRunner;
        impl QueryRunner for NoopRunner {
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                Ok(QueryData::default())
            }
        }

        let request = crate::cell::CellRequest {
            session_id: "s1".to_string(),
            cell_id: "c1".to_string(),
            sql: "select 1".to_string(),
            conn_id: None,
        };
        let err = NoopRunner.run_cell(&request).expect_err("默认实现必须拒绝");
        assert!(err.contains("不支持单元执行"), "{err}");
    }

    /// 【2d】单元目标走 `run_cell`，结论经 `drain_cell_notes` 回主线程（真队列线程）
    #[test]
    fn a_cell_runs_through_the_queue_and_its_note_comes_back() {
        /// 假执行器：记下收到的请求，回一份成功的结论
        struct CellRunner {
            seen: Arc<Mutex<Option<crate::cell::CellRequest>>>,
        }
        impl QueryRunner for CellRunner {
            fn run(
                &self,
                _connection: Option<&str>,
                _schema: Option<&str>,
                _channel: ExecChannel,
                _sql: &str,
                _options: RunOptions,
            ) -> Result<QueryData, String> {
                Ok(QueryData::default())
            }
            fn run_cell(
                &self,
                request: &crate::cell::CellRequest,
            ) -> Result<crate::cell::CellOutcome, String> {
                *self.seen.lock().unwrap() = Some(request.clone());
                Ok(crate::cell::CellOutcome {
                    sql: request.sql.clone(),
                    elapsed_ms: 3,
                    affected_rows: None,
                    truncated: false,
                    result: Ok(QueryData::default()),
                })
            }
        }

        let seen: Arc<Mutex<Option<crate::cell::CellRequest>>> = Arc::new(Mutex::new(None));
        let queue = ExecQueue::new(Arc::new(CellRunner {
            seen: seen.clone(),
        }));
        let request = crate::cell::CellRequest {
            session_id: "s1".to_string(),
            cell_id: "c1".to_string(),
            sql: "select 1".to_string(),
            conn_id: None,
        };
        queue
            .submit(
                DocumentId::new("doc-nb"),
                &ExecTarget::Cell(request.clone()),
                Some("P_orders".to_string()),
                None,
                ResultPlacement::Replace,
                RunOptions::default(),
                ExecChannel::default(),
            )
            .expect("单元提交");

        // 工作线程收请求 + 回执到达
        wait_until_idle(&queue);
        let seen = seen.lock().unwrap().clone().expect("执行器收到请求");
        assert_eq!(seen.session_id, "s1");
        assert_eq!(seen.cell_id, "c1");
        assert_eq!(seen.sql, "select 1");

        let notes = queue.drain_cell_notes();
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.document, DocumentId::new("doc-nb"));
        assert_eq!(note.session_id, "s1");
        assert_eq!(note.cell_id, "c1");
        let outcome = note.result.as_ref().expect("成功回执");
        assert_eq!(outcome.sql, "select 1");
        assert_eq!(outcome.elapsed_ms, 3);
    }
}

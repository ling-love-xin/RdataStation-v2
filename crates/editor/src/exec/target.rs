//! 执行目标（A14「执行什么」）：`ExecTarget` 解析（纯函数）与执行菜单映射。
//! （自 execution.rs 拆出，2026-10-29；`crate::execution::*` 旧路径经 execution.rs 兼容层保持）

use std::ops::Range;

use crate::export::RowKey;
use crate::store::ResultPlacement;

// ===== 执行什么 =====

/// 一次执行的目标
///
/// 【B3】`Analysis` 变体携带 `Option<Arc<FactFrame>>` 事实档快照，不可逐值比较；因此不再
/// derive `PartialEq/Eq`，改由下方手工实现：除 `Analysis` 的 `fact` 快照外逐字段比较，
/// 快照是引用、不代表请求语义差异。
#[derive(Debug, Clone)]
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

impl PartialEq for ExecTarget {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Selection(a), Self::Selection(b)) => a == b,
            (Self::Statement(a), Self::Statement(b)) => a == b,
            (Self::All(a), Self::All(b)) => a == b,
            (Self::Batch(a), Self::Batch(b)) => a == b,
            (
                Self::Segment { sql: a, offset: oa, limit: la },
                Self::Segment { sql: b, offset: ob, limit: lb },
            ) => a == b && oa == ob && la == lb,
            (
                Self::Filtered { sql: a, filter: fa, columns: ca },
                Self::Filtered { sql: b, filter: fb, columns: cb },
            ) => a == b && fa == fb && ca == cb,
            (
                Self::SortedDown { sql: a, column: ca, descending: da },
                Self::SortedDown { sql: b, column: cb, descending: db },
            ) => a == b && ca == cb && da == db,
            // 事实档快照（`fact`）是引用、不参与语义比较；其余字段逐一对上即相等
            (Self::Analysis(a), Self::Analysis(b)) => {
                a.sql == b.sql
                    && a.columns == b.columns
                    && a.rows == b.rows
                    && a.dropped_rows == b.dropped_rows
                    && a.pref == b.pref
            }
            (Self::Script(a), Self::Script(b)) => a == b,
            (Self::Cell(a), Self::Cell(b)) => a == b,
            (
                Self::Commit { sql: a, table: ta, key: ka, updates: ua },
                Self::Commit { sql: b, table: tb, key: kb, updates: ub },
            ) => a == b && ta == tb && ka == kb && ua == ub,
            _ => false,
        }
    }
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
    ///
    /// 【2026-10-27】全部执行项都落**新结果集**：每次执行 = 一个新的结果 tab（用户反馈
    /// 「每次执行的结果应该就是一个 tab，但一个 tab 容纳了好多结果」——原默认 `Replace`
    /// 让多次执行覆盖同一 tab）。`Replace` 保留给「刷新」等显式重查路径（见 store）。
    pub fn placement(self) -> ResultPlacement {
        match self {
            Self::Batch | Self::NewSet => ResultPlacement::NewSet,
            Self::CurrentStatement | Self::Selection | Self::All => ResultPlacement::NewSet,
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


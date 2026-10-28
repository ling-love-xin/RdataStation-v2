//! 执行回执类型（A14「跑完放哪」的载荷）：结论 / 事务 / 提交 / 导出 / 来源动作。
//! （自 execution.rs 拆出，2026-10-29；`crate::execution::*` 旧路径经 execution.rs 兼容层保持）

use std::sync::Arc;

use ::shared::FactFrame;

use crate::exec::runner::QueryData;
use crate::exec::target::{TxAction, TxSnapshot};
use crate::export::RowKey;
use crate::model::DocumentId;
use crate::store::{Pushdown, ResultPlacement};

// ===== 跑完放哪 =====

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
///
/// 【B3】`fact` 是当前结果集事实档快照：带类型档（Parquet / XLSX）优先按 batches 直灌
/// 建表 COPY；`None` 回退显示矩阵猜型降级档。含 `Arc<FactFrame>` 后不可逐值比较，去
/// `PartialEq, Eq`（与 `AnalysisRequest` / `QueryData` 同一口径）。
#[derive(Debug, Clone)]
pub struct DuckDbExportRequest {
    pub document: DocumentId,
    /// 结果集序号（1 基；回执用「结果 N」指代，与标签同口径）
    pub set: usize,
    pub format: crate::export::ExportFormat,
    pub path: std::path::PathBuf,
    pub columns: Vec<String>,
    /// 行（已字符串化，与网格一致：`NULL` 是字面量；仅事实档缺失的降级档使用）
    pub rows: Vec<Vec<String>>,
    /// 这次导的是**筛后的行集**（回执按 B7 的口径要明示）
    pub filtered: bool,
    /// 事实档快照（`None` = 无事实档，走显示矩阵猜型降级档并在报告中标注）
    pub fact: Option<Arc<FactFrame>>,
    /// 【D1】是否走显示矩阵猜型降级档（提交侧按 `fact_export_mode` 纯判定置位，
    /// 与 workbench 执行分流同口径；成功回执据此追加降级口径说明）。
    pub degraded: bool,
}

/// 【B7 切片二】一次 DuckDB 导出的回执
#[derive(Debug, Clone)]
pub struct ExportNote {
    pub document: DocumentId,
    pub format: crate::export::ExportFormat,
    pub path: std::path::PathBuf,
    /// 这次导的是筛后的行集（回执要按 B7 的口径明示）
    pub filtered: bool,
    /// 【D1】是否为猜型降级档（`true` = 整列按文本存储，数字类型可能失真）
    pub degraded: bool,
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
    /// 【A2】这份结果的「形成方式」原料（下发筛选 / 排序下发时随结论回去；
    /// 刷新 ⟳ 按它重查，见 [`Pushdown`]）
    pub pushdown: Option<Pushdown>,
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

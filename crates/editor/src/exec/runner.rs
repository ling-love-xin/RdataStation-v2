//! 执行端口（A14「谁来跑」）：`QueryRunner` trait（宿主注入）+ `QueryData` 产物。
//! （自 execution.rs 拆出，2026-10-29；`crate::execution::*` 旧路径经 execution.rs 兼容层保持）

use std::sync::Arc;

use ::shared::FactFrame;

use crate::channel::ExecChannel;
use crate::exec::notes::{CommitReport, DuckDbExportRequest};
use crate::exec::target::{RunOptions, TxAction, TxSnapshot};
use crate::export::RowKey;

// ===== 谁来跑 =====

/// 一次执行的产物（结果集）
///
/// 【A3】`fact` 持事实帧候选（L0 事实层，方案跳 1）：`None` = 无 batches 回执（DML /
/// DDL、加速档、联邦档、跨进程往返），显示矩阵照旧从 `rows` 派生。故意不派生
/// `PartialEq / Eq`：事实帧是 `Arc<FactFrame>` 共享句柄，整帧逐值比较语义不对；现状也
/// 没有整体比较 `QueryData` 的调用方。
#[derive(Debug, Clone, Default)]
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
    /// 【A3】事实帧候选：源库回执带 batches 时由 `to_data` 构造；其余路径 `None`。
    /// 渲染第一期不读它；分析 / 带类型导出按它选保真路径（A 阶段不消费）。
    pub fact: Option<Arc<FactFrame>>,
    /// 【B4】本次分析触发物化后，回挂**原槽位**的常驻表名（`None` = 未触发 / 复用已有表
    /// 无需回挂新名）。host 回填到 `source_fact_set` 对应的原结果集槽位。
    pub source_fact_table: Option<String>,
    /// 【B4】是否把原槽位提升到表档（释放 Rust 堆 batches）：`true` 时 host 调
    /// `promote_fact_to_table`；`false` 只挂表名、保留 batches（内存档 + 复用表）。
    pub source_fact_release: bool,
    /// 【B4】物化回执归属的原结果集槽位下标（分析触发时的选中结果集）。
    pub source_fact_set: usize,
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


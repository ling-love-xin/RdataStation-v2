//! 执行队列（A14「跑完放哪」）：按连接分组的工作线程 + mpsc + 主线程轮询泵。
//! （自 execution.rs 拆出，2026-10-29；`crate::execution::*` 旧路径经 execution.rs 兼容层保持）

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use crate::exec::notes::{
    CommitNote, DuckDbExportRequest, ExecOutcome, ExportNote, SourceAction, SourceNote,
    SubmitError, TxNote,
};
use crate::exec::runner::QueryRunner;
use crate::exec::target::{ExecTarget, RunOptions, TxAction};
use crate::export::RowKey;
use crate::model::DocumentId;
use crate::store::{Pushdown, ResultPlacement};

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

/// 一个连接的执行队列：一条工作线程 + 未完成作业计数 + 独立取消位
///
/// 阶段②（按连接分组）的单元：**不同连接各自并行**（各有通道与工作线程），
/// **同一连接内排队串行**（单通道天然 FIFO，worker 逐个处理，批量的语句之间不视为空闲）。
struct ConnQueue {
    /// 该连接的作业通道（worker 循环 `recv`；通道存活即 worker 存活，随宿主一起退出）
    tx: Sender<ExecJob>,
    /// 未完成作业数（提交 +1、worker 跑完一个 -1）：>0 即「该连接有执行在跑 / 排队」。
    /// 它取代旧的全局 `busy` 位 + `running_connection` 单槽——提交线程同步 +1，
    /// 「刚按执行就按中断」时取消也能立即看到该连接有作业。
    pending: Arc<AtomicUsize>,
    /// 已请求中断（B3）：该连接的批量里**剩余语句**据此标"已取消"，而不是接着往下跑
    cancel_requested: Arc<AtomicBool>,
}

/// 执行队列：**按连接分组的并发队列**（阶段②） + 一个共享结果队列
///
/// 生命周期：随宿主（面板 / 工作台）存活；`Drop` 时把工作线程放掉（发送端关闭 → 线程退出）。
/// 每个连接第一次提交时懒建 [`ConnQueue`]（连 worker 一起），连接解绑后队列与 worker 仍在
/// （空转等新作业；连接数 = 个位数到十几，量级与旧单 worker 相当，生命周期治理留阶段③）。
///
/// **与「执行通道」（[`crate::channel::ExecChannel`]：源库 / 加速 / 联邦）不是一回事**：那个回答
/// “在哪儿跑”，这个负责“把作业交给线程并收结论”。名字分开是为了不让两件事读起来像同一件。

pub struct ExecQueue {
    /// 连接 → 该连接的队列。键 = 作业携带的连接（`None` 未绑定 → 统一键 `""`，
    /// 未绑定作业最后都落到同一活动连接上，天然该串行）。锁只在 submit / cancel / 查询时短暂持有。
    conns: Mutex<HashMap<String, Arc<ConnQueue>>>,
    done: Arc<Mutex<VecDeque<ExecOutcome>>>,
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

    /// 用一个执行器起队列（连接队列懒建：第一次提交时才 spawn 对应 worker）
    pub fn new(runner: Arc<dyn QueryRunner>) -> Self {
        let done: Arc<Mutex<VecDeque<ExecOutcome>>> = Arc::new(Mutex::new(VecDeque::new()));
        let cancel_notes: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        let tx_notes: Arc<Mutex<VecDeque<TxNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let source_notes: Arc<Mutex<VecDeque<SourceNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let export_notes: Arc<Mutex<VecDeque<ExportNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let commit_notes: Arc<Mutex<VecDeque<CommitNote>>> = Arc::new(Mutex::new(VecDeque::new()));
        let script_notes: Arc<Mutex<VecDeque<crate::env::ScriptNote>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        let cell_notes: Arc<Mutex<VecDeque<crate::cell::CellNote>>> =
            Arc::new(Mutex::new(VecDeque::new()));

        Self {
            conns: Mutex::new(HashMap::new()),
            done,
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

    /// 取 / 懒建指定连接的队列（并发提交时建一次；锁内短暂持有）
    fn conn_queue(&self, connection: &Option<String>) -> Arc<ConnQueue> {
        let key = connection_key(connection);
        let runner = self.runner.clone();
        let done = self.done.clone();
        let script_notes = self.script_notes.clone();
        let cell_notes = self.cell_notes.clone();
        let mut conns = lock(&self.conns);
        conns
            .entry(key)
            .or_insert_with(|| {
                let (tx, rx) = mpsc::channel::<ExecJob>();
                let pending = Arc::new(AtomicUsize::new(0));
                let cancel_requested = Arc::new(AtomicBool::new(false));
                let worker_pending = pending.clone();
                let worker_cancelled = cancel_requested.clone();
                std::thread::Builder::new()
                    .name("rds-editor-exec".to_string())
                    // 驱动解析在 debug 下递归较深（与 nav 工作线程同一考虑）
                    .stack_size(16 * 1024 * 1024)
                    .spawn(move || {
                        while let Ok(job) = rx.recv() {
                            for sql in job.statements {
                                // 中断之后**不再往下跑**（架构 §3.4：剩余语句各自回一条"已取消"），
                                // 否则用户看到的"中断"只是打断了当前那一句。
                                let result = if worker_cancelled.load(Ordering::SeqCst) {
                                    Err(CANCELED.to_string())
                                } else if let Some((offset, limit)) = job.segment {
                                    // 【B5b】取下一段：同一条原 SQL 的后一段（不新开结果集）
                                    runner.fetch_next(
                                        job.connection.as_deref(),
                                        job.schema.as_deref(),
                                        job.channel,
                                        &sql,
                                        offset,
                                        limit,
                                    )
                                } else if let Some((filter, columns)) = job.filtered.as_ref() {
                                    // 【B14】下发源库：拼 WHERE 的活交给执行器（它才知道方言）
                                    runner.run_filtered(
                                        job.connection.as_deref(),
                                        job.schema.as_deref(),
                                        job.channel,
                                        &sql,
                                        filter,
                                        columns,
                                    )
                                } else if let Some((column, descending)) = job.sorted_down.as_ref() {
                                    // 【B14】排序下发：同上，改写与方言差异都在执行器
                                    runner.run_sorted_down(
                                        job.connection.as_deref(),
                                        job.schema.as_deref(),
                                        job.channel,
                                        &sql,
                                        column,
                                        *descending,
                                    )
                                } else if let Some(request) = job.analysis.as_ref() {
                                    // 【B15】本地分析：桥接的行 + 分析 SQL 一起交给执行器（不碰源库）
                                    runner.analyze(request)
                                } else {
                                    // 连接透传给执行器（B1）：`None` = 未绑定 → 执行器自己决定回退口径
                                    runner.run(
                                        job.connection.as_deref(),
                                        job.schema.as_deref(),
                                        job.channel,
                                        &sql,
                                        job.options,
                                    )
                                };
                                // B4：事务状态跟着结论一起回去（界面不必再单独问一句）
                                let transaction =
                                    runner.transaction_snapshot(job.connection.as_deref());
                                if let Ok(mut queue) = done.lock() {
                                    // 【A2】下发载荷（filtered / sorted_down）还原成形成原料，随结论回去
                                    let pushdown = Pushdown::from_job(&job.filtered, &job.sorted_down);
                                    queue.push_back(ExecOutcome {
                                        document: job.document.clone(),
                                        connection: job.connection.clone(),
                                        sql,
                                        placement: job.placement,
                                        channel: job.channel,
                                        transaction,
                                        analysis: job.analysis.is_some(),
                                        lineage: job.lineage,
                                        pushdown,
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
                                    runner.run_script(request)
                                };
                                if let Ok(mut queue) = script_notes.lock() {
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
                                    runner.run_cell(request)
                                };
                                if let Ok(mut queue) = cell_notes.lock() {
                                    queue.push_back(crate::cell::CellNote {
                                        document: job.document.clone(),
                                        session_id: request.session_id.clone(),
                                        cell_id: request.cell_id.clone(),
                                        result,
                                    });
                                }
                            }
                            // 本作业（含批量全部语句）结束：让「该连接有执行」回到按需判定的计数，
                            // 并清掉取消位——取消只作用于**当前这一批**（剩余语句标"已取消"），
                            // 同连接排队的下一个作业是新意图，不该被上一批的取消误伤。
                            worker_pending.fetch_sub(1, Ordering::SeqCst);
                            worker_cancelled.store(false, Ordering::SeqCst);
                        }
                    })
                    .expect("failed to spawn editor exec worker");
                Arc::new(ConnQueue {
                    tx,
                    pending,
                    cancel_requested,
                })
            })
            .clone()
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

    /// 提交一次执行（空目标会被拒；**同连接内排队**，不同连接并行——阶段②）
    ///
    /// `connection` 是**文档绑定的连接**（B1）；`None` = 未绑定，进统一未绑定队列，
    /// 执行器回退到“当前活动连接”。`placement` 决定结果落到当前结果集还是新结果集（B2）；
    /// `options` 携带「自动提交关闭 → 本次执行进事务」（B4）；`channel` 是**执行通道**
    /// （B13，原样回到结论里）。
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
        // 提交线程同步 +1 未完成计数（不能等工作线程收到 job 再写）：
        // 用户可能刚按执行就按中断，那时工作线程还没开始——取消靠“该连接有作业”的计数
        // 立即看到目标（阶段②：计数按连接隔离，取消只命中目标连接）。
        let queue = self.conn_queue(&connection);
        queue.pending.fetch_add(1, Ordering::SeqCst);
        // 新提交复位取消位：同连接的上一批被取消后，这一批是用户的新意图，不该接着被标“已取消”
        queue.cancel_requested.store(false, Ordering::SeqCst);
        if queue
            .tx
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
            // 通道已关（worker 退出）不可能把作业送进去：计数回滚，如实报 Busy
            queue.pending.fetch_sub(1, Ordering::SeqCst);
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
        if self.is_busy_for(&connection) {
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
                        degraded: request.degraded,
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
        if self.is_busy_for(&connection) {
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

    /// 只取回属于指定文档的回执；其他文档的回执留在队列里，等归属面板自己的泵取走。
    ///
    /// 多面板共享同一个队列：若每个面板都全量 [`Self::drain`]，空闲面板会抢先取走别的文档
    /// 回执并丢弃，归属面板队列被抢空、结果区不再刷新（0.2.4 现象 2 竞态）。按文档隔离排空
    /// 从根上消除跨面板抢回执。
    pub fn drain_for(&self, document: &DocumentId) -> Vec<ExecOutcome> {
        let mut queue = match self.done.lock() {
            Ok(queue) => queue,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mine: Vec<ExecOutcome> = queue
            .iter()
            .filter(|outcome| &outcome.document == document)
            .cloned()
            .collect();
        queue.retain(|outcome| &outcome.document != document);
        mine
    }

    /// 中断指定连接的当前作业（B3；阶段②：取消按连接，只命中目标连接）
    ///
    /// 同步只做"能不能中断"的判断（该连接没在跑就直接回绝，理由可读）；真正的中断在一条一次性
    /// 线程上做——端口实现允许阻塞（有的驱动要发 `KILL QUERY` 这样的往返），而 UI 线程不能等。
    ///
    /// 先置中断意图再叫取消：批量里**剩余语句**据此标“已取消”。
    /// `connection` 为 `None` 时取消**未绑定队列**（键 `""`）的当前作业。
    pub fn cancel(&self, connection: Option<String>) -> Result<(), String> {
        let queue = {
            let conns = lock(&self.conns);
            match conns.get(&connection_key(&connection)) {
                Some(queue) => queue.clone(),
                None => return Err("当前没有执行在运行".to_string()),
            }
        };
        if queue.pending.load(Ordering::SeqCst) == 0 {
            return Err("当前没有执行在运行".to_string());
        }
        queue.cancel_requested.store(true, Ordering::SeqCst);
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

    /// 是否有**任一连接**的执行在跑 / 排队（状态栏 / 全局指示用）
    pub fn is_busy(&self) -> bool {
        let conns = lock(&self.conns);
        conns
            .values()
            .any(|queue| queue.pending.load(Ordering::SeqCst) > 0)
    }

    /// 指定连接是否有执行在跑 / 排队（事务 / 提交 / 取消的按连接判断；阶段②）
    pub fn is_busy_for(&self, connection: &Option<String>) -> bool {
        let conns = lock(&self.conns);
        match conns.get(&connection_key(connection)) {
            Some(queue) => queue.pending.load(Ordering::SeqCst) > 0,
            None => false,
        }
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

/// 连接 → 队列键：`None`（未绑定）统一归 `""`——未绑定作业最终都落到同一活动连接，
/// 它们天然该串行（阶段②分组的边界就是“真正的连接”，未绑定是同一目标的别名）。
fn connection_key(connection: &Option<String>) -> String {
    connection.clone().unwrap_or_default()
}

impl Drop for ExecQueue {
    fn drop(&mut self) {
        // 放掉全部连接通道：各 worker 的 `recv` 随即收到关闭，线程自然退出（生命周期：随宿主）
        if let Ok(conns) = self.conns.get_mut() {
            conns.clear();
        }
    }
}


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
//! ## 并发策略（1a → 阶段②）
//!
//! **按连接分组并发**：每个连接一条专属工作线程 + mpsc（懒建），不同连接可同时执行；
//! 同一连接内**排队串行**（单通道天然 FIFO，批量语句在同一 worker 上顺序跑完，不会被别的
//! 提交插队）。未绑定作业（`None`）统一归 `""` 队列——它们最终都落到同一活动连接，天然串行。
//! 忙态按连接计数（[`ExecQueue::is_busy_for`]），取消按连接（[`ExecQueue::cancel`]）。
//! 事务 / 会话亲和（架构 §12 #2）属 1b：那需要 per-session 独占连接，不是这里加锁能解决的。

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use ::shared::FactFrame;

use crate::channel::ExecChannel;
use crate::export::RowKey;
use crate::model::DocumentId;
// 【下移】ResultPlacement / Pushdown 已随结果存储搬到 rds-result-panel（editor 经
// `crate::store` 重导出）：执行层与结果存储共用同一份定义，不再各写一份。
// `pub use` 保持 `crate::execution::ResultPlacement` 旧路径对调用方仍公开。
pub use crate::store::{Pushdown, ResultPlacement};

// ==== 拆分兼容层（2026-10-29）：execution.rs 已拆为 exec/{target,runner,notes,queue}.rs ====
// `crate::exec::execution::*`（即 `crate::execution::*` 旧路径）经以下 re-export 保持不变。
pub use super::notes::{
    CommitNote, CommitReport, DuckDbExportRequest, ExecOutcome, ExportNote, SourceAction, SourceNote,
    SubmitError, TxNote,
};
pub use super::queue::ExecQueue;
pub use super::runner::{QueryData, QueryRunner};
pub use super::target::{
    ExecMenuKind, ExecTarget, RunOptions, SEGMENT_ROWS, TxAction, TxSnapshot, all_target,
    batch_target, resolve_target, statement_target, target_for_menu,
};

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        CommitReport, ExecMenuKind, ExecQueue, ExecTarget, QueryData, QueryRunner, RunOptions,
        SEGMENT_ROWS, SourceAction, SubmitError, TxAction, TxNote, TxSnapshot,
        all_target, batch_target, resolve_target, statement_target, target_for_menu,
    };
    use crate::channel::ExecChannel;
    use crate::model::DocumentId;
    // 【下移】ResultPlacement / Pushdown 在 rds-result-panel，经 crate::store 重导出
    use crate::store::ResultPlacement;
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

    /// 落位：全部执行项都往新结果集放（每次执行 = 一个新结果 tab；2026-10-27 起默认执行
    /// 不再覆盖当前 tab——用户反馈「一个 tab 容纳了好多结果」）
    #[test]
    fn batch_and_new_set_land_in_new_result_sets() {
        assert_eq!(ExecMenuKind::Batch.placement(), ResultPlacement::NewSet);
        assert_eq!(ExecMenuKind::NewSet.placement(), ResultPlacement::NewSet);
        for kind in [
            ExecMenuKind::CurrentStatement,
            ExecMenuKind::Selection,
            ExecMenuKind::All,
        ] {
            assert_eq!(kind.placement(), ResultPlacement::NewSet, "{kind:?}");
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

    /// 【A2】下发载荷还原成形成原料：filtered / sorted_down 互斥，只有其一
    #[test]
    fn pushdown_round_trips_from_the_job_payload() {
        use crate::store::Pushdown;
        assert_eq!(
            Pushdown::from_job(
                &Some(("ab".to_string(), vec!["id".to_string(), "name".to_string()])),
                &None,
            ),
            Some(Pushdown::Filtered {
                filter: "ab".to_string(),
                columns: vec!["id".to_string(), "name".to_string()],
            })
        );
        assert_eq!(
            Pushdown::from_job(&None, &Some(("created_at".to_string(), true))),
            Some(Pushdown::SortedDown {
                column: "created_at".to_string(),
                descending: true,
            })
        );
        // 两个都没有 = 普通执行（没有可保持的形态）
        assert_eq!(Pushdown::from_job(&None, &None), None);
        // 同时有（不该发生）→ 按 filtered 优先（一次只有一个下发载荷）
        let both = Pushdown::from_job(
            &Some(("ab".to_string(), vec!["id".to_string()])),
            &Some(("id".to_string(), false)),
        );
        assert!(matches!(both, Some(Pushdown::Filtered { .. })));
    }

    /// 【A2】血缘摘要：四个下发动作各自有名，普通执行归「原查询」
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
                fact: None,
                pref: ::shared::StoragePref::Auto,
                source_set: 0,
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
                fact: None,
                source_fact_table: None,
                source_fact_release: false,
                source_fact_set: 0,
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
                fact: None,
                source_fact_table: None,
                source_fact_release: false,
                source_fact_set: 0,
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
                fact: None,
                source_fact_table: None,
                source_fact_release: false,
                source_fact_set: 0,
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
        channel
            .cancel(Some("P_orders".to_string()))
            .expect("中断应当被接受");
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
        channel
            .cancel(Some("P_orders".to_string()))
            .expect("中断应当被接受");
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
        let error = channel.cancel(None).expect_err("没在跑就该回绝");
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
        channel.cancel(None).expect("中断应当被接受");
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

    /// 阶段②：**不同连接并行**——连接 A 跑慢查询时，连接 B 的作业照常执行（不被全局串行卡住）
    #[test]
    fn different_connections_run_in_parallel() {
        let (runner, _stop, calls, _seen) = CancellableRunner::new();
        let channel = ExecQueue::new(runner);
        channel
            .submit(
                DocumentId::new("doc-a"),
                &ExecTarget::Statement("select slow".to_string()),
                Some("conn-a".to_string()), None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交 A");
        wait_until_started(&calls, 1);

        // A 在跑（5 秒自旋）时提交 B：B 应立即执行并回执，不等 A 结束
        channel
            .submit(
                DocumentId::new("doc-b"),
                &ExecTarget::Statement("select fast".to_string()),
                Some("conn-b".to_string()), None,
                ResultPlacement::NewSet,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交 B");

        let deadline = Instant::now() + Duration::from_secs(1);
        let mut b_seen = false;
        while !b_seen {
            let done = channel.drain();
            assert!(
                done.iter().all(|o| o.document == DocumentId::new("doc-b")),
                "A 不该在这 1 秒内完成（它在自旋等取消）"
            );
            b_seen = !done.is_empty();
            assert!(
                Instant::now() < deadline,
                "B 被 A 阻塞了：1 秒内没等到 B 的回执"
            );
            std::thread::sleep(Duration::from_millis(5));
        }

        // 收尾：中断 A，等全部回执 + 回到空闲
        channel
            .cancel(Some("conn-a".to_string()))
            .expect("中断 A 应当被接受");
        wait_for(&channel, 1);
        wait_until_idle(&channel);
    }

    /// 阶段②：**同一连接排队串行**——后提交的作业等前一个结束才开跑，且结论保序
    #[test]
    fn same_connection_queues_serially_in_order() {
        let (runner, _stop, calls, _seen) = CancellableRunner::new();
        let channel = ExecQueue::new(runner);
        channel
            .submit(
                DocumentId::new("doc-1"),
                &ExecTarget::Statement("select slow".to_string()),
                Some("conn-s".to_string()), None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交第 1 个（慢）");
        wait_until_started(&calls, 1);
        channel
            .submit(
                DocumentId::new("doc-2"),
                &ExecTarget::Statement("select fast".to_string()),
                Some("conn-s".to_string()), None,
                ResultPlacement::NewSet,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("提交第 2 个（快，排队）");

        // 第 2 个提交时第 1 个还在跑：同连接不得并发，calls 只能是 1
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "同连接排队串行：后提交的作业不得与先行的并发执行"
        );

        // 中断第 1 个，放行队列：第 2 个接着跑完，结论按提交顺序返回
        channel
            .cancel(Some("conn-s".to_string()))
            .expect("中断应当被接受");
        let done = wait_for(&channel, 2);
        assert_eq!(
            done[0].document,
            DocumentId::new("doc-1"),
            "同连接结论保序：先提交的先回"
        );
        assert_eq!(
            done[1].document,
            DocumentId::new("doc-2"),
            "同连接结论保序：后提交的后回"
        );
        assert!(done[1].result.is_ok(), "第 2 个没有被前面的中断误伤");
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
        channel.cancel(None).expect("中断应当被接受");

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

        channel.cancel(None).expect("收尾：中断掉那句慢查询");
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

    /// 阶段②：同一连接忙时第二次提交**排队**（不拒绝、不丢作业；与旧 1a「拒绝」语义的差异点）
    #[test]
    fn a_second_submit_on_the_same_connection_queues_while_one_is_running() {
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

        // 阶段②：同一连接第二次提交**排队**而非拒绝（不同连接才并行，同连接串行不丢作业）
        channel
            .submit(
                document,
                &target,
                None, None,
                ResultPlacement::Replace,
                RunOptions::default(),
                crate::channel::ExecChannel::default(),
            )
            .expect("排队成功：同连接第二次提交不拒绝");

        *release.lock().unwrap() = true;
        let done = wait_for(&channel, 2);
        assert_eq!(done.len(), 2, "两次提交都产生结果");
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
        // 执行与提交同连接（阶段②：忙态按连接判断，只拦同连接的提交）
        queue
            .submit(
                DocumentId::new("doc-busy"),
                &ExecTarget::Statement("select 1".to_string()),
                Some("P_orders".to_string()), None,
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

//! 执行测试 —— 执行队列（CancellableRunner/cancel/parallel/serial）
//!
//! 自 execution.rs tests 拆分（2026-10-29 Step 6）。
//!
use super::*;

    /// 可取消的假执行器：慢查询在 `run` 里自旋等取消（模拟驱动被取消令牌打断）
    pub(crate) struct CancellableRunner {
        stop: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
        seen_cancels: SeenConnections,
    }

    impl CancellableRunner {
        pub(crate) fn new() -> (
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
        pub(crate) struct NothingToCancel {
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

//! 执行测试 —— 事务/刷新/主键/模式（TxRunner/RefreshRunner/PrimaryRunner/SchemaRunner）
//!
//! 自 execution.rs tests 拆分（2026-10-29 Step 6）。
//!
use super::*;
use super::queue::CancellableRunner;

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
//! 执行测试 —— 执行器（FakeRunner/lineage/等待辅助）
//!
//! 自 execution.rs tests 拆分（2026-10-29 Step 6）。
//!
use super::*;

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
    pub(crate) type SeenConnections = Arc<Mutex<Vec<Option<String>>>>;

    /// 假执行器：记录调用次数与收到的连接，返回固定结果（按 SQL 决定成败）
    pub(crate) struct FakeRunner {
        pub(crate) calls: Arc<AtomicUsize>,
        pub(crate) seen_connections: SeenConnections,
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

    pub(crate) fn channel() -> (ExecQueue, Arc<AtomicUsize>, SeenConnections) {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen_connections = Arc::new(Mutex::new(Vec::new()));
        let channel = ExecQueue::new(Arc::new(FakeRunner {
            calls: calls.clone(),
            seen_connections: seen_connections.clone(),
        }));
        (channel, calls, seen_connections)
    }

    /// 轮询等待结果（带超时：通道坏掉时测试要失败而不是挂住）
    pub(crate) fn wait(channel: &ExecQueue) -> Vec<super::ExecOutcome> {
        wait_for(channel, 1)
    }

    /// 轮询等待 N 条结果（批量：语句逐条回来，只等第一条会把后面的漏掉）
    pub(crate) fn wait_for(channel: &ExecQueue, count: usize) -> Vec<super::ExecOutcome> {
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
    pub(crate) fn wait_until_idle(channel: &ExecQueue) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while channel.is_busy() {
            assert!(Instant::now() < deadline, "通道迟迟没回到空闲");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 等一次取消真的送到了执行器（取消在一次性线程上做，得等一小下）
    pub(crate) fn wait_until_cancelled(seen: &SeenConnections) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while seen.lock().expect("锁").is_empty() {
            assert!(Instant::now() < deadline, "取消没送到执行器");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 等工作线程真的把某句发出去了（要中断的是“跑着的查询”，不是“刚提交的”那个瞬间）
    pub(crate) fn wait_until_started(calls: &Arc<AtomicUsize>, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while calls.load(Ordering::SeqCst) < expected {
            assert!(Instant::now() < deadline, "语句迟迟没发出去");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // ===== B3：中断 =====

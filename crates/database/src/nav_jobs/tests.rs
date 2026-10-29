//! `nav_jobs` 单测：队列调度 / 信号量限流 / 搜索命中映射 / 结果回填。
use super::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use crate::model::NavPath;
use engine::persistence::FtsSearchResult;
use engine::ObjectKind;
/// 测试用 `Shared`：不起 worker（不发 job），只验「分发 / 取走」两件事。
fn test_shared() -> Shared {
    let (tx, _rx) = mpsc::channel::<QueuedJob>();
    Shared {
        tx: Mutex::new(tx),
        worker_alive: AtomicBool::new(true),
        warm_active: AtomicBool::new(false),
        warm_done: AtomicUsize::new(0),
        warm_total: AtomicUsize::new(0),
        warm_cancel: AtomicBool::new(false),
        pending_loads: AtomicUsize::new(0),
        pending_props: AtomicUsize::new(0),
        pending_sql: AtomicUsize::new(0),
        pending_test: AtomicUsize::new(0),
        pending_connect: AtomicUsize::new(0),
        pending_search: std::array::from_fn(|_| AtomicUsize::new(0)),
        pending_row_estimates: AtomicUsize::new(0),
        load_results: Mutex::new(Vec::new()),
        props_results: Mutex::new(Vec::new()),
        sql_results: Mutex::new(Vec::new()),
        test_results: Mutex::new(Vec::new()),
        connect_results: Mutex::new(Vec::new()),
        search_results: std::array::from_fn(|_| Mutex::new(Vec::new())),
        row_estimates: Mutex::new(Vec::new()),
        conn_sems: Mutex::new(HashMap::new()),
    }
}

fn result(consumer: SearchConsumer, query: &str) -> SearchResult {
    SearchResult {
        consumer,
        seq: 0,
        query: query.to_string(),
        searched: 1,
        hits: Vec::new(),
    }
}

fn queued(priority: JobPriority, seq: u64) -> QueuedJob {
    QueuedJob {
        priority,
        seq,
        job: Job::Warm {
            conn_id: "P_conn".to_string(),
            project_root: None,
        },
    }
}

/// S2-② stale guard 判定：删除 / 断开 / 改配置任一命中即过期；指纹 `None` 跳过比对。
#[test]
fn stale_guard_drops_removed_disconnected_or_reconfigured_connections() {
    use super::nav_result_is_stale;
    // 正常：存在 + 已连 + 指纹一致（或双方都无指纹）→ 不丢弃
    assert!(!nav_result_is_stale(true, true, Some("fp-a"), Some("fp-a")));
    assert!(!nav_result_is_stale(true, true, None, None), "无指纹跳过比对");
    // 连接被删 / 断开：与指纹无关，直接过期
    assert!(nav_result_is_stale(false, true, None, None), "删除 → 丢弃");
    assert!(nav_result_is_stale(true, false, None, None), "断开 → 丢弃");
    // 配置变更：指纹不一致 → 过期
    assert!(
        nav_result_is_stale(true, true, Some("fp-a"), Some("fp-b")),
        "改配置 → 丢弃"
    );
    // 指纹从有到无也算变更（连接重注册为不可共享）——但 `None` 快照不比对
    assert!(
        nav_result_is_stale(true, true, Some("fp-a"), None),
        "指纹消失 → 丢弃"
    );
    assert!(
        !nav_result_is_stale(true, true, None, Some("fp-b")),
        "快照无指纹时不做反向比对（连接是后注册的，结果仍可用）"
    );
}

/// S2-① 排序契约：用户手点（User）排在后台（Background）前面；
/// 同一优先级内按入队序号（seq）FIFO 公平——高优插队不破坏低优彼此的顺序。
/// （Ord 面向 max-heap：越优先、seq 越小 → 越大。）
#[test]
fn queued_job_orders_priority_then_fifo() {
    use super::JobPriority;
    assert!(
        queued(JobPriority::User, 1) > queued(JobPriority::Background, 0),
        "User 必须排在 Background 前面（即使 Background 先入队）"
    );
    assert!(
        queued(JobPriority::User, 0) > queued(JobPriority::User, 1),
        "同优先级按入队序 FIFO（seq 小者先出）"
    );
    assert!(
        queued(JobPriority::Background, 0) > queued(JobPriority::Background, 1),
        "Background 内部同样 FIFO"
    );
    // BinaryHeap 是 max-heap：`pop` 出**最大**者；Ord 里比较的是「谁更优先」，
    // 堆顶应是最优先的那个（User、seq 最小）。
    let mut heap = std::collections::BinaryHeap::new();
    heap.push(queued(JobPriority::Background, 2));
    heap.push(queued(JobPriority::User, 9));
    heap.push(queued(JobPriority::Background, 1));
    assert_eq!(heap.pop().unwrap().seq, 9, "User(seq=9) 插到 Background 前面");
    assert_eq!(heap.pop().unwrap().seq, 1, "Background 内部 seq=1 先于 seq=2");
    assert_eq!(heap.pop().unwrap().seq, 2);
}

/// 三个消费方的槽互不可见：取走自己的批次不会动到对方（本仓曾经的“互抢”就是这里错的）。
#[test]
fn search_results_are_partitioned_by_consumer() {
    let shared = test_shared();
    shared.push_search_result(
        SearchConsumer::Navigator,
        result(SearchConsumer::Navigator, "nav"),
    );
    shared.push_search_result(
        SearchConsumer::QuickOpen,
        result(SearchConsumer::QuickOpen, "qo"),
    );
    shared.push_search_result(
        SearchConsumer::Panel,
        result(SearchConsumer::Panel, "panel"),
    );

    let quick_open = shared.take_search_results(SearchConsumer::QuickOpen);
    assert_eq!(quick_open.len(), 1);
    assert_eq!(quick_open[0].query, "qo");

    // Quick Open 取走自己的批次后，导航 / 面板的批次仍在各自槽里
    let nav = shared.take_search_results(SearchConsumer::Navigator);
    assert_eq!(nav.len(), 1);
    assert_eq!(nav[0].query, "nav");
    let panel = shared.take_search_results(SearchConsumer::Panel);
    assert_eq!(panel.len(), 1);
    assert_eq!(panel[0].query, "panel");

    // `take` 是取走语义：再取为空
    assert!(
        shared
            .take_search_results(SearchConsumer::QuickOpen)
            .is_empty()
    );
    assert!(
        shared
            .take_search_results(SearchConsumer::Navigator)
            .is_empty()
    );
    assert!(
        shared
            .take_search_results(SearchConsumer::Panel)
            .is_empty()
    );
}

/// 内容档映射：空串转 `None`、catalog 缺位、snippet 带上。
#[test]
fn fts_hit_maps_empty_strings_and_keeps_snippet() {
    let target = SearchTarget {
        conn_id: "P_conn".to_string(),
        label: "营销分析".to_string(),
        driver: "postgres".to_string(),
    };
    let mapped = fts_hit_to_search_hit(
        &target,
        FtsSearchResult {
            search_type: "column".to_string(),
            schema_name: "public".to_string(),
            object_name: "channel_code".to_string(),
            parent_name: String::new(), // FTS 行里的“无父对象”是空串
            snippet: "…下单<mark>渠道</mark>…".to_string(),
        },
        HitSource::Comment,
    );
    assert_eq!(mapped.hit_source, HitSource::Comment);
    assert_eq!(mapped.conn_id, "P_conn");
    assert_eq!(mapped.object_type, "column");
    assert_eq!(mapped.object_name, "channel_code");
    assert_eq!(mapped.parent_name, None, "空串不能当父对象名");
    assert_eq!(mapped.schema.as_deref(), Some("public"));
    assert_eq!(mapped.catalog, None, "FTS 表里没有 catalog");
    assert!(
        mapped
            .snippet
            .as_deref()
            .unwrap_or_default()
            .contains("<mark>"),
        "内容档要把 snippet 带出去（UI 靠它显示“为什么命中”）"
    );
}

/// 计数归零是**异步**的：worker 先回填结果、后 `fetch_sub`（见 [`worker`]），且
/// 并发套件下兄弟测试的在途任务也会让全局计数暂时非零。结果 drain 到手后
/// 还要给计数一个归零窗口再断言，否则 15s 轮询结束的最后一个微窗口会偶发误报
/// （2026-10-15 并发负载下实测 flake 一次；隔离复跑 5/5 通过）。
fn settle_pending_loads(max_wait: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + max_wait;
    while std::time::Instant::now() < deadline {
        if !has_pending_loads() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    !has_pending_loads()
}

/// **一次 panic 不再拖垮整条导航加载队列**（真机踩到的形态：一个坏表 → worker 线程死掉
/// → 之后所有展开都没有回音，界面永远「加载中」，且挂起计数让进度条停在半路）。
///
/// 这条用例走**真实全局 worker**（`enqueue_*` 的生产路径）：先投一个必定 panic 的任务，
/// 再投一个正常任务（连接不存在 → 回填 Err）。两个都要有回音，且计数要归零。
#[test]
fn worker_isolates_panics_and_recovers_from_a_dead_channel() {
    let path = NavPath::Connection;
    submit(
        Job::PanicForTest {
            key: "panic-test".to_string(),
            conn_id: "P_no_such_conn".to_string(),
            project_root: None,
            path: path.clone(),
        },
        JobPriority::User,
    );
    shared().pending_loads.fetch_add(1, Ordering::SeqCst);
    // 贴着生产入口再投一个正常任务
    enqueue_load(
        "P_no_such_conn",
        None,
        "after-panic",
        path,
        false,
        JobPriority::Background,
        None,
        false,
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut results = Vec::new();
    while std::time::Instant::now() < deadline {
        results.extend(drain_load_results());
        if results.len() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let keys: Vec<&str> = results.iter().map(|r| r.key.as_str()).collect();
    assert!(
        keys.contains(&"panic-test") && keys.contains(&"after-panic"),
        "两个任务都要有回音（panic 的那个回错误、后面的正常跑完）：{keys:?}"
    );
    for r in &results {
        assert!(r.result.is_err(), "两者都该是错误结果：{}", r.key);
    }
    assert!(
        settle_pending_loads(std::time::Duration::from_secs(5)),
        "计数必须配平：panic 那条路曾经漏减，界面会永远「加载中」"
    );

    // ---- 自愈：把发件端换到一条**接收端已丢弃**的通道（模拟 worker 线程已退出），
    // 下一次投递应当自动换通道 + 重启 worker，任务照样有回音。
    let (dead_tx, dead_rx) = mpsc::channel::<QueuedJob>();
    drop(dead_rx);
    *lock(&shared().tx) = dead_tx;
    shared().worker_alive.store(false, Ordering::SeqCst);

    enqueue_load(
        "P_no_such_conn",
        None,
        "after-restart",
        NavPath::Connection,
        false,
        JobPriority::Background,
        None,
        false,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut recovered = Vec::new();
    while std::time::Instant::now() < deadline {
        recovered.extend(drain_load_results());
        if !recovered.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let recovered_keys: Vec<String> = recovered.iter().map(|r| r.key.clone()).collect();
    assert!(
        recovered.iter().any(|r| r.key == "after-restart"),
        "worker 死掉后任务要能自愈（换通道 + 重启）：{recovered_keys:?}"
    );
    assert!(
        settle_pending_loads(std::time::Duration::from_secs(5)),
        "自愈那条路同样要配平计数"
    );
}

/// 槽位下标两两不同且落在 `COUNT` 内（`ix()` 与 `COUNT` 的契约）。
#[test]
fn consumer_slots_are_distinct() {
    assert_ne!(
        SearchConsumer::Navigator.ix(),
        SearchConsumer::QuickOpen.ix()
    );
    assert_ne!(
        SearchConsumer::Navigator.ix(),
        SearchConsumer::Panel.ix()
    );
    assert_ne!(
        SearchConsumer::QuickOpen.ix(),
        SearchConsumer::Panel.ix()
    );
    assert!(SearchConsumer::Navigator.ix() < SearchConsumer::COUNT);
    assert!(SearchConsumer::QuickOpen.ix() < SearchConsumer::COUNT);
    assert!(SearchConsumer::Panel.ix() < SearchConsumer::COUNT);
}

fn search_temp_root(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_navsearch_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// 播种一个连接缓存：schema + 一张表 + 一列，重建索引（此后 `search_index` 可命中）。
fn seed_cache(conn_id: &str, root: &std::path::Path, table: &str) {
    use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};
    let mut cache = crate::cache::NavCache::open(conn_id, Some(&root.to_string_lossy())).expect("打开缓存");
    cache.put_schemas("main", &["public".to_string()]);
    cache.mark_schema_complete("main", "public");
    let sid = cache.schema_id("main", "public").expect("schema_id");
    cache.put_objects(sid, &[NodeInfo::new(table, SchemaObjectKind::Table)]);
    cache.put_columns(
        sid,
        table,
        &[ColumnDetail {
            name: "order_id".to_string(),
            data_type: "INTEGER".to_string(),
            nullable: false,
            is_primary_key: true,
            is_foreign_key: false,
            default_value: None,
            comment: None,
            is_identity: false,
            identity_generation: None,
            collation: None,
            extra: Default::default(),
            references: None,
            ordinal: 0,
        }],
    );
    cache.rebuild_index("main", "public");
}

fn target(conn_id: &str) -> SearchTarget {
    SearchTarget {
        conn_id: conn_id.to_string(),
        label: conn_id.to_string(),
        driver: "postgres".to_string(),
    }
}

/// schema.object 两段拆分：恰好一个点且两边非空才拆；多段 / 空段 / 无点不拆。
#[test]
fn split_two_segment_only_on_single_dot() {
    use super::nav_split_two_segment;
    assert_eq!(nav_split_two_segment("public.orders"), (Some("public"), Some("orders")));
    assert_eq!(nav_split_two_segment("orders"), (None, None));
    assert_eq!(nav_split_two_segment("a.b.c"), (None, None));
    assert_eq!(nav_split_two_segment("a."), (None, None));
    assert_eq!(nav_split_two_segment(".b"), (None, None));
}

/// 两段搜索：名称段 LIKE 中缀 + 归属段精筛（schema/catalog 任一命中）。
/// seed_cache 把表播在 `public` schema 下：`public.orders` 命中、`other.orders` 不中。
#[test]
fn two_segment_search_filters_by_schema() {
    let root = search_temp_root("twoseg");
    let root_s = root.to_string_lossy().to_string();
    seed_cache("P_two_0", &root, "orders");
    let targets = [target("P_two_0")];

    let (_, hits) = search_targets(
        SearchKind::Name,
        "public.orders",
        false,
        false,
        NameMatchMode::Contains,
        false,
        false,
        Some(&root_s),
        &targets,
    );
    assert!(
        hits.iter().any(|h| h.object_name == "orders"),
        "public.orders 应命中 public 下的 orders：{hits:?}"
    );

    let (_, hits) = search_targets(
        SearchKind::Name,
        "other.orders",
        false,
        false,
        NameMatchMode::Contains,
        false,
        false,
        Some(&root_s),
        &targets,
    );
    assert!(
        hits.is_empty(),
        "other.orders 不得命中 public 下的 orders：{hits:?}"
    );

    // 单段词不受影响（回归）：`orders` 仍中缀命中。
    let (_, hits) = search_targets(
        SearchKind::Name,
        "orders",
        false,
        false,
        NameMatchMode::Contains,
        false,
        false,
        Some(&root_s),
        &targets,
    );
    assert!(hits.iter().any(|h| h.object_name == "orders"));
    let _ = std::fs::remove_dir_all(&root);
}

/// 纯函数 `search_targets`：保序（target 序）、跳过无缓存连接、每连接命中封顶。
#[test]
fn search_targets_keeps_target_order_and_skips_missing_cache() {
    let root = search_temp_root("chunk");
    let root_s = root.to_string_lossy().to_string();
    // 只播前两个连接的缓存；第三个不建（模拟从未内省过的连接）。
    seed_cache("P_chunk_a", &root, "orders_a");
    seed_cache("P_chunk_b", &root, "orders_b");
    let targets = [target("P_chunk_a"), target("P_chunk_b"), target("P_chunk_c")];

    let (searched, hits) = search_targets(
        SearchKind::Name,
        "order",
        false,
        false,
        NameMatchMode::Contains,
        false,
        false,
        Some(&root_s),
        &targets,
    );
    assert_eq!(searched, 2, "无缓存的连接不计入 searched");
    // 保序：按 target 序（a 的命中在前，b 的在后）
    let conns: Vec<&str> = hits.iter().map(|h| h.conn_id.as_str()).collect();
    let a_first = conns.iter().position(|&c| c == "P_chunk_a").unwrap();
    let b_first = conns.iter().position(|&c| c == "P_chunk_b").unwrap();
    assert!(a_first < b_first, "块内保序：{conns:?}");
    assert!(
        hits.iter().all(|h| h.conn_id != "P_chunk_c"),
        "无缓存连接不得有命中"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 搜索开关精筛（`Aa` 大小写 / `.*` 正则）：LIKE 中缀候选是超集，精筛只收不减；
/// 正则编译失败退化为不筛（不 panic）。
///
/// 注意正则模式 = 搜索词本身：LIKE 已要求该词**字面**中缀命中，故 `^` 锚定、
/// `.` 通配、字符类这类「LIKE 字面不匹配」的模式天然无候选（架构边界，见
/// [`search_targets`]）；正则开关在本架构里的实际能力是大小写联动（`(?i)` 前缀）
/// 与非法模式退化。本测试按此真实行为断言。
#[test]
fn search_targets_applies_case_and_regex_flags() {
    let root = search_temp_root("flags");
    let root_s = root.to_string_lossy().to_string();
    seed_cache("P_f_a", &root, "Orders");
    seed_cache("P_f_b", &root, "orders");
    seed_cache("P_f_c", &root, "orders[");
    let targets = [
        target("P_f_a"),
        target("P_f_b"),
        target("P_f_c"),
    ];
    let names = |hits: &[SearchHit]| -> std::collections::BTreeSet<String> {
        hits.iter()
            .map(|h| h.object_name.clone())
            .collect::<std::collections::BTreeSet<String>>()
    };

    // 默认（LIKE 不敏感）：三种大小写/字形都命中（列 order_id 不含 "orders" 字面，不中）。
    let (_, hits) =
        search_targets(
            SearchKind::Name,
            "orders",
            false,
            false,
            NameMatchMode::Contains,
            false,
            false,
            Some(&root_s),
            &targets,
        );
    assert_eq!(
        names(&hits),
        ["Orders", "orders", "orders["]
            .into_iter()
            .map(String::from)
            .collect(),
        "默认全中"
    );

    // `Aa`：只留精确小写（`contains` 是中缀，`orders[` 含 `orders` 也在列）。
    let (_, hits) =
        search_targets(
            SearchKind::Name,
            "orders",
            true,
            false,
            NameMatchMode::Contains,
            false,
            false,
            Some(&root_s),
            &targets,
        );
    assert_eq!(
        names(&hits),
        ["orders", "orders["].into_iter().map(String::from).collect(),
        "区分大小写只留小写"
    );

    // 正则（忽略大小写）：pattern = "(?i)orders"，`(?i)` 生效——`Orders` 保留
    //（若没 (?i) 会被正则大小写敏感排除）；中缀无锚定，`orders[` 也在。
    let (_, hits) =
        search_targets(
            SearchKind::Name,
            "orders",
            false,
            true,
            NameMatchMode::Contains,
            false,
            false,
            Some(&root_s),
            &targets,
        );
    assert_eq!(
        names(&hits),
        ["Orders", "orders", "orders["]
            .into_iter()
            .map(String::from)
            .collect(),
        "正则 (?i) 生效：Orders 保留"
    );

    // 正则 + 大小写：pattern = "orders"（无 (?i)）→ `Orders` 被正则敏感排除，
    // `orders[` 仍中（无锚定）。
    let (_, hits) =
        search_targets(
            SearchKind::Name,
            "orders",
            true,
            true,
            NameMatchMode::Contains,
            false,
            false,
            Some(&root_s),
            &targets,
        );
    assert_eq!(
        names(&hits),
        ["orders", "orders["].into_iter().map(String::from).collect(),
        "正则 + 大小写：排除 Orders"
    );

    // 非法正则：退化为不筛（候选 `orders[` 保留，不 panic）。
    let (_, hits) =
        search_targets(
            SearchKind::Name,
            "orders[",
            false,
            true,
            NameMatchMode::Contains,
            false,
            false,
            Some(&root_s),
            &targets,
        );
    assert_eq!(
        names(&hits),
        ["orders["].into_iter().map(String::from).collect(),
        "非法正则退化为不筛"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 并行路径走真实 worker：分块并行后按「chunk 序 + target 序」合并，`searched` 全计，
/// 全局 300 截断在合并后统一生效。
#[test]
fn parallel_search_merges_in_order_and_truncates_globally() {
    let root = search_temp_root("parallel");
    let root_s = root.to_string_lossy().to_string();
    let conn_ids = ["P_par_0", "P_par_1", "P_par_2", "P_par_3", "P_par_4", "P_par_5"];
    for (i, conn_id) in conn_ids.iter().enumerate() {
        seed_cache(conn_id, &root, &format!("orders_{i}"));
    }
    let targets: Vec<SearchTarget> = conn_ids.iter().map(|c| target(c)).collect();

    enqueue_search(
        SearchConsumer::Panel,
        0,
        SearchKind::Name,
        "order",
        false,
        false,
        NameMatchMode::Contains,
        false,
        false,
        Some(&root_s),
        targets,
    );
    // 轮询等 worker 回填（与 `worker_isolates_panics…` 同一 deadline 模式）
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut results = Vec::new();
    while std::time::Instant::now() < deadline {
        results.extend(drain_search_results(SearchConsumer::Panel));
        if !results.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!results.is_empty(), "并行搜索应有回音");
    let batch = &results[0];
    assert_eq!(batch.searched, 6, "全部有缓存的连接都应被搜到");
    // 保序：6 张表按 target 序（orders_0..orders_5）出现。
    // 注意每连接还命中列 `order_id`（也是中缀命中），所以整列表是
    // [orders_0, order_id, orders_1, order_id, …]——只断言表命中的相对顺序。
    let table_names: Vec<&str> = batch
        .hits
        .iter()
        .filter(|h| h.object_name.starts_with("orders_"))
        .map(|h| h.object_name.as_str())
        .collect();
    let expected: Vec<String> = (0..6).map(|i| format!("orders_{i}")).collect();
    assert_eq!(table_names, expected, "并行只提速不重排结果");
    // 同一连接内：表命中的所属连接正确
    assert!(
        batch.hits.iter().all(|h| h.object_name == "order_id" || h.conn_id.starts_with("P_par_")),
        "命中所属连接应与 target 一致"
    );
    assert!(
        batch.hits.len() <= SEARCH_MAX_HITS,
        "全局命中数不得超过 SEARCH_MAX_HITS"
    );
    assert!(!has_pending_search(SearchConsumer::Panel), "计数必须配平");
    let _ = std::fs::remove_dir_all(&root);
}

/// B-2-1 per-conn 信号量：同一 `conn_id` 复用同一 `Arc`，不同连接各自独立。
#[test]
fn per_conn_semaphore_reuses_arc_for_same_conn_and_isolates_others() {
    drop_conn_sem("sem_a");
    drop_conn_sem("sem_b");

    let a1 = per_conn_semaphore("sem_a");
    let a2 = per_conn_semaphore("sem_a");
    let b = per_conn_semaphore("sem_b");

    assert!(Arc::ptr_eq(&a1, &a2), "同一连接应复用同一信号量 Arc");
    assert!(!Arc::ptr_eq(&a1, &b), "不同连接信号量必须相互独立");

    drop_conn_sem("sem_a");
    drop_conn_sem("sem_b");
}

/// B-2-1 per-conn 容量 = 1：占用中第二次取锁失败，释放后可再取（让位族据此跳过）。
#[test]
fn per_conn_semaphore_capacity_one_blocks_second_acquire_until_release() {
    drop_conn_sem("sem_cap");
    let sem = per_conn_semaphore("sem_cap");

    let first = sem
        .clone()
        .try_acquire_owned()
        .expect("首次取锁应当成功");
    let second = sem.clone().try_acquire_owned();
    assert!(
        second.is_err(),
        "容量为 1：占用中第二次 try_acquire 必须失败"
    );

    drop(first);
    let third = sem.clone().try_acquire_owned();
    assert!(third.is_ok(), "许可归还后应当能再次取锁");

    drop_conn_sem("sem_cap");
}

/// B-2-1 条目回收：`drop_conn_sem` 移除后再取为全新信号量，旧残留令牌不继承；
/// 在途任务持有的旧 Arc 克隆不受回收影响。
#[test]
fn drop_conn_sem_resets_to_fresh_semaphore_without_inflight_leak() {
    drop_conn_sem("sem_reset");
    let old = per_conn_semaphore("sem_reset");
    let old_permit = old
        .clone()
        .try_acquire_owned()
        .expect("旧信号量首次取锁成功");

    // 断开 / 删除路径只回收 map 条目；在途持有的旧许可仍占住旧信号量、不被打断，
    // 旧 Arc 克隆随许可存活直到任务结束。
    drop_conn_sem("sem_reset");
    assert!(
        old.clone().try_acquire_owned().is_err(),
        "在途任务持有的旧许可仍占住旧信号量，不应被回收打断"
    );

    let fresh = per_conn_semaphore("sem_reset");
    assert!(!Arc::ptr_eq(&old, &fresh), "回收后重建必须是全新信号量");

    drop(old_permit);
    drop_conn_sem("sem_reset");
}

/// D11 FTS 类型串 → 身份类别映射：六类识别，未识别 / 空串返回 `None`。
#[test]
fn fts_type_maps_to_object_kind_and_unknown_yields_none() {
    assert_eq!(fts_type_to_object_kind("table"), Some(ObjectKind::Table));
    assert_eq!(fts_type_to_object_kind("view"), Some(ObjectKind::View));
    assert_eq!(fts_type_to_object_kind("column"), Some(ObjectKind::Column));
    assert_eq!(fts_type_to_object_kind("routine"), Some(ObjectKind::Routine));
    assert_eq!(fts_type_to_object_kind("schema"), Some(ObjectKind::Schema));
    assert_eq!(fts_type_to_object_kind("catalog"), Some(ObjectKind::Catalog));
    assert_eq!(fts_type_to_object_kind("index"), None);
    assert_eq!(fts_type_to_object_kind(""), None);
}

/// D11 注释候选并入：族 `comments` 集合放行类型；名称已命中去重；注释命中标
/// [`HitSource::Comment`] 并带 snippet；关闭注释开关时只走名称档、不并入注释候选。
#[test]
fn comments_candidates_merged_filtered_and_deduped() {
    use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};
    let root = search_temp_root("cmt");
    let root_s = root.to_string_lossy().to_string();

    let mut cache = crate::cache::NavCache::open("P_cmt_0", Some(&root_s)).expect("打开缓存");
    cache.put_schemas("main", &["public".to_string()]);
    cache.mark_schema_complete("main", "public");
    let sid = cache.schema_id("main", "public").expect("schema_id");
    cache.put_objects(sid, &[NodeInfo::new("orders", SchemaObjectKind::Table)]);
    let mk_col = |name: &str, comment: Option<&str>| ColumnDetail {
        name: name.to_string(),
        data_type: "VARCHAR".to_string(),
        nullable: true,
        is_primary_key: false,
        is_foreign_key: false,
        default_value: None,
        comment: comment.map(str::to_string),
        is_identity: false,
        identity_generation: None,
        collation: None,
        extra: Default::default(),
        references: None,
        ordinal: 0,
    };
    cache.put_columns(
        sid,
        "orders",
        &[
            mk_col("channel_code", Some("channel code")),
            mk_col("region", Some("channel region")),
        ],
    );
    cache.rebuild_index("main", "public");
    drop(cache);

    let targets = [SearchTarget {
        conn_id: "P_cmt_0".into(),
        label: "本地库".into(),
        driver: "postgres".into(),
    }];

    // 注释开关开启：名称候选 channel_code 先入；region 注释命中独立并入；channel_code 去重。
    let (_, hits) = search_targets(
        SearchKind::Name,
        "channel",
        false,
        false,
        NameMatchMode::Contains,
        true,
        false,
        Some(&root_s),
        &targets,
    );
    let by_name: std::collections::BTreeMap<String, &SearchHit> =
        hits.iter().map(|h| (h.object_name.clone(), h)).collect();

    let code = by_name
        .get("channel_code")
        .expect("名称候选 channel_code 应命中");
    assert_eq!(code.hit_source, HitSource::Name, "名称先入，来源 = Name");
    let region = by_name
        .get("region")
        .expect("注释候选 region 应并入");
    assert_eq!(region.hit_source, HitSource::Comment, "注释命中来源 = Comment");
    assert!(region.snippet.is_some(), "注释候选带 snippet 副行");
    assert_eq!(
        by_name.len(),
        2,
        "channel_code 去重后只剩两条：{by_name:?}"
    );

    // 注释开关关闭：不并入 region，名称档只出 channel_code。
    let (_, hits) = search_targets(
        SearchKind::Name,
        "channel",
        false,
        false,
        NameMatchMode::Contains,
        false,
        false,
        Some(&root_s),
        &targets,
    );
    assert!(
        hits.iter().all(|h| h.object_name != "region"),
        "关闭注释开关不得并入 region"
    );
    assert!(hits.iter().any(|h| h.object_name == "channel_code"));

    let _ = std::fs::remove_dir_all(&root);
}

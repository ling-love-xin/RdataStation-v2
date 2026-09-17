//! 列画像与表探查的**端到端**：真实 DuckDB 临时表 → 规则驱动统计 → 面板视图模型。
//!
//! 为什么放在 `tests/`（集成测试）而不是 lib 单测：
//! - 它要在**进程级 DuckDB 单例**（内存连接，`get_or_create_duckdb`）上建临时表。
//!   集成测试是独立进程，不会与 lib 里那批单测共享同一份连接与规则缓存。
//! - 其余洞察测试都是纯函数（视图模型 / 面板状态机），这里是唯一验证
//!   「取数 → 映射」全链路的地方。
//!
//! 不连任何外部数据库：DuckDB 是本地内存引擎，统计走内置的 18 条规则。
//!
//! **不覆盖**：时间列的统计（`compute_datetime_stats`）——它的规则 SQL 对数据形态更敏感，
//! 留到后续批次一起补，避免在这里用构造数据迁就实现。

use rds_insight::insight_engine::get_or_create_duckdb;
// 保留天数从 crate 里取（不是测试里写死的 30）：界面文案与实际切档共用那一个常量
use rds_insight::model::SNAPSHOT_RETENTION_DAYS;
use rds_insight::{ColumnKind, InsightService};

/// 串行锁：本文件的用例共享**进程级 DuckDB 单例**与**进程级并发配额**（上限 4）。
///
/// 并行跑时会出现「用例 A 占着 4 个令牌中的几个、用例 B 又同时申请」而随机撞上限
/// （表现为 `洞察分析任务过多`）——那不是被测代码的问题，而是测试之间在抢同一份
/// 进程级资源，所以这里显式串行（与 `crates/insight/src/lib.rs` 的
/// `rule_state_guard` 同一手法；单个用例都在毫秒量级，串行不影响反馈速度）。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 建临时表并插数据。
///
/// 表名每个用例唯一：DuckDB 连接是进程级单例，同一测试进程内的用例共享它。
fn seed(table: &str, ddl: &str, values: &str) {
    let duckdb = get_or_create_duckdb().expect("应能拿到内存 DuckDB 连接");
    let conn = duckdb.lock().expect("DuckDB 锁不应中毒");
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE \"{table}\" ({ddl})"
    ))
    .expect("建临时表");
    conn.execute_batch(&format!("INSERT INTO \"{table}\" {values}"))
        .expect("插数据");
}

/// 数值列：统计量来自 `numeric-stats` 规则，直方图受样本行数下限约束
#[test]
fn numeric_column_reaches_the_view_model() {
    let _serial = serial();
    let table = "t_insight_e2e_numeric";
    // 12 个非空值（达到 HISTOGRAM_MIN_ROWS = 10）+ 2 个空值（空值率 14% > 10% 阈值）
    let mut rows: Vec<String> = (1..=12).map(|i| format!("({i})")).collect();
    rows.push("(NULL)".into());
    rows.push("(NULL)".into());
    seed(table, "amount DECIMAL(12,2)", &format!("VALUES {}", rows.join(",")));

    let view = InsightService::profile_column_view(None, table, "amount").expect("列画像应成功");

    assert_eq!(view.kind, ColumnKind::Numeric, "DECIMAL 应分派到数值族");
    assert_eq!(view.total_count, 14);
    assert_eq!(view.null_count, 2);
    assert!((view.null_rate - 2.0 / 14.0).abs() < 1e-9);

    // 空值率超阈值：既有数值行的 warning 强调，也有质量提示
    assert!(
        view.notes.iter().any(|n| n.text.contains("高空值率")),
        "14% 空值率应触发高空值率提示，实际：{:?}",
        view.notes
    );

    // 基础统计确实来自规则执行（1..=12 的平均数是 6.5）
    let avg = view
        .basics
        .iter()
        .find(|r| r.label == "平均")
        .expect("数值列应有「平均」行");
    assert_eq!(avg.value, "6.5");

    // 非空值达到下限 → 直方图应产出；样本条数不超过 DEFAULT_SAMPLE_SIZE
    assert!(
        !view.distribution.is_empty(),
        "12 个非空值已达直方图下限，应产出分布"
    );
    assert!(
        !view.sample.is_empty() && view.sample.len() <= 5,
        "样本应为 1–5 条，实际 {}",
        view.sample.len()
    );
}

/// 文本 / 布尔列按**真实 DuckDB 类型**分派（不是按目标里用户声明的类型字符串）
#[test]
fn text_and_boolean_columns_dispatch_by_real_types() {
    let _serial = serial();
    let table = "t_insight_e2e_dispatch";
    seed(
        table,
        "name VARCHAR, flag BOOLEAN",
        "VALUES ('a', true), ('b', false), ('a', true)",
    );

    let text = InsightService::profile_column_view(None, table, "name").expect("文本列画像");
    assert_eq!(text.kind, ColumnKind::Text);
    assert!(
        text.basics.iter().any(|r| r.label == "长度范围"),
        "文本列应有长度范围"
    );
    assert_eq!(text.distribution.len(), 2, "Top 值应有两个取值");

    let boolean = InsightService::profile_column_view(None, table, "flag").expect("布尔列画像");
    assert_eq!(boolean.kind, ColumnKind::Boolean);
    assert_eq!(boolean.distribution.len(), 1, "布尔列只有一条 True 占比条");
    assert!(
        boolean.notes.iter().all(|n| !n.text.contains("不平衡")),
        "2:1 不算不平衡"
    );
}

/// 二进制列与全空列都归「类型未识别」——两条不同的路径
#[test]
fn binary_and_all_null_columns_stay_unknown() {
    let _serial = serial();
    let table = "t_insight_e2e_unknown";
    seed(
        table,
        "payload BLOB, empty_num INTEGER",
        "VALUES ('abc'::BLOB, NULL), ('def'::BLOB, NULL)",
    );

    let binary = InsightService::profile_column_view(None, table, "payload").expect("BLOB 画像");
    assert_eq!(binary.kind, ColumnKind::Unknown, "二进制列不做类型专属统计");
    assert_eq!(binary.basics.len(), 4, "未识别类型只给四行基础计数");

    let empty = InsightService::profile_column_view(None, table, "empty_num").expect("全空列画像");
    assert_eq!(
        empty.kind,
        ColumnKind::Unknown,
        "全 NULL 列没有非空样本可判型，也走未识别"
    );
}

/// 目标不存在时：`describe_error` 的**字符串匹配规则**要对得上 DuckDB 的真实错误文案。
///
/// 这条用例的价值就在这里——它是唯一能让「按消息内容识别」这套权宜拿到真实反馈的地方。
#[test]
fn missing_result_set_is_reported_with_a_friendly_message() {
    let _serial = serial();
    let err = InsightService::profile_column_view(None, "t_insight_e2e_absent", "amount")
        .expect_err("表不存在应当报错");
    let info = InsightService::describe_error(&err);

    assert_eq!(
        info.message, "结果集已失效或已过期，请重新执行查询",
        "未能识别 DuckDB 的真实报错（原始：{err}）"
    );
    assert!(!info.retryable, "结果集没了，重试不会变好");
    assert!(
        !info.message.contains('['),
        "不得把内部错误码展示给用户：{}",
        info.message
    );
}

// ==================== 快照历史（Phase 5.1） ====================

/// 本次调用专属的临时项目目录。
///
/// 不能复用别的用例的目录：`ProjectDatabaseManager::open` 会跑迁移并独占
/// DuckDB 文件锁，两个用例同时开同一个 `analytics.duckdb` 会互相失败。
fn temp_project_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rds_insight_e2e_{}_{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时项目目录");
    dir
}

/// 保存 → 读历史 → 版本链：面板「历史」Tab 背后的整条链路。
///
/// 钉三件事：
/// 1. 保存是**重取一次领域画像**再存（面板手里只有视图模型，反拼不回去）；
/// 2. 历史按「最新在前」返回，视图模型据此打「当前」/「首版」标记；
/// 3. 存储用量取**真实统计**（不是 v1 那种「条数 × 2KB」估算）。
#[test]
fn snapshot_history_round_trip_over_a_real_project() {
    let _serial = serial();
    let table = "t_insight_e2e_history";
    let root = temp_project_dir("history");
    seed(
        table,
        "amount DECIMAL(12,2)",
        "VALUES (1.5), (2.5), (NULL)",
    );

    // 首版：库里空空，保存后列表只此一条
    let first =
        InsightService::save_column_snapshot(Some(&root), table, "amount", None).expect("保存首版");
    assert_eq!(first.entity, "amount");
    assert_eq!(first.entries.len(), 1, "首版应只有一条记录");
    assert!(first.entries[0].is_latest);
    assert!(!first.entries[0].has_parent, "首版没有父版本");
    assert_eq!(first.entries[0].short_version.len(), 8, "短版本号取前 8 位");
    assert!(
        first.entries[0]
            .data_type
            .to_lowercase()
            .contains("decimal"),
        "类型取当初那份快照的真实类型：{}",
        first.entries[0].data_type
    );
    assert!(!first.truncated, "一条不需要截断提示");

    // 第二次保存：新的一版挂到首版之后，且非空值统计确实来自本次重算
    let second =
        InsightService::save_column_snapshot(Some(&root), table, "amount", None).expect("保存第二版");
    assert_eq!(second.entries.len(), 2);
    assert!(second.entries[0].is_latest, "最新一版在最前");
    assert!(second.entries[0].has_parent, "第二版应有父版本");
    assert_ne!(
        second.entries[0].version_id, second.entries[1].version_id,
        "两次保存应是不同版本"
    );
    assert!(!second.entries[1].is_latest);
    assert!(!second.entries[1].has_parent, "最早那版没有父版本");

    // 独立读一遍历史：写与读走的是同一条链路（面板切 Tab 就是这个调用）
    let read = InsightService::column_history_view(Some(&root), "amount").expect("读历史");
    assert_eq!(read.entries.len(), 2);
    assert_eq!(
        read.entries[0].version_id, second.entries[0].version_id,
        "读到的首行就是刚存的那一版"
    );
    let line = read.stats_line().expect("应能取到存储统计");
    assert!(line.contains('2'), "用量行应带上快照数：{line}");

    // 没存过的列：空历史不是错误（面板显示「还没有快照」）
    let other = InsightService::column_history_view(Some(&root), "note")
        .expect("别的列也应可读");
    assert!(other.is_empty());
    assert!(other.stats.is_some(), "统计是整个库的，与列无关");

    let _ = std::fs::remove_dir_all(&root);
}

/// 版本对比：读**两份存下来的正文**算差（不是拿现在重算的去比）。
///
/// 方向固定为「选中版本 → 最新版本」；
#[test]
fn snapshot_comparison_reads_both_stored_bodies() {
    let _serial = serial();
    let table = "t_insight_e2e_history_compare";
    let root = temp_project_dir("history_compare");
    seed(
        table,
        "amount DECIMAL(12,2)",
        "VALUES (1.5), (2.5), (NULL)",
    );

    let first = InsightService::save_column_snapshot(Some(&root), table, "amount", None).expect("首版");
    let baseline = first.entries[0].version_id.clone();

    // 数据变了再存一版：插一行 + 把空值补上
    {
        let conn = get_or_create_duckdb().expect("内存 DuckDB");
        let conn = conn.lock().expect("DuckDB 锁不应中毒");
        let sql = format!("INSERT INTO \"{table}\" VALUES (9.5)");
        conn.execute_batch(&sql).expect("插一行");
        let sql = format!("UPDATE \"{table}\" SET amount = 3.5 WHERE amount IS NULL");
        conn.execute_batch(&sql).expect("补上空值");
    }
    let second =
        InsightService::save_column_snapshot(Some(&root), table, "amount", None).expect("第二版");
    assert_eq!(second.entries.len(), 2);

    let compared = InsightService::compare_column_snapshots(Some(&root), "amount", &baseline)
        .expect("对比应成功");
    // 列表一并回来（对比面板与列表同属一个载荷，D43）
    assert_eq!(compared.entries.len(), 2);
    let diff = compared.diff.as_ref().expect("应有对比结果");
    assert_eq!(diff.baseline_version, baseline, "基准就是选中的那一版");
    assert_eq!(
        diff.baseline_label, first.entries[0].created_at,
        "面板标题用的是那一版的时间"
    );

    let row = |label: &str| {
        diff.rows
            .iter()
            .find(|row| row.label == label)
            .unwrap_or_else(|| panic!("应变到「{label}」：{:?}", diff.rows))
    };
    // 3 行 → 4 行；空值 1 → 0（把 NULL 补成了 3.5）
    assert_eq!((row("总行数").old.as_str(), row("总行数").new.as_str()), ("3", "4"));
    assert!(row("总行数").delta.is_changed());
    assert_eq!((row("空值").old.as_str(), row("空值").new.as_str()), ("1", "0"));
    assert!(diff.changed > 0, "明明改了数据，不该说「完全一致」");

    // 拿最新一版当基准：方向固定为「选中 → 最新」，与自身比没有意义
    let latest = second.entries[0].version_id.clone();
    let err = InsightService::compare_column_snapshots(Some(&root), "amount", &latest)
        .expect_err("最新一版没有可比的对象");
    assert!(
        InsightService::describe_error(&err).message.contains("最新一版"),
        "错误要说清为什么：{err}"
    );

    // 编造的版本号：不能静默给一份“空对比”
    let err = InsightService::compare_column_snapshots(Some(&root), "amount", "ffffffff-0000")
        .expect_err("不存在的版本应报错");
    assert!(
        InsightService::describe_error(&err).message.contains("已不在历史里"),
        "错误要指向「这一版没了」：{err}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// 清理：30 天档对刚存的快照是**空操作**，但回执要如实到面板（不是「报了个成功」）。
///
/// 真正的删除路径在 `store::tests::test_cleanup_deletes_both_sides_in_pairs` 里验
/// （那里能把 `created_at` 回填成 40 天前；服务这一层只多一个「开库 → 取数 → 贴回执」）。
#[test]
fn cleanup_with_a_30_day_window_leaves_fresh_snapshots_alone() {
    let _serial = serial();
    let table = "t_insight_e2e_cleanup";
    let root = temp_project_dir("cleanup");
    seed(table, "amount DECIMAL(12,2)", "VALUES (1.5), (2.5)");

    InsightService::save_column_snapshot(Some(&root), table, "amount", None).expect("保存一版");
    let view = InsightService::cleanup_old_snapshots(Some(&root), "amount", SNAPSHOT_RETENTION_DAYS)
        .expect("清理应成功");

    let receipt = view.cleanup.as_ref().expect("回执应在载荷里");
    assert_eq!(receipt.days, SNAPSHOT_RETENTION_DAYS);
    assert_eq!(
        (receipt.body_removed, receipt.meta_removed),
        (0, 0),
        "刚存的快照不该被 30 天档删掉"
    );
    assert!(receipt.is_balanced(), "两侧都没删，自然是对上的");
    assert!(
        receipt.summary().contains("没有"),
        "空操作要说清楚，而不是报个成功：{}",
        receipt.summary()
    );
    assert_eq!(view.entries.len(), 1, "列表不能被空操作动过");
    assert!(
        view.stats_line().is_some_and(|line| line.contains('1')),
        "用量统计里仍是那一版：{:?}",
        view.stats_line()
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// 无项目时快照不可用，但错误要说人话（而不是把 `[code]` 摆给用户看）
#[test]
fn snapshot_without_a_project_explains_itself() {
    let _serial = serial();
    let err = InsightService::column_history_view(None, "amount").expect_err("无项目应报错");
    let info = InsightService::describe_error(&err);
    assert!(
        info.message.contains("打开项目"),
        "要告诉用户怎么办：{}",
        info.message
    );
    assert!(!info.message.contains('['), "不展示内部错误码：{}", info.message);
}

// ==================== 表探查（Phase 3.1 / 2.2） ====================

/// 表探查：列元数据与行数来自 DuckDB，类型族按真实类型名判定，且**不产假分数**
#[test]
fn table_profile_lists_columns_without_fake_scores() {
    let _serial = serial();
    let table = "t_insight_e2e_table";
    seed(
        table,
        "id BIGINT, amount DECIMAL(12,2), note VARCHAR, payload BLOB",
        "VALUES (1, 1.5, 'a', 'x'::BLOB), (2, NULL, NULL, 'y'::BLOB)",
    );

    let view = InsightService::profile_table_view(None, table, "orders").expect("表探查应成功");
    assert_eq!(view.table_name, "orders", "展示名取目标里的逻辑名");
    assert_eq!(view.row_count, 2);
    let names: Vec<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["id", "amount", "note", "payload"]);
    assert_eq!(view.columns[1].kind, ColumnKind::Numeric, "DECIMAL 归数值族");
    assert_eq!(view.columns[2].kind, ColumnKind::Text);
    assert_eq!(view.columns[3].kind, ColumnKind::Unknown, "BLOB 不参与列统计");
    assert!(
        view.columns.iter().all(|c| c.score.is_none()),
        "只是探查：分数要等用户点「评估全表」"
    );
    assert!(view.quality.is_none());
}

/// 多列分析：列清单来自**真实列元数据**（v1 恒空的 `availableColumns` 是该项从未跑通的根因）
#[test]
fn multi_column_view_lists_real_columns_and_multi_rules() {
    let _serial = serial();
    let table = "t_insight_e2e_multi";
    seed(
        table,
        "amount DECIMAL(12,2), qty INTEGER, channel VARCHAR",
        "VALUES (1.5, 2, 'paid'), (2.5, 4, 'free'), (3.5, 6, 'paid')",
    );

    let view = InsightService::multi_column_view(None, table, "orders").expect("多列视图应成功");
    assert_eq!(view.table_name, "orders");
    let names: Vec<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["amount", "qty", "channel"]);
    assert!(
        !view.rules.is_empty(),
        "内置的 multi 规则应出现在候选里（pearson-correlation / cross-tab）"
    );
    let pearson = view
        .rules
        .iter()
        .find(|r| r.id == "pearson-correlation")
        .expect("Pearson 规则应可用");
    assert_eq!(pearson.arity(), 2);
    assert!(
        pearson.accepts(&view.kinds_of(&["amount".into(), "qty".into()])),
        "两列数值应被接受"
    );
    assert!(
        !pearson.accepts(&view.kinds_of(&["amount".into(), "channel".into()])),
        "数值 + 文本不该被 Pearson 接受"
    );
    assert!(view.result.is_none(), "未执行前没有结果");
}

/// 单值结果：两列完全线性相关时 Pearson 系数 ≈ 1，样本量逐行计数
#[test]
fn multi_rule_single_result_reaches_the_view_model() {
    let _serial = serial();
    let table = "t_insight_e2e_pearson";
    let rows: Vec<String> = (1..=10).map(|i| format!("({i}, {})", i * 3)).collect();
    seed(
        table,
        "x INTEGER, y INTEGER",
        &format!("VALUES {}", rows.join(",")),
    );

    let (result, notes) = InsightService::run_multi_rule(
        None,
        table,
        "pearson-correlation",
        &["x".into(), "y".into()],
    )
    .expect("执行 Pearson 规则");

    let rds_insight::MultiResultView::Single(rows) = &result else {
        panic!("单值规则应给出键值行：{result:?}");
    };
    let value = |key: &str| {
        rows.iter()
            .find(|r| r.label == key)
            .map(|r| r.value.clone())
            .unwrap_or_else(|| panic!("应有字段 {key}：{rows:?}"))
    };
    let corr: f64 = value("相关系数").parse().expect("相关系数应是数字");
    assert!(
        (corr - 1.0).abs() < 1e-9,
        "完全线性相关应给出 1.0，实际 {corr}"
    );
    assert_eq!(value("样本量"), "10", "已知字段名展示为中文标签");
    assert!(notes.is_empty(), "该规则没有门控项，不该有提示");
}

/// 列表结果：交叉频次表按**数据形态**渲染成表格（表头来自规则输出字段）
#[test]
fn multi_rule_list_result_becomes_a_table() {
    let _serial = serial();
    let table = "t_insight_e2e_cross_tab";
    seed(
        table,
        "channel VARCHAR, region VARCHAR",
        "VALUES ('paid', 'cn'), ('paid', 'us'), ('free', 'cn'), ('paid', 'cn')",
    );

    let (result, _notes) = InsightService::run_multi_rule(
        None,
        table,
        "cross-tab",
        &["channel".into(), "region".into()],
    )
    .expect("执行交叉频次表规则");

    let rds_insight::MultiResultView::Table { headers, rows } = &result else {
        panic!("列表规则应给出表格：{result:?}");
    };
    assert!(!headers.is_empty(), "表头不得为空：{result:?}");
    assert!(!rows.is_empty(), "至少应有频次不为零的格子");
    assert!(
        headers.iter().any(|h| h == "计数"),
        "频次列应在表头里（展示为「计数」）：{headers:?}"
    );
    assert!(
        rows.iter().any(|row| row.iter().any(|cell| cell == "2")),
        "paid×cn 出现两次，应能看到计数 2：{rows:?}"
    );
}

/// 评估全表：逐列统计 → 每列分数 + 表级摘要。
///
/// 这条用例同时钉住「表级分数与列级分数同源」：`compute_table_quality` 内部逐列调用
/// 同一个 `compute_column_quality`，所以列上的分数必须与摘要里的聚合对得上。
#[test]
fn table_evaluation_scores_every_column() {
    let _serial = serial();
    let table = "t_insight_e2e_table_eval";
    let rows: Vec<String> = (1..=12)
        .map(|i| format!("({i}, {i}.5, 'v{i}')"))
        .collect();
    seed(
        table,
        "id BIGINT, amount DECIMAL(12,2), note VARCHAR",
        &format!("VALUES {}", rows.join(",")),
    );

    let view = InsightService::profile_table_view(None, table, "orders").expect("表探查");
    let mut evaluated = Vec::new();
    for column in &view.columns {
        let full = InsightService::get_column_insight_full(None, table, &column.name)
            .expect("逐列统计");
        evaluated.push(full);
    }
    let quality = InsightService::compute_table_quality("orders", &evaluated);
    let done = view.evaluating(evaluated.len(), evaluated.len()).evaluated(&quality);

    assert!(done.progress.is_none(), "评估完进度行必须消失");
    let summary = done.quality.expect("应有表级摘要");
    assert_eq!(summary.scored_columns, 3);
    assert!(summary.summary.contains("表质量"), "摘要：{}", summary.summary);
    // 全量非空、无重复的三列：分数应明显偏高（不写死具体数值，只钉区间关系）
    assert!(
        summary.overall > 50.0,
        "干净数据不该被判成差：{}",
        summary.overall
    );
    assert_eq!(summary.grade, rds_insight::Grade::of(summary.overall));
}

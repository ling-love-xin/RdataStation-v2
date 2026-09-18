//! 编辑器结果集 → 洞察面板的**宿主接线**（M8 §10 #6）。
//!
//! 链路：结果集列头右键「洞察此列」→ `ResultGridDelegate` 的钩子 → `EditorHostPanel::insight_column`
//! → `EditorShared` 上宿主注入的端口（[`workbench::services::editor_insight::attach`]）
//! → `Shared::open_insight_source_column`（展开右 Dock + 递 `InsightTarget::SourceColumn`）。
//!
//! 本用例从**端口**那一环往下验（面板取当前结果集那一环由 editor 侧用例覆盖）：
//! 装了真 `InsightView` 弱句柄，调端口，断言右 Dock 展开 + 面板目标确实指向「源列 + 源取样」。
//! **不物化结果集**：端口送出去的是「连接 + 那段 SQL + 标签」，取样由洞察侧重跑（带 LIMIT）。

use gpui_kit::{AppContext as _, TestAppContext};

use editor::shared::{EditorShared, InsightColumnRequest};
use rds_workbench::panels::Shared;
use workbench_shell::model::{RightPanel, SidebarMode};

#[gpui_kit::test]
fn the_editor_column_port_points_the_insight_panel_at_the_source(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = Shared::with_connections(Vec::new(), None);
    let editor_shared = EditorShared::new();
    // 未接端口时入口不该出现（菜单项的判据就是“端口在不在”）
    assert!(
        editor_shared.insight_column_port().is_none(),
        "新编辑器实例还没接宿主端口"
    );
    rds_workbench::services::editor_insight::attach(&editor_shared, &shared);

    // 面板实体：生产由 `RightSidebarPanel` 构造期登记弱句柄，这里补上
    let view = cx.update(|cx| cx.new(insight::InsightView::new));
    *shared.insight_panel.borrow_mut() = Some(view.downgrade());

    let port = editor_shared
        .insight_column_port()
        .expect("attach 之后应有端口");
    cx.update(|cx| {
        port(
            InsightColumnRequest {
                column: "amount".to_string(),
                sql: "SELECT id, amount FROM orders WHERE status = 1".to_string(),
                connection: "G_1".to_string(),
                label: "结果 2".to_string(),
            },
            cx,
        );
    });

    assert_eq!(
        shared.right_mode.get(),
        SidebarMode::Expanded,
        "入口要展开右 Dock，否则用户看不到结果"
    );
    assert_eq!(shared.active_right.get(), RightPanel::Insight);

    cx.update(|cx| {
        let target = view.read(cx).target().expect("面板应已被指向源列");
        assert_eq!(target.title(), "amount", "目标标题是列名");
        let source = target.source().expect("源列目标要带源取样信息");
        assert_eq!(source.conn_id(), Some("G_1"), "取样走产生结果的那条连接");
        assert_eq!(source.sql, "SELECT id, amount FROM orders WHERE status = 1");
        assert_eq!(source.label, "结果 2", "来源标签给面板副标题 / 快照用");
        // 类型此刻还不知道（编辑器只有字符串化的行）——目标头不该编一个类型出来
        assert!(
            !target.detail().unwrap_or_default().contains(" · "),
            "类型未知时目标头只显示来源：{:?}",
            target.detail()
        );
    });
}

// ==================== 真机：未绑定连接 → 活动连接 → 取样 ====================

/// 走真执行器 + 真库，把「编辑器入口」整条链验证一遍：
///
/// 不绑定连接执行（即 B1 的「跟随当前活动连接」）→ 活动连接由**执行器**解析
/// （新加的 `QueryRunner::active_connection`，与 `resolve_conn_id` 同一处口径）→
/// 端口 → 洞察面板目标 → 洞察侧拿那段 SQL **重跑取样**（带 LIMIT）→ 列画像出真数。
///
/// 未设 `RDS_TEST_MYSQL_URL` 自动跳过；在真库建/删 `rds_probe_editor_insight`。
#[gpui_kit::test]
fn the_editor_entry_re_samples_on_the_active_connection(cx: &mut TestAppContext) {
    use engine::services::sql_service::{SqlExecuteOptions, SqlService};
    use engine::{AutoDriverRegistrar, DriverConnectionConfig};

    cx.update(gpui_kit::init);
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    AutoDriverRegistrar::register_builtin_drivers();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let manager = engine::connection_manager::get_connection_manager().clone();

    let opts = || SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: Some(15000),
        use_cache: false,
    };
    let run = |conn_id: &str, sql: String| {
        let service = SqlService::new(manager.clone());
        runtime.block_on(async move {
            service
                .execute(Some(conn_id.to_string()), &sql, opts())
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    };

    // 建连并设为活动连接（未绑定文档的执行就落到它上面）
    let conn_id = runtime
        .block_on(async {
            let mut config = DriverConnectionConfig::new("mysql");
            config.name = Some("M8 编辑器入口探针".to_string());
            config.url_override = Some(url.clone());
            let (id, _db) = manager.create_connection_with_registry(config).await?;
            manager.set_active_connection(id.clone()).await;
            Ok::<String, shared::error::CoreError>(id)
        })
        .expect("建连");

    const TABLE: &str = "rds_probe_editor_insight";
    let _ = run(&conn_id, format!("DROP TABLE IF EXISTS {TABLE}"));
    run(
        &conn_id,
        format!("CREATE TABLE {TABLE} (id INT, amount INT, note VARCHAR(20))"),
    )
    .expect("建探针表");
    run(
        &conn_id,
        format!("INSERT INTO {TABLE} VALUES (1, 10, 'a'), (2, 20, 'b'), (3, 30, 'c')"),
    )
    .expect("插数据");

    // 编辑器：不绑定连接 + 真执行器 + 洞察端口
    let shared = Shared::with_connections(Vec::new(), None);
    let editor_shared = EditorShared::new();
    rds_workbench::services::editor_exec::attach(&editor_shared);
    rds_workbench::services::editor_insight::attach(&editor_shared, &shared);
    let view = cx.update(|cx| cx.new(insight::InsightView::new));
    *shared.insight_panel.borrow_mut() = Some(view.downgrade());

    let sql = format!("SELECT id, amount, note FROM {TABLE}");
    let document = editor_shared
        .open(editor::service::OpenRequest::untitled(
            sql.clone(),
            editor::model::EditorMode::Sql,
        ))
        .id()
        .clone();
    editor_shared
        .submit(
            document.clone(),
            &editor::execution::ExecTarget::All(sql.clone()),
            editor::execution::ResultPlacement::Replace,
            editor::execution::RunOptions::default(),
        )
        .expect("提交执行");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let entry = loop {
        // 生产里由面板的轮询泵做（`drain_exec` → 落 `ResultStore`），测试手动照做
        for outcome in editor_shared.drain_exec() {
            let connection = outcome.connection.clone();
            let channel = outcome.channel;
            let entry = match outcome.result {
                Ok(data) => editor::store::ResultEntry::success(
                    outcome.document,
                    outcome.sql,
                    data.elapsed_ms,
                    data.truncated,
                    data.columns,
                    data.rows,
                )
                .with_affected_rows(data.affected_rows)
                .with_has_more(data.has_more)
                .with_channel(channel)
                .with_connection(connection),
                Err(error) => editor::store::ResultEntry::failure(
                    outcome.document,
                    outcome.sql,
                    error,
                    0,
                )
                .with_channel(channel),
            };
            editor_shared.update_results(|store| store.push(entry, outcome.placement));
        }
        if let Some(entry) = editor_shared
            .results_active(&document)
            .filter(|entry| entry.row_count() == 3)
        {
            break entry;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "没等到 3 行结果（编辑器执行链）"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert!(
        entry.connection.is_none(),
        "这份文档没绑定连接——正是“跟随活动连接”那一档"
    );
    assert!(entry.can_insight_column(), "只读查询 + 有列：条目本身可洞察");
    assert_eq!(
        editor_shared.active_connection().as_deref(),
        Some(conn_id.as_str()),
        "执行器要报出未绑定时执行会落到的那条连接"
    );

    // 端口 → 面板目标（输入用的就是执行器解析出的连接）
    let port = editor_shared.insight_column_port().expect("端口已注入");
    let active = editor_shared.active_connection().expect("活动连接");
    cx.update(|cx| {
        port(
            InsightColumnRequest {
                column: "amount".to_string(),
                sql: entry.sql.clone(),
                connection: active,
                label: "结果 1".to_string(),
            },
            cx,
        );
    });

    // 洞察侧：拿那段 SQL **重跑取样**（带 LIMIT）→ 列画像出真数
    let source = cx.update(|cx| {
        view.read(cx)
            .target()
            .and_then(|target| target.source().cloned())
            .expect("面板目标应带源取样")
    });
    let (temp_table, profile) =
        insight::InsightService::profile_source_column(None, &source, "amount").expect("源取样 + 列画像");
    eprintln!(
        "✅ 编辑器入口：样本表 {temp_table} · amount {} 行 / {} 空值 / {:?}",
        profile.total_count, profile.null_count, profile.kind
    );
    assert_eq!(profile.total_count, 3, "取样应取回真库那 3 行");
    assert!(
        temp_table.starts_with("tmp_i_"),
        "样本表要带洞察前缀：{temp_table}"
    );

    let _ = run(&conn_id, format!("DROP TABLE IF EXISTS {TABLE}"));
}

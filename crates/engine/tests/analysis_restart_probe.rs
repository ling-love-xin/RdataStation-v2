//! 重启后「意图清单 replay」的 headless 探针（K3 收口的验收，2026-11-27）
//!
//! 不起 GUI、不连网络：只用一个本地 `.duckdb` 源文件 + 一个临时项目根（`.RSmeta/project.db`）。
//!
//! 两阶段模拟重启——进程退出时真正丢掉的是**进程级快照**，所以阶段一结束用
//! `clear_project()` 把它清掉（等同 `PROJECT_ROOT / ALIASES_BY_CONN / PRIMARY_ALIAS` 随进程消失），
//! 阶段二只凭项目库 `publish_project(root)` 重建（这是 `view::lifecycle::new` 重启后唯一的动作），
//! 再按登记行重建挂载，断言三件事回到原位：
//!
//! 1. 登记行从库读回来（别名清单）；
//! 2. 主源从 `analysis_primary` replay（不靠会话内存）；
//! 3. catalog 事实面来自 DuckDB 自己（`list_tables_of_alias` 有表、未限定名查得到）。
//!
//! 跑的姿势：`cargo test -p rds-engine --test analysis_restart_probe -- --nocapture`

use std::time::Duration;

use duckdb::Connection;
use rds_engine::duckdb::federation::registry::FederatedSource;
use rds_engine::duckdb::instance::AnalysisInstance;
use rds_engine::persistence::analysis_sources::{
    self, AnalysisMountKind, AnalysisSourceKind, AnalysisSourceRow,
};

const ALIAS: &str = "shop";

fn temp_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("rds_restart_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("建临时项目根");
    root
}

 #[test]
fn a_registered_source_and_its_primary_survive_a_restart() {
    let root = temp_root("k3");
    let source_path = root.join("src.duckdb");
    Connection::open(&source_path)
        .expect("建源文件")
        .execute_batch("CREATE TABLE orders AS SELECT * FROM (VALUES (1, 10), (2, 20)) t(id, amount)")
        .expect("建源表");

    // ── 进程 1：登记 + 设主源 + 发布 ────────────────────────────────
    let conn = analysis_sources::open_in_project(&root).expect("项目库");
    analysis_sources::insert(
        &conn,
        &AnalysisSourceRow {
            id: "f1".into(),
            name: "shop.duckdb".into(),
            kind: AnalysisSourceKind::DbFile,
            connection_id: None,
            path: Some(source_path.to_string_lossy().into_owned()),
            alias: ALIAS.into(),
            mount_kind: AnalysisMountKind::Federation,
            created_at: String::new(),
            updated_at: String::new(),
        },
    )
    .expect("登记");
    analysis_sources::set_primary(&conn, ALIAS).expect("设主源");
    let rows = analysis_sources::publish_project(&root).expect("发布");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        analysis_sources::primary_alias_cached().as_deref(),
        Some(ALIAS)
    );
    drop(conn);

    // ── 进程退出：进程级快照全清（重启丢掉的就是这些）──────────────
    analysis_sources::clear_project();
    assert!(
        analysis_sources::primary_alias_cached().is_none(),
        "清空后快照不该残留（否则这条探针证明不了 replay）"
    );
    assert!(
        analysis_sources::current_project_root().is_none(),
        "清空后项目根也不该残留（否则执行侧会拿旧项目开会话）"
    );

    // ── 进程 2：只凭项目库重建 ──────────────────────────────────────
    let rows = analysis_sources::publish_project(&root).expect("重启后发布");
    assert_eq!(rows.len(), 1, "登记行要从库读回来");
    assert_eq!(
        analysis_sources::primary_alias_cached().as_deref(),
        Some(ALIAS),
        "主源要从 analysis_primary replay，不能靠会话内存"
    );

    // ── 按登记行重建挂载（等价于文档打开 → 组装 plan → 挂载）────────
    let instance =
        AnalysisInstance::open_tagged(&format!("probe_{}", std::process::id()), Duration::from_secs(5))
            .expect("开实例");
    let session = instance
        .open_session("doc-probe", "编辑器 · 探针.sql")
        .expect("开会话");
    let source = FederatedSource::new(
        rows[0]
            .connection_id
            .clone()
            .unwrap_or_else(|| format!("file:{}", rows[0].id))
            .as_str(),
        &rows[0].alias,
        "duckdb",
        rows[0].path.as_deref().expect("登记行该有 path"),
    )
    .expect("造源");
    session.mount_sources(&[source]).expect("挂源");
    session.use_catalog(&rows[0].alias).expect("设主源");

    assert_eq!(session.primary_alias().as_deref(), Some(ALIAS));
    assert_eq!(
        instance.list_tables_of_alias(ALIAS).expect("查表清单"),
        vec!["orders".to_string()],
        "catalog 事实面要能重建：表清单来自 DuckDB 自己"
    );
    let sum = session
        .run("SELECT sum(amount) FROM orders")
        .expect("未限定名查询")
        .to_rows()
        .first()
        .and_then(|row| row.first())
        .map(|value| value.to_string())
        .expect("有结果");
    assert_eq!(sum, "30", "源表要查得到（search_path 指向它）");

    assert_eq!(
        instance.catalog_holders(ALIAS, &[]),
        vec!["编辑器 · 探针.sql".to_string()],
        "引用计数读口要能点名持有者"
    );

    instance.close_all_sessions();
    let _ = std::fs::remove_dir_all(&root);
}

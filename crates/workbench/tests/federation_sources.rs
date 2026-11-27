//! 联邦源的组装与跨源执行（真机）
//!
//! 验的是「L1 收尾」后的口径：**项目登记过的连接都可以当联邦源**
//!（源是 DuckDB 自己 `ATTACH` 的），不要求应用先建连——这也正是没有原生驱动的库
//! （Oracle 这类）能参与的前提（它单独在 `oracle_federation.rs` 里验，两个文件各自一份临时库）。
//! 2026-11-27 起「能不能当源」只看登记（`analysis_sources`），连接记录上已没有开关字段。
//!
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! cargo test -p rds-workbench -j 2 --test federation_sources -- --nocapture --test-threads=1
//! ```
//!
//! **sh / bash 下一律加单引号**（反斜杠会被吃掉，见 `editor_exec_real.rs` 的同一警告）。
//!
//! 单例按进程只可设置一次：本文件启动时注入临时全局库（与其余测试二进制隔离）。

use std::path::PathBuf;
use std::sync::OnceLock;

use connection::model::{ConnectionScope, DataSourceSaveInput};
use editor::channel::ExecChannel;
use editor::execution::RunOptions;
use engine::persistence::global_db::GlobalDatabaseManager;
use rds_workbench::services::data_source_service::DataSourceService;
use rds_workbench::services::editor_exec;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 注入临时全局库到应用单例（进程内一次）——`DataSourceService::global()` 因此能用临时库，
/// 执行器组装源清单时读的就是它。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_fed_src_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let manager = rt
                .block_on(GlobalDatabaseManager::new(
                    dir.join("global.db"),
                    dir.join("analytics.duckdb"),
                    2,
                ))
                .expect("init global db");
            engine::migration::install_global_db_manager(manager).expect("inject singleton");
            std::mem::forget(rt);
            dir
        })
        .clone()
}

/// 建一条连接记录（**当源与否由项目登记决定**，2026-11-27 起连接上不再有开关字段）
fn marked(name: &str, db_type: &str, url: &str) -> DataSourceSaveInput {
    let mut input = DataSourceSaveInput::new(name, db_type, url);
    input.scope = ConnectionScope::Global;
    input
}

/// 把两条连接登记进临时项目（写 `analysis_sources` 并发布进程级别名快照）。
fn register_sources(root: &std::path::Path, ids: &[(&str, &str)]) {
    for (conn_id, name) in ids {
        engine::analysis::registration::ensure_conn_registered(root, conn_id, name)
            .expect("登记为项目源");
    }
    engine::persistence::analysis_sources::publish_project(root).expect("发布别名快照");
}

#[test]
fn marked_connections_become_federated_sources() {
    let (Ok(mysql_url), Ok(sqlite_path)) = (
        std::env::var("RDS_TEST_MYSQL_URL"),
        std::env::var("RDS_TEST_SQLITE_PATH"),
    ) else {
        eprintln!("⏭️ 未设 RDS_TEST_MYSQL_URL / RDS_TEST_SQLITE_PATH，跳过联邦源组装探针");
        return;
    };
    let _ = base_dir();
    engine::driver::AutoDriverRegistrar::register_builtin_drivers();
    let service = DataSourceService::global().expect("全局服务");
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    // 两条**标记过的**连接（注意：全程没有用应用驱动去连它们——源是 DuckDB 自己挂的）
    // 驱动 id 用应用里的叫法（`mysql_native`）：连接记录里的 `driver` 就是它，
    // 源组装时要把它归一成扫描器认的 `mysql://`（见 `accel::normalize_scheme`）
    let mysql_id = rt
        .block_on(service.save(
            &marked(
                "mysql_src",
                "mysql_native",
                &mysql_url.replace("mysql://", "mysql_native://"),
            ),
            None,
        ))
        .expect("保存 MySQL 连接");
    let sqlite_id = rt
        .block_on(service.save(
            &marked(
                "sqlite_src",
                "sqlite",
                &format!("sqlite://{sqlite_path}"),
            ),
            None,
        ))
        .expect("保存 SQLite 连接");
    // 当源是**项目意图**：两条都登记进临时项目（连接上已没有开关字段可写）。
    register_sources(
        &base_dir(),
        &[(&mysql_id, "mysql_src"), (&sqlite_id, "sqlite_src")],
    );
    assert!(
        !rt.block_on(
            engine::connection_manager::get_connection_manager().has_connection(&mysql_id)
        ),
        "本用例刻意不建原生连接：源应该由 DuckDB 直接挂"
    );

    let runner = editor_exec::runner_for_test().expect("建执行器");
    let sql = "SELECT count(*) AS n FROM mysql_src.mysql.user u \
               JOIN sqlite_src.main.blob b ON 1 = 1";
    let data = runner
        .run(
            "federation_sources",
            Some(&mysql_id),
            None,
            ExecChannel::Federated,
            sql,
            RunOptions {
                use_transaction: false,
                lineage: None,
                // 探针只跑 SELECT：不是「写确认」的对象（老调用方口径）
                dml_confirmed: false,
            },
        )
        .expect("跨源查询该成功");

    assert_eq!(data.rows.len(), 1, "聚合该回一行：{:?}", data.rows);
    let count: i64 = data.rows[0][0].parse().expect("计数是数字");
    assert!(
        count > 0,
        "MySQL（主源）与 SQLite 两个源都该有数据：{:?}",
        data.rows
    );
    let notice = data.notice.clone().unwrap_or_default();
    assert!(
        notice.contains("mysql_src") && notice.contains("sqlite_src"),
        "结果区小字要说清挂了哪些源：{notice}"
    );
    eprintln!("✅ 跨源（不需要应用先建连）：{count} 行 · {notice}");

    // 【T1.6】换主源：引擎侧的真值要跟着换（未限定名的解析者）
    //
    // 注：DuckDB 里“未限定名”是按**主源 catalog + 默认 schema** 解析的；SQLite / MySQL 源的
    // schema 不是默认那个，所以跨源查询要写全限定名（结果区那行小字一直在这么说）。
    let switch = runner
        .set_federated_primary("federation_sources", Some(&mysql_id), "sqlite_src")
        .expect("换主源该成功");
    assert!(switch.contains("sqlite_src"), "{switch}");
    let snapshot =
        engine::duckdb::federation::session::snapshot_for(&mysql_id).expect("会话该还在");
    assert_eq!(
        snapshot.primary.as_deref(),
        Some("sqlite_src"),
        "{snapshot:?}"
    );
    // 换完之后跨源查询照常（全限定名不受主源影响）
    let after = runner
        .run(
            "federation_sources",
            Some(&mysql_id),
            None,
            ExecChannel::Federated,
            "SELECT count(*) AS n FROM sqlite_src.main.blob",
            RunOptions {
                use_transaction: false,
                lineage: None,
                dml_confirmed: false,
            },
        )
        .expect("换主源后该照常能查");
    assert_eq!(after.rows.len(), 1, "{:?}", after.rows);
    eprintln!(
        "✅ 换主源：{switch} · 全限定名照常（blob → {} 行）",
        after.rows[0][0]
    );

    // 【T1.6】重挂：单源（源清单里的行级动作）与全挂（菜单里的那项）
    let one = runner
        .refresh_sources("federation_sources", Some(&mysql_id), ExecChannel::Federated, Some("mysql_src"))
        .expect("重挂单个源");
    assert!(one.contains("mysql_src"), "{one}");
    let all = runner
        .refresh_sources("federation_sources", Some(&mysql_id), ExecChannel::Federated, None)
        .expect("全挂");
    assert!(all.contains("2 个源"), "{all}");
    eprintln!("✅ 重挂：{one} · {all}");

    // 【K4c】撤源 = 删登记行（连接上已没有开关可翻）：源清单从 2 条回到 1 条
    //（只剩本文档连接自己）；单源照样联邦（2026-11-25 用户口径）。
    let root = base_dir();
    {
        let conn =
            engine::persistence::analysis_sources::open_in_project(&root).expect("打开项目库");
        let rows = engine::persistence::analysis_sources::list(&conn).expect("读登记");
        let sqlite_row = rows
            .iter()
            .find(|row| row.connection_id.as_deref() == Some(sqlite_id.as_str()))
            .expect("sqlite 应已登记")
            .id
            .clone();
        engine::persistence::analysis_sources::delete(&conn, &sqlite_row).expect("撤 sqlite 登记");
    }
    engine::persistence::analysis_sources::publish_project(&root).expect("重发别名快照");

    let single = runner
        .run(
            "federation_sources",
            Some(&mysql_id),
            None,
            ExecChannel::Federated,
            "SELECT count(*) AS n FROM mysql_src.mysql.user u",
            RunOptions {
                use_transaction: false,
                lineage: None,
                dml_confirmed: false,
            },
        )
        .expect("单源联邦该照跑");
    assert_eq!(single.rows.len(), 1, "{:?}", single.rows);
    eprintln!("✅ 单源联邦照跑：{} 行", single.rows[0][0]);
}

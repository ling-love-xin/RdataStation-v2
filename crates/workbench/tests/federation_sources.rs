//! 联邦源的组装与跨源执行（真机）
//!
//! 验的是「L1 收尾」后的口径：**连接记录里开了「DuckDB 直连」的都可以当联邦源**，
//! 不要求应用先建连（源是 DuckDB 自己 `ATTACH` 的）——这也正是没有原生驱动的库
//! （Oracle 这类）能参与的前提。
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

/// 建一条**标记为联邦源**的连接记录
fn marked(name: &str, db_type: &str, url: &str, fed: bool) -> DataSourceSaveInput {
    let mut input = DataSourceSaveInput::new(name, db_type, url);
    input.scope = ConnectionScope::Global;
    input.use_duckdb_fed = Some(fed);
    input
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
                true,
            ),
            None,
        ))
        .expect("保存 MySQL 连接");
    let sqlite_id = rt
        .block_on(service.save(
            &marked("sqlite_src", "sqlite", &format!("sqlite://{sqlite_path}"), true),
            None,
        ))
        .expect("保存 SQLite 连接");
    assert!(
        !rt.block_on(engine::connection_manager::get_connection_manager().has_connection(&mysql_id)),
        "本用例刻意不建原生连接：源应该由 DuckDB 直接挂"
    );

    let runner = editor_exec::runner_for_test().expect("建执行器");
    let sql = "SELECT count(*) AS n FROM mysql_src.mysql.user u \
               JOIN sqlite_src.main.blob b ON 1 = 1";
    let data = runner
        .run(
            Some(&mysql_id),
            ExecChannel::Federated,
            sql,
            RunOptions {
                use_transaction: false,
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

    // 撤掉 SQLite 的标记 → 只剩一个源：入口就拒，且说清还差什么
    rt.block_on(service.update(
        &sqlite_id,
        &marked("sqlite_src", "sqlite", &format!("sqlite://{sqlite_path}"), false),
        None,
    ))
    .expect("撤标记");
    let error = runner
        .run(
            Some(&mysql_id),
            ExecChannel::Federated,
            "SELECT 1",
            RunOptions {
                use_transaction: false,
            },
        )
        .expect_err("只剩一个源该拒");
    assert!(error.contains("至少需要两个源"), "{error}");
    eprintln!("✅ 撤掉标记后如实拒绝：{error}");

    let _ = sqlite_id;
}

//! 全局写守卫的真机探针：user / process 会话只读 `rds_global`，system 会话可写。
//!
//! 与 `analysis_global_probe` 的区别：那边验“根连接把全局库挂进实例”，这里验
//! **闸门真的拦在 `AnalysisSession::run_with_timeout` 入口**，且内部 `USE rds_global`
//! 不会被自己的闸门挡住（全局读取会话仍能取样）。

use std::path::PathBuf;
use std::time::Duration;

use rds_engine::duckdb::federation::session::{set_bases, BaseMount};
use rds_engine::duckdb::instance::{AnalysisInstance, SessionRole};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rds-guard-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn user_and_process_cannot_write_global_but_system_can() {
    let dir = temp_dir("write");
    let project = dir.join("project-analytics.duckdb");
    let global = dir.join("global-analytics.duckdb");

    // 两个底座都先建成合法 DuckDB 文件；全局库先放一张种子表。
    for path in [&project, &global] {
        let conn = duckdb::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE seed (id INTEGER)")
            .unwrap();
        conn.execute_batch("INSERT INTO seed VALUES (1)").unwrap();
    }

    set_bases(vec![
        BaseMount {
            alias: "analytics".to_string(),
            path: project.to_string_lossy().to_string(),
        },
        BaseMount {
            alias: "rds_global".to_string(),
            path: global.to_string_lossy().to_string(),
        },
    ]);
    let instance =
        AnalysisInstance::open_tagged("guard-probe", Duration::from_millis(10_000)).unwrap();

    // 用户会话：读全局放行，写全局 / 切全局 catalog / ATTACH 全拒。
    let user = instance.ensure_session("doc:guard", "editor").unwrap();
    assert_eq!(user.role(), SessionRole::User);
    let read = user.run("SELECT * FROM rds_global.seed").unwrap();
    assert_eq!(read.total_rows, 1);
    let denied = user
        .run("INSERT INTO rds_global.seed VALUES (2)")
        .unwrap_err();
    assert!(
        denied.to_string().contains("全局分析库 rds_global"),
        "错误里要能看出是全局写守卫拦的：{denied}"
    );
    assert!(user.run("CREATE TABLE rds_global.bad AS SELECT 1").is_err());
    assert!(user.run("USE rds_global").is_err());
    assert!(user.run("ATTACH 'other.duckdb' AS other").is_err());
    // 临时表照常可写（闸门只拦全局底座）。
    assert!(user.run("CREATE TEMP TABLE tmp_guard AS SELECT 1").is_ok());

    // 进程面同样只读全局。
    let process = instance.process_session().unwrap();
    assert_eq!(process.role(), SessionRole::Process);
    assert!(process.run("DELETE FROM rds_global.seed").is_err());

    // 系统会话是唯一写通道。
    let system = instance.ensure_system_session("system:guard").unwrap();
    assert_eq!(system.role(), SessionRole::System);
    system
        .run("INSERT INTO rds_global.seed VALUES (2)")
        .expect("系统会话必须能写全局");
    system
        .run("CREATE TABLE rds_global.system_created AS SELECT 1 AS v")
        .expect("系统会话必须能建全局表");

    // 全局读取会话（内部 `use_catalog`）不能被闸门挡住：直接用 rds_global 默认 catalog 取样。
    let global_reader = instance
        .ensure_session("global:guard", "federated")
        .unwrap();
    global_reader.use_catalog("rds_global").unwrap();
    assert_eq!(global_reader.default_catalog(), "rds_global");
    assert!(global_reader.run("SELECT * FROM seed").is_ok());

    drop(instance);
    set_bases(Vec::new());
    let _ = std::fs::remove_dir_all(dir);
}

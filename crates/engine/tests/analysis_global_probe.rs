//! 全局分析库会话的真跑验证（2026-11-26 所有权收口后）。
//!
//! 全局分析库不再由本模块另开文件连接：`shared::duckdb_root` 把它挂成
//! `rds_global`，分析实例从同一条根连接复制会话。这里验三件事：
//! **根连接能挂上并查到表**、**`global_tables()` 走生产路径能列清单**、
//! **真库路径与系统目录一致**。

use std::path::PathBuf;

/// 造一个临时全局分析库文件（不依赖 `RDS_HOME`，离线也能跑）。
fn temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("rds_global_probe");
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join(format!("{tag}.duckdb"));
    let _ = std::fs::remove_file(&path);
    {
        let conn = duckdb::Connection::open(&path).expect("建库");
        conn.execute_batch(
            "create table global_connections (id varchar, driver varchar);
             insert into global_connections values ('G_conn_SQLite_Example', 'sqlite');",
        )
        .expect("写测试数据");
    }
    path
}

/// 根连接挂上全局分析库后，`rds_global.<表>` 能查（不是「计划里有、实际没挂」）。
#[test]
fn global_db_is_attached_through_the_process_root_and_queryable() {
    let _ = rds_engine::duckdb::instance::drop_instance();
    let path = temp_db("root_attach");
    shared::duckdb_root::ensure(None, Some(path.as_path())).expect("根连接挂全局库");
    let instance = rds_engine::duckdb::instance::instance().expect("分析实例");
    let session = instance.process_session().expect("process 会话");
    let result = session
        .run("select count(*) as n from rds_global.global_connections")
        .expect("查根连接上的全局表");
    assert_eq!(result.to_rows()[0][0].to_string(), "1");
    let _ = std::fs::remove_file(&path);
}

/// `global_tables()` 列的是全局分析库里的表（选择器「全局分析表」档的数据源）。
///
/// 造在测试根（`get_system_dir()` 的落点）上：走的就是生产路径，不另开测试专用入口。
#[test]
fn global_tables_lists_the_tables_of_the_analysis_db() {
    let _ = rds_engine::duckdb::instance::drop_instance();
    let dir = rds_engine::migration::get_system_dir().expect("系统目录");
    std::fs::create_dir_all(&dir).expect("建系统目录");
    let path = dir.join("analytics.duckdb");
    let _ = std::fs::remove_file(&path);
    let conn = duckdb::Connection::open(&path).expect("建全局分析库");
    conn.execute_batch("create table g_orders(id integer); create table g_customers(id integer);")
        .expect("写表");
    drop(conn); // 先释放建库连接，根连接再挂

    let tables = rds_engine::analysis::global::global_tables("C_global_list_probe")
        .expect("列全局分析表");
    assert_eq!(tables, vec!["g_customers".to_string(), "g_orders".to_string()]);
}

/// 真全局分析库（`RDS_HOME` 显式设置时才验）：空闲时能通过根连接列出表。
#[test]
fn the_real_global_analysis_db_is_queryable_when_idle() {
    let Ok(home) = std::env::var("RDS_HOME") else {
        eprintln!("跳过：未设 RDS_HOME，无法定位真全局分析库");
        return;
    };
    let path = std::path::PathBuf::from(home)
        .join("data")
        .join("system")
        .join("analytics.duckdb");
    if !path.exists() {
        eprintln!("跳过：{} 不存在（宿主未初始化）", path.display());
        return;
    }
    let tables = rds_engine::analysis::global::global_tables("C_global_real_probe")
        .expect("真全局分析库应能通过根连接列出表");
    eprintln!("真全局分析库表数：{}（{}）", tables.len(), path.display());
}

/// `global_analysis_db_path()` 的落点必须与系统目录一致（路径规则只此一处），
/// 且**指向 DuckDB 分析库**（不是 SQLite 的 `global.db`）。
#[test]
fn global_analysis_db_path_follows_the_system_dir() {
    let system_dir = rds_engine::migration::get_system_dir().expect("系统目录");
    let path = rds_engine::analysis::global::global_analysis_db_path().expect("全局分析库路径");
    assert_eq!(path, system_dir.join("analytics.duckdb"));
    assert!(
        !path.to_string_lossy().ends_with("global.db"),
        "别把元数据库当分析库：{}",
        path.display()
    );
}

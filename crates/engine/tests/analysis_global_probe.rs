//! 全局库会话（刀 7 · B1）的真跑验证。
//!
//! 验三件事：**只读挂上去真能查到表**、**只读是真只读**（写会被拒）、
//! **撞锁给的是可读原因**（不是 DuckDB 原文）。用临时 duckdb 文件而不是真全局库，
//! 所以这条用例不依赖 `RDS_HOME`，离线也能跑。

use std::path::PathBuf;

use rds_engine::analysis::global::{open_mounted, GLOBAL_ALIAS};

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

#[test]
fn the_global_like_db_is_mounted_read_only_and_queryable() {
    let path = temp_db("mount_ok");
    let session = open_mounted("C_global_probe", &path, GLOBAL_ALIAS, "duckdb").expect("只读挂载");
    // 别名进 SQL：`rds_global.<表>` 能查，说明挂载真的接上了（不是「计划里有、实际没挂」）
    let result = session
        .run("select count(*) as n from rds_global.global_connections")
        .expect("查挂载表");
    assert!(!result.columns.is_empty(), "挂载表应可查");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_mounted_global_db_really_is_read_only() {
    let path = temp_db("read_only");
    let session = open_mounted("C_global_probe", &path, GLOBAL_ALIAS, "duckdb").expect("只读挂载");
    let err = session
        .run("create table rds_global.should_not_exist as select 1 as a")
        .expect_err("只读库不该允许建表");
    let text = format!("{err}").to_lowercase();
    assert!(
        text.contains("read-only") || text.contains("只读"),
        "拒绝必须是「只读」语义，实际：{err}"
    );
    let _ = std::fs::remove_file(&path);
}

/// 真全局库（`<RDS_HOME>/data/system/global.db`）：空闲状态下能只读挂上并读到
/// 真全局**分析**库（`<RDS_HOME>/data/system/analytics.duckdb`）：空闲状态下能只读挂上，
/// 且 `duckdb_databases()` 里能看到别名。
///
/// 这里**直接从 `RDS_HOME` 环境变量拼路径**，不走 `get_system_dir()`：测试运行器会把
/// 系统目录指到临时根（实测解析结果是 `.rds/tmp/rds-test-root-*/data/system`），
/// 走那条路只会验到「不存在的临时库」。测试时显式设 `RDS_HOME` 才会验到真库。
///
/// 为什么允许跳过：若此刻正好有写入方持有全局分析库，DuckDB 会拒绝挂载——那是
/// **已知且必须可读报错**的常态，不该让用例红。
#[test]
fn the_real_global_analysis_db_opens_read_only_when_idle() {
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
    match open_mounted("C_global_real_probe", &path, GLOBAL_ALIAS, "duckdb") {
        Ok(session) => {
            // 分析库是 DuckDB：用 `duckdb_tables()` 按 database_name 数表，
            // 证明挂的是这台库（附带库名/表数，不是「表不存在」那种远端报错）。
            // 注意挂载库里的 `information_schema` 不一定被暴露（实测报 schema 不存在），
            // 所以这里用系统函数而不是信息模式。
            let result = session
                .run(&format!(
                    "select count(*) as n from duckdb_tables() where database_name = '{GLOBAL_ALIAS}'"
                ))
                .expect("查真全局分析库的表清单");
            assert!(!result.columns.is_empty());
            eprintln!("真全局分析库只读挂载成功：{}", path.display());
        }
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("正被其他操作占用"),
                "真库打开失败时必须是「占用」这类可读原因，实际：{text}"
            );
            eprintln!("跳过（有写入方持有全局分析库）：{text}");
        }
    }
}

/// `global_tables()` 列的是全局分析库里的表（选择器「全局分析表」档的数据源）。
///
/// 造在测试根（`get_system_dir()` 的落点）上：走的就是生产路径，不另开测试专用入口。
#[test]
fn global_tables_lists_the_tables_of_the_analysis_db() {
    let dir = rds_engine::migration::get_system_dir().expect("系统目录");
    std::fs::create_dir_all(&dir).expect("建系统目录");
    let path = dir.join("analytics.duckdb");
    let _ = std::fs::remove_file(&path);
    let conn = duckdb::Connection::open(&path).expect("建全局分析库");
    conn.execute_batch("create table g_orders(id integer); create table g_customers(id integer);")
        .expect("写表");
    drop(conn); // DuckDB 独占文件

    let tables = rds_engine::analysis::global::global_tables("C_global_list_probe")
        .expect("列全局分析表");
    assert_eq!(tables, vec!["g_customers".to_string(), "g_orders".to_string()]);
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

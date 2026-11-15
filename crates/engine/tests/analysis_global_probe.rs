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
/// `global_connections`。
///
/// 这里**直接从 `RDS_HOME` 环境变量拼路径**，不走 `get_system_dir()`：测试运行器会把
/// 系统目录指到临时根（实测解析结果是 `.rds/tmp/rds-test-root-*/data/system`），
/// 走那条路只会验到「不存在的临时库」。测试时显式设 `RDS_HOME` 才会验到真库。
///
/// 为什么允许跳过：若此刻正好有写入方（连接保存 / 归档 / 洞察索引）持有全局库，
/// DuckDB 会拒绝挂载——那是**已知且必须可读报错**的常态，不该让用例红。
#[test]
fn the_real_global_db_opens_read_only_when_idle() {
    let Ok(home) = std::env::var("RDS_HOME") else {
        eprintln!("跳过：未设 RDS_HOME，无法定位真全局库");
        return;
    };
    let path = std::path::PathBuf::from(home)
        .join("data")
        .join("system")
        .join("global.db");
    if !path.exists() {
        eprintln!("跳过：{} 不存在（宿主未初始化）", path.display());
        return;
    }
    match open_mounted("C_global_real_probe", &path, GLOBAL_ALIAS, "sqlite") {
        Ok(session) => {
            let result = session
                .run(&format!(
                    "select count(*) as n from {GLOBAL_ALIAS}.global_connections"
                ))
                .expect("查真全局库的连接表");
            assert!(!result.columns.is_empty());
            eprintln!("真全局库只读挂载成功：{}", path.display());
        }
        Err(error) => {
            let text = error.to_string();
            assert!(
                text.contains("正被其他操作占用"),
                "真库打开失败时必须是「占用」这类可读原因，实际：{text}"
            );
            eprintln!("跳过（有写入方持有全局库）：{text}");
        }
    }
}

/// `global_db_path()` 的落点必须与系统目录一致（路径规则只此一处）。
#[test]
fn global_db_path_follows_the_system_dir() {
    let system_dir = rds_engine::migration::get_system_dir().expect("系统目录");
    assert_eq!(
        rds_engine::analysis::global::global_db_path().expect("全局库路径"),
        system_dir.join("global.db")
    );
}

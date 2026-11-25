//! 进程面与源库是不是**同一个实例**（2f 的验收口）。
//!
//! 重构前：`DuckDBManager` 的"进程内存库"是另开的一条 `open_in_memory`，加速源挂在
//! 另一条连接/另一个库上，`tmp_q_ JOIN 源` 直接 `Catalog Error`（遗留 ⑪）。
//! 重构后：进程面就是分析实例里 `owner = process` 的那条会话，catalog 共享。
//!
//! 这里只用 **DuckDB 文件源**（离线可跑，不需要任何网络扩展）；真机版在
//! `crates/engine/tests/duckdb_accel_probe.rs`。

use rds_engine::duckdb::accel::{self, AccelKind, AccelSource};use rds_engine::duckdb::instance;
use rds_engine::services::duckdb_service::DuckDbService;

fn workdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_proc_face_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建目录");
    dir
}

/// 造一个 DuckDB 文件源（3 行 `items`）
fn duckdb_file_source(dir: &std::path::Path) -> String {
    let path = dir.join("src.duckdb");
    let conn = duckdb::Connection::open(&path).expect("建源文件");
    conn.execute_batch(
        "CREATE TABLE items AS SELECT i AS id, 'v' || i AS name FROM range(3) AS r(i)",
    )
    .expect("建表");
    drop(conn);
    path.to_str().expect("路径").to_string()
}

fn query_rows(sql: &str) -> i64 {
    let conn = DuckDbService::get_or_create_duckdb().expect("进程面句柄");
    let conn = conn.lock().expect("锁");
    let mut stmt = conn.prepare(sql).expect("准备语句");
    let mut rows = stmt.query([]).expect("查询");
    let row = rows.next().expect("取行").expect("有行");
    row.get::<_, i64>(0).expect("读第一列")
}

#[test]
fn the_process_face_lives_in_the_analysis_instance() {
    let dir = workdir("same_instance");
    let path = duckdb_file_source(&dir);

    // 1. 源挂在实例上（加速档入口）
    let source = AccelSource {
        conn_id: "probe_proc_face".to_string(),
        kind: AccelKind::DuckDb,
        connection_string: path.clone(),
    };
    let (session, alias) = accel::ensure_on_instance(&source).expect("挂源");
    assert!(alias.starts_with("rds_src"), "别名按源分配：{alias}");

    // 2. 进程面（历史 `DuckDBManager` 单例）能直接查到那个源 —— 这就是"同一个实例"
    assert_eq!(query_rows(&format!("SELECT count(*) FROM {alias}.items")), 3);
    // 未限定名也落在实例上（进程面的默认 catalog 是 memory，所以这里只验限定名 + rds_tmp）

    // 3. 进程面写进 `rds_tmp`（跨会话共享临时区）后，编辑器会话能看见并跟源 JOIN
    {
        let conn = DuckDbService::get_or_create_duckdb().expect("进程面句柄");
        let conn = conn.lock().expect("锁");
        conn.execute_batch(
            "CREATE OR REPLACE TABLE rds_tmp.tmp_proc_probe AS
             SELECT i AS id, 'v' || i AS name FROM range(3) AS r(i)",
        )
        .expect("写 rds_tmp");
    }
    let instance = instance::instance().expect("取实例");
    instance.register_temp("rds_tmp.tmp_proc_probe", "2f 探针", "进程面写入的临时结果");
    let editor_session = instance.ensure_session("doc:probe", "编辑器").expect("开会话");
    let joined = editor_session
        .run(&format!(
            "SELECT count(*) AS n FROM rds_tmp.tmp_proc_probe p JOIN {alias}.items s ON p.id = s.id"
        ))
        .expect("会话里 JOIN 进程面临时表与源");
    assert_eq!(joined.to_rows()[0][0].to_string(), "3", "应能 JOIN 上");

    // 4. 反向：会话写的 `rds_tmp` 表，进程面也看得见
    editor_session
        .run("CREATE OR REPLACE TABLE rds_tmp.tmp_editor_probe AS SELECT 2 AS id")
        .expect("会话写 rds_tmp");
    assert_eq!(
        query_rows(
            "SELECT count(*) FROM rds_tmp.tmp_editor_probe e JOIN rds_tmp.tmp_proc_probe p ON e.id = p.id"
        ),
        1
    );

    // 5. 收尾：手动清理（默认不静默删）
    let _ = instance.drop_temp("rds_tmp.tmp_proc_probe");
    let _ = instance.drop_temp("rds_tmp.tmp_editor_probe");
    let _ = std::fs::remove_dir_all(&dir);
}

//! 实测探针：项目写连接持有 `analytics.duckdb` 时，从**另一个连接**写它到底行不行。
//!
//! 这条事实决定结果集持久化（刀 ⑥-②）走哪条路：
//! - 若行 → 可以直接 ATTACH（`duckdb::temp_table::promote`）写项目库；
//! - 若不行 → 必须经 `ProjectDatabaseManager` 的既有写连接写，数据要跨库搬运。
//!
//! 结论以**真实报错文本**为准，不据注释推断。用例同时钉住现状：`ProjectDuckdbConnection`
//! 在项目打开期持有一条长连接（`project_db.rs:276-278` 原文：Windows 上 DuckDB 对同一
//! 文件仅允许一个连接句柄）。
//!
//! # 2026-11-15 实测结论（Windows，本机）
//!
//! ```text
//! ATTACH 失败：挂载目标库失败: IO Error: Cannot open file "...\.RSmeta\analytics.duckdb":
//!   另一个程序正在使用此文件，进程无法访问。
//!   File is already open in ...analysis_db_exclusive_probe.exe (PID 24516)
//! 经项目连接写表：成功
//! ```
//!
//! 因此：**项目打开（写模式）期间，任何「第二个连接 + ATTACH」的写法都写不进项目分析库**；
//! 唯一可行的是经 `ProjectDatabaseManager::duckdb_conn()` 那条既有写连接。
//! 这条事实直接判定刀 ⑥-②（结果集持久化）必须做**跨库搬运**（内存单例 `tmp_q_` → 项目写连接），
//! 而不是 ATTACH 复制。
//!
//! 顺带浮出一个**既有缺陷**：`workbench/src/services/mock_generator.rs` 的
//! `open_analysis_db()` / `persist_table_at()` / `append_table_at()` 全是「新开连接写项目库」，
//! 与本结论冲突；`mock/tests/persistence_roundtrip.rs` 只覆盖 SQLite store，没有真库用例
//! 碰这条路径——所以它在项目打开时是否必然失败，需要一次真机验证（未验证，不与结论混写）。

use std::path::PathBuf;

use rds_engine::persistence::project_db::ProjectDatabaseManager;

/// 自己实现「另一个连接写目标文件库」这一句（等价于 `promote_with_conn` 的写入步）：
/// 那个函数是 crate 内部口径，探针要验的是**能否 ATTACH**，不是复用实现。
fn create_in_file_db(
    conn: &duckdb::Connection,
    db_path: &std::path::Path,
    source_table: &str,
    target: &str,
) -> Result<(), String> {
    let alias = "rds_probe";
    let _ = conn.execute_batch(&format!("DETACH {alias}"));
    conn.execute_batch(&format!(
        "ATTACH '{}' AS {alias}",
        db_path.to_string_lossy().replace('\'', "''")
    ))
    .map_err(|e| format!("挂载目标库失败: {e}"))?;
    let created = conn
        .execute_batch(&format!(
            "CREATE TABLE {alias}.main.\"{target}\" AS SELECT * FROM \"{source_table}\""
        ))
        .map_err(|e| format!("建表失败: {e}"));
    let _ = conn.execute_batch(&format!("DETACH {alias}"));
    created
}

fn temp_root(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "rds_exclusive_{tag}_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("临时项目目录");
    root
}

/// 项目打开（写模式）→ 项目分析库被 `ProjectDuckdbConnection` 持有 →
/// 进程内存单例若要写它，走 ATTACH 会怎样。
#[tokio::test(flavor = "multi_thread")]
async fn attaching_the_project_analytics_db_while_project_is_open() {
    let root = temp_root("attach");
    // 写模式打开：内部会建 `.RSmeta/analytics.duckdb` 并持有一条长连接
    let manager = ProjectDatabaseManager::open(&root, 2)
        .await
        .expect("打开项目库（写模式）");
    let db_path = ProjectDatabaseManager::analysis_db_path(&root);
    assert!(
        db_path.exists(),
        "写模式打开项目后，分析库文件应当已经建好（{}）",
        db_path.display()
    );

    // 另一个连接（这里用独立内存连接模拟「进程内存单例」，数据面等价：
    // 关键是被写的目标文件是否已被别人持有）
    let conn = duckdb::Connection::open_in_memory().expect("内存连接");
    conn.execute_batch("CREATE TABLE tmp_q_src AS SELECT 1 AS a UNION ALL SELECT 2 AS a")
        .expect("建临时表");

    let result = create_in_file_db(&conn, &db_path, "tmp_q_src", "orders_copy");
    match &result {
        Ok(()) => {
            // 真行了：那就该记下来——刀 ⑥-② 不需要跨库搬运
            println!("ATTACH 成功：项目写连接不阻塞第二个连接写同一文件");
        }
        Err(error) => {
            // 不行：把真实报错原样打出来（这就是路线的依据）
            println!("ATTACH 失败：{error}");
            assert!(
                error.contains("另一个程序正在使用此文件")
                    || error.contains("already open")
                    || error.contains("Cannot open file"),
                "失败原因必须是文件独占（而不是别的错）：{error}"
            );
        }
    }

    // 项目写连接自己能不能写同一张表？这是「经既有连接写」那条路的可行性凭证
    let project_conn = manager
        .duckdb_conn()
        .expect("写模式下项目连接应当存在");
    let can_write = {
        let guard = project_conn.acquire().await.expect("取项目 DuckDB 连接");
        let mut slot = guard.lock().await;
        match slot.as_mut() {
            Some(conn) => conn
                .execute_batch("CREATE TABLE probe_via_manager AS SELECT 1 AS a")
                .map(|_| ()),
            None => Err(duckdb::Error::DuckDBFailure(
                duckdb::ffi::Error::new(0),
                Some("连接已被关闭".to_string()),
            )),
        }
    };
    println!("经项目连接写表：{}", if can_write.is_ok() { "成功" } else { "失败" });
    assert!(
        can_write.is_ok(),
        "项目自己的写连接必须能写：{can_write:?}"
    );

    drop(manager);
    let _ = std::fs::remove_dir_all(&root);
}

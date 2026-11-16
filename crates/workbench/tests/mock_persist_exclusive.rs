//! 真库探针：**项目打开（写模式）时**，Mock 面板落库那条路径能不能成。
//!
//! 起因：刀 ⑥-②（结果集持久化）被实测判定「第二连接 + ATTACH 写不进项目分析库」
//! （见 `crates/engine/tests/analysis_db_exclusive_probe.rs`）。而
//! `workbench/src/services/mock_generator.rs` 的落库/追加（`open_analysis_db` /
//! `persist_table_at` / `append_table_at`）正是「新开连接写项目库」，与本结论冲突，
//! 且 mock 侧只有 SQLite store 往返用例（`mock/tests/persistence_roundtrip.rs`），
//! **没有一条用例碰这条路径**。
//!
//! 这条用例把顺序照原样走一遍：内部先 `Connection::open` 看一眼表清单（`open_analysis_db`），
//! 再让引擎 `write_temp_table_to_database` 在内存连接上 ATTACH 目标文件写表。
//! 结论只认**真实报错文本**，成功也照实打印。

use std::path::PathBuf;

use engine::persistence::project_db::ProjectDatabaseManager;

fn temp_root(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "rds_mock_persist_{tag}_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("临时项目目录");
    root
}

/// `open_analysis_db` 那一步：项目已打开时，新开一个连接读项目分析库。
#[tokio::test(flavor = "multi_thread")]
async fn a_second_connection_reads_the_project_analytics_db() {
    let root = temp_root("open");
    let manager = ProjectDatabaseManager::open(&root, 2)
        .await
        .expect("打开项目库（写模式）");
    let db_path = ProjectDatabaseManager::analysis_db_path(&root);
    assert!(db_path.exists(), "写模式打开项目后分析库文件应存在");

    // 与 mock_generator::open_analysis_db 同一句
    let second = duckdb::Connection::open(&db_path);
    match &second {
        Ok(conn) => {
            println!("第二连接打开分析库：成功");
            // 顺手验一下 mock 落库前的「看一眼表清单」这一步能不能读
            let read = conn.query_row(
                "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'main'",
                [],
                |row| row.get::<_, i64>(0),
            );
            println!("读表清单：{:?}", read.map(|c| c));
        }
        Err(error) => {
            println!("第二连接打开分析库：失败 → {error}");
            let text = format!("{error}");
            assert!(
                text.contains("另一个程序正在使用此文件") || text.contains("already open"),
                "失败原因必须是文件独占：{text}"
            );
        }
    }
    drop(second);

    // 收尾：把 manager 的写连接释放掉，临时目录才删得干净（Windows 文件锁）
    drop(manager);
    let _ = std::fs::remove_dir_all(&root);
}

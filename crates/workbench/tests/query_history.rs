//! Round 29：query_history 集成测试——临时目录注入，验证去重、上限、持久化。

use std::path::PathBuf;

use rds_workbench::services::query_history::{append_history_at, load_history_from};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_qh_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[test]
fn history_roundtrip_and_dedupe() {
    let dir = temp_dir("dedupe");

    append_history_at(&dir, "SELECT 1").expect("append1");
    append_history_at(&dir, "SELECT 2").expect("append2");
    let hist = append_history_at(&dir, "SELECT 1").expect("append3");

    assert_eq!(hist.first().map(String::as_str), Some("SELECT 1"), "同 SQL 移到最前");
    assert_eq!(hist.len(), 2, "去重后共 2 条");

    // 重新读取（持久化验证）
    let reloaded = load_history_from(&dir);
    assert_eq!(reloaded, hist);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_cap_at_20_newest_first() {
    let dir = temp_dir("cap");

    let mut last = Vec::new();
    for i in 0..30 {
        last = append_history_at(&dir, &format!("SELECT {i}")).expect("append");
    }
    assert_eq!(last.len(), 20, "上限 20 条");
    assert_eq!(last.first().map(String::as_str), Some("SELECT 29"), "最新在前");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_missing_file_loads_empty_and_blank_sql_ignored() {
    let dir = temp_dir("blank");

    let hist = load_history_from(&dir);
    assert!(hist.is_empty(), "缺失文件 → 空历史");

    let hist2 = append_history_at(&dir, "   ").expect("blank append");
    assert!(hist2.is_empty(), "空白 SQL 不写入");

    let _ = std::fs::remove_dir_all(&dir);
}

//! Round 27：query_export 集成测试——CSV 转义、落盘读回、空结果。

use std::path::PathBuf;

use rds_workbench::services::query_export::{QueryOutput, export_csv, export_to_default};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_qe_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[test]
fn export_csv_escapes_commas_quotes_and_newlines() {
    let dir = temp_dir("escape");
    let path = dir.join("out.csv");
    let out = QueryOutput {
        columns: vec!["name".to_string(), "note".to_string()],
        rows: vec![
            vec!["a,b".to_string(), "say \"hi\"".to_string()],
            vec!["line1\nline2".to_string(), "plain".to_string()],
        ],
        row_count: 2,
    };

    export_csv(&out, &path).expect("export csv");

    let text = std::fs::read_to_string(&path).expect("read csv");
    assert_eq!(
        text,
        "name,note\n\"a,b\",\"say \"\"hi\"\"\"\n\"line1\nline2\",plain\n"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_to_default_writes_readable_file() {
    let out = QueryOutput {
        columns: vec!["order_id".to_string(), "status".to_string()],
        rows: vec![
            vec!["1".to_string(), "paid".to_string()],
            vec!["2".to_string(), "pending".to_string()],
        ],
        row_count: 2,
    };

    let path = export_to_default(&out).expect("export to default");
    assert!(path.exists(), "file should exist: {}", path.display());

    let text = std::fs::read_to_string(&path).expect("read exported file");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "header + 2 rows");
    assert_eq!(lines[0], "order_id,status");
    assert_eq!(lines[1], "1,paid");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn export_csv_empty_rows_only_header() {
    let dir = temp_dir("empty");
    let path = dir.join("empty.csv");
    let out = QueryOutput {
        columns: vec!["a".to_string(), "b".to_string()],
        rows: vec![],
        row_count: 0,
    };

    export_csv(&out, &path).expect("export empty");
    let text = std::fs::read_to_string(&path).expect("read csv");
    assert_eq!(text, "a,b\n");

    let _ = std::fs::remove_dir_all(&dir);
}

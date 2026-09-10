# -*- coding: utf-8 -*-
"""Round 23：save_connection 目录注入 + 集成测试。"""
import pathlib

# ---- 1. loader：拆 save_connection_at ----
w = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\workspace_loader.rs')
t = w.read_text(encoding='utf-8')

old = '''pub fn save_connection(
    name: &str,
    db_type: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let dir = default_global_dir();
    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");'''
new = '''pub fn save_connection(
    name: &str,
    db_type: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let dir = default_global_dir();
    save_connection_at(&dir, name, db_type, url, username, password)
}

/// 保存连接到指定目录（目录可注入，便于测试与后续数据目录切换）。
pub fn save_connection_at(
    dir: &Path,
    name: &str,
    db_type: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(), String> {
    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");'''
assert old in t
t = t.replace(old, new)
w.write_text(t, encoding='utf-8')

# ---- 2. 集成测试：save → load 读回 ----
r = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\tests\real_connections.rs')
t2 = r.read_text(encoding='utf-8')

add = '''
#[tokio::test]
async fn save_connection_at_then_load_roundtrip() {
    let dir = temp_dir("save_at");
    // 写路径（同步入口，测试线程无 tokio 上下文亦可直接调用）。
    rds_workbench::services::workspace_loader::save_connection_at(
        &dir,
        "表单新建库",
        "postgres",
        "postgres://127.0.0.1:5432/analytics",
        "admin",
        "secret",
    )
    .expect("save ok");

    let (items, notice) =
        rds_workbench::services::workspace_loader::load_persisted_connections_from(&dir);
    assert!(notice.is_none());
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "表单新建库");
    assert_eq!(items[0].driver, "postgres");
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(5432));
    assert_eq!(items[0].database.as_deref(), Some("analytics"));
    assert_eq!(items[0].use_duckdb_fed, true);

    let _ = std::fs::remove_dir_all(&dir);
}
'''
# 追加到文件末尾
t2 = t2.rstrip() + '\n' + add
r.write_text(t2, encoding='utf-8')
print('test added')

# -*- coding: utf-8 -*-
"""Round 24：集成测试 delete_connection_at roundtrip。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\tests\real_connections.rs')
t = p.read_text(encoding='utf-8')

add = '''
#[test]
fn delete_connection_at_removes_from_list() {
    let dir = temp_dir("delete_at");
    rds_workbench::services::workspace_loader::save_connection_at(
        &dir,
        "待删除库",
        "mysql",
        "mysql://10.0.0.8:3306/orders",
        "",
        "",
    )
    .expect("save ok");

    let (items, _) = rds_workbench::services::workspace_loader::load_persisted_connections_from(&dir);
    assert_eq!(items.len(), 1);
    let conn_id = items[0].id.clone();

    // 删除后列表为空（物理删除，is_active=1 过滤下自然消失）。
    rds_workbench::services::workspace_loader::delete_connection_at(&dir, &conn_id)
        .expect("delete ok");
    let (items2, _) = rds_workbench::services::workspace_loader::load_persisted_connections_from(&dir);
    assert!(items2.is_empty(), "删除后连接应消失");

    let _ = std::fs::remove_dir_all(&dir);
}
'''
t = t.rstrip() + '\n' + add
p.write_text(t, encoding='utf-8')
print('delete test added')

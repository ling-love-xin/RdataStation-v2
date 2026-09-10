# -*- coding: utf-8 -*-
"""补插 Round 22 元数据断言（锚点修正版）。"""
import pathlib

r = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\tests\real_connections.rs')
t = r.read_text(encoding='utf-8')

old = '''    // save 后连接默认激活（is_active=1），映射如实呈现。
    assert_eq!(items[0].connected, true);

    drop(manager);'''
new = '''    // save 后连接默认激活（is_active=1），映射如实呈现。
    assert_eq!(items[0].connected, true);
    // Round 22：真实元数据字段完整映射。
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(3306));
    assert_eq!(items[0].database.as_deref(), Some("test"));
    assert_eq!(items[0].use_duckdb_fed, true);
    assert_eq!(items[0].description.as_deref(), Some("round 21 集成测试"));

    drop(manager);'''
assert old in t
t = t.replace(old, new)
r.write_text(t, encoding='utf-8')
print('assert patched')

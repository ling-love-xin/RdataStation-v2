# -*- coding: utf-8 -*-
"""补 tests/real_connections.rs 映射函数字段。"""
import pathlib

r = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\tests\real_connections.rs')
t = r.read_text(encoding='utf-8')

old = '''        .map(|c| ConnectionItem {
            id: c.id,
            name: c.name,
            driver: c.driver,
            connected: c.is_active,
        })'''
new = '''        .map(|c| ConnectionItem {
            id: c.id,
            name: c.name,
            driver: c.driver,
            connected: c.is_active,
            host: c.host,
            port: c.port,
            database: c.database,
            schema: c.schema_name,
            description: c.description,
            use_duckdb_fed: c.use_duckdb_fed,
            created_at: c.created_at,
            updated_at: c.updated_at,
        })'''
assert old in t, 'test map not found'
t = t.replace(old, new)
r.write_text(t, encoding='utf-8')
print('test map patched')

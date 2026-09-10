# -*- coding: utf-8 -*-
"""修 db_navigator 测试：DuckDB 文件锁——建表后 drop 连接。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\tests\db_navigator.rs')
t = p.read_text(encoding='utf-8')

old1 = '    .expect("create tables");\n\n    let tree = rds_workbench::services::db_navigator::load_navigator_tree(&db_path)'
new1 = '    .expect("create tables");\n    drop(conn);\n\n    let tree = rds_workbench::services::db_navigator::load_navigator_tree(&db_path)'
assert t.count(old1) == 1
t = t.replace(old1, new1)

old2 = '    let _conn = duckdb::Connection::open(&db_path).expect("open empty duckdb");\n\n    let tree = rds_workbench::services::db_navigator::load_navigator_tree(&db_path)'
new2 = '    let conn = duckdb::Connection::open(&db_path).expect("open empty duckdb");\n    drop(conn);\n\n    let tree = rds_workbench::services::db_navigator::load_navigator_tree(&db_path)'
assert t.count(old2) == 1
t = t.replace(old2, new2)

p.write_text(t, encoding='utf-8')
print('drop conn added')

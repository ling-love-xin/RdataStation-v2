# -*- coding: utf-8 -*-
"""修 seed_demo E0382：duckdb PathBuf move 后借用。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\examples\seed_demo.rs')
t = p.read_text(encoding='utf-8')

old1 = '    let rt = tokio::runtime::Runtime::new().expect("runtime");\n    rt.block_on(async {\n        let mgr = GlobalDatabaseManager::new(sqlite, duckdb, 2)'
new1 = '    let duckdb_str = duckdb.to_string_lossy().to_string();\n    let rt = tokio::runtime::Runtime::new().expect("runtime");\n    rt.block_on(async {\n        let mgr = GlobalDatabaseManager::new(sqlite, duckdb.clone(), 2)'
assert t.count(old1) == 1
t = t.replace(old1, new1)

old2 = '    let duckdb_path = duckdb.to_string_lossy().to_string();\n    let conn = duckdb::Connection::open(&duckdb_path)'
new2 = '    let duckdb_path = duckdb_str;\n    let conn = duckdb::Connection::open(&duckdb_path)'
assert t.count(old2) == 1
t = t.replace(old2, new2)

p.write_text(t, encoding='utf-8')
print('seed E0382 fixed')

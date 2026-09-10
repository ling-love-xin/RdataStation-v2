# -*- coding: utf-8 -*-
"""Round 22：ConnectionItem 扩展真实元数据字段 + loader 映射 + 测试同步。"""
import pathlib

# ---- 1. view.rs：扩展 ConnectionItem ----
v = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = v.read_text(encoding='utf-8')

old = '''/// 连接条目（Round 21 起由全局系统库真实数据填充）。
#[derive(Debug, Clone)]
pub struct ConnectionItem {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub connected: bool,
}'''
new = '''/// 连接条目（Round 21 起由全局系统库真实数据填充；Round 22 扩展完整元数据）。
#[derive(Debug, Clone)]
pub struct ConnectionItem {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub connected: bool,
    /// 真实元数据（Round 22）：主机 / 端口 / 数据库 / Schema。
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub description: Option<String>,
    /// DuckDB 联邦（本地加速）开关。
    pub use_duckdb_fed: bool,
    pub created_at: String,
    pub updated_at: String,
}'''
assert old in t
t = t.replace(old, new)
v.write_text(t, encoding='utf-8')

# ---- 2. workspace_loader.rs：映射新字段 ----
w = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\services\workspace_loader.rs')
t2 = w.read_text(encoding='utf-8')
old_map = '''                .map(|c| ConnectionItem {
                    id: c.id,
                    name: c.name,
                    driver: c.driver,
                    connected: c.is_active,
                })'''
new_map = '''                .map(|c| ConnectionItem {
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
assert old_map in t2
t2 = t2.replace(old_map, new_map)
w.write_text(t2, encoding='utf-8')

# ---- 3. tests/real_connections.rs：映射函数同步 + 新断言 ----
r = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\tests\real_connections.rs')
t3 = r.read_text(encoding='utf-8')
old_fn = '''        .map(|c| ConnectionItem {
            id: c.id,
            name: c.name,
            driver: c.driver,
            connected: c.is_active,
        })'''
new_fn = '''        .map(|c| ConnectionItem {
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
assert old_fn in t3
t3 = t3.replace(old_fn, new_fn)

old_as = '''    assert_eq!(items[0].driver, "mysql");
    assert_eq!(items[0].connected, true);'''
new_as = '''    assert_eq!(items[0].driver, "mysql");
    assert_eq!(items[0].connected, true);
    // Round 22：真实元数据字段完整映射。
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(3306));
    assert_eq!(items[0].database.as_deref(), Some("test"));
    assert_eq!(items[0].use_duckdb_fed, true);
    assert_eq!(items[0].description.as_deref(), Some("round 21 集成测试"));'''
assert old_as in t3
t3 = t3.replace(old_as, new_as)
r.write_text(t3, encoding='utf-8')

print('all patched')

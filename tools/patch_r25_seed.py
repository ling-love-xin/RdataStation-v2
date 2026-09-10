# -*- coding: utf-8 -*-
"""Round 25：seed_demo 加 DuckDB 演示表（幂等）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\examples\seed_demo.rs')
t = p.read_text(encoding='utf-8')

# 1) 头注释更新
old_head = '''//! Round 21 演示种子：向默认全局库写入一条演示连接。
//!
//! 运行：`cargo run -p rds-workbench --example seed_demo`
//! 效果：工作台启动时侧边栏「连接」列表显示该真实连接（名称/驱动/激活状态）。
//! 清理：删除 `%APPDATA%\\rdata-station\\global` 目录即可回到空态。'''
new_head = '''//! Round 21/25 演示种子：向默认全局库写入演示连接 + DuckDB 分析演示表。
//!
//! 运行：`cargo run -p rds-workbench --example seed_demo`
//! 效果：
//! - 侧边栏「连接」列表显示演示连接（conn-demo-mysql，DuckDB 联邦开启）；
//! - 选中连接后「数据库导航」区显示 global.duckdb 的三张演示分析表（orders/order_items/customers）。
//! 幂等：表用 `CREATE TABLE IF NOT EXISTS`，重复运行不报错。
//! 清理：删除 `%APPDATA%\\rdata-station\\global` 目录即可回到空态。'''
assert old_head in t
t = t.replace(old_head, new_head)

# 2) 在保存连接后追加建表逻辑
old_tail = '''        .await
        .expect("save demo connection");
        println!("seeded: {}", dir.display());
    });
}'''
new_tail = '''        .await
        .expect("save demo connection");
    });

    // Round 25：分析引擎库建演示表（幂等，供「数据库导航」区展示真实元数据）。
    let duckdb_path = duckdb.to_string_lossy().to_string();
    let conn = duckdb::Connection::open(&duckdb_path).expect("open global duckdb");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
             order_id      INTEGER PRIMARY KEY,
             customer_id   INTEGER NOT NULL,
             region        VARCHAR,
             amount        DECIMAL(12,2),
             status        VARCHAR,
             created_at    TIMESTAMP
         );
         CREATE TABLE IF NOT EXISTS order_items (
             item_id    INTEGER PRIMARY KEY,
             order_id   INTEGER NOT NULL,
             product_id INTEGER,
             quantity   INTEGER,
             unit_price DECIMAL(10,2)
         );
         CREATE TABLE IF NOT EXISTS customers (
             customer_id INTEGER PRIMARY KEY,
             name        VARCHAR,
             segment     VARCHAR,
             country     VARCHAR,
             signup_at   TIMESTAMP
         );
         CREATE VIEW IF NOT EXISTS v_order_summary AS
             SELECT o.order_id, o.amount, o.status, COUNT(i.item_id) AS items
             FROM orders o LEFT JOIN order_items i ON o.order_id = i.order_id
             GROUP BY o.order_id, o.amount, o.status;",
    )
    .expect("create demo tables");
    println!("seeded duckdb demo tables: {}", duckdb_path);
}'''
assert old_tail in t
t = t.replace(old_tail, new_tail)
p.write_text(t, encoding='utf-8')
print('seed_demo extended')

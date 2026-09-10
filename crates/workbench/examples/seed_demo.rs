//! Round 21/25 演示种子：向默认全局库写入演示连接 + DuckDB 分析演示表。
//!
//! 运行：`cargo run -p rds-workbench --example seed_demo`
//! 效果：
//! - 侧边栏「连接」列表显示演示连接（conn-demo-mysql，DuckDB 联邦开启）；
//! - 选中连接后「数据库导航」区显示 global.duckdb 的三张演示分析表（orders/order_items/customers）。
//! 幂等：表用 `CREATE TABLE IF NOT EXISTS`，重复运行不报错。
//! 清理：删除 `%APPDATA%\rdata-station\global` 目录即可回到空态。

use engine::persistence::global_db::{GlobalConnectionSaveInput, GlobalDatabaseManager};

fn default_global_dir() -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("rdata-station")
        .join("global")
}

fn main() {
    let dir = default_global_dir();
    std::fs::create_dir_all(&dir).expect("create global dir");

    let sqlite = dir.join("global.db");
    let duckdb = dir.join("global.duckdb");

    let duckdb_str = duckdb.to_string_lossy().to_string();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let mgr = GlobalDatabaseManager::new(sqlite, duckdb.clone(), 2)
            .await
            .expect("init global db");
        mgr.save_global_connection(GlobalConnectionSaveInput {
            conn_id: "conn-demo-mysql",
            name: "演示 MySQL 分析库",
            db_type: "mysql",
            url: "mysql://127.0.0.1:3306/demo",
            username: Some("root"),
            password: None,
            tags: None,
            server_version: None,
            description: Some("Round 21 真实数据接入演示"),
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            options: None,
            driver_properties: None,
            advanced_options: None,
            use_duckdb_fed: Some(true),
            metadata_path: None,
            schema_name: None,
        })
        .await
        .expect("save demo connection");
    });

    // Round 25：分析引擎库建演示表（幂等，供「数据库导航」区展示真实元数据）。
    let duckdb_path = duckdb_str;
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
}

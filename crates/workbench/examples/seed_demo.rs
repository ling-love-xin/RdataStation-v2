//! 数据源导航 · 演示种子数据。
//!
//! 运行：`cargo run -p rds-workbench --example seed_demo`
//! 可选：设置 `RDS_PROJECT_PATH=<项目根>` 时，同时写入项目库（P_/GP_ 连接 + 分组 + 标签 + 展开态）。
//!
//! 写入位置（与 app 实际读取一致）：
//! - 全局库：`{data_dir}/RdataStation/system/global.db`（`engine::migration::get_system_dir()`）
//! - 项目库：`{RDS_PROJECT_PATH}/.RSMETA/project.db`
//! - 种子数据文件：`{system}/seed/seed_ok.sqlite`、`{system}/seed/seed_ok.duckdb`
//!
//! 内容：
//! - 2 条**可正常连接**的连接（本地 SQLite / DuckDB 文件，含演示表，对象树可浏览）；
//! - 5 条**用于验证失败态**的连接（连接被拒 / 主机不可达 / 端口错误 / 密码错误 / 文件不存在）；
//! - 部分连接带标签；项目模式下写入分组与成员。
//!
//! 幂等：`INSERT OR REPLACE` / `CREATE TABLE IF NOT EXISTS`。
//! 清理：删除 `{data_dir}/RdataStation` 目录即可回到空态。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::persistence::global_db::{GlobalConnectionSaveInput, GlobalDatabaseManager};
use engine::persistence::project_connection_store::{ProjectConnection, ProjectConnectionStore};
use engine::persistence::project_db::ProjectDatabaseManager;

/// 一条演示连接的描述。
struct SeedConn {
    id: &'static str,
    name: &'static str,
    driver: &'static str,
    url: String,
    username: Option<&'static str>,
    password: Option<&'static str>,
    description: &'static str,
    tags: &'static str,
    use_duckdb_fed: bool,
}

fn main() {
    // 1. 解析 app 真实系统目录。
    let system_dir = match engine::migration::get_system_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("无法定位系统目录: {e}");
            std::process::exit(1);
        }
    };
    let seed_dir = system_dir.join("seed");
    std::fs::create_dir_all(&seed_dir).expect("create seed dir");
    println!("系统目录: {}", system_dir.display());

    // 2. 构造可正常连接的本地文件库（SQLite / DuckDB）。
    let sqlite_path = seed_dir.join("seed_ok.sqlite");
    let duckdb_path = seed_dir.join("seed_ok.duckdb");
    make_sqlite_demo(&sqlite_path);
    make_duckdb_demo(&duckdb_path);

    // 3. 连接清单。
    let missing_duckdb = seed_dir.join("does_not_exist.duckdb");
    let conns = vec![
        SeedConn {
            id: "G_conn_seed_duckdb",
            name: "演示 DuckDB（可连接）",
            driver: "duckdb",
            url: format!("duckdb://{}", duckdb_path.display()),
            username: None,
            password: None,
            description: "本地 DuckDB 文件，含 orders/customers 演示表",
            tags: "[\"demo\",\"local\"]",
            use_duckdb_fed: true,
        },
        SeedConn {
            id: "G_conn_seed_sqlite",
            name: "演示 SQLite（可连接）",
            driver: "sqlite",
            url: format!("sqlite://{}", sqlite_path.display()),
            username: None,
            password: None,
            description: "本地 SQLite 文件，含 orders 演示表",
            tags: "[\"demo\",\"local\"]",
            use_duckdb_fed: false,
        },
        SeedConn {
            id: "G_conn_seed_pg_refused",
            name: "测试 PG（连接被拒）",
            driver: "postgres",
            url: "postgres://127.0.0.1:5432/postgres".to_string(),
            username: Some("postgres"),
            password: None,
            description: "本机未监听 5432 时应报连接被拒",
            tags: "[\"test\",\"error\"]",
            use_duckdb_fed: false,
        },
        SeedConn {
            id: "G_conn_seed_pg_badhost",
            name: "测试 PG（主机不可达）",
            driver: "postgres",
            url: "postgres://10.255.255.1:5432/postgres".to_string(),
            username: Some("postgres"),
            password: None,
            description: "不可达地址，用于验证超时/网络错误提示",
            tags: "[\"test\",\"error\"]",
            use_duckdb_fed: false,
        },
        SeedConn {
            id: "G_conn_seed_mysql_badport",
            name: "测试 MySQL（端口错误）",
            driver: "mysql",
            url: "mysql://127.0.0.1:13306/demo".to_string(),
            username: Some("root"),
            password: Some("whatever"),
            description: "错误端口 13306，用于验证连接被拒",
            tags: "[\"test\",\"error\"]",
            use_duckdb_fed: false,
        },
        SeedConn {
            id: "G_conn_seed_mysql_badpass",
            name: "测试 MySQL（密码错误）",
            driver: "mysql",
            url: "mysql://root:definitely_wrong@127.0.0.1:3306/mysql".to_string(),
            username: Some("root"),
            password: Some("definitely_wrong"),
            description: "本机 MySQL 在线时应报认证失败",
            tags: "[\"test\",\"error\"]",
            use_duckdb_fed: false,
        },
        SeedConn {
            id: "G_conn_seed_duckdb_missing",
            name: "测试 DuckDB（文件不存在）",
            driver: "duckdb",
            url: format!("duckdb://{}", missing_duckdb.display()),
            username: None,
            password: None,
            description: "指向不存在的文件，用于验证打开失败提示",
            tags: "[\"test\",\"error\"]",
            use_duckdb_fed: false,
        },
    ];

    // 4. 写入全局库。
    let global_sqlite = match engine::migration::get_global_db_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("无法定位全局库: {e}");
            std::process::exit(1);
        }
    };
    let global_duckdb = engine::migration::get_global_duckdb_path()
        .unwrap_or_else(|_| system_dir.join("analytics.duckdb"));

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let mgr = GlobalDatabaseManager::new(global_sqlite.clone(), global_duckdb.clone(), 2)
            .await
            .expect("init global db");
        for c in &conns {
            mgr.save_global_connection(GlobalConnectionSaveInput {
                conn_id: c.id,
                name: c.name,
                db_type: c.driver,
                url: &c.url,
                username: c.username,
                password: c.password,
                tags: Some(c.tags),
                server_version: None,
                description: Some(c.description),
                driver_id: None,
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                options: None,
                driver_properties: None,
                advanced_options: None,
                use_duckdb_fed: Some(c.use_duckdb_fed),
                metadata_path: None,
                schema_name: None,
            })
            .await
            .unwrap_or_else(|e| panic!("写入连接 {} 失败: {e}", c.id));
            println!("  + 全局连接 {}", c.name);
        }
    });

    // 5. 可选：项目库（P_/GP_ 连接 + 分组 + 成员 + 标签 + 展开态）。
    if let Some(root) = std::env::var_os("RDS_PROJECT_PATH").map(PathBuf::from) {
        if root.is_dir() {
            println!("项目目录: {}", root.display());
            seed_project(&rt, &root, &sqlite_path);
        } else {
            eprintln!("RDS_PROJECT_PATH 不是有效目录，跳过项目种子");
        }
    } else {
        println!("（未设置 RDS_PROJECT_PATH，跳过项目种子；全局库已就绪）");
    }

    println!("\n完成。启动 app 后：数据源面板可见 {} 条全局连接（2 条可连接、5 条用于验证错误态）。", conns.len());
    println!("清理：删除 {} 即可回到空态。", system_dir.display());
}

/// 建一个含演示表的 SQLite 文件（幂等）。
fn make_sqlite_demo(path: &Path) {
    let conn = rusqlite::Connection::open(path).expect("open seed sqlite");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
             order_id    INTEGER PRIMARY KEY,
             customer_id INTEGER NOT NULL,
             amount      DECIMAL(12,2),
             status      VARCHAR(20),
             created_at  TIMESTAMP
         );
         INSERT OR IGNORE INTO orders VALUES (1, 101, 1299.00, 'paid', '2024-08-01 09:12:44');
         INSERT OR IGNORE INTO orders VALUES (2, 102, 88.50,  'paid', '2024-08-01 09:15:02');",
    )
    .expect("create sqlite demo table");
    println!("  种子 SQLite: {}", path.display());
}

/// 建一个含演示表/视图的 DuckDB 文件（幂等）。
fn make_duckdb_demo(path: &Path) {
    {
        let conn = duckdb::Connection::open(path).expect("open seed duckdb");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS orders (
                 order_id    INTEGER PRIMARY KEY,
                 customer_id INTEGER NOT NULL,
                 region      VARCHAR,
                 amount      DECIMAL(12,2),
                 status      VARCHAR,
                 created_at  TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS customers (
                 customer_id INTEGER PRIMARY KEY,
                 name        VARCHAR,
                 segment     VARCHAR,
                 country     VARCHAR
             );
             CREATE VIEW IF NOT EXISTS v_order_summary AS
                 SELECT status, COUNT(*) AS cnt, SUM(amount) AS total
                 FROM orders GROUP BY status;",
        )
        .expect("create duckdb demo tables");
    }
    println!("  种子 DuckDB: {}", path.display());
}

/// 项目级种子：2 条项目连接 + 1 条共享快照 + 分组 + 成员 + 标签 + 展开态。
fn seed_project(rt: &tokio::runtime::Runtime, root: &Path, sqlite_path: &Path) {
    let root_str = root.to_string_lossy().to_string();

    rt.block_on(async {
        let mgr = match ProjectDatabaseManager::open(root, 2).await {
            Ok(m) => Arc::new(m),
            Err(e) => {
                eprintln!("打开项目库失败: {e}");
                return;
            }
        };
        let store = ProjectConnectionStore::new(mgr);

        // 固定时间戳字符串（example 未依赖 chrono）
        let now = "2026-09-11T00:00:00Z".to_string();
        let project_conns = vec![
            ProjectConnection {
                id: "P_conn_seed_localmysql".to_string(),
                name: "项目：本地 MySQL".to_string(),
                driver: "mysql".to_string(),
                host: Some("127.0.0.1".to_string()),
                port: Some(3306),
                database: Some("demo".to_string()),
                schema_name: None,
                username: Some("root".to_string()),
                password_encrypted: None,
                options: None,
                tags: Some("[\"project\"]".to_string()),
                use_duckdb_fed: false,
                metadata_path: None,
                is_active: true,
                server_version: None,
                description: Some("项目连接示例（若本机无 MySQL 将连接失败）".to_string()),
                driver_id: None,
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                driver_properties: None,
                advanced_options: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
            ProjectConnection {
                id: "P_conn_seed_sqlite".to_string(),
                name: "项目：本地 SQLite".to_string(),
                driver: "sqlite".to_string(),
                host: None,
                port: None,
                database: Some(sqlite_path.to_string_lossy().to_string()),
                schema_name: None,
                username: None,
                password_encrypted: None,
                options: None,
                tags: Some("[\"project\",\"local\"]".to_string()),
                use_duckdb_fed: false,
                metadata_path: None,
                is_active: true,
                server_version: None,
                description: Some("项目连接示例（可连接）".to_string()),
                driver_id: None,
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                driver_properties: None,
                advanced_options: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
            ProjectConnection {
                id: "GP_conn_seed_sharedduckdb_20260911".to_string(),
                name: "共享：DuckDB 分析库".to_string(),
                driver: "duckdb".to_string(),
                host: None,
                port: None,
                database: Some(sqlite_path.with_extension("duckdb").to_string_lossy().to_string()),
                schema_name: None,
                username: None,
                password_encrypted: None,
                options: None,
                tags: Some("[\"shared\"]".to_string()),
                use_duckdb_fed: true,
                metadata_path: None,
                is_active: true,
                server_version: None,
                description: Some("全局共享快照样例（GP_）".to_string()),
                driver_id: None,
                environment_id: None,
                auth_config_id: None,
                auth_method: None,
                network_config_id: None,
                driver_properties: None,
                advanced_options: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        ];

        for c in &project_conns {
            if let Err(e) = store.create_connection(c).await {
                eprintln!("  写入项目连接 {} 失败: {e}", c.id);
            } else {
                println!("  + 项目连接 {}", c.name);
            }
        }
    });

    // 分组 + 成员 + 标签 + 展开态（原始 SQL，直接操作项目库）。
    let db_path = root.join(".RSMETA").join("project.db");
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("打开项目库失败: {e}");
            return;
        }
    };
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS connection_groups (
             id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT,
             sort_order INTEGER NOT NULL DEFAULT 0,
             created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
             updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
         );
         CREATE TABLE IF NOT EXISTS connection_group_members (
             group_id TEXT NOT NULL, connection_id TEXT NOT NULL,
             sort_order INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (group_id, connection_id)
         );
         CREATE TABLE IF NOT EXISTS connection_tags (
             connection_id TEXT NOT NULL, tag TEXT NOT NULL,
             created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
             PRIMARY KEY (connection_id, tag)
         );
         CREATE TABLE IF NOT EXISTS navigator_state (
             conn_id TEXT PRIMARY KEY, scope TEXT NOT NULL DEFAULT 'project',
             expanded_keys TEXT NOT NULL DEFAULT '[]', selected_key TEXT,
             filter_text TEXT NOT NULL DEFAULT '', version INTEGER NOT NULL DEFAULT 1,
             updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
         );",
    );

    let _ = conn.execute(
        "INSERT OR REPLACE INTO connection_groups (id, name, description, sort_order) VALUES
            ('grp_seed_core', '核心库', '演示分组：核心数据源', 0),
            ('grp_seed_report', '报表', '演示分组：报表数据源', 1)",
        [],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO connection_group_members (group_id, connection_id, sort_order) VALUES
            ('grp_seed_core', 'P_conn_seed_sqlite', 0),
            ('grp_seed_core', 'GP_conn_seed_sharedduckdb_20260911', 1),
            ('grp_seed_report', 'P_conn_seed_localmysql', 0)",
        [],
    );
    let _ = conn.execute(
        "INSERT OR REPLACE INTO connection_tags (connection_id, tag) VALUES
            ('P_conn_seed_sqlite', 'local'),
            ('P_conn_seed_sqlite', 'core'),
            ('GP_conn_seed_sharedduckdb_20260911', 'shared')",
        [],
    );
    // 展开态：默认展开共享 DuckDB 连接（连接根 key = conn_id）。
    let _ = conn.execute(
        "INSERT OR REPLACE INTO navigator_state (conn_id, scope, expanded_keys) VALUES (?1, 'project', ?2)",
        rusqlite::params![
            "GP_conn_seed_sharedduckdb_20260911",
            "[\"GP_conn_seed_sharedduckdb_20260911\"]"
        ],
    );
    println!("  + 分组/成员/标签/展开态（项目库）: {}", root_str);
}

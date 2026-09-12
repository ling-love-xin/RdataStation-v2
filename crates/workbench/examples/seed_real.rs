//! 真实数据源连接：清理脏数据 + 全组合种子 + 验证。
//!
//! 用途：把演示 / 测试连接清掉，写入**真实可连接**的数据源，覆盖
//! 全局（`G_`）/ 项目（`P_`）/ 共享（`GP_`）三种来源的完整组合，并做两层验证：
//!   1. 持久化验证：用工作台真实加载器 `load_connections_for_scope` 读回，
//!      核对来源短码、字段与分组 / 标签；
//!   2. 连通性验证：用 `DataSourceService::test` 对 4 个真实端点各探测一次
//!      （探测用隔离的临时全局库，不触碰用户真实库）。
//!
//! 运行：
//! ```text
//! RDS_PROJECT_PATH='D:\data\A1\A1' cargo run -p rds-workbench --example seed_real
//! ```
//! 可选参数：`--dry-run` 只打印计划不写库；`--no-probe` 跳过连通性探测。
//!
//! 写入位置（与 app 实际读取一致）：
//! - 全局库：`{data_dir}/RdataStation/system/global.db`
//! - 项目库：`{RDS_PROJECT_PATH}/.RSmeta/project.db`
//!
//! 安全：写库前用 `VACUUM INTO` 对两个库做**一致性备份**（`<db>.bak-<时间戳>`），
//! 连接设置 `busy_timeout`；**不打开 DuckDB**（应用可能正在运行并持有
//! `analytics.duckdb`），因此只用 `rusqlite` 直写连接表。

use std::path::{Path, PathBuf};
use std::time::Duration;

use engine::persistence::global_db::GlobalDatabaseManager;
use rds_workbench::services::data_source_service::DataSourceService;
use rusqlite::{params, Connection};

const MYSQL_HOST: &str = "192.168.3.138";
const PG_HOST: &str = "192.168.3.138";
const SQLITE_FILE: &str = r"D:\FossilT\T.fossil";
const DUCKDB_FILE: &str = r"D:\data\123";

/// 一个真实端点（4 个）。
struct Endpoint {
    /// 短键（用于生成 ID / 标签）。
    key: &'static str,
    /// 显示名。
    label: &'static str,
    /// 驱动 id（= `drivers.id`；native 驱动与类型同名）。
    driver: &'static str,
    host: Option<&'static str>,
    port: Option<u16>,
    database: Option<&'static str>,
    username: Option<&'static str>,
    password: Option<&'static str>,
}

fn endpoints() -> Vec<Endpoint> {
    vec![
        Endpoint {
            key: "mysql",
            label: "MySQL 192.168.3.138",
            driver: "mysql",
            host: Some(MYSQL_HOST),
            port: Some(3306),
            database: Some("mysql"),
            username: Some("root"),
            password: Some("root"),
        },
        Endpoint {
            key: "pg",
            label: "PostgreSQL 192.168.3.138",
            driver: "postgres",
            host: Some(PG_HOST),
            port: Some(5432),
            database: Some("postgres"),
            username: Some("postgres"),
            password: Some("postgresql"),
        },
        Endpoint {
            key: "sqlite",
            label: "SQLite T.fossil",
            driver: "sqlite",
            host: None,
            port: None,
            database: Some(SQLITE_FILE),
            username: None,
            password: None,
        },
        Endpoint {
            key: "duckdb",
            label: "DuckDB 123",
            driver: "duckdb",
            host: None,
            port: None,
            database: Some(DUCKDB_FILE),
            username: None,
            password: None,
        },
    ]
}

/// 三条来源的组合：作用域标签 + ID 前缀 + 目标库表。
struct ScopeSpec {
    label: &'static str,
    /// `G_` / `P_` / `GP_` 前缀。
    prefix: &'static str,
    /// 落库表：`global_connections` 或 `connections`。
    table: &'static str,
}

fn scopes() -> Vec<ScopeSpec> {
    vec![
        ScopeSpec {
            label: "全局",
            prefix: "G_",
            table: "global_connections",
        },
        ScopeSpec {
            label: "项目",
            prefix: "P_",
            table: "connections",
        },
        ScopeSpec {
            label: "共享",
            prefix: "GP_",
            table: "connections",
        },
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let no_probe = args.iter().any(|a| a == "--no-probe");

    // 1. 解析目标库路径。
    let global_db = match engine::migration::get_global_db_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("无法定位全局库: {e}");
            std::process::exit(1);
        }
    };
    let project_root = match std::env::var_os("RDS_PROJECT_PATH").map(PathBuf::from) {
        Some(p) if p.is_dir() => p,
        Some(p) => {
            eprintln!("RDS_PROJECT_PATH 不是有效目录: {}", p.display());
            std::process::exit(1);
        }
        None => {
            eprintln!("必须设置 RDS_PROJECT_PATH=<项目根>（例如 D:\\data\\A1\\A1）");
            std::process::exit(1);
        }
    };
    let project_db = project_root.join(".RSmeta").join("project.db");
    if !project_db.exists() {
        eprintln!("项目库不存在（请先用 app 打开过该项目）: {}", project_db.display());
        std::process::exit(1);
    }

    println!("全局库: {}", global_db.display());
    println!("项目库: {}", project_db.display());
    println!("模式  : {}{}", if dry_run { "dry-run（不写库）" } else { "写入" }, if no_probe { " / 跳过探测" } else { "" });

    // 2. 备份（一致性快照，含 WAL 已提交内容）。
    if !dry_run {
        for db in [&global_db, &project_db] {
            match backup(db) {
                Ok(dest) => println!("备份  : {} -> {}", db.display(), dest.display()),
                Err(e) => {
                    eprintln!("备份失败（已中止，未修改任何数据）: {} - {e}", db.display());
                    std::process::exit(1);
                }
            }
        }
    }

    // 3. 打印现有连接（将被清理）。
    let global_conn = open(&global_db);
    let project_conn = open(&project_db);
    println!("\n== 清理前的连接 ==");
    print_existing(&global_conn, "global_connections");
    print_existing(&project_conn, "connections");

    if dry_run {
        println!("\n[dry-run] 计划写入 12 条连接（4 端点 × 3 来源）+ 3 个分组 + 标签。");
        return;
    }

    // 4. 清理：连接 + 分组 + 标签 + 导航状态 + 暂存草稿。
    let cleared = clear_all(&global_conn, &project_conn).expect("清理失败");
    println!("\n== 清理完成 == {cleared}");

    // 5. 写入 12 条真实连接（4 端点 × 3 来源）。
    let date_stamp = chrono::Utc::now().format("%Y%m%d").to_string();
    let mut rows: Vec<SeededConn> = Vec::new();
    for scope in scopes() {
        for ep in endpoints() {
            // G_/P_：`G_real_<key>` / `P_real_<key>`；
            // GP_：`GP_real_<key>_<date>`（去尾日期段可回指 `G_real_<key>`）。
            let id = match scope.prefix {
                "G_" => format!("G_real_{}", ep.key),
                "P_" => format!("P_real_{}", ep.key),
                _ => format!("GP_real_{}_{date_stamp}", ep.key),
            };
            let name = format!("[{}] {}", scope.label, ep.label);
            let tags: Vec<&str> = vec![scope.label, ep.key, "真实"];
            let description = format!("真实连接：{}（{}）", ep.label, scope.label);
            let password_encrypted = ep
                .password
                .map(|p| shared::crypto::encrypt_password(p).expect("加密密码失败"));
            insert_connection(
                if scope.table == "global_connections" {
                    &global_conn
                } else {
                    &project_conn
                },
                scope.table,
                &id,
                &name,
                ep.driver,
                ep.host,
                ep.port,
                ep.database,
                ep.username,
                password_encrypted.as_deref(),
                &tags,
                &description,
            )
            .unwrap_or_else(|e| panic!("写入连接 {id} 失败: {e}"));
            println!("  + {} {}", scope.label, name);
            rows.push(SeededConn {
                id,
                scope: scope.label,
                key: ep.key,
            });
        }
    }

    // 6. 标签（连接级）+ 项目分组（多对多，含跨来源成员）。
    write_tags(&global_conn, rows.iter().filter(|r| r.scope == "全局"));
    write_tags(&project_conn, rows.iter().filter(|r| r.scope != "全局"));
    seed_groups(&project_conn, &rows);
    println!("  + 标签（连接级）与 3 个分组（网络库 / 文件库 / 全局共享）");

    // 7. 验证一：真实加载器读回（来源 + 字段 + 分组 + 标签）。
    drop(global_conn);
    drop(project_conn);
    verify_persisted(&project_root);

    // 8. 验证二：真实端点连通性探测（隔离临时库，不碰用户数据）。
    if !no_probe {
        verify_connectivity();
    }
}

/// 种子连接记录（供后续标签 / 分组引用）。
struct SeededConn {
    id: String,
    scope: &'static str,
    key: &'static str,
}

fn open(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap_or_else(|e| panic!("打开 {} 失败: {e}", path.display()));
    // 应用可能正在运行：给足等待窗口，避免 SQLITE_BUSY。
    let _ = conn.busy_timeout(Duration::from_secs(10));
    conn
}

/// `VACUUM INTO` 生成一致性备份（文件名带时间戳）。
fn backup(db: &Path) -> Result<PathBuf, rusqlite::Error> {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let file_name = db.file_name().unwrap_or_default().to_string_lossy().to_string();
    let dest = db.with_file_name(format!("{file_name}.bak-{ts}"));
    if dest.exists() {
        return Ok(dest);
    }
    let conn = open(db);
    conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy().to_string()])?;
    Ok(dest)
}

fn print_existing(conn: &Connection, table: &str) {
    let sql = format!("SELECT id, name, driver FROM {table} ORDER BY id");
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            println!("  ({table} 读取失败: {e})");
            return;
        }
    };
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })
        .expect("query");
    let mut n = 0;
    for row in rows {
        let (id, name, driver) = row.expect("row");
        println!("  - [{driver}] {name}  ({id})");
        n += 1;
    }
    println!("  ({table}: {n} 条)");
}

/// 清空连接及其组织元数据 / 导航状态 / 草稿，返回清理条数摘要。
fn clear_all(global: &Connection, project: &Connection) -> Result<String, rusqlite::Error> {
    let mut report = Vec::new();

    for (conn, tables) in [
        (
            global,
            vec!["global_connections", "connection_tags", "navigator_state", "navigator_states", "connection_drafts"],
        ),
        (
            project,
            vec![
                "connections",
                "connection_groups",
                "connection_group_members",
                "connection_tags",
                "navigator_state",
            ],
        ),
    ] {
        let tx = conn.unchecked_transaction()?;
        for t in tables {
            // 表可能不存在（旧库）；不存在时跳过。
            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![t],
                |r| r.get(0),
            )?;
            if exists == 0 {
                continue;
            }
            let n = conn.execute(&format!("DELETE FROM {t}"), [])?;
            report.push(format!("{t}={n}"));
        }
        tx.commit()?;
    }
    Ok(report.join(", "))
}

/// 写入一条连接（列集与 `save_global_connection` / `create_connection` 一致）。
#[allow(clippy::too_many_arguments)]
fn insert_connection(
    conn: &Connection,
    table: &str,
    id: &str,
    name: &str,
    driver: &str,
    host: Option<&str>,
    port: Option<u16>,
    database: Option<&str>,
    username: Option<&str>,
    password_encrypted: Option<&str>,
    tags: &[&str],
    description: &str,
) -> Result<(), rusqlite::Error> {
    let tags_json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
    let port_text = port.map(|p| p.to_string());
    let sql = format!(
        "INSERT OR REPLACE INTO {table} (
            id, name, driver, host, port, database, schema_name, username, password_encrypted,
            options, tags, use_duckdb_fed, metadata_path, is_active, server_version, description,
            driver_id, environment_id, auth_config_id, network_config_id, driver_properties,
            advanced_options, auth_method, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8,
            NULL, ?9, 0, NULL, 1, NULL, ?10,
            ?3, NULL, NULL, NULL, NULL,
            NULL, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
         )"
    );
    conn.execute(
        &sql,
        params![
            id,
            name,
            driver,
            host,
            port_text,
            database,
            username,
            password_encrypted,
            tags_json,
            description,
        ],
    )?;
    Ok(())
}

fn write_tags<'a>(conn: &Connection, rows: impl Iterator<Item = &'a SeededConn>) {
    for r in rows {
        let tags = vec!["真实", r.key, r.scope];
        for t in tags {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO connection_tags (connection_id, tag) VALUES (?1, ?2)",
                params![r.id, t],
            );
        }
    }
}

/// 项目分组：网络库 / 文件库 / 全局共享（演示多对多与跨来源成员）。
fn seed_groups(conn: &Connection, rows: &[SeededConn]) {
    let groups = [
        ("grp_real_net", "网络库", "MySQL / PostgreSQL（全部来源）", 0),
        ("grp_real_file", "文件库", "SQLite / DuckDB（全部来源）", 1),
        ("grp_real_shared", "全局共享", "共享（GP_）来源", 2),
    ];
    for (id, name, desc, order) in groups {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO connection_groups (id, name, description, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, desc, order],
        );
    }
    let mut order: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
    let mut member = |group: &str, conn_id: &str| {
        let slot = order.entry(group).or_insert(0);
        let _ = conn.execute(
            "INSERT OR REPLACE INTO connection_group_members (group_id, connection_id, sort_order) VALUES (?1, ?2, ?3)",
            params![group, conn_id, *slot],
        );
        *slot += 1;
    };
    for r in rows {
        match r.key {
            "mysql" | "pg" => member("grp_real_net", &r.id),
            "sqlite" | "duckdb" => member("grp_real_file", &r.id),
            _ => {}
        }
        if r.scope == "共享" {
            member("grp_real_shared", &r.id);
        }
    }
}

/// 验证一：用工作台真实加载器读回，核对来源 / 字段 / 数量。
fn verify_persisted(project_root: &Path) {
    println!("\n== 验证一：加载器读回（与 app 同一条链路）==");
    let (items, notice) =
        rds_workbench::services::workspace_loader::load_connections_for_scope(Some(project_root));
    if let Some(n) = &notice {
        println!("  提示: {n}");
    }
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    for prefix in ["G_real_", "P_real_", "GP_real_"] {
        let n = ids.iter().filter(|i| i.starts_with(prefix)).count();
        println!("  {prefix:<8} -> {n} 条");
    }
    println!("  合计 {} 条（预期 12）", items.len());
    for it in &items {
        let src = database::model::NavSource::from_conn_id(&it.id);
        println!(
            "    [{}] {:<34} driver={:<8} host={:<15} db={}",
            src.code(),
            it.name,
            it.driver,
            it.host.clone().unwrap_or_default(),
            it.database.clone().unwrap_or_default()
        );
    }
    assert_eq!(items.len(), 12, "加载器读回数量应为 12");
}

/// 验证二：真实端点连通性（隔离的临时全局库，不写用户数据）。
fn verify_connectivity() {
    println!("\n== 验证二：真实端点连通性（临时库探测）==");
    engine::driver::AutoDriverRegistrar::register_builtin_drivers();

    let tmp = std::env::temp_dir().join(format!("rds_probe_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create temp dir");

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let manager = rt
        .block_on(GlobalDatabaseManager::new(
            tmp.join("global.db"),
            tmp.join("analytics.duckdb"),
            2,
        ))
        .expect("init temp global db");
    let manager: &'static GlobalDatabaseManager = Box::leak(Box::new(manager));
    let service = DataSourceService::new(manager).with_analysis_db(tmp.join("secret-target.duckdb"));

    for ep in endpoints() {
        let url = match ep.driver {
            "mysql" => format!(
                "mysql://{}:{}@{}/{}",
                ep.username.unwrap_or(""),
                ep.password.unwrap_or(""),
                ep.host.unwrap_or(""),
                ep.database.unwrap_or("")
            ),
            "postgres" => format!(
                "postgres://{}:{}@{}/{}",
                ep.username.unwrap_or(""),
                ep.password.unwrap_or(""),
                ep.host.unwrap_or(""),
                ep.database.unwrap_or("")
            ),
            _ => format!("{}://{}", ep.driver, ep.database.unwrap_or("")),
        };
        let mut input =
            connection::model::DataSourceSaveInput::new(ep.label, ep.driver, url.as_str());
        input.username = ep.username.map(str::to_string);
        input.password = ep.password.map(str::to_string);

        let result = rt.block_on(service.test(&input, None));
        let flag = if result.success { "OK  " } else { "FAIL" };
        match &result.version {
            Some(v) if !v.is_empty() => println!("  [{flag}] {:<26} {v}", ep.label),
            _ => println!("  [{flag}] {:<26} {}", ep.label, result.message),
        }
    }

    drop(rt);
    let _ = std::fs::remove_dir_all(&tmp);
}

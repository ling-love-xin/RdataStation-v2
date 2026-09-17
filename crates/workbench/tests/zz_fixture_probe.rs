//! 临时诊断（**未跟踪文件**）：用户提供的四条真机连接自检。
//!
//! 覆盖两条链路（与 UI 同源）：
//!   1. 服务层「测试连接」：`DataSourceService::test`（对话框按钮走的就是这条）；
//!   2. 真实连接：`ConnectionService::connect_with_type`（导航「连接」同源）。
//!
//! 运行：`cargo test -p rds-workbench --test zz_fixture_probe -j 2 -- --nocapture`
//!
//! ⚠️ 含**内网明文凭据**（192.168.3.138）：仅本机使用，**不要提交**。

use std::sync::Arc;

use connection::model::DataSourceSaveInput;
use engine::connection_manager::{ConnectionManager, ConnectionType};
use rds_workbench::services::connection_service::{ConnectRequest, ConnectionService};
use rds_workbench::services::data_source_service::DataSourceService;

/// （显示名, 驱动 id, URL）—— 文件型用裸路径（与对话框落库形态一致）。
fn fixtures() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "MySQL",
            "mysql",
            "mysql://root:root@192.168.3.138:3306/mysql",
        ),
        (
            "PostgreSQL",
            "postgres",
            "postgres://postgres:postgresql@192.168.3.138:5432/postgres",
        ),
        ("SQLite", "sqlite", r"D:\FossilT\T.fossil"),
        ("DuckDB", "duckdb", r"D:\data\123"),
    ]
}

#[test]
fn probe_and_connect_all_fixtures() {
    engine::driver::AutoDriverRegistrar::auto_register();
    let rt = tokio::runtime::Runtime::new().expect("rt");

    // 服务层：临时全局库（不碰用户真实数据目录）
    let dir = std::env::temp_dir().join(format!("rds_fixture_probe_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let manager = rt
        .block_on(engine::persistence::global_db::GlobalDatabaseManager::new(
            dir.join("global.db"),
            dir.join("analytics.duckdb"),
            2,
        ))
        .expect("init global db");
    let manager: &'static _ = Box::leak(Box::new(manager));
    let service = DataSourceService::new(manager).with_analysis_db(dir.join("secret.duckdb"));

    let conn_service = ConnectionService::new(Arc::new(ConnectionManager::new()));

    let mut failures: Vec<String> = Vec::new();
    println!("\n=== 真机自检（测试连接 / 真实连接）===");

    for (name, driver, url) in fixtures() {
        // ---- 1) 测试连接（与对话框「测试连接」按钮同源）----
        let input = DataSourceSaveInput::new(name, driver, url);
        let t = rt.block_on(service.test(&input, None));
        println!(
            "[测试连接] {name:<10} success={:<5} 耗时={:>5?}ms 版本={:?}\n            msg={}",
            t.success, t.latency_ms, t.version, t.message
        );
        if !t.success {
            failures.push(format!("{name}: 测试连接失败 → {}", t.message));
        }

        // ---- 2) 真实连接（与导航「连接」同源）----
        let req = ConnectRequest {
            conn_id: Some(format!("zz_probe_{driver}")),
            db_type: driver.to_string(),
            url: url.to_string(),
            name: Some(name.to_string()),
            connection_type: ConnectionType::Global,
            project_path: None,
            description: None,
            driver_id: Some(driver.to_string()),
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
            options: None,
            tags: None,
            metadata_path: None,
            schema_name: None,
            use_duckdb_fed: Some(false),
            password: None,
            skip_persistence: Some(true),
            network_method: None,
        };
        let started = std::time::Instant::now();
        match rt.block_on(conn_service.connect_with_type(req)) {
            Ok((cid, _db)) => {
                println!(
                    "[真实连接] {name:<10} OK   运行时 id={cid} 耗时={}ms",
                    started.elapsed().as_millis()
                );
            }
            Err(e) => {
                println!("[真实连接] {name:<10} FAIL {e}");
                failures.push(format!("{name}: 真实连接失败 → {e}"));
            }
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
    if !failures.is_empty() {
        panic!("自检失败 {} 条：\n{}", failures.len(), failures.join("\n"));
    }
    println!("=== 四条连接全部通过 ===\n");
}

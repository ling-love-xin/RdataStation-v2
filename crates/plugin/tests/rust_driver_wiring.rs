//! Rust 原生驱动插件（Oracle / SQL Server）的宿主接线端到端验证。
//!
//! 为什么单独一个用例：这两个包的 `[backend] runtime = "rust"` 一度让
//! `RuntimeResolver` 去找「rust 解释器」而激活失败（`supervisor.rs` 已前置
//! `BackendRuntime::Rust => None`）。只跑 `driver_manifests` 验清单不够——
//! 本用例走真实生命周期：`initialize_global_system` → `load_plugin_from_dir`
//! → `activate_plugin` → `drivers` 表（连接对话框下拉的唯一数据源）。
//!
//! 断言三件事：
//! 1. 激活能成功（不再卡在运行时解析）；
//! 2. `drivers` 表里两行 `driver_kind='sidecar'`、`enabled=1`、端口正确；
//! 3. 停用后同一行翻成 `enabled=0`（下拉置灰的既有契约不被破坏）。
//!
//! 不连真库：激活只部署清单 + 注册工厂，不建立会话。
//! 真库链路另见文件末尾的 `oracle_plugin_serves_a_real_query`（需 `RDS_TEST_ORACLE_URL`）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rds_plugin::manager::PluginManager;
use rds_plugin::sidecar::driver::{QueryRequest, SessionDriver};
use rds_plugin::sidecar::runtime::{PluginRuntimeLoop, RUNTIME_TICK};
use rds_plugin::sidecar::supervisor::SessionOpened;
use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("仓库根应当存在")
        .to_path_buf()
}

fn plugin_root() -> PathBuf {
    repo_root().join(".rds/plugins")
}

fn plugin_dir(package_id: &str) -> PathBuf {
    // 用真实安装目录（目录名必须等于清单 id；源目录 `oracle-rust-plugin` 过不了
    // `load_plugin_from_dir` 的目录名一致性校验）。
    plugin_root().join(package_id)
}

/// 从 `drivers` 表读一行（id, driver_kind, enabled, default_port, name, auth, config_schema）。
async fn read_driver_row(
    id: &str,
) -> Option<(String, String, bool, Option<i64>, String, Option<String>, String)> {
    use engine::persistence::driver_store::Driver;

    let db = engine::migration::get_global_db_manager().expect("全局库应当已初始化");
    let drivers: Vec<Driver> = db.get_all_drivers().await.expect("读 drivers 表应当成功");
    drivers.into_iter().find(|d| d.id == id).map(|d| {
        (
            d.id,
            d.driver_kind,
            d.enabled,
            d.default_port.map(i64::from),
            d.name,
            d.supported_auth_types,
            d.config_schema,
        )
    })
}

#[tokio::test]
async fn rust_drivers_activate_and_land_enabled_in_the_drivers_table() {
    engine::migration::initialize_global_system()
        .await
        .expect("测试环境应当能初始化全局库");

    let manager = PluginManager::new(PathBuf::new());
    let oracle_dir = plugin_dir("rds.driver.oracle-rust");
    let mssql_dir = plugin_dir("rds.driver.mssql-rust");

    if !oracle_dir.join("plugin.toml").exists() || !mssql_dir.join("plugin.toml").exists() {
        eprintln!("两个 Rust 插件包未安装（先跑 tools\\plugin-drivers 的 build/install），跳过本用例");
        return;
    }

    for (id, dir) in [
        ("rds.driver.oracle-rust", &oracle_dir),
        ("rds.driver.mssql-rust", &mssql_dir),
    ] {
        assert!(
            dir.join("plugin.toml").exists(),
            "插件包应当存在：{}",
            dir.display()
        );
        let loaded = manager
            .load_plugin_from_dir(dir)
            .await
            .unwrap_or_else(|e| panic!("{id} 加载失败：{e}"));
        assert_eq!(loaded.id, id);
        manager
            .activate_plugin(id)
            .await
            .unwrap_or_else(|e| panic!("{id} 激活失败（检查 runtime=rust 的部署路径）：{e}"));
    }

    let oracle = read_driver_row("oracle")
        .await
        .expect("激活后 drivers 表应当有 oracle 行");
    assert_eq!(oracle.1, "sidecar", "oracle 行应归插件所有");
    assert!(oracle.2, "Oracle 插件已激活，行必须 enabled=1（否则下拉不显示）");
    assert_eq!(oracle.3, Some(1521));
    assert_eq!(oracle.4, "Oracle Database", "显示名不应带 (Rust) 后缀");
    assert_eq!(
        oracle.5.as_deref(),
        Some(r#"["password"]"#),
        "credentials=database 应派生 password 认证方法"
    );
    assert!(
        oracle.6.contains("\"key\":\"host\"") && oracle.6.contains("\"key\":\"password\""),
        "config_schema 应带 host/password 字段：{}",
        oracle.6
    );

    let mssql = read_driver_row("mssql")
        .await
        .expect("激活后 drivers 表应当有 mssql 行");
    assert_eq!(mssql.1, "sidecar", "mssql 行应归插件所有");
    assert!(mssql.2, "SQL Server 插件已激活，行必须 enabled=1");
    assert_eq!(mssql.3, Some(1433));
    assert_eq!(mssql.4, "SQL Server");
    assert_eq!(mssql.5.as_deref(), Some(r#"["password"]"#));

    // 停用一侧，验证行随插件状态翻灰（而不是永远留在 enabled=1）
    manager
        .deactivate_plugin("rds.driver.oracle-rust")
        .await
        .expect("停用应当成功");
    let oracle_off = read_driver_row("oracle")
        .await
        .expect("停用后行不应被删掉");
    assert!(!oracle_off.2, "停用插件后 oracle 行应当 enabled=0");

    // 启动路径复现：宿主 `init_plugin_manager` 只建目录，真正的首轮加载/激活
    // 由 `PluginRuntimeLoop::tick_once` 的热插拔 diff 完成（快照为空 → 目录全部算新增）。
    // 这里用新管理器跑一拍，等于「重启宿主」，验证停用过的行会随激活回到 enabled=1。
    let startup = Arc::new(PluginManager::new(plugin_root()));
    startup.init().await.expect("启动管理器应当初始化成功");
    PluginRuntimeLoop::new(startup.clone(), RUNTIME_TICK)
        .tick_once()
        .await;
    let oracle_restart = read_driver_row("oracle")
        .await
        .expect("启动拍后 oracle 行应当还在");
    assert!(
        oracle_restart.2,
        "宿主启动首拍应当自动加载并激活已启用插件（否则下拉仍然看不到）"
    );
}

/// 真机链路：真实安装目录 → 真实激活 → supervisor 开会话 → 真库查询。
///
/// 与 `sidecar_conformance` 的区别：那里手工拼 `Deployment` 直接起进程，
/// 这里走产品自己的 `PluginManager` 与安装目录，验证「宿主实际路径」也能查库。
/// 未设置 `RDS_TEST_ORACLE_URL` 时跳过（与仓库其他真机用例同口径）。
#[tokio::test]
async fn oracle_plugin_serves_a_real_query() {
    let Ok(url) = std::env::var("RDS_TEST_ORACLE_URL") else {
        eprintln!("跳过：未设置 RDS_TEST_ORACLE_URL");
        return;
    };
    let Some(params) = oracle_params(&url) else {
        eprintln!("跳过：RDS_TEST_ORACLE_URL 解析失败（期望 oracle://user:pass@host:port/service）");
        return;
    };

    let oracle_dir = plugin_dir("rds.driver.oracle-rust");
    if !oracle_dir.join("plugin.toml").exists() {
        eprintln!("Oracle 插件包未安装，跳过本用例");
        return;
    }

    let manager = PluginManager::new(PathBuf::new());
    manager
        .load_plugin_from_dir(&oracle_dir)
        .await
        .expect("Oracle 插件应当能加载");
    manager
        .activate_plugin("rds.driver.oracle-rust")
        .await
        .expect("Oracle 插件应当能激活");

    let supervisor = manager.sidecar_supervisor();
    let mut sup = supervisor.lock().await;
    let session_id = "real-oracle-query";
    let opened = sup
        .open_session(
            "rds.driver.oracle-rust",
            "oracle",
            session_id,
            params,
            Instant::now(),
        )
        .await
        .unwrap_or_else(|e| panic!("真实会话打不开：{e}"));
    assert!(matches!(opened, SessionOpened::Live { .. }), "会话应当直接开好：{opened:?}");

    let (rows, columns) = {
        let driver = SessionDriver::new(
            sup.session_conn(session_id).expect("会话应当有连接"),
            session_id,
        );
        let page = driver
            .execute(&QueryRequest::new("real-oracle-1", "select 1 as n from dual"))
            .await
            .unwrap_or_else(|e| panic!("真库查询失败：{e}"));
        (page.rows_here(), page.columns.len())
    };
    assert_eq!(columns, 1, "`select 1 as n from dual` 应当回一列");
    assert_eq!(rows, 1, "真库查询应当回一行");

    let _ = sup.close_session(session_id, Instant::now()).await;
    sup.shutdown_all(Instant::now(), Duration::from_secs(10)).await;
}

/// 真机链路（SQL Server）：与 Oracle 用例同一套宿主路径，凭据到位后一条命令验收。
///
/// 连接串形如 `mssql://user:pass@host:1433/master`。未设置 `RDS_TEST_MSSQL_URL`
/// 时跳过，不把「没有凭据」伪装成通过。
#[tokio::test]
async fn mssql_plugin_serves_a_real_query() {
    let Ok(url) = std::env::var("RDS_TEST_MSSQL_URL") else {
        eprintln!("跳过：未设置 RDS_TEST_MSSQL_URL");
        return;
    };
    let Some(params) = mssql_params(&url) else {
        eprintln!("跳过：RDS_TEST_MSSQL_URL 解析失败（期望 mssql://user:pass@host:port/database）");
        return;
    };

    let mssql_dir = plugin_dir("rds.driver.mssql-rust");
    if !mssql_dir.join("plugin.toml").exists() {
        eprintln!("SQL Server 插件包未安装，跳过本用例");
        return;
    }

    let manager = PluginManager::new(PathBuf::new());
    manager
        .load_plugin_from_dir(&mssql_dir)
        .await
        .expect("SQL Server 插件应当能加载");
    manager
        .activate_plugin("rds.driver.mssql-rust")
        .await
        .expect("SQL Server 插件应当能激活");

    let supervisor = manager.sidecar_supervisor();
    let mut sup = supervisor.lock().await;
    let session_id = "real-mssql-query";
    let opened = sup
        .open_session(
            "rds.driver.mssql-rust",
            "mssql",
            session_id,
            params,
            Instant::now(),
        )
        .await
        .unwrap_or_else(|e| panic!("真实会话打不开：{e}"));
    assert!(matches!(opened, SessionOpened::Live { .. }), "会话应当直接开好：{opened:?}");

    let (rows, columns) = {
        let driver = SessionDriver::new(
            sup.session_conn(session_id).expect("会话应当有连接"),
            session_id,
        );
        let page = driver
            .execute(&QueryRequest::new("real-mssql-1", "select 1 as n"))
            .await
            .unwrap_or_else(|e| panic!("真库查询失败：{e}"));
        (page.rows_here(), page.columns.len())
    };
    assert_eq!(columns, 1, "`select 1 as n` 应当回一列");
    assert_eq!(rows, 1, "真库查询应当回一行");

    let _ = sup.close_session(session_id, Instant::now()).await;
    sup.shutdown_all(Instant::now(), Duration::from_secs(10)).await;
}

/// `oracle://user:pass@host:port/service` → `session.open` 参数。
fn oracle_params(url: &str) -> Option<Value> {
    let rest = url.strip_prefix("oracle://")?;
    let (userinfo, tail) = rest.rsplit_once('@')?;
    let (username, password) = userinfo.split_once(':')?;
    let (host_port, database) = tail.split_once('/')?;
    let (host, port) = host_port.split_once(':')?;
    Some(json!({
        "host": host,
        "port": port.parse::<u16>().ok()?,
        "database": database,
        "username": username,
        "password": password,
        "connect_timeout": 10,
    }))
}

/// `mssql://user:pass@host:port/database` → `session.open` 参数。
///
/// 与 Oracle 的唯一差别是库名（SQL Server 的 `database`），其余键一致。
fn mssql_params(url: &str) -> Option<Value> {
    let rest = url.strip_prefix("mssql://")?;
    let (userinfo, tail) = rest.rsplit_once('@')?;
    let (username, password) = userinfo.split_once(':')?;
    let (host_port, database) = tail.split_once('/')?;
    let (host, port) = host_port.split_once(':')?;
    Some(json!({
        "host": host,
        "port": port.parse::<u16>().ok()?,
        "database": database,
        "username": username,
        "password": password,
        "connect_timeout": 10,
    }))
}


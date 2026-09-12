//! 连接作用域可见性与运行态测试（M3 + M4 衔接）
//!
//! - `load_connections_for_scope`：未打开项目仅全局可见；打开项目后合并 P_/GP_；
//! - `connected` 字段来自连接管理器运行态（不再冒充记录有效性 is_active）。
//!
//! 单例按进程只可设置一次：本文件启动时注入临时全局库（与其余测试二进制隔离）。

use std::path::PathBuf;
use std::sync::OnceLock;

use connection::model::{ConnectionScope, DataSourceSaveInput};
use engine::persistence::global_db::GlobalDatabaseManager;
use rds_workbench::services::data_source_service::DataSourceService;
use rds_workbench::services::workspace_loader::load_connections_for_scope;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 注入临时全局库到应用单例（进程内一次）。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_scope_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let manager = rt
                .block_on(GlobalDatabaseManager::new(
                    dir.join("global.db"),
                    dir.join("analytics.duckdb"),
                    2,
                ))
                .expect("init global db");
            engine::migration::install_global_db_manager(manager).expect("inject singleton");
            std::mem::forget(rt);
            dir
        })
        .clone()
}

fn sqlite_input(name: &str, path: &str) -> DataSourceSaveInput {
    let mut input = DataSourceSaveInput::new(name, "sqlite", &format!("sqlite://{path}"));
    input.use_duckdb_fed = Some(false);
    input
}

#[test]
fn loader_degrades_on_non_project_root_without_creating_dirs() {
    let base = base_dir();
    let bogus = base.join("not-a-project");
    std::fs::create_dir_all(&bogus).expect("mkdir bogus");

    // 非项目根：不给项目侧连接，也不能在磁盘上造出 `.RSmeta` 骨架（脏数据）。
    let (items, notice) = load_connections_for_scope(Some(&bogus));
    assert!(notice.is_some(), "非项目根应给出降级提示");
    assert!(
        !items.iter().any(|i| i.id.starts_with('P')),
        "不应混入项目侧条目"
    );
    assert!(
        !bogus.join(".RSmeta").exists(),
        "加载器不得创建项目骨架：{}",
        bogus.display()
    );

    let _ = std::fs::remove_dir_all(&bogus);
}

#[test]
fn scope_visibility_and_runtime_state() {
    let base = base_dir();
    let project_root = base.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");

    let service = DataSourceService::global()
        .expect("单例可用")
        .with_analysis_db(base.join("secret-target.duckdb"));
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let root_str = project_root.to_string_lossy().to_string();

    // 三种作用域各落一条。
    rt.block_on(service.save(&sqlite_input("only_global", "/tmp/g.db"), None))
        .expect("save global");
    let mut gp = sqlite_input("both_scopes", "/tmp/b.db");
    gp.scope = ConnectionScope::GlobalAndProject;
    let gp_id = rt
        .block_on(service.save(&gp, Some(&root_str)))
        .expect("save global+project");
    let mut p = sqlite_input("only_project", "/tmp/p.db");
    p.scope = ConnectionScope::Project;
    let p_id = rt
        .block_on(service.save(&p, Some(&root_str)))
        .expect("save project");

    // 未打开项目：仅全局可见（项目侧不可见）。
    let (items, notice) = load_connections_for_scope(None);
    assert!(notice.is_none(), "{notice:?}");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&"G_conn_only_global"), "{ids:?}");
    assert!(
        !ids.iter().any(|id| id.starts_with("P_") || id.starts_with("GP_")),
        "未打开项目不应出现项目侧连接：{ids:?}"
    );

    // 打开项目：全局 + P_ + GP_ 合并可见。
    let (items, notice) = load_connections_for_scope(Some(&project_root));
    assert!(notice.is_none(), "{notice:?}");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&"G_conn_only_global"), "{ids:?}");
    assert!(ids.contains(&gp_id.as_str()), "{ids:?}");
    assert!(ids.contains(&p_id.as_str()), "{ids:?}");

    // 运行态：尚未连接 → 全部 false。
    assert!(items.iter().all(|i| !i.connected), "无运行时连接应为 false");

    // 通过连接管理器注册一个真实 SQLite 运行态连接（ID 与记录一致）。
    // 单例测试进程需先注册内置驱动。
    engine::driver::auto_register::AutoDriverRegistrar::register_builtin_drivers();
    let sqlite_file = base.join("runtime.db");
    // SQLite 工厂以 `config.database` 作为文件路径。
    let config = engine::driver::registry::DriverConnectionConfig::new("sqlite")
        .with_database(sqlite_file.display().to_string());
    let db = rt.block_on(async {
        let factory = engine::driver::registry::DriverRegistry::get("sqlite").expect("sqlite driver");
        factory.create(config.clone()).await.expect("create sqlite db")
    });
    let info = engine::connection_manager::ConnectionInfo {
        id: "G_conn_only_global".to_string(),
        name: "only_global".to_string(),
        db_type: "sqlite".to_string(),
        url: String::new(),
        server_version: None,
        connection_type: engine::connection_manager::ConnectionType::Global,
        project_id: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
        created_at: std::time::Instant::now(),
    };
    rt.block_on(engine::get_connection_manager().add_connection(
        "G_conn_only_global".to_string(),
        db,
        info,
        config,
    ))
    .expect("register runtime connection");

    // 运行态填充：仅已注册的连接为 true。
    let (items, notice) = load_connections_for_scope(Some(&project_root));
    assert!(notice.is_none(), "{notice:?}");
    let by_id = |id: &str| items.iter().find(|i| i.id == id).cloned();
    assert!(
        by_id("G_conn_only_global").expect("exists").connected,
        "已注册连接应为已连接"
    );
    assert!(
        !by_id(&gp_id).expect("exists").connected,
        "未连接的项目快照应为 false"
    );
}

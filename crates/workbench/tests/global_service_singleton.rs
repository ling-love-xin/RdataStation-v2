//! 全局单例生产路径集成测试（M3 数据源连接）
//!
//! 覆盖 `install_global_db_manager` 之后的生产分支：
//! - `DataSourceService::global()`（其余连接测试均因单例未初始化而走 `new(&'static)` 注入）；
//! - `workspace_loader::load_persisted_connections()` 的单例优先分支（不再按路径降级）。
//!
//! 单例按进程只可设置一次，故断言集中在单个测试内；
//! 临时全局库与 Secret 目标库均隔离在临时目录。

use std::path::PathBuf;
use std::sync::OnceLock;

use connection::model::DataSourceSaveInput;
use engine::persistence::global_db::GlobalDatabaseManager;
use rds_workbench::services::data_source_service::DataSourceService;
use rds_workbench::services::workspace_loader::load_persisted_connections;

static SINGLETON_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 注入临时全局库到应用单例（进程内一次），返回临时目录。
fn ensure_singleton() -> PathBuf {
    SINGLETON_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_singleton_{}", std::process::id()));
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
            // 运行时常驻：避免连接池依赖的后台上下文随初始化结束被取消。
            std::mem::forget(rt);
            dir
        })
        .clone()
}

#[test]
fn singleton_service_and_loader_production_path() {
    let dir = ensure_singleton();

    // 生产路径①：单例已就绪，`global()` 可用。
    assert!(
        engine::migration::get_global_db_manager().is_some(),
        "单例应已注入"
    );
    let service = DataSourceService::global()
        .expect("单例全局库可用")
        .with_analysis_db(dir.join("secret-target.duckdb"));

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let mut input = DataSourceSaveInput::new(
        "singleton_demo",
        "postgres",
        "postgres://u:p@127.0.0.1:5432/singleton",
    );
    input.use_duckdb_fed = Some(true);

    let id = rt
        .block_on(service.save(&input, None))
        .expect("save via singleton");
    assert!(id.starts_with("G_conn_"), "{id}");

    // 生产路径②：列表加载走单例优先分支。
    let (items, notice) = load_persisted_connections();
    assert!(notice.is_none(), "{notice:?}");
    assert!(
        items.iter().any(|i| i.id == id && i.name == "singleton_demo"),
        "单例分支应读到新连接：{items:?}"
    );

    // Secret 联动落在隔离目标库。
    let mgr = connection::secret::SecretManager::open_with_dir(
        dir.join("secret-target.duckdb"),
        Some(&dir.join("secrets")),
    )
    .expect("open secret db");
    let secrets = mgr.list().expect("list");
    assert_eq!(secrets.len(), 1, "{secrets:?}");
    assert_eq!(secrets[0].name, id.to_lowercase());
    drop(mgr);

    // 删除后列表同步消失，Secret 一并清理。
    let result = rt
        .block_on(service.delete(&id, None))
        .expect("delete via singleton");
    assert!(result.removed_secret, "{result:?}");
    let (items, notice) = load_persisted_connections();
    assert!(notice.is_none(), "{notice:?}");
    assert!(
        !items.iter().any(|i| i.id == id),
        "删除后不应再出现：{items:?}"
    );
}

#[test]
fn singleton_reinstall_is_rejected() {
    // 单例已注入（或已由上一个测试注入）→ 再次注入必须返回错误。
    let dir = ensure_singleton();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let another = rt
        .block_on(GlobalDatabaseManager::new(
            dir.join("global2.db"),
            dir.join("analytics2.duckdb"),
            2,
        ))
        .expect("init second db");
    let reinstall = engine::migration::install_global_db_manager(another);
    assert!(reinstall.is_err(), "重复注入应被拒绝");
}

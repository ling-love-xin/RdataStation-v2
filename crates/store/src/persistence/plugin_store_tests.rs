//! 插件存储的 manager 级集成测试（真实 SQLite + DuckDB 临时库）。
//!
//! 2026-10-16 复活：本文件此前用不存在的 `created_at` 字段、`GlobalDatabaseManager::new` 也还是
//! 旧签名，且没有在 `persistence/mod.rs` 注册 —— 从未被编译过。现在按现行 API 重写，
//! 覆盖 `global_db.rs` 的异步包装（`plugin_store` 内的纯函数已另有内联单测）。

use std::fs;
use std::path::PathBuf;

use uuid::Uuid;

use crate::persistence::global_db::GlobalDatabaseManager;
use crate::persistence::plugin_store::{Plugin, PluginDependency, PluginGlobalConfig};

/// 临时目录 + 已经跑完迁移的全局库；返回后用完调用 `finish`。
async fn create_test_global_store(tag: &str) -> (GlobalDatabaseManager, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "rds-plugin-store-{tag}-{}",
        Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    let sqlite_path = dir.join("global.db");
    let duckdb_path = dir.join("analytics.duckdb");

    // `new` 自己会跑 `init_sqlite_tables` / `init_duckdb_tables`（迁移 001 含 plugins 表）
    let manager = GlobalDatabaseManager::new(sqlite_path, duckdb_path, 2)
        .await
        .expect("create global manager");
    (manager, dir)
}

async fn finish(manager: GlobalDatabaseManager, dir: PathBuf) {
    manager.close().await.expect("close manager");
    let _ = fs::remove_dir_all(&dir);
}

fn plugin(code: &str, version: &str, is_enabled: bool) -> Plugin {
    let now = chrono::Utc::now().to_rfc3339();
    Plugin {
        id: Uuid::new_v4().to_string(),
        code: code.to_string(),
        name: format!("Plugin {code}"),
        version: version.to_string(),
        author: Some("Test Author".to_string()),
        description: Some("A test plugin".to_string()),
        repo_url: None,
        plugin_type: "wasm".to_string(),
        manifest_json: Some("{}".to_string()),
        install_path: format!("/test/{code}"),
        is_enabled,
        is_builtin: false,
        installed_at: now.clone(),
        updated_at: now,
    }
}

#[tokio::test]
async fn register_then_read_back_by_uuid_and_by_code_version() {
    let (manager, dir) = create_test_global_store("read-back").await;
    let p = plugin("test.plugin", "1.0.0", true);

    manager.register_plugin(&p).await.expect("register plugin");

    let fetched = manager
        .get_plugin(&p.id)
        .await
        .expect("get plugin")
        .expect("row exists");
    assert_eq!(fetched.code, p.code);
    assert_eq!(fetched.name, p.name);

    let by_code = manager
        .get_plugin_by_code_version(&p.code, &p.version)
        .await
        .expect("get by code");
    assert!(by_code.is_some());

    finish(manager, dir).await;
}

#[tokio::test]
async fn list_returns_every_registered_plugin() {
    let (manager, dir) = create_test_global_store("list").await;
    for i in 0..3 {
        manager
            .register_plugin(&plugin(&format!("test.plugin{i}"), "1.0.0", i % 2 == 0))
            .await
            .expect("register");
    }

    let all = manager.get_all_plugins().await.expect("get all");
    assert_eq!(all.len(), 3);

    finish(manager, dir).await;
}

#[tokio::test]
async fn update_enabled_by_uuid_and_by_code_both_land() {
    let (manager, dir) = create_test_global_store("enable").await;
    let p = plugin("test.plugin", "1.0.0", true);
    manager.register_plugin(&p).await.expect("register");

    manager
        .update_plugin_enabled(&p.id, false)
        .await
        .expect("update by uuid");
    let fetched = manager
        .get_plugin(&p.id)
        .await
        .expect("get")
        .expect("row exists");
    assert!(!fetched.is_enabled);

    // 运行时用的是清单 id（= code），这条必须也能命中（此前按 uuid 写会 0 行）
    manager
        .update_plugin_enabled_by_code(&p.code, true)
        .await
        .expect("update by code");
    let fetched = manager
        .get_plugin_by_code(&p.code)
        .await
        .expect("get by code")
        .expect("row exists");
    assert!(fetched.is_enabled);

    finish(manager, dir).await;
}

#[tokio::test]
async fn delete_by_uuid_and_by_code_remove_the_row() {
    let (manager, dir) = create_test_global_store("delete").await;
    let by_uuid = plugin("test.uuid", "1.0.0", true);
    let by_code = plugin("test.code", "1.0.0", true);
    manager.register_plugin(&by_uuid).await.expect("register");
    manager.register_plugin(&by_code).await.expect("register");

    manager
        .delete_plugin(&by_uuid.id)
        .await
        .expect("delete by uuid");
    assert!(
        manager
            .get_plugin(&by_uuid.id)
            .await
            .expect("get")
            .is_none()
    );

    manager
        .delete_plugin_by_code(&by_code.code)
        .await
        .expect("delete by code");
    assert!(
        manager
            .get_plugin_by_code(&by_code.code)
            .await
            .expect("get by code")
            .is_none()
    );

    finish(manager, dir).await;
}

#[tokio::test]
async fn global_config_round_trips() {
    let (manager, dir) = create_test_global_store("config").await;
    let plugin_id = Uuid::new_v4().to_string();

    let config = PluginGlobalConfig {
        plugin_id: plugin_id.clone(),
        key: "test.key".to_string(),
        value: Some("test.value".to_string()),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };

    manager
        .set_plugin_global_config(&config)
        .await
        .expect("set config");

    let configs = manager
        .get_plugin_global_configs(&plugin_id)
        .await
        .expect("get configs");
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].key, "test.key");
    assert_eq!(configs[0].value, Some("test.value".to_string()));

    finish(manager, dir).await;
}

/// 依赖行必须跟插件登记一起落库，并在同 code 重装时跟着旧行一起换掉。
/// 只登记插件不登记依赖，「被依赖不让卸载」的反向检查就永远查不到引用者。
#[tokio::test]
async fn dependencies_are_registered_and_replaced_with_the_plugin_row() {
    let (manager, dir) = create_test_global_store("deps").await;
    let p = plugin("test.consumer", "1.0.0", true);
    let dep = PluginDependency {
        plugin_id: p.id.clone(),
        dep_code: "test.base".to_string(),
        dep_version_range: "1.0.0".to_string(),
        is_optional: false,
    };

    manager
        .register_plugin_with_dependencies(&p, std::slice::from_ref(&dep))
        .await
        .expect("register with deps");
    let deps = manager
        .get_plugin_dependencies(&p.id)
        .await
        .expect("get deps");
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].dep_code, "test.base");

    // 同 code 换新版本重装（清单不再依赖任何插件）：旧 uuid 的依赖行必须消失。
    let reinstalled = plugin("test.consumer", "2.0.0", true);
    manager
        .register_plugin_with_dependencies(&reinstalled, &[])
        .await
        .expect("reinstall without deps");
    assert!(
        manager
            .get_plugin_dependencies(&p.id)
            .await
            .expect("get old deps")
            .is_empty(),
        "旧安装实例的依赖行应随旧插件行一起清掉"
    );
    assert!(
        manager
            .get_plugin_dependencies(&reinstalled.id)
            .await
            .expect("get new deps")
            .is_empty()
    );

    finish(manager, dir).await;
}

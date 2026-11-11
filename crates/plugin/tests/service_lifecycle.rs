//! service 生命周期入口与全局管理器接线（单独测试进程，便于初始化全局单例）
//!
//! 只覆盖一条：`PluginService::enable_plugin` 在管理器已初始化时交给 manager，
//! 激活失败必须原样返回，不能写库后吞掉错误。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rds_plugin::manager::{LoadStatus, PluginManager};
use rds_plugin::{PluginService, get_plugin_manager, init_plugin_manager};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rds-plugin-service-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 可解析、可登记，但 sidecar 二进制不存在的插件：扫描能进索引，激活必失败。
fn write_broken_sidecar(dir: &Path, id: &str) {
    let plugin_dir = dir.join(id);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        format!(
            r#"[plugin]
id = "{id}"
name = "{id}"
version = "1.0.0"
publisher = "test"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
runtime = "rust"
executable = "bin/missing"
transport = "framed"
protocol = "rds-driver/1"
platforms = ["win-x64"]
max_instances = 1

[permissions]
sidecar = ["spawn.child_process"]

[capabilities.driver]
concurrency = "serial"
"#
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn service_enable_propagates_activation_failure() {
    if engine::migration::get_global_db_manager().is_none() {
        engine::migration::initialize_global_system()
            .await
            .expect("初始化全局库");
    }

    let dir = TempDir::new("enablefail");
    write_broken_sidecar(&dir.0, "com.example.serviceenablefail");
    init_plugin_manager(dir.0.clone())
        .await
        .expect("初始化全局插件管理器");

    let manager: &Arc<PluginManager> = get_plugin_manager().expect("管理器已初始化");
    manager.scan_and_diff_plugins().await.unwrap();
    assert!(manager.is_loaded("com.example.serviceenablefail"));
    assert!(matches!(
        manager
            .get_loaded_plugin("com.example.serviceenablefail")
            .unwrap()
            .status,
        LoadStatus::ActivationFailed(_)
    ));

    let db = engine::migration::get_global_db_manager().expect("全局库已初始化");
    let service = PluginService::new(db);
    let err = service
        .enable_plugin("com.example.serviceenablefail")
        .await
        .expect_err("激活失败必须返回错误");
    assert!(!format!("{err}").is_empty(), "{err}");

    // 与 manager 同口径：DB 标记已启用，失败留给面板显示“启用但激活失败”，
    // 用户可重试；这里只钉住 service 不再把失败吞掉。
    let row = service
        .get_plugin_by_code("com.example.serviceenablefail")
        .await
        .unwrap()
        .expect("登记行存在");
    assert!(row.is_enabled);
}

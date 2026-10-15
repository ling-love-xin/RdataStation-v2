//! 插件生命周期端到端（热插拔 diff / 磁盘卸载 / 启停 / 全量重载）
//!
//! 不依赖真 sidecar 进程：插件用 frontend 形态摆（无进程依赖），临时安装目录
//! 随测试 Drop 清理。覆盖 P4 生命周期的四条主线：
//!
//! - 热插拔：目录出现 → 自动加载激活；目录消失 → 自动停用移除；清单变化 → needs_reload
//! - 磁盘卸载：目录删除 + 索引清理 + 事件（依赖拒绝走真实 DB，不在本文件 —— 见 #20 注记）
//! - 启停：disable 停用 + 事件；enable 重新激活 + 事件
//! - 全量重载：清快照后重扫，全部插件重新加载激活

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rds_plugin::events::{get_event_manager, PluginEvent};
use rds_plugin::manager::{PluginDiff, PluginManager};

/// 一次性临时安装目录（Drop 时清掉，不留 .rds/tmp 外的过程产物）
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rds-plugin-lifecycle-{tag}-{}",
            std::process::id()
        ));
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

/// 摆一个最小 frontend 插件目录（无进程依赖，热插拔/启停测试专用）。
///
/// `marker` 写进清单注释里：改它就能让内容 hash 变化（测 needs_reload）。
fn write_plugin(dir: &Path, id: &str, marker: &str) {
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

[capabilities.frontend]
entry = "main.js"

# {marker}
"#
        ),
    )
    .unwrap();
    std::fs::write(plugin_dir.join("main.js"), "// stub").unwrap();
}

/// 收广播直到出现 `pred` 命中的事件（低频生命周期事件，宽容超时）。
///
/// `rx` 必须在触发动作**之前**订阅：broadcast 不重放订阅前的旧消息。
fn recv_matching<F>(rx: &mut tokio::sync::broadcast::Receiver<PluginEvent>, pred: F) -> PluginEvent
where
    F: Fn(&PluginEvent) -> bool,
{
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match rx.try_recv() {
            Ok(event) if pred(&event) => return event,
            Ok(_) => {}
            Err(_) => {}
        }
        assert!(
            std::time::Instant::now() < deadline,
            "等待插件事件超时"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 热插拔：空目录 → 摆插件（出现）→ 删目录（消失）→ 改清单（变化置重载）。
#[tokio::test]
async fn hot_plug_adds_removes_and_marks_reload() {
    let dir = TempDir::new("hotplug");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    // 空目录：diff 为空
    assert_eq!(manager.scan_and_diff_plugins().await.unwrap(), PluginDiff::default());

    // 目录出现 = 插件出现：自动加载并激活（frontend 激活只记状态）
    write_plugin(&dir.0, "com.example.hot", "v1");
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.added, vec!["com.example.hot"]);
    assert!(manager.get_loaded_plugin("com.example.hot").is_some());

    // 清单内容变化：置 needs_reload，不热生效
    write_plugin(&dir.0, "com.example.hot", "v2-changed");
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.changed, vec!["com.example.hot"]);
    assert_eq!(manager.pending_reloads(), vec!["com.example.hot"]);

    // 目录消失 = 插件消失：停用 + 移出运行时索引与安装态缓存
    std::fs::remove_dir_all(dir.0.join("com.example.hot")).unwrap();
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.removed, vec!["com.example.hot"]);
    assert!(manager.get_loaded_plugin("com.example.hot").is_none());
    // 变化标记随插件消失一并清掉（否则面板会提示一个已不存在的插件要重载）
    assert!(manager.pending_reloads().is_empty());
}

/// 磁盘卸载：目录删除 + 索引清理 + PluginUninstalled 事件。
#[tokio::test]
async fn uninstall_removes_dir_and_index_and_emits() {
    let dir = TempDir::new("uninstall");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    write_plugin(&dir.0, "com.example.bye", "v1");
    manager.scan_and_diff_plugins().await.unwrap();
    assert!(dir.0.join("com.example.bye").exists());

    // 订阅必须在动作前（broadcast 不重放订阅前的旧消息）
    let mut rx = get_event_manager().subscribe();
    manager.uninstall_plugin("com.example.bye").await.unwrap();

    assert!(
        !dir.0.join("com.example.bye").exists(),
        "插件目录要删掉"
    );
    assert!(manager.get_loaded_plugin("com.example.bye").is_none());
    let event = recv_matching(&mut rx, |e| {
        matches!(e, PluginEvent::PluginUninstalled { plugin_id } if plugin_id == "com.example.bye")
    });
    assert!(matches!(event, PluginEvent::PluginUninstalled { .. }));

    // 卸载后重复卸载：目录已不存在也幂等成功（VS Code 卸载已卸载扩展同样无操作）
    manager.uninstall_plugin("com.example.bye").await.unwrap();
}

/// 卸载 id 不合法（拼路径前的安全校验）必须被拒，不能碰磁盘。
#[tokio::test]
async fn uninstall_rejects_an_unsafe_id() {
    let dir = TempDir::new("unsafeid");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    let err = manager.uninstall_plugin("../../evil").await.unwrap_err();
    assert!(format!("{err}").contains("不合法"), "{err}");
}

/// 启停：disable 停用 + 事件；enable 重新激活 + 事件（DB 未初始化时持久化跳过，
/// 动作照走）。
#[tokio::test]
async fn disable_then_enable_roundtrips() {
    let dir = TempDir::new("toggledisable");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    write_plugin(&dir.0, "com.example.toggle", "v1");
    manager.scan_and_diff_plugins().await.unwrap();
    assert!(manager.get_loaded_plugin("com.example.toggle").is_some());

    // 先订阅后动作（broadcast 不重放旧消息）
    let mut rx = get_event_manager().subscribe();
    manager.disable_plugin("com.example.toggle").await.unwrap();
    let event = recv_matching(&mut rx, |e| {
        matches!(e, PluginEvent::PluginDisabled { plugin_id } if plugin_id == "com.example.toggle")
    });
    assert!(matches!(event, PluginEvent::PluginDisabled { .. }));

    let mut rx = get_event_manager().subscribe();
    manager.enable_plugin("com.example.toggle").await.unwrap();
    let event = recv_matching(&mut rx, |e| {
        matches!(e, PluginEvent::PluginEnabled { plugin_id } if plugin_id == "com.example.toggle")
    });
    assert!(matches!(event, PluginEvent::PluginEnabled { .. }));
}

/// 全量重载（VS Code Reload Window）：清快照 → 重扫 → 全部插件重新加载激活。
#[tokio::test]
async fn reload_reloads_everything() {
    let dir = TempDir::new("reload");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    write_plugin(&dir.0, "com.example.one", "v1");
    write_plugin(&dir.0, "com.example.two", "v1");
    manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(manager.list_loaded_plugins().len(), 2);

    // 先制造一个 needs_reload（模拟运行中清单被换过）
    write_plugin(&dir.0, "com.example.one", "v2");
    manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(manager.pending_reloads().len(), 1);

    // 重载后：重载标记清空，两个插件重新在运行时（快照清空 → 全部走「新增」）
    let diff = manager.reload_plugins().await.unwrap();
    assert_eq!(diff.added.len(), 2, "{diff:?}");
    assert!(manager.pending_reloads().is_empty());
    assert_eq!(manager.list_loaded_plugins().len(), 2);
    assert!(manager.get_loaded_plugin("com.example.one").is_some());
    assert!(manager.get_loaded_plugin("com.example.two").is_some());
}

/// 没有安装目录（空 install_dir，未配置）时扫描是空操作，不 panic。
#[tokio::test]
async fn scan_without_install_dir_is_a_noop() {
    let manager = Arc::new(PluginManager::new(PathBuf::new()));
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff, PluginDiff::default());
}

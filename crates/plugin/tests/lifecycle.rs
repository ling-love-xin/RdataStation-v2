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

use rds_plugin::events::{PluginEvent, get_event_manager};
use rds_plugin::manager::{PluginDiff, PluginInfo, PluginManager};
use rds_plugin::PluginService;

/// 一次性临时安装目录（Drop 时清掉，不留 .rds/tmp 外的过程产物）
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rds-plugin-lifecycle-{tag}-{}", std::process::id()));
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
    write_plugin_with_contributes(dir, id, marker, "");
}

/// 同 [`write_plugin`]，额外拼一段贡献点声明（注册表用例用）。
fn write_plugin_with_contributes(dir: &Path, id: &str, marker: &str, contributes: &str) {
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

{contributes}
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
        assert!(std::time::Instant::now() < deadline, "等待插件事件超时");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 带 `checksums.json` 的包在**加载时**也要校验：安装期校验的是解压产物，
/// 装好之后被改（热插拔直接拷目录、手改文件）同样必须被拦住。
#[tokio::test]
async fn load_rejects_a_package_whose_checksums_no_longer_match() {
    let dir = TempDir::new("tampered");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();
    write_plugin(&dir.0, "com.example.tampered", "v1");

    let checksums = r#"{
        "algorithm": "sha256",
        "files": {
            "plugin.toml": "0000000000000000000000000000000000000000000000000000000000000000",
            "main.js": "0000000000000000000000000000000000000000000000000000000000000000"
        }
    }"#;
    std::fs::write(
        dir.0.join("com.example.tampered").join("checksums.json"),
        checksums,
    )
    .unwrap();

    let err = manager
        .load_plugin_from_dir(&dir.0.join("com.example.tampered"))
        .await
        .expect_err("哈希不匹配的包不应加载成功");
    let message = format!("{err}");
    assert!(
        message.contains("完整性校验失败"),
        "错误信息应点明完整性校验：{message}"
    );

    // 校验失败的包仍要留在会话缓存里，面板才看得到「出错」并能直接卸载；
    // 早期实现直接 return，坏包从列表里消失，用户只剩手删目录一条路。
    let listed = manager.list_plugins().unwrap();
    let entry = listed
        .iter()
        .find(|p| p.id == "com.example.tampered")
        .expect("校验失败的包应仍出现在列表里");
    assert_eq!(entry.state, rds_plugin::wasm::PluginState::Error);
    assert!(
        entry
            .error
            .as_deref()
            .unwrap_or("")
            .contains("完整性校验失败"),
        "列表行应带上失败原因：{:?}",
        entry.error
    );
}

/// 清单解析失败时，目录仍要留在列表里并带错误原因。
///
/// 这类目录连 `PluginManifest` 都建不出来，以前直接 `return Err`，面板列表里
/// 没有这一行；用户只能去磁盘上猜哪个目录坏了。
#[tokio::test]
async fn a_plugin_with_a_broken_manifest_stays_in_the_list_with_the_reason() {
    let dir = TempDir::new("broken-manifest");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let broken = dir.0.join("com.example.broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.toml"), "this is not = [valid toml").unwrap();

    let err = manager
        .load_plugin_from_dir(&broken)
        .await
        .expect_err("清单解析失败不应加载成功");
    assert!(!format!("{err}").is_empty());

    let listed = manager.list_plugins().unwrap();
    let entry = listed
        .iter()
        .find(|p| p.id == "com.example.broken")
        .expect("解析失败的包应仍出现在列表里");
    assert_eq!(entry.state, rds_plugin::wasm::PluginState::Error);
    assert!(
        entry.error.as_deref().is_some_and(|e| !e.is_empty()),
        "列表行应带上解析失败原因：{:?}",
        entry.error
    );
}

/// 坏包也得能从面板卸载：按目录名删掉，并从错误行列表里消失。
#[tokio::test]
async fn a_package_with_a_broken_manifest_can_be_uninstalled_by_its_directory_name() {
    let dir = TempDir::new("broken-uninstall");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let broken = dir.0.join("com.example.broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.toml"), "this is not = [valid toml").unwrap();
    manager.load_plugin_from_dir(&broken).await.unwrap_err();

    let ids = |plugins: Vec<PluginInfo>| plugins.into_iter().map(|p| p.id).collect::<Vec<_>>();
    assert!(ids(manager.list_plugins().unwrap()).contains(&"com.example.broken".to_string()));

    manager
        .uninstall_plugin("com.example.broken")
        .await
        .expect("坏包应能按目录名卸载");
    assert!(!broken.exists(), "卸载后目录应被删除");
    assert!(
        !ids(manager.list_plugins().unwrap()).contains(&"com.example.broken".to_string()),
        "卸载后不该再留下错误行"
    );
}

/// 坏包不能被“启用”：没有可激活的运行时，持久标记也不该被翻成启用。
#[tokio::test]
async fn enable_is_refused_for_a_plugin_with_a_broken_manifest() {
    let dir = TempDir::new("broken-enable");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let broken = dir.0.join("com.example.broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.toml"), "this is not = [valid toml").unwrap();
    manager.load_plugin_from_dir(&broken).await.unwrap_err();

    let err = manager
        .enable_plugin("com.example.broken")
        .await
        .expect_err("清单损坏的包不应启用成功");
    assert!(
        format!("{err}").contains("清单损坏"),
        "启用失败应点明清单损坏：{err}"
    );
}

/// 目录名与清单 id 不一致时，按目录名留一行错误，面板才能按 id 删对目录。
#[tokio::test]
async fn a_plugin_whose_directory_name_disagrees_with_the_manifest_stays_in_the_list() {
    let dir = TempDir::new("dir-name-mismatch");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    write_plugin(&dir.0, "com.example.declared", "v1");
    let actual = dir.0.join("com.example.alias");
    std::fs::rename(dir.0.join("com.example.declared"), &actual).unwrap();

    let err = manager
        .load_plugin_from_dir(&actual)
        .await
        .expect_err("目录名与清单 id 不一致不应加载成功");
    assert!(
        format!("{err}").contains("目录名与清单 id 不一致"),
        "错误信息应点明目录名不一致：{err}"
    );

    let listed = manager.list_plugins().unwrap();
    let entry = listed
        .iter()
        .find(|p| p.id == "com.example.alias")
        .expect("目录名不一致的包应仍出现在列表里");
    assert_eq!(entry.state, rds_plugin::wasm::PluginState::Error);
    assert!(
        entry
            .error
            .as_deref()
            .is_some_and(|e| e.contains("目录名与清单 id 不一致")),
        "列表行应带上目录名不一致的原因：{:?}",
        entry.error
    );
}

/// 两个插件声明同一个命令 id：第二个在加载期被拒绝，第一个保持可用。
///
/// 冲突若拖到查注册表时才报，两个插件都会显示「已加载」，宿主拿到的是整体错误。
/// 现在第二个插件以错误行留在列表（可看原因、可卸载），修好清单后能重新加载。
#[tokio::test]
async fn a_contribution_id_conflict_rejects_the_second_plugin() {
    use rds_plugin::manager::PluginKind;

    let dir = TempDir::new("contrib-conflict");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let command = r#"[[contributes.commands]]
id = "shared.cmd"
title = "共享命令"
"#;
    write_plugin_with_contributes(&dir.0, "com.example.a", "v1", command);
    write_plugin_with_contributes(&dir.0, "com.example.b", "v1", command);
    let plugin_a = dir.0.join("com.example.a");
    let plugin_b = dir.0.join("com.example.b");
    manager.load_plugin_from_dir(&plugin_a).await.unwrap();

    let error = manager.load_plugin_from_dir(&plugin_b).await.unwrap_err();
    let error = error.to_string();
    assert!(error.contains("命令贡献点 id 冲突"), "{error}");
    assert!(error.contains("shared.cmd"), "{error}");
    assert!(error.contains("com.example.a"), "{error}");
    assert!(error.contains("com.example.b"), "{error}");

    // 被拒的插件不进加载缓存，但以错误行留在列表：面板看得到原因、也能卸载。
    assert!(manager.get_loaded_plugin("com.example.b").is_none());
    let row = manager
        .list_plugins()
        .unwrap()
        .into_iter()
        .find(|plugin| plugin.id == "com.example.b")
        .expect("被拒插件应留在列表");
    assert_eq!(row.kind, PluginKind::Unknown);
    assert!(row.error.as_deref().is_some_and(|e| e.contains("贡献点")));

    // 注册表只剩先加载的插件，不再整体失败。
    let registry = manager.contributions().unwrap();
    assert_eq!(registry.commands().len(), 1);
    assert_eq!(registry.commands()[0].plugin_id, "com.example.a");

    // 同名插件重载不算冲突（排除自身，否则改完清单也永远装不回来）。
    manager.load_plugin_from_dir(&plugin_a).await.unwrap();

    // 改掉冲突 id 后第二个插件可正常加载，错误行被清掉。
    let unique = r#"[[contributes.commands]]
id = "unique.cmd"
title = "独立命令"
"#;
    write_plugin_with_contributes(&dir.0, "com.example.b", "v2-fixed", unique);
    manager.load_plugin_from_dir(&plugin_b).await.unwrap();
    let registry = manager.contributions().unwrap();
    assert_eq!(registry.commands().len(), 2);
    let row = manager
        .list_plugins()
        .unwrap()
        .into_iter()
        .find(|plugin| plugin.id == "com.example.b")
        .expect("修好后应恢复为正常行");
    assert!(row.error.is_none());

    // 卸载一侧后，另一侧的声明仍在。
    manager.uninstall_plugin("com.example.a").await.unwrap();
    let registry = manager.contributions().unwrap();
    assert_eq!(registry.commands().len(), 1);
    assert_eq!(registry.commands()[0].id, "unique.cmd");
    assert_eq!(registry.commands()[0].plugin_id, "com.example.b");
}

/// 冲突方卸载后，被拒插件可显式重试加载（不用重启应用）。
#[tokio::test]
async fn a_conflict_rejected_plugin_can_be_retried_after_the_other_is_uninstalled() {
    let dir = TempDir::new("contrib-retry");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let command = r#"[[contributes.commands]]
id = "shared.cmd"
title = "共享命令"
"#;
    write_plugin_with_contributes(&dir.0, "com.example.a", "v1", command);
    write_plugin_with_contributes(&dir.0, "com.example.b", "v1", command);
    manager
        .load_plugin_from_dir(&dir.0.join("com.example.a"))
        .await
        .unwrap();
    assert!(
        manager
            .load_plugin_from_dir(&dir.0.join("com.example.b"))
            .await
            .is_err()
    );

    manager.uninstall_plugin("com.example.a").await.unwrap();
    manager.retry_load_plugin("com.example.b").await.unwrap();

    let registry = manager.contributions().unwrap();
    assert_eq!(registry.commands().len(), 1);
    assert_eq!(registry.commands()[0].id, "shared.cmd");
    assert_eq!(registry.commands()[0].plugin_id, "com.example.b");
    let row = manager
        .list_plugins()
        .unwrap()
        .into_iter()
        .find(|plugin| plugin.id == "com.example.b")
        .expect("重试成功后应恢复为正常行");
    assert!(row.error.is_none());
}

/// 按插件统计贡献点：命令 / 面板 / 设置来自注册表，驱动来自清单。
///
/// 宿主面板据此显示「贡献 N」；驱动型插件不能因为驱动不在注册表里就显示 0。
#[tokio::test]
async fn contribution_counts_cover_all_four_kinds_per_plugin() {
    let dir = TempDir::new("contrib-counts");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let contributes = r#"
[[contributes.commands]]
id = "count.cmd.a"
title = "命令 A"

[[contributes.commands]]
id = "count.cmd.b"
title = "命令 B"

[[contributes.panels]]
id = "count.panel"
title = "面板"
location = "left"

[[contributes.settings]]
key = "count.setting"
type = "boolean"
default = false

[[contributes.drivers]]
id = "count.driver"
display_name = "计数驱动"
"#;
    write_plugin_with_contributes(&dir.0, "com.example.counts", "v1", contributes);
    manager
        .load_plugin_from_dir(&dir.0.join("com.example.counts"))
        .await
        .unwrap();

    let counts = manager.contribution_counts().unwrap();
    assert_eq!(counts.len(), 1, "只应有加载成功的插件");
    let counts = counts
        .get("com.example.counts")
        .expect("加载成功的插件必须有计数");
    assert_eq!(counts.commands, 2);
    assert_eq!(counts.panels, 1);
    assert_eq!(counts.settings, 1);
    assert_eq!(counts.drivers, 1);
    assert_eq!(counts.total(), 5);
}

/// 注册表输出按插件 id 排序，不受加载顺序（`HashMap` 遍历顺序）影响。
#[tokio::test]
async fn contributions_are_sorted_by_plugin_id() {
    let dir = TempDir::new("contrib-order");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    let command_a = "[[contributes.commands]]\nid = \"a.cmd\"\ntitle = \"A\"\n";
    let command_b = "[[contributes.commands]]\nid = \"b.cmd\"\ntitle = \"B\"\n";
    write_plugin_with_contributes(&dir.0, "com.example.b", "v1", command_b);
    write_plugin_with_contributes(&dir.0, "com.example.a", "v1", command_a);
    manager
        .load_plugin_from_dir(&dir.0.join("com.example.b"))
        .await
        .unwrap();
    manager
        .load_plugin_from_dir(&dir.0.join("com.example.a"))
        .await
        .unwrap();

    let registry = manager.contributions().unwrap();
    assert_eq!(
        registry
            .commands()
            .iter()
            .map(|command| command.plugin_id.as_str())
            .collect::<Vec<_>>(),
        vec!["com.example.a", "com.example.b"]
    );
}

/// 热插拔：空目录 → 摆插件（出现）→ 删目录（消失）→ 改清单（变化置重载）。
#[tokio::test]
async fn hot_plug_adds_removes_and_marks_reload() {
    let dir = TempDir::new("hotplug");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    // 空目录：diff 为空
    assert_eq!(
        manager.scan_and_diff_plugins().await.unwrap(),
        PluginDiff::default()
    );

    // 目录出现 = 插件出现：自动加载并激活（frontend 激活只记状态）
    write_plugin(&dir.0, "com.example.hot", "v1");
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.added, vec!["com.example.hot"]);
    assert!(manager.get_loaded_plugin("com.example.hot").is_some());

    // 清单内容变化：置 needs_reload，不热生效；并发出 PluginChanged 供面板
    // 立刻显示「重载 N」（此前只改内存标记、不发事件，面板订阅了也看不到）。
    let mut rx = get_event_manager().subscribe();
    write_plugin(&dir.0, "com.example.hot", "v2-changed");
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.changed, vec!["com.example.hot"]);
    assert_eq!(manager.pending_reloads(), vec!["com.example.hot"]);
    let event = recv_matching(&mut rx, |event| {
        matches!(
            event,
            PluginEvent::PluginChanged { plugin_id } if plugin_id == "com.example.hot"
        )
    });
    assert!(matches!(event, PluginEvent::PluginChanged { .. }));

    // 目录消失 = 插件消失：停用 + 移出运行时索引与安装态缓存
    std::fs::remove_dir_all(dir.0.join("com.example.hot")).unwrap();
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.removed, vec!["com.example.hot"]);
    assert!(manager.get_loaded_plugin("com.example.hot").is_none());
    // 变化标记随插件消失一并清掉（否则面板会提示一个已不存在的插件要重载）
    assert!(manager.pending_reloads().is_empty());
}

/// 面板读 `list_plugins`：运行中的插件必须报 `Active`。
///
/// 此前 sidecar / frontend 分支一律写死 `Inactive`，面板把正在跑的插件显示成「已停用」，
/// 行尾只给「启用」——用户在面板里根本停不掉一个运行中的插件。
#[tokio::test]
async fn list_reports_running_plugins_as_active() {
    use rds_plugin::wasm::PluginState;

    let dir = TempDir::new("liststate");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    write_plugin(&dir.0, "com.example.run", "v1");
    manager.scan_and_diff_plugins().await.unwrap();

    let state_of = |manager: &PluginManager| {
        manager
            .list_plugins()
            .unwrap()
            .into_iter()
            .find(|p| p.id == "com.example.run")
            .expect("插件应出现在清单里")
            .state
    };
    assert_eq!(state_of(&manager), PluginState::Active);

    manager.disable_plugin("com.example.run").await.unwrap();
    assert_eq!(state_of(&manager), PluginState::Inactive);

    manager.enable_plugin("com.example.run").await.unwrap();
    assert_eq!(state_of(&manager), PluginState::Active);
}

/// 停用被依赖的插件：API 入口自己查反向依赖并拒绝，不依赖面板缓存。
///
/// 面板缓存可能过期（插件刚装上 / 刚改清单 / 其它调用方直接走 manager），
/// 所以 `disable_plugin` 必须在写 `is_enabled` 前拦一次。
#[tokio::test]
async fn disable_rejects_a_plugin_with_a_dependent() {
    // 守卫读的是 global.db 的反向依赖；测试里得先初始化全局库，
    // 否则 `get_global_db_manager()` 为 None，只会测到「没有库就不拦」。
    engine::migration::initialize_global_system()
        .await
        .expect("初始化全局库");
    let dir = TempDir::new("disableguard");
    let manager = PluginManager::new(dir.0.clone());
    manager.init().await.unwrap();

    write_plugin(&dir.0, "com.example.base", "v1");
    write_plugin_with_contributes(
        &dir.0,
        "com.example.child",
        "v1",
        r#"[[dependencies]]
id = "com.example.base"
version = "^1.0.0"
"#,
    );
    manager.scan_and_diff_plugins().await.unwrap();

    let err = manager
        .disable_plugin("com.example.base")
        .await
        .expect_err("被其他插件依赖的插件不应允许停用");
    let message = format!("{err}");
    assert!(message.contains("被以下插件依赖"), "{message}");
    assert!(message.contains("com.example.child"), "{message}");

    // service 是另一条写库入口，同样不能绕过依赖守卫；被拒后 DB 必须保持启用。
    let db = engine::migration::get_global_db_manager().expect("全局库已初始化");
    let service = PluginService::new(db);
    let err = service
        .disable_plugin("com.example.base")
        .await
        .expect_err("service 入口也不应允许停用被依赖插件");
    assert!(format!("{err}").contains("com.example.child"));
    let row = service
        .get_plugin_by_code("com.example.base")
        .await
        .unwrap()
        .expect("base 已登记");
    assert!(row.is_enabled, "被拒的停用不能把 is_enabled 改成 0");

    // 登记行卸载同样要查依赖，否则会留下引用 base 的孤儿 child。
    let err = service
        .uninstall_plugin("com.example.base")
        .await
        .expect_err("service 卸载登记不应允许被依赖插件");
    assert!(format!("{err}").contains("com.example.child"));
    assert!(
        service
            .get_plugin_by_code("com.example.base")
            .await
            .unwrap()
            .is_some(),
        "被拒的卸载不能删掉登记行"
    );
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

    assert!(!dir.0.join("com.example.bye").exists(), "插件目录要删掉");
    assert!(manager.get_loaded_plugin("com.example.bye").is_none());
    let event = recv_matching(
        &mut rx,
        |e| matches!(e, PluginEvent::PluginUninstalled { plugin_id } if plugin_id == "com.example.bye"),
    );
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
    let event = recv_matching(
        &mut rx,
        |e| matches!(e, PluginEvent::PluginDisabled { plugin_id } if plugin_id == "com.example.toggle"),
    );
    assert!(matches!(event, PluginEvent::PluginDisabled { .. }));

    let mut rx = get_event_manager().subscribe();
    manager.enable_plugin("com.example.toggle").await.unwrap();
    let event = recv_matching(
        &mut rx,
        |e| matches!(e, PluginEvent::PluginEnabled { plugin_id } if plugin_id == "com.example.toggle"),
    );
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

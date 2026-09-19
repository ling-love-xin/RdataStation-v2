//! 工厂路径的真实进程端到端测试：从「注册」到「连上」再到「交还」
//!
//! 靶子同前，但这一次走的是**引擎的入口**：
//!
//! ```text
//! 清单 → register_sidecar_drivers → DriverRegistry（下拉里出现）
//!      → DriverRegistry::get(id).create(配置) → DynDatabase（连接面板拿到的就是它）
//!      → query / 丢掉句柄 → 会话交还
//! ```
//!
//! 这才是「插件驱动」对宿主真正的样子：连接面板与编辑器不必知道 sidecar 的存在。
//! 真库（PostgreSQL）那一步要真库在场，属实机验收（见 `plugin-dev-plan.md` §5 P1）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use engine::driver::DriverFactory;
use engine::driver::registry::{DriverConnectionConfig, DriverKind, DriverRegistry};
use rds_plugin::manifest::PluginManifest;
use rds_plugin::sidecar::factory::{SidecarDriverFactory, register_sidecar_drivers};
use rds_plugin::sidecar::supervisor::SidecarSupervisor;
use shared::error::{ConnectionError, CoreError};

/// 造一个"已安装插件"的目录：把靶子当插件二进制放进去，返回（插件目录, 清单）。
///
/// 走的是真路径（`paths::plugin_dir`）：清单里的 `executable` 是相对插件目录的，
/// 而这正是注册期要解析的东西。
fn install_fixture_plugin(plugin_id: &str, driver_id: &str) -> (PathBuf, PluginManifest) {
    let plugin_dir = paths::plugin_dir(plugin_id);
    let bin_dir = plugin_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("建插件目录");

    // Windows 上清单写 `bin/agent`，实际文件叫 `bin/agent.exe`（注册期会自动补后缀）
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture"));
    let target = if cfg!(windows) {
        bin_dir.join("agent.exe")
    } else {
        bin_dir.join("agent")
    };
    let _ = std::fs::remove_file(&target);
    if std::fs::hard_link(&fixture, &target).is_err() {
        std::fs::copy(&fixture, &target).expect("把靶子放进插件目录");
    }

    let toml = format!(
        r#"
[plugin]
id = "{plugin_id}"
name = "Fixture Driver"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/agent"
max_instances = 1

[[contributes.drivers]]
id = "{driver_id}"
display_name = "Fixture DB"
default_port = 15432
features = ["cancel"]

[capabilities.driver]
concurrency = "serial"
"#
    );
    let manifest: PluginManifest = toml::from_str(&toml).expect("清单应当能解析");
    (plugin_dir, manifest)
}

/// 等"没有活会话"（会话交还由后台任务做，得给它机会拿到锁）。
async fn wait_until_no_sessions(supervisor: &Arc<Mutex<SidecarSupervisor>>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        {
            let mut supervisor = supervisor.lock().await;
            supervisor.drain_events(Instant::now()).await;
            if supervisor.registry().session_count() == 0 {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

fn config_for(driver_id: &str) -> DriverConnectionConfig {
    let mut config = DriverConnectionConfig::new(driver_id);
    config.name = Some("本地靶子".to_string());
    config.host = Some("127.0.0.1".to_string());
    config.port = Some(15432);
    config.database = Some("demo".to_string());
    config.username = Some("u".to_string());
    config.password = Some("p".to_string());
    config
}

#[tokio::test]
async fn a_registered_driver_connects_and_queries_through_the_engine() {
    let (plugin_dir, manifest) = install_fixture_plugin("test.factory.basic", "fixture-basic");
    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));

    let registered = register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .expect("注册应当成功");
    assert_eq!(registered, vec!["fixture-basic".to_string()]);

    // 下拉里拿到的就是它：描述符来自清单（注册期就知道的那部分）
    let factory = DriverRegistry::get("fixture-basic").expect("注册表里应当有它");
    let descriptor = factory.descriptor();
    assert_eq!(descriptor.name, "Fixture DB");
    assert_eq!(descriptor.driver_kind, DriverKind::Sidecar);
    assert_eq!(descriptor.default_port, Some(15432));

    // 连接：这一步背后是「起进程 → 握手 → 开会话 → describe」
    let database = factory
        .create(config_for("fixture-basic"))
        .await
        .expect("连接应当成功");

    let result = database
        .query("select rows=3000")
        .await
        .expect("查询应当成功");
    assert_eq!(result.total_rows(), 3000, "3000 行应当经引擎的結果模型回来");
    assert_eq!(result.to_rows().len(), 3000);

    // 丢掉句柄 = 关连接：会话要交还给内核（否则它会一直占着那个串行实例）
    drop(database);

    // 等会话被交还：**每次只短暂持锁** —— 负责关会话的是一个后台任务，
    // 一直握着锁睡觉的话它永远拿不到锁（这条踩过）。
    assert!(
        wait_until_no_sessions(&supervisor).await,
        "句柄丢掉之后会话应当被交还"
    );

    supervisor
        .lock()
        .await
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// 串行驱动的第二条连接：排队 → 等不到就**明确失败**（而不是挂在那儿），
/// 第一条关掉之后第三条就能连上。
#[tokio::test]
async fn a_second_connection_queues_and_times_out_with_a_clear_error() {
    let (plugin_dir, manifest) = install_fixture_plugin("test.factory.queue", "fixture-queue");
    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));
    register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .expect("注册应当成功");

    // 直接建工厂：这条用例要一个很短的排队等待（默认 30s 会拖慢测试）
    let factory = SidecarDriverFactory::new(
        Arc::clone(&supervisor),
        "test.factory.queue",
        DriverRegistry::get("fixture-queue")
            .expect("注册表里应当有它")
            .descriptor(),
    )
    .with_queue_wait(Duration::from_millis(300));

    let first = factory
        .create(config_for("fixture-queue"))
        .await
        .expect("第一条连接应当成功");

    let error = match factory.create(config_for("fixture-queue")).await {
        Ok(_) => panic!("串行驱动第二条连接应当排队并超时"),
        Err(error) => error,
    };
    match error {
        CoreError::Connection(ConnectionError::Timeout { duration_ms, .. }) => {
            assert_eq!(duration_ms, 300);
        }
        other => panic!("排队超时应当进连接域：{other:?}"),
    }

    // 第一条关掉 → 第三条能连上（排队也随着被撤掉了）
    drop(first);
    assert!(
        wait_until_no_sessions(&supervisor).await,
        "第一条连接应当被交还"
    );

    let third = factory
        .create(config_for("fixture-queue"))
        .await
        .expect("第一条关掉之后应当连得上");
    assert!(third.query("select rows=5").await.is_ok(), "连接应当可用");
    drop(third);

    let mut supervisor = supervisor.lock().await;
    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// 连不上时要说得出原因（起不来 / 对端拒绝），而不是一句"查询失败"。
#[tokio::test]
async fn a_broken_plugin_reports_a_connection_error() {
    let plugin_id = "test.factory.broken";
    let plugin_dir = paths::plugin_dir(plugin_id);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let manifest: PluginManifest = toml::from_str(&format!(
        r#"
[plugin]
id = "{plugin_id}"
name = "Broken Driver"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/does-not-exist"

[[contributes.drivers]]
id = "fixture-broken"
display_name = "Broken"
"#
    ))
    .unwrap();

    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));
    register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .expect("注册期只解析路径，不该在这里失败（二进制可能还没装）");

    let factory = DriverRegistry::get("fixture-broken").expect("注册表里应当有它");
    let error = match factory.create(config_for("fixture-broken")).await {
        Ok(_) => panic!("可执行文件不在，应当连不上"),
        Err(error) => error,
    };
    match &error {
        CoreError::Connection(ConnectionError::Refused { reason, .. }) => {
            assert!(reason.contains("起不来"), "{reason}");
        }
        other => panic!("应当进连接域并说清原因：{other:?}"),
    }
}

/// 取消令牌也要能穿过工厂建出来的连接（这一条是"真取消"在工厂路径上的回归）。
#[tokio::test]
async fn cancel_still_works_through_the_factory() {
    let (plugin_dir, manifest) = install_fixture_plugin("test.factory.cancel", "fixture-cancel");
    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));
    register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .unwrap();

    let database = DriverRegistry::get("fixture-cancel")
        .unwrap()
        .create(config_for("fixture-cancel"))
        .await
        .expect("连接应当成功");

    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        trigger.cancel();
    });

    let started = Instant::now();
    let error = database
        .query_with_cancel("select hold_ms=5000", token)
        .await
        .expect_err("应当被取消");
    assert!(error.to_string().contains("取消"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    drop(database);
    supervisor
        .lock()
        .await
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

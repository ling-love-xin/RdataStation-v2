//! 载荷热更新（真进程）：插件目录里的**可执行文件**被替换、清单一个字节没变时，
//! 扫描要认出「载荷变了」并把该插件重启——旧进程停掉，下一个会话用新二进制。
//!
//! 为什么要这条用例：重编驱动（`agent.exe`）、换 wasm、改 py 脚本时清单通常不动。
//! 只看清单 hash 的旧实现会判「无变化」，运行中的 sidecar 一直跑旧内存镜像——
//! 2026-11-12 重编两个 Rust 驱动后实测就是这个结果：三处副本都换了，已连接的
//! Oracle/MSSQL 会话仍走旧二进制，只能重连或重启宿主。
//!
//! 靶子是 `tests/fixture/sidecar.rs`（独立实现的帧对端），不连真库：
//! 只验「进程真的换了」，不断言驱动语义。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rds_plugin::manager::PluginManager;
use rds_plugin::sidecar::supervisor::SessionOpened;
use serde_json::json;

/// 一次性临时安装目录（Drop 时清掉）
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rds-plugin-payload-{tag}-{}", std::process::id()));
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

/// 靶子二进制（`[[bin]] rds-sidecar-fixture`）
fn fixture_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rds-sidecar-fixture")
}

/// 摆一个 sidecar 插件包：清单照真实驱动包的形状写（`bin/agent` 由 platforms 解析成
/// `bin/win-x64/agent.exe`），载荷用 fixture 二进制复制一份。
fn write_sidecar_plugin(dir: &Path, id: &str) -> PathBuf {
    let plugin_dir = dir.join(id);
    let bin_dir = plugin_dir.join("bin").join("win-x64");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::copy(fixture_bin(), bin_dir.join("agent.exe")).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        format!(
            r#"[plugin]
id = "{id}"
name = "Payload Fixture"
version = "1.0.0"
publisher = "test"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
runtime = "rust"
executable = "bin/agent"
transport = "framed"
protocol = "rds-driver/1"
platforms = ["win-x64"]
max_instances = 1

[permissions]
sidecar = ["spawn.child_process", "net.connect", "fs.read_plugin_data", "credentials.database"]

[capabilities.driver]
concurrency = "serial"

[[contributes.drivers]]
id = "fixture"
display_name = "Fixture Driver"
default_port = 0
url_template = "fixture://{{host}}:{{port}}"
features = ["schemas"]
"#
        ),
    )
    .unwrap();
    plugin_dir
}

async fn open_fixture_session(manager: &PluginManager, plugin_id: &str, session_id: &str) {
    let supervisor = manager.sidecar_supervisor();
    let mut sup = supervisor.lock().await;
    let opened = sup
        .open_session(plugin_id, "fixture", session_id, json!({}), Instant::now())
        .await
        .unwrap_or_else(|e| panic!("靶子会话打不开：{e}"));
    assert!(
        matches!(opened, SessionOpened::Live { .. }),
        "靶子会话应当直接开好：{opened:?}"
    );
}

#[tokio::test]
async fn replaced_binary_restarts_the_plugin_and_drops_the_old_process() {
    let dir = TempDir::new("binary");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    write_sidecar_plugin(&dir.0, "test.payload.fixture");
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.added, vec!["test.payload.fixture"]);

    // 先开一个会话：进程真的起来（这一步是「旧镜像」的化身）
    open_fixture_session(&manager, "test.payload.fixture", "s1").await;
    {
        let supervisor = manager.sidecar_supervisor();
        let sup = supervisor.lock().await;
        assert!(
            !sup.instances_of("test.payload.fixture").is_empty(),
            "开会话后该插件应当有活着的实例"
        );
    }

    // 模拟用户用完：关掉会话但进程还活着（实例不因会话归零立刻消失）。
    // 「有活动会话时换载荷」是另一条用例（`payload_change_waits_for_active_sessions`），
    // 那条验的是「不擅自断」；这条验「没人用的时候换载荷会真的重启」。
    {
        let supervisor = manager.sidecar_supervisor();
        let mut sup = supervisor.lock().await;
        sup.close_session("s1", Instant::now()).await.unwrap();
        assert_eq!(
            sup.session_count_of("test.payload.fixture"),
            0,
            "关掉会话后该插件应当没有活动会话"
        );
    }

    // 换载荷：**重命名**旧 exe 再放新的，不直接覆盖。
    // Windows 上运行中的 exe 不允许被覆盖（ERROR_SHARING_VIOLATION），但允许改名——
    // 这也正是人手动更新插件时唯一可行的姿势（先改名腾位置，再放新文件）。
    let exe = dir.0.join("test.payload.fixture").join("bin").join("win-x64").join("agent.exe");
    std::fs::rename(&exe, exe.with_extension("exe.old"))
        .expect("运行中的 exe 应当允许改名（Windows 语义）");
    std::fs::copy(fixture_bin(), &exe).unwrap();

    // 清单没动，扫描必须认出「载荷变了」
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.reloaded, vec!["test.payload.fixture"]);
    assert!(
        diff.changed.is_empty(),
        "载荷变化应当是自动重启，不该要求人工重载"
    );

    // 旧进程必须死透：不能留一个还在跑旧镜像的实例接客
    {
        let supervisor = manager.sidecar_supervisor();
        let sup = supervisor.lock().await;
        assert!(
            sup.instances_of("test.payload.fixture").is_empty(),
            "重启后旧实例应当被停掉，实际还剩 {} 个",
            sup.instances_of("test.payload.fixture").len()
        );
        assert!(
            sup.session_conn("s1").is_none(),
            "旧会话应当如实作废（重启不是「假装没断」）"
        );
    }

    // 新会话走的是新文件：靶子还能正常应答
    open_fixture_session(&manager, "test.payload.fixture", "s2").await;

    let supervisor = manager.sidecar_supervisor();
    let mut sup = supervisor.lock().await;
    sup.shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// 有活动会话时**不许**擅自重启。
///
/// 重启会真的停进程，那个插件上的会话连同正在跑的查询一起作废。用户不知道
/// 二进制被换了，只会看到查询莫名断了。所以这类插件先退回「待重载」提示，
/// 等会话归零后由下一轮扫描自己补上重启。
#[tokio::test]
async fn payload_change_waits_for_active_sessions() {
    let dir = TempDir::new("busy");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    write_sidecar_plugin(&dir.0, "test.payload.busy");
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.added, vec!["test.payload.busy"]);

    // 用户正在用这个插件
    open_fixture_session(&manager, "test.payload.busy", "busy-1").await;

    let exe = dir.0.join("test.payload.busy").join("bin").join("win-x64").join("agent.exe");
    std::fs::rename(&exe, exe.with_extension("exe.old")).expect("运行中的 exe 应当允许改名");
    std::fs::copy(fixture_bin(), &exe).unwrap();

    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert!(
        diff.reloaded.is_empty(),
        "有活动会话时不该自动重启，实际重启了 {:?}",
        diff.reloaded
    );
    assert_eq!(
        diff.changed,
        vec!["test.payload.busy"],
        "应当退回「待重载」，让面板给出提示"
    );

    // 这一轮什么都没断：进程还在，会话的连接还拿得到
    {
        let supervisor = manager.sidecar_supervisor();
        let sup = supervisor.lock().await;
        assert_eq!(
            sup.session_count_of("test.payload.busy"),
            1,
            "会话应当还挂着（没被静默断掉）"
        );
        assert!(
            sup.session_conn("busy-1").is_some(),
            "正在用的连接不该被断"
        );
    }

    // 用户自己关掉连接 → 下一轮扫描把重启补上
    {
        let supervisor = manager.sidecar_supervisor();
        let mut sup = supervisor.lock().await;
        sup.close_session("busy-1", Instant::now()).await.unwrap();
    }
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(
        diff.reloaded,
        vec!["test.payload.busy"],
        "会话结束后应当补上自动重启，而不是一直等用户手动点"
    );
    {
        let supervisor = manager.sidecar_supervisor();
        let sup = supervisor.lock().await;
        assert!(
            sup.instances_of("test.payload.busy").is_empty(),
            "补做后旧实例应当停掉"
        );
        assert!(
            sup.session_conn("busy-1").is_none(),
            "作废的会话不该还能发查询"
        );
    }

    // 补做之后开的会话走新文件
    open_fixture_session(&manager, "test.payload.busy", "busy-2").await;

    let supervisor = manager.sidecar_supervisor();
    let mut sup = supervisor.lock().await;
    sup.shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// 随包 `checksums.json` 不符时不许自动重启。
///
/// 载荷指纹只 stat（相对路径 + 长度 + 修改时间），不读内容——拷了半截、被写坏的
/// 文件同样「看起来变了」。直接重启等于把坏字节当新版本拉起来，只能等加载时失败。
/// 这里用手写的 `checksums.json`（哈希全 0，格式合法但与文件不符）反证：
/// 校验不过就不重启、退回待重载。
#[tokio::test]
async fn bad_checksums_blocks_automatic_restart() {
    let dir = TempDir::new("checksums");
    let manager = Arc::new(PluginManager::new(dir.0.clone()));
    manager.init().await.unwrap();

    let plugin_dir = write_sidecar_plugin(&dir.0, "test.payload.checksums");
    // 作者签发的哈希故意写错：64 位十六进制，格式合法，内容对不上
    let zeros = "0".repeat(64);
    std::fs::write(
        plugin_dir.join("checksums.json"),
        format!(
            r#"{{"algorithm":"sha256","files":{{"plugin.toml":"{zeros}","bin/win-x64/agent.exe":"{zeros}"}}}}"#
        ),
    )
    .unwrap();

    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert_eq!(diff.added, vec!["test.payload.checksums"]);
    assert!(
        diff.changed.is_empty(),
        "首次加载不涉及载荷重启，不该有变更提示：{:?}",
        diff.changed
    );

    let exe = plugin_dir.join("bin").join("win-x64").join("agent.exe");
    std::fs::rename(&exe, exe.with_extension("exe.old")).expect("运行中的 exe 应当允许改名");
    std::fs::copy(fixture_bin(), &exe).unwrap();

    // 没有任何活动会话（这条不受上一刀的保护影响），本该直接重启 ——
    // 唯一能拦住它的是随包哈希不符。
    let diff = manager.scan_and_diff_plugins().await.unwrap();
    assert!(
        diff.reloaded.is_empty(),
        "哈希不符时不该自动重启，实际重启了 {:?}",
        diff.reloaded
    );
    assert_eq!(
        diff.changed,
        vec!["test.payload.checksums"],
        "应当退回待重载"
    );
    {
        let supervisor = manager.sidecar_supervisor();
        let sup = supervisor.lock().await;
        assert!(
            sup.instances_of("test.payload.checksums").is_empty(),
            "被拦下的插件不该有进程在跑"
        );
    }
}

//! 真实进程边界上的端到端测试（`sidecar/process.rs` 的 I/O 那一半）
//!
//! 单测（`tokio::io::duplex`）已经把协议语义测过了；这一层要证明的是**进程侧**：
//! 三根管道真的接对了、调用真的过了进程边界、收摊真的不留孤儿。
//!
//! 靶子是 `tests/fixture/sidecar.rs`（`[[bin]] rds-sidecar-fixture`）：它是一个
//! **独立的**帧编解码实现，两侧不会「错得一致」。
//!
//! ⚠️ 这里**不**测驱动语义（SQL、Arrow 附件）：那是验收靶子（PostgreSQL 包一层）的活。

use std::time::{Duration, Instant};

use serde_json::json;

use rds_plugin::sidecar::conn::{CallError, ConnEvent};
use rds_plugin::sidecar::process::{ProcessError, ReapOutcome, SidecarProcess, SpawnSpec};

/// 收摊宽限：正常路径下对端应当**立刻**退，这个值只是防挂死。
const GRACE: Duration = Duration::from_secs(15);

/// 靶子的插件 id（会过 `paths::validate_plugin_id` 白名单）。
fn fixture_spec(plugin_id: &str, args: &[&str]) -> SpawnSpec {
    let mut spec = SpawnSpec::new(plugin_id, 0, env!("CARGO_BIN_EXE_rds-sidecar-fixture"));
    for arg in args {
        spec = spec.arg(*arg);
    }
    spec
}

/// 起一个靶子进程，失败时直接 panic（测试里没有「优雅降级」这回事）。
async fn spawn_fixture(plugin_id: &str, args: &[&str]) -> SidecarProcess {
    SidecarProcess::spawn(fixture_spec(plugin_id, args))
        .await
        .unwrap_or_else(|e| panic!("起靶子进程 {plugin_id} 失败：{e}"))
}

#[tokio::test]
async fn greets_over_a_real_process_and_retires_cleanly() {
    let process = spawn_fixture("test.fixture.greet", &[]).await;

    let greeting = process
        .conn()
        .initialize("rds-test", "0.0.0")
        .await
        .expect("握手应当成功");
    assert_eq!(greeting.result["protocol"], json!(1));
    assert_eq!(greeting.result["driver_ids"][0], json!("fixture"));

    let pong = process
        .conn()
        .call("ping", json!({}), Duration::from_secs(10))
        .await
        .expect("ping 应当成功");
    assert_eq!(pong.result["pong"], json!(true));

    let outcome = process.retire(GRACE).await;
    assert!(
        outcome.is_clean(),
        "对端应当见 stdin EOF 自己退（§4.2.1）：{outcome:?}"
    );
}

#[tokio::test]
async fn a_missing_program_is_an_explicit_error() {
    let spec = SpawnSpec::new(
        "test.fixture.missing",
        0,
        "definitely-not-a-real-program-xyz",
    );
    let Err(err) = SidecarProcess::spawn(spec).await else {
        panic!("不存在的可执行文件不该起得来");
    };
    match err {
        ProcessError::Launch { program, .. } => {
            assert!(
                program
                    .to_string_lossy()
                    .contains("definitely-not-a-real-program")
            );
        }
        other => panic!("应当报「起不来」而不是别的：{other:?}"),
    }
}

/// id 会进到我们创建的路径里，所以**先白名单、再建目录**。
#[tokio::test]
async fn a_plugin_id_that_escapes_the_data_root_is_refused() {
    let spec = SpawnSpec::new("../../evil", 0, env!("CARGO_BIN_EXE_rds-sidecar-fixture"));
    let Err(err) = SidecarProcess::spawn(spec).await else {
        panic!("逃出数据根的 id 不该起得来");
    };
    match err {
        ProcessError::BadPluginId { plugin_id } => assert_eq!(plugin_id, "../../evil"),
        other => panic!("应当被白名单挡住：{other:?}"),
    }
}

/// 超时放弃的是**那一次调用**，不是这条连接（也不该顺手把进程弄死）。
#[tokio::test]
async fn a_request_that_never_answers_times_out_without_killing_the_process() {
    let process = spawn_fixture("test.fixture.timeout", &[]).await;

    let err = process
        .conn()
        .call("hold", json!({}), Duration::from_millis(250))
        .await
        .expect_err("这一路必然超时");
    assert!(matches!(err, CallError::Timeout { .. }), "{err:?}");

    // 连接照旧可用：后续调用不受影响
    let pong = process
        .conn()
        .call("ping", json!({}), Duration::from_secs(10))
        .await
        .expect("放弃一次调用不该让连接失效");
    assert_eq!(pong.result["pong"], json!(true));

    assert!(process.retire(GRACE).await.is_clean());
}

/// 崩溃时要同时给出两件事：**在飞调用被立刻交还** + **退出码**（决策内核据此把
/// `ExitCause` 分成 `Crash` 与 `Shutdown`）。
#[tokio::test]
async fn a_crash_releases_in_flight_calls_and_leaves_an_exit_code() {
    let process = spawn_fixture("test.fixture.crash", &[]).await;

    let err = process
        .conn()
        .call("crash", json!({}), Duration::from_secs(30))
        .await
        .expect_err("对端直接死掉，不会回");
    assert!(
        matches!(err, CallError::Disconnected { .. }),
        "崩溃时必须当场交还，不能等超时：{err:?}"
    );

    let outcome = process.retire(GRACE).await;
    assert!(outcome.exited_on_its_own(), "{outcome:?}");
    assert_eq!(outcome.exit_code(), Some(3), "退出码要拿得到：{outcome:?}");
    assert!(!outcome.is_clean(), "非零退出码不该算「干净」");
}

/// **崩溃检测不能依赖「下一次调用」**：对端自己死掉时，断线要自己冒出来。
#[tokio::test]
async fn a_process_that_dies_on_its_own_reports_a_disconnect() {
    let mut process = spawn_fixture("test.fixture.selfexit", &["--exit-ms=150"]).await;

    let event = tokio::time::timeout(
        Duration::from_secs(10),
        process.events().expect("事件流还在").recv(),
    )
    .await
    .expect("应当有事件")
    .expect("连接结束前不该是 None");
    assert!(
        matches!(event, ConnEvent::Disconnected { .. }),
        "没调用也要发现它死了：{event:?}"
    );

    let outcome = process.retire(GRACE).await;
    assert_eq!(outcome.exit_code(), Some(3));
}

/// 对端不守 EOF 约定时的兜底：宽限期到点强杀。
#[tokio::test]
async fn a_sidecar_that_ignores_eof_gets_killed() {
    let process = spawn_fixture("test.fixture.stubborn", &["--ignore-eof"]).await;
    process
        .conn()
        .initialize("rds-test", "0.0.0")
        .await
        .expect("握手应当成功");

    let started = Instant::now();
    let outcome = process.retire(Duration::from_millis(300)).await;
    assert!(
        matches!(outcome, ReapOutcome::Forced { .. }),
        "没守约定就该走强杀那一路：{outcome:?}"
    );
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "应当先等满宽限期再动手"
    );
}

/// stderr 落 `plugin-cache/<id>/sidecar.log`，每次启动留抬头（诊断用）。
#[tokio::test]
async fn stderr_lands_in_the_plugin_cache_log() {
    let process = spawn_fixture("test.fixture.stderr", &[]).await;
    let log_path = process.log_path().to_path_buf();

    assert!(process.retire(GRACE).await.is_clean());

    let text = std::fs::read_to_string(&log_path)
        .unwrap_or_else(|e| panic!("读日志 {} 失败：{e}", log_path.display()));
    assert!(
        text.contains("fixture 启动"),
        "对端 stderr 应当落盘：{text:?}"
    );
    assert!(text.contains("pid="), "每次启动要有抬头：{text:?}");
    assert!(text.contains("宿主已走"), "收场那句话也该在里面：{text:?}");
}

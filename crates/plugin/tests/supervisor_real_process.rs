//! supervisor 的真实进程端到端测试（P1 验收里「自动做得到」的那部分）
//!
//! 靶子是 `tests/fixture/sidecar.rs`，**真的起进程**。这里考察的是**接线**：
//!
//! - 决策内核的动作真的落到进程上（`Spawn` → 真进程；`Kill` → 真的收掉）
//! - 放行真的开出会话（`Decision::Open` → `session.open` 过进程边界）
//! - 崩溃真的如实作废，并要求人工重启（§4.1 规则 5）
//! - 心跳真的能发现「不响应的进程」（规则 5 的另一半）
//! - 空闲回收真的收进程，且**旧的断开事件不会误伤新实例**
//!
//! 不考察协议语义（在 `spawn_real_process.rs`）与驱动语义（要等 PostgreSQL 靶子）。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;

use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::lifecycle::{
    Concurrency, IDLE_TIMEOUT, ProcessSpec, ProcessState, RejectReason,
};
use rds_plugin::sidecar::supervisor::{
    Deployment, SessionOpened, SidecarSupervisor, SupervisorError, SupervisorEvent,
};

/// 每个用例一个插件 id：工作目录与日志都按 id 分，测试是并行跑的。
fn deployment(plugin_id: &str, args: &[&str], max_instances: usize) -> Deployment {
    Deployment {
        plugin_id: plugin_id.to_string(),
        spec: ProcessSpec::new(["fixture"], max_instances, Concurrency::Serial),
        command: BackendCommand {
            program: PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture")),
            args: args.iter().map(|a| (*a).to_string()).collect(),
        },
        env: Vec::new(),
        protocol: Some("rds-driver/1".to_string()),
    }
}

fn collect_events(supervisor: &mut SidecarSupervisor) -> Vec<SupervisorEvent> {
    let mut events = Vec::new();
    while let Some(event) = supervisor.try_event() {
        events.push(event);
    }
    events
}

/// 反复 `drain_events` 直到出现**想要的那类事件**（或超时），把期间攒到的都返回。
///
/// 断线是**异步**来的：进程退出 → 读侧 EOF → driver 任务收摊 → 事件泵转发，
/// 中间隔着几次任务切换，不能假设一次 drain 就到位。
///
/// 为什么要传 `want` 而不是「攒到任意事件就收工」：开机本来就会冒
/// `SessionReleased` 这类事件，不区分的话循环第一轮就退出了（实测踩过）。
async fn drain_until<F>(
    supervisor: &mut SidecarSupervisor,
    now: Instant,
    want: F,
) -> Vec<SupervisorEvent>
where
    F: Fn(&SupervisorEvent) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut events = Vec::new();
    while std::time::Instant::now() < deadline {
        supervisor.drain_events(now).await;
        events.extend(collect_events(supervisor));
        if events.iter().any(&want) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    events
}

#[tokio::test]
async fn opens_a_session_on_a_real_process() {
    let id = "test.supervisor.open";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor.deploy(deployment(id, &[], 1)).unwrap();

    let opened = supervisor
        .open_session(id, "fixture", "s1", json!({ "dsn": "x" }), now)
        .await
        .expect("开会话应当成功");
    assert!(
        matches!(opened, SessionOpened::Live { index: 0, .. }),
        "{opened:?}"
    );
    assert_eq!(supervisor.live_instances(), 1);
    assert!(
        supervisor.session_conn("s1").is_some(),
        "会话应当有连接可用"
    );

    // 对端自述（`initialize` 的返回）留着做诊断
    let greeting = supervisor.greeting_of(id, 0).expect("应当有自述");
    assert_eq!(greeting["driver_ids"][0], json!("fixture"));

    // 关会话：进程还留着（规则 1：进程按插件复用，不是每连接一进程）
    supervisor.close_session("s1", now).await.unwrap();
    assert!(supervisor.session("s1").is_none());
    assert_eq!(supervisor.live_instances(), 1, "关会话不该顺手收进程");

    supervisor.shutdown_all(now, Duration::from_secs(10)).await;
    assert_eq!(supervisor.live_instances(), 0);
}

/// `serial`：第二个会话排队，第一个关掉后自动开出。
#[tokio::test]
async fn a_serial_second_session_queues_and_is_released() {
    let id = "test.supervisor.queue";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor.deploy(deployment(id, &[], 1)).unwrap();

    supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap();
    let queued = supervisor
        .open_session(id, "fixture", "s2", json!({}), now)
        .await
        .unwrap();
    assert!(
        matches!(queued, SessionOpened::Queued { position: 1, .. }),
        "{queued:?}"
    );
    assert!(supervisor.session_conn("s2").is_none(), "排队中还没有连接");

    supervisor.close_session("s1", now).await.unwrap();

    let events = collect_events(&mut supervisor);
    assert!(
        events.iter().any(|e| matches!(
            e,
            SupervisorEvent::SessionReleased { session_id, .. } if session_id == "s2"
        )),
        "排队者应当被放行：{events:?}"
    );
    assert!(supervisor.session_conn("s2").is_some());

    supervisor.shutdown_all(now, Duration::from_secs(10)).await;
}

/// 放行的会话开不出来时：**如实作废**，而且不让后面的人一直等。
#[tokio::test]
async fn a_failed_session_open_is_reported_and_does_not_wedge_the_instance() {
    let id = "test.supervisor.failopen";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor.deploy(deployment(id, &[], 1)).unwrap();

    supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap();
    // 靶子按 `params.fail` 故意让这次连接失败
    let queued = supervisor
        .open_session(id, "fixture", "s2", json!({ "fail": true }), now)
        .await
        .unwrap();
    assert!(matches!(queued, SessionOpened::Queued { .. }), "{queued:?}");

    supervisor.close_session("s1", now).await.unwrap();

    let events = collect_events(&mut supervisor);
    assert!(
        events.iter().any(|e| matches!(
            e,
            SupervisorEvent::SessionInvalidated { session_id, .. } if session_id == "s2"
        )),
        "开不出来要如实报：{events:?}"
    );
    assert!(supervisor.session("s2").is_none(), "失败后不该留下记录");
    assert!(supervisor.session_conn("s2").is_none());

    // 实例没被这次失败卡住：下一个会话照常开
    let opened = supervisor
        .open_session(id, "fixture", "s3", json!({}), now)
        .await
        .expect("实例应当还能用");
    assert!(matches!(opened, SessionOpened::Live { .. }), "{opened:?}");

    supervisor.shutdown_all(now, Duration::from_secs(10)).await;
}

/// 崩溃（§4.1 规则 5）：会话如实作废 → 实例判死 → **明确拒绝**新会话 → 人工重启后恢复。
#[tokio::test]
async fn a_crash_invalidates_sessions_and_requires_a_manual_restart() {
    let id = "test.supervisor.crash";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    // 靶子启动 200ms 后自己退出（退出码 3）
    supervisor
        .deploy(deployment(id, &["--exit-ms=200"], 1))
        .unwrap();

    supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap();

    let events = drain_until(&mut supervisor, now, |event| {
        matches!(event, SupervisorEvent::ProcessError { .. })
    })
    .await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            SupervisorEvent::SessionInvalidated { session_id, .. } if session_id == "s1"
        )),
        "崩溃要把会话如实作废：{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SupervisorEvent::ProcessError { .. })),
        "崩溃要报实例判死：{events:?}"
    );
    assert!(supervisor.session("s1").is_none());
    assert_eq!(supervisor.live_instances(), 0, "进程要收干净（不留孤儿）");

    // 再来一个会话：**明确拒绝**并说清「要手动重启」，不是默默重连
    let err = supervisor
        .open_session(id, "fixture", "s2", json!({}), now)
        .await
        .unwrap_err();
    match err {
        SupervisorError::Rejected { reason, .. } => {
            assert_eq!(reason, RejectReason::NeedsManualRestart, "{reason}");
        }
        other => panic!("崩溃后应当要求人工重启：{other:?}"),
    }

    // 手动重启 → 又能开会话（规则 5 的另一半）
    assert_eq!(supervisor.manual_restart(id, now).await.unwrap(), 1);
    let opened = supervisor
        .open_session(id, "fixture", "s3", json!({}), now)
        .await
        .expect("重启后应当能开会话");
    assert!(matches!(opened, SessionOpened::Live { .. }), "{opened:?}");

    supervisor.shutdown_all(now, Duration::from_secs(10)).await;
}

/// 心跳（§4.1 规则 5）：连续两次没回应 → 判死。
///
/// 场景摆法：让靶子自己死掉，但**先不让内核知道**（不去 drain 那条断开）——
/// 这正是心跳要兜的那种情况（断线事件可能因为任何原因没被处理）。
#[tokio::test]
async fn ping_marks_a_process_that_stopped_answering_as_dead() {
    let id = "test.supervisor.ping";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor.deploy(deployment(id, &[], 1)).unwrap();
    supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap();

    // 靶子收到 `crash` 直接退出（不回）
    let err = supervisor
        .session_conn("s1")
        .expect("会话有连接")
        .call("crash", json!({}), Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(
        format!("{err}").contains("断开") || format!("{err}").contains("Closed"),
        "{err}"
    );

    assert_eq!(supervisor.ping_all(now).await, 0, "第一次失败还不判死");
    assert_eq!(supervisor.ping_all(now).await, 1, "连续两次失败判死");

    let events = collect_events(&mut supervisor);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SupervisorEvent::ProcessError { .. })),
        "{events:?}"
    );
    assert_eq!(supervisor.live_instances(), 0);
}

/// 空闲回收（规则 4）：收掉进程；再开会话起的是**新**进程，
/// 而且旧连接遗留的断开事件不会把新实例判死。
#[tokio::test]
async fn idle_sweep_reaps_and_a_stale_disconnect_does_not_kill_the_new_instance() {
    let id = "test.supervisor.idle";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor.deploy(deployment(id, &[], 1)).unwrap();

    supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap();
    supervisor.close_session("s1", now).await.unwrap();

    // 一步跨过 30 分钟（时间是入参，不用 sleep）
    assert_eq!(
        supervisor
            .sweep_idle(now + IDLE_TIMEOUT * 2, IDLE_TIMEOUT)
            .await,
        1,
        "空闲实例应当被回收"
    );
    assert_eq!(supervisor.live_instances(), 0);

    // 再来一个会话：起新进程
    supervisor
        .open_session(id, "fixture", "s2", json!({}), now)
        .await
        .expect("回收后应当能再起");
    assert!(matches!(
        supervisor.instances_of(id).first().map(|i| i.state),
        Some(ProcessState::Ready)
    ));

    // 上一代连接收摊时冒出来的断开事件：不能误伤新实例
    let _ = supervisor.drain_events(now).await;
    assert!(supervisor.session("s2").is_some(), "新会话不该被旧事件干掉");
    assert_eq!(
        supervisor.instances_of(id).first().map(|i| i.state),
        Some(ProcessState::Ready)
    );

    supervisor.shutdown_all(now, Duration::from_secs(10)).await;
}

/// 起不来（可执行文件不存在）时：**不能**在内核里留一个永远不就绪的 `Starting`
/// ——否则该插件的后续请求会被排到一个永远不会就绪的实例上，谁也开不出会话。
#[tokio::test]
async fn a_failed_spawn_leaves_a_condemned_instance_not_a_hung_one() {
    let id = "test.supervisor.nospawn";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor
        .deploy(Deployment {
            command: BackendCommand {
                program: PathBuf::from("definitely-not-a-real-program-xyz"),
                args: Vec::new(),
            },
            ..deployment(id, &[], 1)
        })
        .unwrap();

    let err = supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap_err();
    assert!(matches!(err, SupervisorError::Process { .. }), "{err:?}");

    let instances = supervisor.instances_of(id);
    assert_eq!(instances.len(), 1, "内核里要留一条记录（判死），不是空");
    assert_eq!(
        instances[0].state,
        ProcessState::Error,
        "起不来就要标判死，不能停在 Starting"
    );

    // 后续请求被**明确拒绝**（提示手动重启），而不是默默排队
    let err = supervisor
        .open_session(id, "fixture", "s2", json!({}), now)
        .await
        .unwrap_err();
    match err {
        SupervisorError::Rejected { reason, .. } => {
            assert_eq!(reason, RejectReason::NeedsManualRestart, "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

/// 全体收摊：会话如实作废 + 进程收干净。
#[tokio::test]
async fn shutdown_all_invalidates_sessions_and_reaps_every_process() {
    let id = "test.supervisor.shutdown";
    let now = Instant::now();
    let mut supervisor = SidecarSupervisor::new();
    supervisor.deploy(deployment(id, &[], 2)).unwrap();

    // 两个会话各占一个实例（`max_instances = 2`）
    let first = supervisor
        .open_session(id, "fixture", "s1", json!({}), now)
        .await
        .unwrap();
    let second = supervisor
        .open_session(id, "fixture", "s2", json!({}), now)
        .await
        .unwrap();
    assert!(
        matches!(first, SessionOpened::Live { index: 0, .. }),
        "{first:?}"
    );
    assert!(
        matches!(second, SessionOpened::Live { index: 1, .. }),
        "{second:?}"
    );
    assert_eq!(supervisor.live_instances(), 2);

    supervisor.shutdown_all(now, Duration::from_secs(10)).await;

    let events = collect_events(&mut supervisor);
    let invalidated = events
        .iter()
        .filter(|e| matches!(e, SupervisorEvent::SessionInvalidated { .. }))
        .count();
    assert_eq!(invalidated, 2, "{events:?}");
    assert_eq!(supervisor.live_instances(), 0);
    assert!(supervisor.instances_of(id).is_empty());
}

//! Jupyter 内核会话：启动内核进程、ZMQ 四通道连接、执行与事件回传
//!
//! 结构：`spawn` 起一个后台 tokio 线程（端口挑选 → 写 connection file →
//! 启动 `python -m ipykernel_launcher -f <connfile>` → 连接 iopub/shell/control →
//! 事件循环），调用方通过 `execute / interrupt / shutdown` 发命令，
//! 通过 `take_events()` 消费 [`KernelEvent`]。
//!
//! 设计取舍：
//! - 内核**进程与 connection file 由本模块管理**（精确控制超时 / 清理 / 生命周期）；
//!   ZMQ 四通道与消息签名委托 jupyter-zmq-client（纯 Rust zeromq，无 libzmq 依赖）。
//! - 富输出（`execute_result` / `display_data`）以原始 mime bundle（[`jupyter_protocol::Media`]）
//!   原样回传，**渲染由上层分发器负责**（GPUI 原生：PNG / markdown / 文本 / 表格）。
//! - 会话为「单命令队列」：同一时间只执行一个单元（与笔记本 `has_running` 防并发一致），
//!   事件循环天然串行化 execute。

use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcCommand, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use jupyter_protocol::messaging::{
    ExecuteRequest, InterruptRequest, JupyterMessage, JupyterMessageContent, ReplyStatus,
    ShutdownRequest,
};
use jupyter_protocol::{ConnectionInfo, ExecutionState, Media, Transport};
use jupyter_zmq_client::{
    ClientControlConnection, ClientIoPubConnection, ClientShellConnection,
    create_client_control_connection, create_client_iopub_connection,
    create_client_shell_connection_with_identity, peer_identity_for_session, peek_ports,
    wait_for_iopub_welcome,
};
use tokio::sync::mpsc;

/// 内核生命周期状态（UI 侧据此渲染状态文案 / 禁用控件）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelStatus {
    /// 连接建立中（端口 / 进程 / 握手）
    Starting,
    /// 空闲，可执行
    Idle,
    /// 正在执行单元
    Busy,
    /// 已关闭
    Stopped,
}

impl KernelStatus {
    pub fn label(&self) -> &'static str {
        match self {
            KernelStatus::Starting => "启动中",
            KernelStatus::Idle => "空闲",
            KernelStatus::Busy => "执行中",
            KernelStatus::Stopped => "已停止",
        }
    }
}

/// 内核事件（回传调用方，UI 按需分发）
#[derive(Debug, Clone)]
pub enum KernelEvent {
    /// 生命周期状态变化（busy / idle 由内核 status 消息驱动）
    Status(KernelStatus),
    /// stdout / stderr 流（`name` = "stdout" | "stderr"）
    Stream { name: String, text: String },
    /// 富输出：`display_data` / `update_display_data` / `execute_result` 统一收口
    Display(Media),
    /// shell 通道的执行回复（到达即本次执行完成；`status` = ok / error / abort）
    ExecuteReply {
        status: String,
        execution_count: Option<i64>,
    },
    /// 执行错误（traceback 逐行文本）
    Error { ename: String, evalue: String, traceback: Vec<String> },
    /// 通道层失败（连接断开 / 协议错 / 内核崩溃）；此后会话不可用
    Failed(String),
}

/// 发给内核线程的命令（单命令队列，串行执行）
#[derive(Debug)]
enum Command {
    Execute(String),
    Interrupt,
    Shutdown,
}

/// 一个已启动的内核会话。
///
/// 持有命令发送端 + 事件接收端；Drop 时请求关闭（线程收尾 kill 内核进程）。
/// 非 Clone：一个会话对应一个内核进程 / 一套事件流。
pub struct KernelSession {
    commands: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<KernelEvent>,
}

impl KernelSession {
    /// 启动一个 Python 内核（`python` 指向带 ipykernel 的解释器可执行文件）。
    ///
    /// 同步返回：内部起后台 tokio 线程做端口 / 进程 / 握手，成功后本函数才返回
    /// （启动失败 = 这里直接 Err，不留半死会话）。
    pub fn spawn(python: &Path) -> Result<Self> {
        // 转 owned：线程闭包要求 'static（&Path 生命周期不够）
        let python = python.to_path_buf();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<LaunchParts>>();

        // 内核线程：tokio runtime + 启动 + 事件循环，全程不碰 UI 线程
        let handle = std::thread::Builder::new()
            .name("rds-kernel".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(anyhow::anyhow!("tokio runtime 创建失败：{e}")));
                        return;
                    }
                };
                let _ = rt.block_on(run_kernel_launch(&python, ready_tx));
            })
            .context("spawn rds-kernel 线程失败")?;

        // 等启动结果（带超时兜底，防内核进程挂死握手）
        let parts = match ready_rx.blocking_recv() {
            Ok(Ok(parts)) => parts,
            Ok(Err(e)) => {
                let _ = handle.join();
                return Err(e);
            }
            Err(_) => {
                let _ = handle.join();
                bail!("内核启动线程提前退出（未回传结果）");
            }
        };

        Ok(Self {
            commands: parts.commands,
            events: parts.events,
        })
    }

    /// 取走事件接收端（只能取一次；UI 侧挂 mpsc 轮询消费）
    pub fn take_events(&mut self) -> mpsc::UnboundedReceiver<KernelEvent> {
        std::mem::replace(&mut self.events, mpsc::unbounded_channel().1)
    }

    /// 执行一段代码（排到单命令队列；结果经事件流回传）
    pub fn execute(&self, code: String) {
        let _ = self.commands.send(Command::Execute(code));
    }

    /// 中断当前执行（走 control 通道的 InterruptRequest）
    pub fn interrupt(&self) {
        let _ = self.commands.send(Command::Interrupt);
    }

    /// 请求关闭内核（线程发 ShutdownRequest + kill 兜底）
    pub fn shutdown(&self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

impl Drop for KernelSession {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

/// 启动成功后回传给 `spawn` 的组装件（线程内还留着 iopub/shell/control 跑事件循环）
struct LaunchParts {
    commands: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<KernelEvent>,
}

/// 内核启动全流程（tokio 线程内）：端口 → 进程 → 握手 → 交还控制端 → 事件循环
async fn run_kernel_launch(
    python: &Path,
    ready: tokio::sync::oneshot::Sender<Result<LaunchParts>>,
) -> Result<()> {    let session_id = new_session_id();
    let launched = async {
        // 1. 挑 5 个空闲端口（顺序 = shell / iopub / stdin / control / hb，与 connection file 字段对应）
        let ports = peek_ports(std::net::IpAddr::from([127, 0, 0, 1]), 5)
            .await
            .context("挑选内核端口失败")?;
        let info = ConnectionInfo {
            ip: "127.0.0.1".to_string(),
            transport: Transport::TCP,
            shell_port: ports[0],
            iopub_port: ports[1],
            stdin_port: ports[2],
            control_port: ports[3],
            hb_port: ports[4],
            key: new_key_hex(),
            signature_scheme: "hmac-sha256".to_string(),
            kernel_name: Some("python3".to_string()),
        };

        // 2. connection file（临时目录；内核进程读它绑定端口）
        let conn_file = write_connection_file(&info).context("写 connection file 失败")?;

        // 3. 启动内核进程（stderr 弃置；日志后续版本再收）
        let mut child = ProcCommand::new(python)
            .arg("-m")
            .arg("ipykernel_launcher")
            .arg("-f")
            .arg(&conn_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("启动内核进程失败（python = {}）", python.display()))?;

        // 4. ZMQ 四通道（stdin 按需再连；heartbeat 不做硬依赖）
        // iopub 需 `mut`：握手时 `wait_for_iopub_welcome(&mut …)`
        let mut iopub = create_client_iopub_connection(&info, "", &session_id)
            .await
            .context("连接 iopub 失败")?;
        let shell_identity = peer_identity_for_session(&session_id)?;
        let shell = create_client_shell_connection_with_identity(
            &info,
            &session_id,
            shell_identity,
        )
        .await
        .context("连接 shell 失败")?;
        let control = create_client_control_connection(&info, &session_id)
            .await
            .context("连接 control 失败")?;

        // 5. 等 iopub 握手（内核活着且订阅生效）；超时即判定启动失败
        match tokio::time::timeout(
            std::time::Duration::from_secs(8),
            wait_for_iopub_welcome(&mut iopub, std::time::Duration::from_secs(8)),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&conn_file);
                bail!("内核握手失败：{e}");
            }
            Err(_) => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&conn_file);
                bail!("内核握手超时（python 是否有 ipykernel？）");
            }
        }

        Ok::<_, anyhow::Error>((info, conn_file, child, iopub, shell, control))
    }
    .await;

    match launched {
        Ok((_info, conn_file, child, iopub, shell, control)) => {
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
            let (ev_tx, ev_rx) = mpsc::unbounded_channel::<KernelEvent>();
            let _ = ev_tx.send(KernelEvent::Status(KernelStatus::Starting));
            // 启动成功 → 交还命令端与事件端
            let _ = ready.send(Ok(LaunchParts {
                commands: cmd_tx,
                events: ev_rx,
            }));
            // 事件循环直接跑在 launch 的 tokio runtime 上（runtime 存活期 = 会话存活期；
            // 若这里提前返回，runtime drop 会把未完成任务 abort 掉，事件通道随之断开）
            event_loop(cmd_rx, ev_tx, iopub, shell, control, child, conn_file).await;
            Ok(())
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            Ok(())
        }
    }
}

/// 事件循环：命令（execute/interrupt/shutdown）× iopub × shell 三路 select。
/// 退出条件：收到 Shutdown / 任一路通道错误。
async fn event_loop(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    ev_tx: mpsc::UnboundedSender<KernelEvent>,
    mut iopub: ClientIoPubConnection,
    mut shell: ClientShellConnection,
    mut control: ClientControlConnection,
    mut child: Child,
    conn_file: PathBuf,
) {
    loop {
        tokio::select! {
            // 命令通道
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    break; // 发送端全部 drop = 会话结束
                };
                match cmd {
                    Command::Execute(code) => {
                        let req = ExecuteRequest {
                            code,
                            silent: false,
                            store_history: true,
                            user_expressions: None,
                            allow_stdin: false,
                            stop_on_error: true,
                        };
                        let msg: JupyterMessage = req.into();
                        if let Err(e) = shell.send(msg).await {
                            let _ = ev_tx.send(KernelEvent::Failed(format!("发送执行请求失败：{e}")));
                            break;
                        }
                    }
                    Command::Interrupt => {
                        let msg: JupyterMessage = InterruptRequest {}.into();
                        if let Err(e) = control.send(msg).await {
                            let _ = ev_tx.send(KernelEvent::Failed(format!("发送中断请求失败：{e}")));
                            break;
                        }
                    }
                    Command::Shutdown => {
                        let msg: JupyterMessage = ShutdownRequest { restart: false }.into();
                        let _ = control.send(msg).await;
                        break;
                    }
                }
            }
            // iopub：内核广播（status / stream / display / error）
            msg = iopub.read() => {
                match msg {
                    Ok(msg) => match msg.content {
                        JupyterMessageContent::Status(s) => {
                            let status = match s.execution_state {
                                ExecutionState::Idle => KernelStatus::Idle,
                                ExecutionState::Busy => KernelStatus::Busy,
                                ExecutionState::Starting => KernelStatus::Starting,
                                _ => KernelStatus::Starting,
                            };
                            let _ = ev_tx.send(KernelEvent::Status(status));
                        }
                        JupyterMessageContent::StreamContent(s) => {
                            let name = match s.name {
                                jupyter_protocol::messaging::Stdio::Stdout => "stdout",
                                jupyter_protocol::messaging::Stdio::Stderr => "stderr",
                            };
                            let _ = ev_tx.send(KernelEvent::Stream { name: name.to_string(), text: s.text });
                        }
                        JupyterMessageContent::DisplayData(d) => {
                            let _ = ev_tx.send(KernelEvent::Display(d.data));
                        }
                        JupyterMessageContent::UpdateDisplayData(d) => {
                            let _ = ev_tx.send(KernelEvent::Display(d.data));
                        }
                        JupyterMessageContent::ExecuteResult(r) => {
                            let _ = ev_tx.send(KernelEvent::Display(r.data));
                        }
                        JupyterMessageContent::ErrorOutput(e) => {
                            let _ = ev_tx.send(KernelEvent::Error { ename: e.ename, evalue: e.evalue, traceback: e.traceback });
                        }
                        JupyterMessageContent::IoPubWelcome(_) => { /* 已握手，忽略后续 */ }
                        _ => { /* execute_input / comm 等：本版本不消费 */ }
                    },
                    Err(e) => {
                        let _ = ev_tx.send(KernelEvent::Failed(format!("iopub 通道错误：{e}")));
                        break;
                    }
                }
            }
            // shell：执行回复（到达 = 本次执行完成）
            msg = shell.read() => {
                match msg {
                    Ok(msg) => match msg.content {
                        JupyterMessageContent::ExecuteReply(r) => {
                            let status = match r.status {
                                ReplyStatus::Ok => "ok",
                                ReplyStatus::Error => "error",
                                ReplyStatus::Aborted => "abort",
                            };
                            let _ = ev_tx.send(KernelEvent::ExecuteReply {
                                status: status.to_string(),
                                execution_count: Some(r.execution_count.value() as i64),
                            });
                        }
                        _ => { /* interrupt_reply / kernel_info_reply 等：忽略 */ }
                    },
                    Err(e) => {
                        let _ = ev_tx.send(KernelEvent::Failed(format!("shell 通道错误：{e}")));
                        break;
                    }
                }
            }
        }
    }

    let _ = ev_tx.send(KernelEvent::Status(KernelStatus::Stopped));
    // 内核进程兜底清理（正常关闭时已自行退出；异常时强杀）
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&conn_file);
}

/// 生成会话 id（内核消息 header 的 session 字段，唯一即可）
fn new_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("rds-{}-{}", std::process::id(), nanos)
}

/// 生成 HMAC key（Jupyter 惯例：32 字节 hex = 64 字符）
fn new_key_hex() -> String {
    // 两个 uuid（32 hex × 2 = 64 hex）拼成 32 字节 key，避免额外引入随机源
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// 写 connection file（JSON；内核进程启动参数指向它）
fn write_connection_file(info: &ConnectionInfo) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("rds-kernel-{}-{}.json", std::process::id(), new_key_hex()));
    let json = serde_json::to_vec_pretty(info).context("序列化 connection file 失败")?;
    std::fs::write(&path, json).context("写 connection file 失败")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_file_round_trips_through_serde() {
        let info = ConnectionInfo {
            ip: "127.0.0.1".to_string(),
            transport: Transport::TCP,
            shell_port: 10001,
            iopub_port: 10002,
            stdin_port: 10003,
            control_port: 10004,
            hb_port: 10005,
            key: "a".repeat(64),
            signature_scheme: "hmac-sha256".to_string(),
            kernel_name: Some("python3".to_string()),
        };
        let json = serde_json::to_value(&info).expect("序列化");
        let back: ConnectionInfo = serde_json::from_value(json).expect("反序列化");
        assert_eq!(back.shell_port, 10001);
        assert_eq!(back.iopub_url(), "tcp://127.0.0.1:10002");
        assert_eq!(back.signature_scheme, "hmac-sha256");
    }

    #[test]
    fn session_id_is_unique() {
        let a = new_session_id();
        let b = new_session_id();
        assert_ne!(a, b);
    }

    #[test]
    fn key_hex_is_64_chars() {
        assert_eq!(new_key_hex().len(), 64);
    }

    /// 真实内核集成验证：启动 ipykernel → 执行 `print` → 断言 stdout 流 + ok 回复。
    /// 默认跳过（依赖本机 ipykernel）；显式跑：
    /// `cargo test -p rds-kernel -- --ignored --nocapture`，
    /// python 路径经 `RDS_KERNEL_PYTHON` 指定（缺省用 PATH 上的 `python`）。
    #[test]
    #[ignore = "需要本机安装 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
    fn launches_python_kernel_and_executes_code() {
        let python =
            std::env::var("RDS_KERNEL_PYTHON").unwrap_or_else(|_| "python".to_string());
        let mut session = KernelSession::spawn(Path::new(&python))
            .unwrap_or_else(|e| panic!("启动内核失败（python={python}）：{e:#}"));
        let mut events = session.take_events();
        session.execute("print('hello-kernel')".to_string());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut saw_stdout = false;
        let mut saw_reply = false;
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "等待内核事件超时（stdout={saw_stdout} reply={saw_reply}）"
            );
            // UnboundedReceiver 无带超时的同步读；失败路径会先收到 Failed 事件再 break，
            // 因此阻塞等待是安全的（外层 deadline 兜底防内核挂死）
            match events.blocking_recv() {
                Some(KernelEvent::Stream { text, .. }) => {
                    if text.contains("hello-kernel") {
                        saw_stdout = true;
                    }
                }
                Some(KernelEvent::ExecuteReply { status, .. }) => {
                    assert_eq!(status, "ok", "执行应成功返回");
                    saw_reply = true;
                }
                Some(KernelEvent::Failed(reason)) => panic!("内核通道失败：{reason}"),
                Some(KernelEvent::Error { evalue, .. }) => panic!("执行出错：{evalue}"),
                Some(_) => {}
                None => panic!("事件通道断开"),
            }
            if saw_stdout && saw_reply {
                break;
            }
        }
        session.shutdown();
    }

    /// 富输出与错误路径：`1+1` → Display（text/plain）；`raise` → Error + error 回复。
    /// 依赖 ipykernel，默认跳过；与 `launches_python_kernel_and_executes_code` 同条件显式跑。
    #[test]
    #[ignore = "需要本机安装 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
    fn kernel_delivers_rich_display_error_and_reply() {
        let python =
            std::env::var("RDS_KERNEL_PYTHON").unwrap_or_else(|_| "python".to_string());
        let mut session = KernelSession::spawn(Path::new(&python))
            .unwrap_or_else(|e| panic!("启动内核失败（python={python}）：{e:#}"));
        let mut events = session.take_events();

        // 第一段：`1+1` 的 execute_result 应带 text/plain
        session.execute("1+1".to_string());
        let mut saw_plain = false;
        let mut saw_ok_reply = false;
        loop {
            match events.blocking_recv() {
                Some(KernelEvent::Display(media)) => {
                    let plain = media
                        .content
                        .iter()
                        .any(|m| matches!(m, jupyter_protocol::MediaType::Plain(_)));
                    if plain {
                        saw_plain = true;
                    }
                }
                Some(KernelEvent::ExecuteReply { status, .. }) => {
                    assert_eq!(status, "ok");
                    saw_ok_reply = true;
                }
                Some(KernelEvent::Failed(reason)) => panic!("内核通道失败：{reason}"),
                Some(KernelEvent::Error { evalue, .. }) => panic!("`1+1` 不应报错：{evalue}"),
                Some(_) => {}
                None => panic!("事件通道断开"),
            }
            if saw_plain && saw_ok_reply {
                break;
            }
        }

        // 第二段：抛异常 → Error 事件 + error 回复
        session.execute("raise ValueError('boom')".to_string());
        let mut saw_error = false;
        let mut saw_err_reply = false;
        loop {
            match events.blocking_recv() {
                Some(KernelEvent::Error { ename, evalue, .. }) => {
                    assert_eq!(ename, "ValueError");
                    assert!(evalue.contains("boom"));
                    saw_error = true;
                }
                Some(KernelEvent::ExecuteReply { status, .. }) => {
                    assert_eq!(status, "error");
                    saw_err_reply = true;
                }
                Some(KernelEvent::Failed(reason)) => panic!("内核通道失败：{reason}"),
                Some(_) => {}
                None => panic!("事件通道断开"),
            }
            if saw_error && saw_err_reply {
                break;
            }
        }

        session.shutdown();
    }
}

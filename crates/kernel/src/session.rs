//! Jupyter 内核会话：启动内核进程、ZMQ 四通道连接、执行与事件回传
//!
//! 结构：`spawn_spec` 起一个后台 tokio 线程（端口挑选 → 写 connection file →
//! 按 kernelspec 的 argv 模板启动内核进程 → 连接 iopub/shell/control → 事件循环），
//! 调用方通过 `execute / interrupt / shutdown` 发命令，通过 `take_events()` 消费
//! [`KernelEvent`]。
//!
//! 设计取舍：
//! - 内核**进程与 connection file 由本模块管理**（精确控制超时 / 清理 / 生命周期）；
//!   ZMQ 四通道与消息签名委托 jupyter-zmq-client（纯 Rust zeromq，无 libzmq 依赖）。
//! - **启动命令来自 Jupyter kernelspec**（`kernel.json` 的 `argv` / `env`），所以
//!   R / Julia / 任意已装内核都能起；`{connection_file}` 占位由本层替换。#20 之前的
//!   `python -m ipykernel_launcher` 写死路径退化为 [`KernelLaunch::ipykernel`] 兜底。
//! - `interrupt_mode = "signal"`（IRkernel 声明）目前**仍走 control 通道**：Windows
//!   没有给子进程投递 SIGINT 的标准方式，硬杀会丢掉内核态。这条是已知限制，不是
//!   实现遗漏（R 单元的「中断」按钮效果因此与 Python 相同）。
//! - 富输出（`execute_result` / `display_data`）以原始 mime bundle（[`jupyter_protocol::Media`]）
//!   原样回传，**渲染由上层分发器负责**（GPUI 原生：PNG / markdown / 文本 / 表格）。
//! - 会话为「单命令队列」：同一时间只执行一个单元（与笔记本 `has_running` 防并发一致），
//!   事件循环天然串行化 execute。

use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcCommand, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use jupyter_protocol::messaging::{
    ExecuteRequest, Header, InputReply, InterruptRequest, JupyterMessage, JupyterMessageContent,
    ReplyStatus, ShutdownRequest,
};
use jupyter_protocol::{ConnectionInfo, ExecutionState, Media, Transport};
use jupyter_zmq_client::{
    ClientControlConnection, ClientIoPubConnection, ClientShellConnection, DealerRecvConnection,
    DealerSendConnection, create_client_control_connection, create_client_iopub_connection,
    create_client_shell_connection_with_identity, create_client_stdin_connection_with_identity,
    peek_ports, peer_identity_for_session, wait_for_iopub_welcome,
};
use tokio::sync::mpsc;

use crate::registry::KernelLaunch;

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
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
    /// 通道层失败（连接断开 / 协议错 / 内核崩溃）；此后会话不可用
    Failed(String),
    /// 内核请求用户输入（stdin 通道；Python `input()` / `getpass()`）
    ///
    /// 消费方必须用 [`KernelSession::input`] 回送值，否则内核一直阻塞。
    InputRequest { prompt: String, password: bool },
}

/// 一条事件 + 它所属的 `execute_request` 的 msg_id。
///
/// **为什么要有归属标记**：iopub 与 shell 是两条独立 socket，事件到达顺序不保证
/// 与「哪次执行」对齐。没有归属时，上一单元迟到的 `stream` / `display` 会被当成本
/// 单元的输出，迟到的 `execute_reply` 还会把本单元**提前收口**。
///
/// 会话级事件（`Status(Starting/Stopped)` / 通道失败 `Failed`）没有 parent，`msg_id = None`，
/// 消费方应无条件接受。
#[derive(Debug, Clone)]
pub struct KernelEventEnvelope {
    pub msg_id: Option<String>,
    pub event: KernelEvent,
}

impl KernelEventEnvelope {
    /// 会话级事件（无归属）
    pub fn session(event: KernelEvent) -> Self {
        Self { msg_id: None, event }
    }

    /// 归属某次执行的回复 / 输出
    pub fn for_execution(msg_id: Option<String>, event: KernelEvent) -> Self {
        Self { msg_id, event }
    }
}

/// 发给内核线程的命令（单命令队列，串行执行）
#[derive(Debug)]
enum Command {
    /// `msg_id` 由本层生成并写进 `execute_request` 的 header，供事件归属对账
    Execute { code: String, msg_id: String },
    Interrupt,
    /// 把用户输入回给内核（回应 `InputRequest`；由 stdin 通道写出）
    Input { value: String },
    Shutdown,
}

/// 一个已启动的内核会话。
///
/// 持有命令发送端 + 事件接收端；Drop 时请求关闭（线程收尾 kill 内核进程）。
/// 非 Clone：一个会话对应一个内核进程 / 一套事件流。
pub struct KernelSession {
    commands: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<KernelEventEnvelope>,
}

impl KernelSession {
    /// 启动一个 Python 内核（`python` 指向带 ipykernel 的解释器可执行文件）。
    ///
    /// 等价于 [`Self::spawn_spec`] 的 ipykernel 直启模板，保留是为了不打断既有
    /// 调用点与集成测试；新代码应直接用 `spawn_spec`（它按 kernelspec 启动）。
    pub fn spawn(python: &Path) -> Result<Self> {
        let command = python.display().to_string();
        Self::spawn_spec(&KernelLaunch::ipykernel(&command), "python3")
    }

    /// 按 Jupyter kernelspec 的启动参数起内核（`{connection_file}` 由本层替换）。
    ///
    /// 同步返回：内部起后台 tokio 线程做端口 / 进程 / 握手，成功后本函数才返回
    /// （启动失败 = 这里直接 Err，不留半死会话）。
    pub fn spawn_spec(launch: &KernelLaunch, kernel_name: &str) -> Result<Self> {
        Self::spawn_spec_with_waker(launch, kernel_name, None)
    }

    /// 同 [`Self::spawn_spec`]，但每次事件写入后额外调用 `wake`。
    ///
    /// `wake` 是 push 化的唯一钩子：消费方拿它挂一个唤醒（如 `Notify::notify_one`），
    /// 就不必定时轮询事件通道。**不依赖任何 UI 类型**——它只是一个回调。
    pub fn spawn_spec_with_waker(
        launch: &KernelLaunch,
        kernel_name: &str,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self> {
        if !launch.is_launchable() {
            bail!("内核启动参数不完整（argv 必须包含 {{connection_file}} 占位）");
        }
        // 转 owned：线程闭包要求 'static（&KernelLaunch 生命周期不够）
        let launch = launch.clone();
        let kernel_name = kernel_name.to_string();
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
                let _ = rt.block_on(run_kernel_launch(&launch, &kernel_name, ready_tx, wake));
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
    pub fn take_events(&mut self) -> mpsc::UnboundedReceiver<KernelEventEnvelope> {
        std::mem::replace(&mut self.events, mpsc::unbounded_channel().1)
    }

    /// 执行一段代码（排到单命令队列；结果经事件流回传）
    ///
    /// 返回本次执行的 `msg_id`：调用方存下它，消费事件时只接受
    /// `KernelEventEnvelope.msg_id == Some(本 id)` 的输出与 `ExecuteReply`。
    /// 这样上一单元迟到的事件不会被误算进本单元，也不会把本单元提前收口。
    pub fn execute(&self, code: String) -> String {
        let msg_id = format!("rds-exec-{}", uuid::Uuid::new_v4().simple());
        let _ = self.commands.send(Command::Execute {
            code,
            msg_id: msg_id.clone(),
        });
        msg_id
    }

    /// 中断当前执行（走 control 通道的 InterruptRequest）
    pub fn interrupt(&self) {
        let _ = self.commands.send(Command::Interrupt);
    }

    /// 回送用户输入（回应 [`KernelEvent::InputRequest`]；值经 stdin 通道写出）
    ///
    /// 非阻塞：只入命令队列，真正写 socket 在内核线程里。
    pub fn input(&self, value: String) {
        let _ = self.commands.send(Command::Input { value });
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
    events: mpsc::UnboundedReceiver<KernelEventEnvelope>,
}

/// 事件出口：写入事件后立即调用 `wake`（push 化的唯一钩子）。
///
/// 没有 `wake` 时行为与旧的裸 `mpsc::UnboundedSender` 完全一致——轮询方不受影响。
struct EventSink {
    tx: mpsc::UnboundedSender<KernelEventEnvelope>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl EventSink {
    fn new(tx: mpsc::UnboundedSender<KernelEventEnvelope>, wake: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        Self { tx, wake }
    }

    /// 写事件 + 唤醒。顺序固定为「先入队再唤醒」：唤醒方醒来时事件必然已可取。
    fn send(&self, event: KernelEventEnvelope) {
        let _ = self.tx.send(event);
        if let Some(wake) = self.wake.as_ref() {
            wake();
        }
    }
}

/// 内核启动全流程（tokio 线程内）：端口 → 进程 → 握手 → 交还控制端 → 事件循环
async fn run_kernel_launch(
    launch: &KernelLaunch,
    kernel_name: &str,
    ready: tokio::sync::oneshot::Sender<Result<LaunchParts>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Result<()> {
    let session_id = new_session_id();
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
            kernel_name: Some(kernel_name.to_string()),
        };

        // 2. connection file（临时目录；内核进程读它绑定端口）
        let conn_file = write_connection_file(&info).context("写 connection file 失败")?;

        // 3. 启动内核进程（argv 来自 kernelspec；`{connection_file}` 换成真实路径）
        //    stdin 固定 null：`allow_stdin=false`（见事件循环），内核不会等输入。
        let mut command = ProcCommand::new(&launch.argv[0]);
        for arg in &launch.argv[1..] {
            command.arg(if arg.as_str() == "{connection_file}" {
                conn_file.as_os_str()
            } else {
                std::ffi::OsStr::new(arg.as_str())
            });
        }
        if !launch.env.is_empty() {
            command.envs(
                launch
                    .env
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("启动内核进程失败（{}）", launch.argv[0]))?;

        // 4. ZMQ 四通道（stdin 按需再连；heartbeat 不做硬依赖）
        // iopub 需 `mut`：握手时 `wait_for_iopub_welcome(&mut …)`
        let mut iopub = create_client_iopub_connection(&info, "", &session_id)
            .await
            .context("连接 iopub 失败")?;
        let shell_identity = peer_identity_for_session(&session_id)?;
        let shell = create_client_shell_connection_with_identity(
            &info,
            &session_id,
            shell_identity.clone(),
        )
        .await
        .context("连接 shell 失败")?;
        let control = create_client_control_connection(&info, &session_id)
            .await
            .context("连接 control 失败")?;
        // stdin：必须与 shell 共用同一个 PeerIdentity，内核才把 input_request 路由到本客户端。
        // 拆成读写两半（读在 select 里、写在命令处理里，避免同一 socket 双重借用）。
        // 连接失败不致命：降级为「不支持 input()」，execute 的 allow_stdin 会置 false。
        let stdin = match create_client_stdin_connection_with_identity(
            &info,
            &session_id,
            shell_identity,
        )
        .await
        {
            Ok(conn) => Some(conn.split()),
            Err(_) => None,
        };

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

        Ok::<_, anyhow::Error>((info, conn_file, child, iopub, shell, control, stdin))
    }
    .await;

    match launched {
        Ok((_info, conn_file, child, iopub, shell, control, stdin)) => {
            let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
            let (ev_tx, ev_rx) = mpsc::unbounded_channel::<KernelEventEnvelope>();
            let sink = EventSink::new(ev_tx, wake);
            sink.send(KernelEventEnvelope::session(KernelEvent::Status(
                KernelStatus::Starting,
            )));
            // 启动成功 → 交还命令端与事件端
            let _ = ready.send(Ok(LaunchParts {
                commands: cmd_tx,
                events: ev_rx,
            }));
            // 事件循环直接跑在 launch 的 tokio runtime 上（runtime 存活期 = 会话存活期；
            // 若这里提前返回，runtime drop 会把未完成任务 abort 掉，事件通道随之断开）
            event_loop(cmd_rx, &sink, iopub, shell, control, stdin, child, conn_file).await;
            Ok(())
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            Ok(())
        }
    }
}

/// 事件循环：命令（execute/interrupt/input/shutdown）× iopub × shell × stdin 四路 select。
/// 退出条件：收到 Shutdown / 任一路通道错误。
#[allow(clippy::too_many_arguments)]
async fn event_loop(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    sink: &EventSink,
    mut iopub: ClientIoPubConnection,
    mut shell: ClientShellConnection,
    mut control: ClientControlConnection,
    stdin: Option<(DealerSendConnection, DealerRecvConnection)>,
    mut child: Child,
    conn_file: PathBuf,
) {
    // stdin 拆两半：读半边进 select，写半边留给 Command::Input（同一 socket 不能双重借用）
    let (mut stdin_send, mut stdin_recv) = match stdin {
        Some((send, recv)) => (Some(send), Some(recv)),
        None => (None, None),
    };
    // 最近一次 input_request 的 header：input_reply 的 parent_header 指回它
    let mut pending_input: Option<Header> = None;
    let stdin_enabled = stdin_recv.is_some();
    loop {
        tokio::select! {
            // 命令通道
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else {
                    break; // 发送端全部 drop = 会话结束
                };
                match cmd {
                    Command::Execute { code, msg_id } => {
                        let req = ExecuteRequest {
                            code,
                            silent: false,
                            store_history: true,
                            user_expressions: None,
                            // 没有 stdin 通道时如实声明，避免内核等一个永不到来的输入
                            allow_stdin: stdin_enabled,
                            stop_on_error: true,
                        };
                        let mut msg: JupyterMessage = req.into();
                        // 本层生成的 msg_id 覆盖 `from()` 的随机 id：
                        // 内核回传的消息 parent_header 即此 id，消费方据此归属事件
                        msg.header.msg_id = msg_id;
                        if let Err(e) = shell.send(msg).await {
                            sink.send(KernelEventEnvelope::session(
                                KernelEvent::Failed(format!("发送执行请求失败：{e}")),
                            ));
                            break;
                        }
                    }
                    Command::Interrupt => {
                        let msg: JupyterMessage = InterruptRequest {}.into();
                        if let Err(e) = control.send(msg).await {
                            sink.send(KernelEventEnvelope::session(
                                KernelEvent::Failed(format!("发送中断请求失败：{e}")),
                            ));
                            break;
                        }
                    }
                    Command::Input { value } => {
                        if let Some(conn) = stdin_send.as_mut() {
                            let mut msg: JupyterMessage = InputReply {
                                value,
                                status: ReplyStatus::Ok,
                                error: None,
                            }
                            .into();
                            // input_reply 的 parent_header 必须指回 input_request
                            msg.parent_header = pending_input.take();
                            if let Err(e) = conn.send(msg).await {
                                sink.send(KernelEventEnvelope::session(KernelEvent::Stream {
                                    name: "stderr".to_string(),
                                    text: format!("发送输入失败：{e}"),
                                }));
                            }
                        } else {
                            sink.send(KernelEventEnvelope::session(KernelEvent::Stream {
                                name: "stderr".to_string(),
                                text: "该内核未启用 stdin：输入无法送达".to_string(),
                            }));
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
                    Ok(msg) => {
                        // 归属：iopub 消息的 parent_header 指向触发它的 execute_request
                        let parent = msg.parent_header.as_ref().map(|h| h.msg_id.clone());
                        match msg.content {
                            JupyterMessageContent::Status(s) => {
                                let status = match s.execution_state {
                                    ExecutionState::Idle => KernelStatus::Idle,
                                    ExecutionState::Busy => KernelStatus::Busy,
                                    ExecutionState::Starting => KernelStatus::Starting,
                                    _ => KernelStatus::Starting,
                                };
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent.clone(),
                                    KernelEvent::Status(status),
                                ));
                            }
                            JupyterMessageContent::StreamContent(s) => {
                                let name = match s.name {
                                    jupyter_protocol::messaging::Stdio::Stdout => "stdout",
                                    jupyter_protocol::messaging::Stdio::Stderr => "stderr",
                                };
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent.clone(),
                                    KernelEvent::Stream { name: name.to_string(), text: s.text },
                                ));
                            }
                            JupyterMessageContent::DisplayData(d) => {
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent.clone(),
                                    KernelEvent::Display(d.data),
                                ));
                            }
                            JupyterMessageContent::UpdateDisplayData(d) => {
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent.clone(),
                                    KernelEvent::Display(d.data),
                                ));
                            }
                            JupyterMessageContent::ExecuteResult(r) => {
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent.clone(),
                                    KernelEvent::Display(r.data),
                                ));
                            }
                            JupyterMessageContent::ErrorOutput(e) => {
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent.clone(),
                                    KernelEvent::Error { ename: e.ename, evalue: e.evalue, traceback: e.traceback },
                                ));
                            }
                            JupyterMessageContent::IoPubWelcome(_) => { /* 已握手，忽略后续 */ }
                            _ => { /* execute_input / comm 等：本版本不消费 */ }
                        }
                    },
                    Err(e) => {
                        sink.send(KernelEventEnvelope::session(
                            KernelEvent::Failed(format!("iopub 通道错误：{e}")),
                        ));
                        break;
                    }
                }
            }
            // shell：执行回复（到达 = 本次执行完成）
            msg = shell.read() => {
                match msg {
                    Ok(msg) => {
                        let parent = msg.parent_header.as_ref().map(|h| h.msg_id.clone());
                        match msg.content {
                            JupyterMessageContent::ExecuteReply(r) => {
                                let status = match r.status {
                                    ReplyStatus::Ok => "ok",
                                    ReplyStatus::Error => "error",
                                    ReplyStatus::Aborted => "abort",
                                };
                                sink.send(KernelEventEnvelope::for_execution(
                                    parent,
                                    KernelEvent::ExecuteReply {
                                        status: status.to_string(),
                                        execution_count: Some(r.execution_count.value() as i64),
                                    },
                                ));
                            }
                            _ => { /* interrupt_reply / kernel_info_reply 等：忽略 */ }
                        }
                    },
                    Err(e) => {
                        sink.send(KernelEventEnvelope::session(
                            KernelEvent::Failed(format!("shell 通道错误：{e}")),
                        ));
                        break;
                    }
                }
            }
            // stdin：内核请求输入（Python input() / getpass()）
            msg = read_stdin(&mut stdin_recv) => {
                match msg {
                    Ok(msg) => {
                        let parent = msg.parent_header.as_ref().map(|h| h.msg_id.clone());
                        if let JupyterMessageContent::InputRequest(r) = msg.content {
                            pending_input = Some(msg.header.clone());
                            sink.send(KernelEventEnvelope::for_execution(
                                parent,
                                KernelEvent::InputRequest {
                                    prompt: r.prompt,
                                    password: r.password,
                                },
                            ));
                        }
                    }
                    Err(e) => {
                        // 读失败只降级 stdin（不结束会话）：后续 input() 由内核侧自行报错
                        stdin_recv = None;
                        sink.send(KernelEventEnvelope::session(KernelEvent::Stream {
                            name: "stderr".to_string(),
                            text: format!("stdin 通道断开：{e}"),
                        }));
                    }
                }
            }
        }
    }

    sink.send(KernelEventEnvelope::session(KernelEvent::Status(
        KernelStatus::Stopped,
    )));
    // 内核进程兜底清理（正常关闭时已自行退出；异常时强杀）
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&conn_file);
}

/// 读 stdin 通道；无 stdin（或已降级）时该分支永不就绪（select 里的「挂空」）
async fn read_stdin(
    stdin: &mut Option<DealerRecvConnection>,
) -> jupyter_zmq_client::Result<JupyterMessage> {
    match stdin.as_mut() {
        Some(conn) => conn.read().await,
        None => std::future::pending::<jupyter_zmq_client::Result<JupyterMessage>>().await,
    }
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
    let path = std::env::temp_dir().join(format!(
        "rds-kernel-{}-{}.json",
        std::process::id(),
        new_key_hex()
    ));
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

    #[test]
    fn event_sink_wakes_after_every_send() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<KernelEventEnvelope>();
        let sink = EventSink::new(
            tx,
            Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })),
        );
        sink.send(KernelEventEnvelope::session(KernelEvent::Status(
            KernelStatus::Idle,
        )));
        sink.send(KernelEventEnvelope::session(KernelEvent::Status(
            KernelStatus::Stopped,
        )));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "每次 send 必须唤醒一次（R2 push 的成立前提）"
        );
        // 顺序不变量：唤醒发生时事件已经在通道里（先入队再唤醒）
        assert!(rx.try_recv().is_ok(), "第一条事件应已可读");
        assert!(rx.try_recv().is_ok(), "第二条事件应已可读");
    }

    #[test]
    fn event_sink_without_waker_keeps_old_behavior() {
        let (tx, mut rx) = mpsc::unbounded_channel::<KernelEventEnvelope>();
        let sink = EventSink::new(tx, None);
        sink.send(KernelEventEnvelope::session(KernelEvent::Status(
            KernelStatus::Stopped,
        )));
        assert!(rx.try_recv().is_ok(), "无 wake 时只入队，行为与旧裸 sender 一致");
        assert!(rx.try_recv().is_err());
    }

    /// 真实内核集成验证：启动 ipykernel → 执行 `print` → 断言 stdout 流 + ok 回复。
    /// 默认跳过（依赖本机 ipykernel）；显式跑：
    /// `cargo test -p rds-kernel -- --ignored --nocapture`，
    /// python 路径经 `RDS_KERNEL_PYTHON` 指定（缺省用 PATH 上的 `python`）。
    #[test]
    #[ignore = "需要本机安装 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
    fn launches_python_kernel_and_executes_code() {
        let python = std::env::var("RDS_KERNEL_PYTHON").unwrap_or_else(|_| "python".to_string());
        let mut session = KernelSession::spawn(Path::new(&python))
            .unwrap_or_else(|e| panic!("启动内核失败（python={python}）：{e:#}"));
        let mut events = session.take_events();
        let msg_id = session.execute("print('hello-kernel')".to_string());

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
                Some(env) => {
                    // R8 归属断言：带归属的事件必须属于本次 execute_request
                    if let Some(id) = env.msg_id.as_ref() {
                        assert_eq!(id, &msg_id, "事件归属错误：应属于本次执行");
                    }
                    match env.event {
                        KernelEvent::Stream { text, .. } => {
                            if text.contains("hello-kernel") {
                                saw_stdout = true;
                            }
                        }
                        KernelEvent::ExecuteReply { status, .. } => {
                            assert_eq!(status, "ok", "执行应成功返回");
                            saw_reply = true;
                        }
                        KernelEvent::Failed(reason) => panic!("内核通道失败：{reason}"),
                        KernelEvent::Error { evalue, .. } => panic!("执行出错：{evalue}"),
                        _ => {}
                    }
                }
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
        let python = std::env::var("RDS_KERNEL_PYTHON").unwrap_or_else(|_| "python".to_string());
        let mut session = KernelSession::spawn(Path::new(&python))
            .unwrap_or_else(|e| panic!("启动内核失败（python={python}）：{e:#}"));
        let mut events = session.take_events();

        // 第一段：`1+1` 的 execute_result 应带 text/plain
        let msg_id_plain = session.execute("1+1".to_string());
        let mut saw_plain = false;
        let mut saw_ok_reply = false;
        // 收尾的 `status: idle` 也是带归属的事件：不等它到齐，第二段就会先读到这里，
        // 被归属断言判成「串台」（实测）——产品没错，是这段没排空。
        let mut saw_idle = false;
        loop {
            match events.blocking_recv() {
                Some(env) => {
                    if let Some(id) = env.msg_id.as_ref() {
                        assert_eq!(id, &msg_id_plain, "第一段事件应归属 `1+1` 的 execute_request");
                    }
                    match env.event {
                        KernelEvent::Display(media) => {
                            let plain = media
                                .content
                                .iter()
                                .any(|m| matches!(m, jupyter_protocol::MediaType::Plain(_)));
                            if plain {
                                saw_plain = true;
                            }
                        }
                        KernelEvent::ExecuteReply { status, .. } => {
                            assert_eq!(status, "ok");
                            saw_ok_reply = true;
                        }
                        KernelEvent::Status(KernelStatus::Idle) => saw_idle = true,
                        KernelEvent::Failed(reason) => panic!("内核通道失败：{reason}"),
                        KernelEvent::Error { evalue, .. } => {
                            panic!("`1+1` 不应报错：{evalue}")
                        }
                        _ => {}
                    }
                }
                None => panic!("事件通道断开"),
            }
            if saw_plain && saw_ok_reply && saw_idle {
                break;
            }
        }

        // 第二段：抛异常 → Error 事件 + error 回复
        let msg_id_error = session.execute("raise ValueError('boom')".to_string());
        assert_ne!(
            msg_id_error, msg_id_plain,
            "每次 execute 的 msg_id 必须唯一（否则归属对账失效）"
        );
        let mut saw_error = false;
        let mut saw_err_reply = false;
        loop {
            match events.blocking_recv() {
                Some(env) => {
                    if let Some(id) = env.msg_id.as_ref() {
                        assert_eq!(id, &msg_id_error, "第二段事件应归属抛异常那次执行");
                    }
                    match env.event {
                        KernelEvent::Error { ename, evalue, .. } => {
                            assert_eq!(ename, "ValueError");
                            assert!(evalue.contains("boom"));
                            saw_error = true;
                        }
                        KernelEvent::ExecuteReply { status, .. } => {
                            assert_eq!(status, "error");
                            saw_err_reply = true;
                        }
                        KernelEvent::Failed(reason) => panic!("内核通道失败：{reason}"),
                        _ => {}
                    }
                }
                None => panic!("事件通道断开"),
            }
            if saw_error && saw_err_reply {
                break;
            }
        }

        session.shutdown();
    }

    /// `input()` 端到端：内核请求输入 → 上报 `InputRequest` → `session.input` 回送 REPLY
    /// → 单元正常收口，回送值进了 Python 变量。
    ///
    /// 这条钉住三件事：①`allow_stdin` 真的打开了（否则内核直接抛
    /// `StdinNotImplementedError`，收不到请求）；②提示串原样带回来；③REPLY 被内核接受
    /// （`got=doubao` 出现在 stdout 才算真的拿到值）。
    ///
    /// 默认跳过（依赖 ipykernel）；显式跑：
    /// `cargo test -p rds-kernel -- --ignored --nocapture`，
    /// python 路径经 `RDS_KERNEL_PYTHON` 指定。
    #[test]
    #[ignore = "需要本机安装 ipykernel（RDS_KERNEL_PYTHON 指向对应解释器）"]
    fn input_request_is_replied_and_execution_completes() {
        let python = std::env::var("RDS_KERNEL_PYTHON").unwrap_or_else(|_| "python".to_string());
        let mut session = KernelSession::spawn(Path::new(&python))
            .unwrap_or_else(|e| panic!("启动内核失败（python={python}）：{e:#}"));
        let mut events = session.take_events();

        // 内核阻塞在 `input()` 上，直到收到 REPLY 才会继续往下跑
        let msg_id = session.execute("x = input('name? '); print('got=' + x)".to_string());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut saw_prompt = false;
        let mut saw_value = false;
        let mut saw_reply = false;
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "等待输入请求 / 回执超时（prompt={saw_prompt} value={saw_value} reply={saw_reply}）"
            );
            let Some(env) = events.blocking_recv() else {
                panic!("事件通道断开");
            };
            // R8 归属：带归属的事件必须属于本次 execute_request
            if let Some(id) = env.msg_id.as_ref() {
                assert_eq!(id, &msg_id, "事件归属错误：应属于本次执行");
            }
            match env.event {
                KernelEvent::InputRequest { prompt, password } => {
                    assert!(
                        prompt.contains("name?"),
                        "提示串要原样带回来，实际是：{prompt:?}"
                    );
                    assert!(!password, "`input()` 不是密码请求");
                    saw_prompt = true;
                    // 回送到 stdin 通道（一条请求只回一次）
                    session.input("doubao".to_string());
                }
                KernelEvent::Stream { text, .. } => {
                    if text.contains("got=doubao") {
                        saw_value = true;
                    }
                }
                KernelEvent::ExecuteReply { status, .. } => {
                    assert_eq!(status, "ok", "回送之后执行应成功收口");
                    saw_reply = true;
                }
                KernelEvent::Failed(reason) => panic!("内核通道失败：{reason}"),
                KernelEvent::Error { evalue, .. } => {
                    panic!("`input()` 不应报错（说明 stdin 通道没通）：{evalue}")
                }
                _ => {}
            }
            if saw_value && saw_reply {
                break;
            }
        }
        assert!(saw_prompt, "必须先收到输入请求");
        session.shutdown();
    }
}

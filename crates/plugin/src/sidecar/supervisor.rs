//! sidecar 运行时的宿主侧：把决策内核、真实进程、RPC 三者接起来。
//!
//! 分工（设计出处 `plugin-dev-plan.md` §4.1 / §4.2）：
//!
//! | 层 | 管什么 | 在哪 |
//! | --- | --- | --- |
//! | 决策内核 | 该起 / 该杀 / 该排队 / 该判死（sans-io） | `lifecycle.rs` |
//! | 进程层 | 起收真实进程、stdio 接线、EOF 回收 | `process.rs` |
//! | 线路层 | 分帧、在飞表、超时放弃 | `conn.rs` |
//! | **本模块** | 执行上面的决策、把结果与坏消息如实报出去 | 这里 |
//!
//! 三条纪律：
//!
//! 1. **时间是入参**（与 `lifecycle` 同一条）：`sweep_idle(now, ..)` / `ping_all(now)` 由宿主
//!    在自己那一拍上驱动，测试里就能一步跨过 30 分钟。同一次入口调用内部只用一个 `now`
//!    （不中途再取时钟：规则判定要可重放）。
//! 2. **事件落地是显式的**：连接上的通知 / 断线先由「事件泵」任务收进一条总通道，再由
//!    [`SidecarSupervisor::drain_events`] 落到内核上。**不在后台任务里碰 registry** ——
//!    那会给所有状态套一层锁，而宿主本来就是单线程事件循环。
//! 3. **崩溃不静默重连**（§4.1 规则 5）：进程没了就把会话如实作废、实例标 `Error`，
//!    等用户点「重启」；重启走 [`SidecarSupervisor::manual_restart`]。
//!
//! # 两处口径（已写进 dev-plan §4.1 / §4.2.2 附录）
//!
//! - **`session_id` 由宿主生成并在 `session.open` 里带下去**：会话句柄是宿主的资源名，
//!   sidecar 只回声（否则一个会话要维护两张 id 表，迟早对不上）。
//! - **进程存活探针用进程级 `ping()`，不是 `session.ping`**：空闲实例没有会话，
//!   而它恰恰是最需要探活的那种（§4.1 规则 5 原文写的是 `session.ping`）。

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::conn::{CallError, ConnEvent, ConnEvents, SidecarConn};
use super::lifecycle::{
    Action, Decision, Event, ExitCause, ProcessInstance, ProcessSpec, Registry, RejectReason,
    SessionId, SessionRecord,
};
use super::process::{DEFAULT_REAP_GRACE, SidecarProcess, SpawnSpec};
use crate::manifest::{BackendCommand, PluginManifest};

/// 会话级 RPC 的超时（`session.open` 要等真库握手，给宽一点）。
pub const SESSION_RPC_TIMEOUT: Duration = Duration::from_secs(30);
/// 心跳超时：活着的进程回一个 `pong` 是毫秒级的事。
pub const PING_TIMEOUT: Duration = Duration::from_secs(10);
/// 收进程的宽限（与进程层一致）。
pub const SHUTDOWN_GRACE: Duration = DEFAULT_REAP_GRACE;

/// 握手时自报家门（`initialize` 的 `host` 字段）。
#[derive(Debug, Clone)]
pub struct HostInfo {
    pub name: String,
    pub version: String,
}

impl Default for HostInfo {
    fn default() -> Self {
        Self {
            name: "RdataStation".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// 一个插件的部署信息（清单里解析出来的那一份）。
#[derive(Debug, Clone)]
pub struct Deployment {
    pub plugin_id: String,
    /// 进程池要的那份规格（drivers / max_instances / concurrency）。
    pub spec: ProcessSpec,
    /// 起进程的命令（`PluginBackend::command` 的产物）。
    pub command: BackendCommand,
    /// 额外环境变量（清单声明的）。
    pub env: Vec<(String, String)>,
    /// 诊断用：清单声明的协议标识。
    pub protocol: Option<String>,
}

impl Deployment {
    /// 从清单与插件目录解析；`Ok(None)` = 这个插件没有子进程形态（wasm / 纯前端）。
    ///
    /// 清单层面的矛盾（形态 / 传输 / 平台 / 路径）在这里就报出来，不要拖到起进程才发现。
    pub fn from_manifest(
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) -> Result<Option<Self>, shared::error::CoreError> {
        let Some(spec) = manifest.process_spec() else {
            return Ok(None);
        };
        let backend = manifest
            .backend
            .as_ref()
            // `process_spec()` 有值就必然有 `[backend]`
            .expect("process_spec 有值必然有 [backend]");

        Ok(Some(Self {
            plugin_id: manifest.plugin.id.clone(),
            spec,
            command: backend.command(plugin_dir)?,
            env: Vec::new(),
            protocol: backend.protocol.clone(),
        }))
    }
}

/// 开会话的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOpened {
    /// 会话已经在跑（`session.open` 已成功）。
    Live { plugin_id: String, index: usize },
    /// 还在排队（`serial` 实例忙）；等前面那个关掉会自动开出，
    /// 之后会收到 [`SupervisorEvent::SessionReleased`]。
    Queued {
        plugin_id: String,
        index: usize,
        position: usize,
    },
}

/// 交给宿主的事件——**坏消息也要如实报**。
#[derive(Debug, Clone, PartialEq)]
pub enum SupervisorEvent {
    /// 排队中的会话被放行，且 `session.open` 已经成功。
    SessionReleased {
        session_id: SessionId,
        plugin_id: String,
        index: usize,
    },
    /// 会话失效（进程崩溃 / 判死 / **开会话失败**）：宿主必须让用户看到，不静默重连。
    SessionInvalidated {
        session_id: SessionId,
        reason: String,
    },
    /// 实例判死，需要人工介入（下一步是 [`SidecarSupervisor::manual_restart`]）。
    ProcessError {
        plugin_id: String,
        index: usize,
        reason: String,
    },
    /// 插件的通知（`log` / `progress`）。
    Notification {
        plugin_id: String,
        method: String,
        params: Value,
    },
    /// 线路层面的不对（错位帧、超时后迟到的响应……）。
    Issue { plugin_id: String, detail: String },
}

/// supervisor 的错误。
#[derive(Debug)]
pub enum SupervisorError {
    /// 插件 id 不合法（会进到路径里，必须先过白名单）。
    BadPluginId { plugin_id: String },
    /// 这个插件没登记过部署信息（未安装 / 未启用 / 不是子进程形态）。
    NotDeployed { plugin_id: String },
    /// 有活实例时换部署（先收掉再来）。
    Busy { plugin_id: String },
    /// 部署解析失败（清单自相矛盾：形态 / 传输 / 平台 / 路径）。
    Deploy { plugin_id: String, detail: String },
    /// 决策内核**明确拒绝**（理由可读，可以直接给用户看）。
    Rejected {
        session_id: SessionId,
        reason: RejectReason,
    },
    /// 起进程 / 握手失败。
    Process { plugin_id: String, error: String },
    /// RPC 失败（含超时 / 断线）。
    Rpc { method: String, error: CallError },
    /// 内部不一致（都是 bug）。
    Internal { detail: String },
}

impl std::fmt::Display for SupervisorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPluginId { plugin_id } => write!(f, "插件 id 不合法：{plugin_id:?}"),
            Self::NotDeployed { plugin_id } => {
                write!(
                    f,
                    "插件 {plugin_id} 没有可起进程的部署信息（未登记或不是 sidecar）"
                )
            }
            Self::Busy { plugin_id } => write!(f, "插件 {plugin_id} 还有活实例，先收掉再换部署"),
            Self::Deploy { plugin_id, detail } => {
                write!(f, "插件 {plugin_id} 的部署声明不可用：{detail}")
            }
            Self::Rejected { session_id, reason } => {
                write!(f, "会话 {session_id} 没能打开：{reason}")
            }
            Self::Process { plugin_id, error } => {
                write!(f, "插件 {plugin_id} 起进程失败：{error}")
            }
            Self::Rpc { method, error } => write!(f, "{method} 调用失败：{error}"),
            Self::Internal { detail } => write!(f, "内部状态不一致：{detail}"),
        }
    }
}

impl std::error::Error for SupervisorError {}

/// 放行失败的会话：已经作废并报过事件，这里只把原因带回给**调用方**。
struct OpenFailure {
    session_id: SessionId,
    error: CallError,
}

/// 事件泵送进总通道的东西（带上「是哪个实例的哪一代」）。
enum InstanceEvent {
    Notification {
        plugin_id: String,
        generation: u64,
        method: String,
        params: Value,
    },
    Issue {
        plugin_id: String,
        generation: u64,
        detail: String,
    },
    Disconnected {
        plugin_id: String,
        index: usize,
        generation: u64,
        reason: String,
    },
}

/// 一个活着的实例。
struct Instance {
    process: SidecarProcess,
    /// `initialize` 的对端自述（诊断：driver_ids / runtime）。
    greeting: Value,
    /// 第几代（同 `(plugin_id, index)` 重启后会变）。
    ///
    /// 为什么需要它：旧连接收摊时也会冒一条 `Disconnected`。若此时同 `(plugin_id, index)`
    /// 已经重启过，拿这条旧事件去判死会把**新实例**一起带走。
    generation: u64,
    /// 事件泵任务（连接结束它自己退；这里只用于主动收摊时 abort）。
    pump: JoinHandle<()>,
}

type InstanceKey = (String, usize);

/// sidecar 运行时的宿主侧。
pub struct SidecarSupervisor {
    registry: Registry,
    deployments: BTreeMap<String, Deployment>,
    instances: BTreeMap<InstanceKey, Instance>,
    /// 内核已经登记、但 `session.open` 还没发出去的会话（等进程起好 / 等队首轮到它）。
    pending_opens: BTreeMap<SessionId, Value>,
    instance_events_tx: mpsc::UnboundedSender<InstanceEvent>,
    instance_events_rx: mpsc::UnboundedReceiver<InstanceEvent>,
    events_out_tx: mpsc::UnboundedSender<SupervisorEvent>,
    events_out_rx: mpsc::UnboundedReceiver<SupervisorEvent>,
    host: HostInfo,
    next_generation: u64,
}

impl Default for SidecarSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl SidecarSupervisor {
    pub fn new() -> Self {
        Self::with_host(HostInfo::default())
    }

    pub fn with_host(host: HostInfo) -> Self {
        let (instance_events_tx, instance_events_rx) = mpsc::unbounded_channel();
        let (events_out_tx, events_out_rx) = mpsc::unbounded_channel();
        Self {
            registry: Registry::new(),
            deployments: BTreeMap::new(),
            instances: BTreeMap::new(),
            pending_opens: BTreeMap::new(),
            instance_events_tx,
            instance_events_rx,
            events_out_tx,
            events_out_rx,
            host,
            next_generation: 1,
        }
    }

    // ==================== 部署 ====================

    /// 登记（或覆盖）一个插件的部署信息。
    ///
    /// 有活实例又换了形态 / 命令时**拒绝**（`Busy`）：正跑着的连接不该被悄悄换掉实现。
    /// 同形态重复登记是幂等的（安装流程与打开项目会各登记一次）。
    pub fn deploy(&mut self, deployment: Deployment) -> Result<(), SupervisorError> {
        let plugin_id = deployment.plugin_id.clone();
        paths::validate_plugin_id(&plugin_id).map_err(|_| SupervisorError::BadPluginId {
            plugin_id: plugin_id.clone(),
        })?;

        if let Some(existing) = self.deployments.get(&plugin_id) {
            let unchanged = existing.spec == deployment.spec
                && existing.command == deployment.command
                && existing.env == deployment.env;
            if !unchanged && self.has_instances(&plugin_id) {
                return Err(SupervisorError::Busy { plugin_id });
            }
        }

        self.registry
            .register_spec(&plugin_id, deployment.spec.clone());
        self.deployments.insert(plugin_id, deployment);
        Ok(())
    }

    /// 从清单直接登记（惯用入口）；返回 `false` = 这不是子进程形态的插件（wasm / 纯前端）。
    pub fn deploy_manifest(
        &mut self,
        manifest: &PluginManifest,
        plugin_dir: &Path,
    ) -> Result<bool, SupervisorError> {
        match Deployment::from_manifest(manifest, plugin_dir) {
            Ok(Some(deployment)) => {
                self.deploy(deployment)?;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(SupervisorError::Deploy {
                plugin_id: manifest.plugin.id.clone(),
                detail: e.to_string(),
            }),
        }
    }

    fn has_instances(&self, plugin_id: &str) -> bool {
        self.instances.keys().any(|(p, _)| p == plugin_id)
    }

    // ==================== 事物流出 ====================

    /// 取一条给宿主的事件（非阻塞）。宿主在自己那一拍里排空它。
    pub fn try_event(&mut self) -> Option<SupervisorEvent> {
        self.events_out_rx.try_recv().ok()
    }

    /// 把「连接上的事件」落到内核上：断线要判死、通知要转出去。
    ///
    /// 返回收到并处理了几条（诊断与测试用）。
    ///
    /// 实现上**先排空再处理**：处理过程要可变借整个 supervisor，不能一边持着接收端
    /// 一边动 registry。
    pub async fn drain_events(&mut self, now: Instant) -> usize {
        let mut incoming = Vec::new();
        while let Ok(event) = self.instance_events_rx.try_recv() {
            incoming.push(event);
        }
        let received = incoming.len();

        for event in incoming {
            match event {
                InstanceEvent::Notification {
                    plugin_id,
                    generation,
                    method,
                    params,
                } => {
                    if self.is_current(&plugin_id, generation) {
                        self.out(SupervisorEvent::Notification {
                            plugin_id,
                            method,
                            params,
                        });
                    }
                }
                InstanceEvent::Issue {
                    plugin_id,
                    generation,
                    detail,
                } => {
                    if self.is_current(&plugin_id, generation) {
                        self.out(SupervisorEvent::Issue { plugin_id, detail });
                    }
                }
                InstanceEvent::Disconnected {
                    plugin_id,
                    index,
                    generation,
                    reason,
                } => {
                    self.handle_disconnect(&plugin_id, index, generation, reason, now)
                        .await;
                }
            }
        }
        received
    }

    /// 这条事件是不是来自**当前**那一代进程（旧连接的遗留事件直接丢）。
    fn is_current(&self, plugin_id: &str, generation: u64) -> bool {
        self.instances
            .iter()
            .any(|((p, _), i)| p == plugin_id && i.generation == generation)
    }

    /// 连接断开 = 进程没了（stdout EOF）。如实作废会话与实例，并把进程收掉。
    async fn handle_disconnect(
        &mut self,
        plugin_id: &str,
        index: usize,
        generation: u64,
        reason: String,
        now: Instant,
    ) {
        let key: InstanceKey = (plugin_id.to_string(), index);
        if self.instances.get(&key).map(|i| i.generation) != Some(generation) {
            // 旧代的遗留事件：**不能**拿它判死（重启后的新实例会被误伤）
            tracing::debug!(
                plugin_id,
                index,
                generation,
                reason = %reason,
                "忽略上一代连接的断开事件"
            );
            return;
        }

        let events = self
            .registry
            .on_process_exit(plugin_id, index, ExitCause::Crash, now);
        self.forget_pending_opens(&events);
        self.forward(events);

        if let Some(instance) = self.instances.remove(&key) {
            let outcome = instance.process.retire(SHUTDOWN_GRACE).await;
            tracing::warn!(
                plugin_id,
                index,
                generation,
                reason = %reason,
                exit_code = ?outcome.exit_code(),
                "sidecar 进程没了：会话已如实作废，实例等人工重启"
            );
        }
    }

    fn forward(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::SessionInvalidated { session_id, reason } => {
                    self.out(SupervisorEvent::SessionInvalidated { session_id, reason });
                }
                Event::ProcessError {
                    plugin_id,
                    index,
                    reason,
                } => {
                    self.out(SupervisorEvent::ProcessError {
                        plugin_id,
                        index,
                        reason,
                    });
                }
            }
        }
    }

    /// 会话作废了就把它的排队参数丢掉（否则 `pending_opens` 会慢慢长大）。
    fn forget_pending_opens(&mut self, events: &[Event]) {
        for event in events {
            if let Event::SessionInvalidated { session_id, .. } = event {
                self.pending_opens.remove(session_id);
            }
        }
    }

    fn out(&mut self, event: SupervisorEvent) {
        let _ = self.events_out_tx.send(event);
    }

    // ==================== 会话 ====================

    /// 开一个会话。
    ///
    /// 三种结局：直接开好（`Live`）、排队（`Queued`，之后自动放行）、明确拒绝（`Err`）。
    pub async fn open_session(
        &mut self,
        plugin_id: &str,
        driver_id: &str,
        session_id: &str,
        params: Value,
        now: Instant,
    ) -> Result<SessionOpened, SupervisorError> {
        match self.registry.acquire(plugin_id, driver_id, session_id, now) {
            Decision::Rejected { session_id, reason } => {
                Err(SupervisorError::Rejected { session_id, reason })
            }

            Decision::Open {
                plugin_id,
                index,
                session_id,
            } => {
                if let Err(e) = self
                    .call_session_open(&plugin_id, index, &session_id, params)
                    .await
                {
                    // RPC 没成：内核已经登记了它，必须撤销（否则这个 id 永远占着），
                    // 并把因此放行的下一个排队者接着推进
                    self.out(SupervisorEvent::SessionInvalidated {
                        session_id: session_id.clone(),
                        reason: e.to_string(),
                    });
                    let released = self.registry.release(&session_id, now);
                    self.execute_decisions(released, now).await;
                    return Err(e);
                }
                Ok(SessionOpened::Live { plugin_id, index })
            }

            Decision::Spawn {
                plugin_id,
                index,
                session_id,
            } => {
                // 参数先存起来：`session.open` 要等进程起好、`on_process_ready` 放行时才发
                self.pending_opens.insert(session_id.clone(), params);
                let failures = self.spawn_instance(&plugin_id, index, now).await?;

                // 起进程成功 ≠ 会话开出来了：放行后的 `session.open` 可能失败
                if let Some(failure) = failures.into_iter().find(|f| f.session_id == session_id) {
                    return Err(SupervisorError::Rpc {
                        method: "session.open".to_string(),
                        error: failure.error,
                    });
                }
                if self.registry.session(&session_id).is_none() {
                    return Err(SupervisorError::Internal {
                        detail: format!("实例起好了，但放行路径没产出会话 {session_id}"),
                    });
                }
                Ok(SessionOpened::Live { plugin_id, index })
            }

            Decision::Queued {
                plugin_id,
                index,
                session_id,
                position,
            } => {
                self.pending_opens.insert(session_id, params);
                Ok(SessionOpened::Queued {
                    plugin_id,
                    index,
                    position,
                })
            }
        }
    }

    /// 关一个会话（正常关闭 / 出错释放都走这里）。
    ///
    /// `session.close` 调用失败**不阻断**本地撤销：会话在宿主侧一定要撤掉，否则表里会留下
    /// 一个谁也关不掉的幽灵（只 `warn`，不返回错）。
    pub async fn close_session(
        &mut self,
        session_id: &str,
        now: Instant,
    ) -> Result<(), SupervisorError> {
        // 还没开出 RPC 的（排队中 / 进程还在起）：撤掉请求即可
        if self.pending_opens.remove(session_id).is_some() {
            let released = self.registry.release(session_id, now);
            self.execute_decisions(released, now).await;
            return Ok(());
        }

        let Some(record) = self.registry.session(session_id) else {
            return Ok(());
        };
        let key: InstanceKey = (record.plugin_id.clone(), record.index);

        if let Some(instance) = self.instances.get(&key) {
            let params = json!({ "session_id": session_id });
            if let Err(e) = instance
                .process
                .conn()
                .call("session.close", params, SESSION_RPC_TIMEOUT)
                .await
            {
                tracing::warn!(session_id, error = %e, "session.close 调用失败；本地照撤");
            }
        }

        let released = self.registry.release(session_id, now);
        self.execute_decisions(released, now).await;
        Ok(())
    }

    /// 只负责把 `session.open` 发出去（**不改内核状态**：开了没开成由调用方决定怎么善后）。
    ///
    /// 之所以把它和善后拆开：善后要 `release` → 可能又放行下一个排队者 → 又调本方法，
    /// 合在一起就成了 async 递归（必须装箱，还容易漏）。拆开后由 [`Self::execute_decisions`]
    /// 的工作队列统一推进。
    async fn call_session_open(
        &mut self,
        plugin_id: &str,
        index: usize,
        session_id: &str,
        params: Value,
    ) -> Result<(), SupervisorError> {
        let key: InstanceKey = (plugin_id.to_string(), index);
        let Some(instance) = self.instances.get(&key) else {
            return Err(SupervisorError::Internal {
                detail: format!("决策要求实例 {plugin_id}#{index} 开会话，但它不在表里"),
            });
        };

        let driver_id = self
            .registry
            .session(session_id)
            .map(|s| s.driver_id.clone())
            .unwrap_or_default();
        let call = json!({
            "session_id": session_id,
            "driver_id": driver_id,
            "params": params,
        });

        instance
            .process
            .conn()
            .call("session.open", call, SESSION_RPC_TIMEOUT)
            .await
            .map(|_| ())
            .map_err(|error| SupervisorError::Rpc {
                method: "session.open".to_string(),
                error,
            })
    }

    /// 内核放行出来的会话（`Decision::Open`）逐个兑现。
    ///
    /// 用工作队列而不是递归：一次失败会释放下一个排队者，链式推进但长度有限。
    /// 返回开了 RPC 但没开成的那些（已经作废并报过事件，这里只把原因带给调用方）。
    async fn execute_decisions(
        &mut self,
        mut decisions: Vec<Decision>,
        now: Instant,
    ) -> Vec<OpenFailure> {
        let mut failures = Vec::new();
        while let Some(decision) = decisions.pop() {
            let Decision::Open {
                plugin_id,
                index,
                session_id,
            } = decision
            else {
                // `on_process_ready` / `release` 只会产出 `Open`；走到这里说明内核契约变了
                tracing::warn!(decision = ?decision, "放行路径收到意外决策，已忽略");
                continue;
            };

            let params = self
                .pending_opens
                .remove(&session_id)
                .unwrap_or(Value::Null);
            match self
                .call_session_open(&plugin_id, index, &session_id, params)
                .await
            {
                Ok(()) => self.out(SupervisorEvent::SessionReleased {
                    session_id,
                    plugin_id,
                    index,
                }),
                Err(error) => {
                    // 开不出来：**如实作废**这个会话（排队的用户不能一直等一个不会来的会话），
                    // 并把因此放行的下一个排队者接着推进
                    tracing::warn!(session_id, error = %error, "放行的会话没能开出");
                    self.out(SupervisorEvent::SessionInvalidated {
                        session_id: session_id.clone(),
                        reason: error.to_string(),
                    });
                    decisions.extend(self.registry.release(&session_id, now));
                    if let SupervisorError::Rpc { error, .. } = error {
                        failures.push(OpenFailure { session_id, error });
                    }
                }
            }
        }
        failures
    }

    // ==================== 进程 ====================

    /// 起一个实例：起进程 → 握手 → 挂事件泵 → 告诉内核就绪（放行排队者）。
    ///
    /// 返回放行失败的会话（原因带回给 `open_session` 的调用方）。
    async fn spawn_instance(
        &mut self,
        plugin_id: &str,
        index: usize,
        now: Instant,
    ) -> Result<Vec<OpenFailure>, SupervisorError> {
        match self.do_spawn_instance(plugin_id, index, now).await {
            Ok(failures) => Ok(failures),
            Err(e) => {
                // 起不来 / 握不上手 = 这个实例没了：**必须告诉内核**，否则它是个永远不就绪的
                // `Starting`，会把该插件后续的请求全排住（`max_instances = 1` 时尤其明显）。
                // 按 `Crash` 处理 → 实例标 `Error`，用户看到「需要手动重启」并能真的重启。
                let events = self
                    .registry
                    .on_process_exit(plugin_id, index, ExitCause::Crash, now);
                self.forget_pending_opens(&events);
                self.forward(events);
                tracing::warn!(plugin_id, index, error = %e, "实例起不来");
                Err(e)
            }
        }
    }

    async fn do_spawn_instance(
        &mut self,
        plugin_id: &str,
        index: usize,
        now: Instant,
    ) -> Result<Vec<OpenFailure>, SupervisorError> {
        let key: InstanceKey = (plugin_id.to_string(), index);
        if self.instances.contains_key(&key) {
            return Err(SupervisorError::Internal {
                detail: format!("实例 {plugin_id}#{index} 已经在跑，不该再起一个"),
            });
        }
        let Some(deployment) = self.deployments.get(plugin_id).cloned() else {
            return Err(SupervisorError::NotDeployed {
                plugin_id: plugin_id.to_string(),
            });
        };

        // 目录 / 日志 / 环境统一由 `SpawnSpec` 从 `paths::*` 取（不在别处拼路径）
        let mut spec = SpawnSpec::new(plugin_id, index, deployment.command.program.clone())
            .args(deployment.command.args.iter().cloned());
        for (env_key, env_value) in &deployment.env {
            spec = spec.with_env(env_key.clone(), env_value.clone());
        }

        let mut process =
            SidecarProcess::spawn(spec)
                .await
                .map_err(|e| SupervisorError::Process {
                    plugin_id: plugin_id.to_string(),
                    error: e.to_string(),
                })?;

        // 握手（版本闸在 `initialize` 里；不匹配当场拒绝）
        let greeting = match process
            .conn()
            .initialize(&self.host.name, &self.host.version)
            .await
        {
            Ok(outcome) => outcome.result,
            Err(e) => {
                // 握不上手也要把进程收干净：不能留一个不会说协议的进程在那儿
                let _ = process.retire(SHUTDOWN_GRACE).await;
                return Err(SupervisorError::Rpc {
                    method: "initialize".to_string(),
                    error: e,
                });
            }
        };

        let Some(events) = process.take_events() else {
            let _ = process.retire(SHUTDOWN_GRACE).await;
            return Err(SupervisorError::Internal {
                detail: "这条连接的事件流已经被取走过一次".to_string(),
            });
        };

        let generation = self.next_generation;
        self.next_generation += 1;
        let pump = spawn_event_pump(
            plugin_id.to_string(),
            index,
            generation,
            events,
            self.instance_events_tx.clone(),
        );

        let pid = process.pid();
        tracing::info!(plugin_id, index, pid = ?pid, "sidecar 实例已就绪");
        self.instances.insert(
            key,
            Instance {
                process,
                greeting,
                generation,
                pump,
            },
        );

        let decisions = self.registry.on_process_ready(plugin_id, index, pid, now);
        Ok(self.execute_decisions(decisions, now).await)
    }

    /// 收掉一个实例（内核已经把它从表里摘了）。
    async fn kill_instance(&mut self, plugin_id: &str, index: usize, grace: Duration) {
        let key: InstanceKey = (plugin_id.to_string(), index);
        if let Some(instance) = self.instances.remove(&key) {
            instance.pump.abort();
            let outcome = instance.process.retire(grace).await;
            if !outcome.is_clean() {
                tracing::warn!(plugin_id, index, outcome = ?outcome, "收实例时它不是干净退出的");
            }
        }
    }

    /// 空闲回收（§4.1 规则 4）：返回收掉了几个实例。
    ///
    /// 不用作废会话：内核只回收**无会话且无排队**的实例（`ProcessInstance::is_idle`）。
    pub async fn sweep_idle(&mut self, now: Instant, idle_timeout: Duration) -> usize {
        let actions = self.registry.sweep_idle(now, idle_timeout);
        let mut killed = 0;
        for action in actions {
            if let Action::Kill {
                plugin_id, index, ..
            } = action
            {
                self.kill_instance(&plugin_id, index, SHUTDOWN_GRACE).await;
                killed += 1;
            }
        }
        killed
    }

    /// 心跳（§4.1 规则 5）：返回被判死的实例数。
    pub async fn ping_all(&mut self, now: Instant) -> usize {
        let mut dead = 0;
        for action in self.registry.ping_targets() {
            let Action::Ping { plugin_id, index } = action else {
                continue;
            };
            let key: InstanceKey = (plugin_id.clone(), index);
            let ok = match self.instances.get(&key) {
                Some(instance) => instance
                    .process
                    .conn()
                    .call("ping", json!({}), PING_TIMEOUT)
                    .await
                    .is_ok(),
                // 内核说要心跳、我们却没有进程：状态不一致，按失败走（判死路径会说明）
                None => false,
            };

            let events = self.registry.on_ping(&plugin_id, index, ok, now);
            if !events.is_empty() {
                dead += 1;
                self.forget_pending_opens(&events);
                self.forward(events);
                // 判死的实例要把进程也收掉：留着只会是个不会应答的僵尸
                self.kill_instance(&plugin_id, index, SHUTDOWN_GRACE).await;
            }
        }
        dead
    }

    /// 人工重启（§4.1 规则 5 的另一半）：返回起了几个实例。
    pub async fn manual_restart(
        &mut self,
        plugin_id: &str,
        now: Instant,
    ) -> Result<usize, SupervisorError> {
        if !self.deployments.contains_key(plugin_id) {
            return Err(SupervisorError::NotDeployed {
                plugin_id: plugin_id.to_string(),
            });
        }

        let mut spawned = 0;
        for action in self.registry.manual_restart(plugin_id, now) {
            if let Action::Spawn {
                plugin_id, index, ..
            } = action
            {
                self.spawn_instance(&plugin_id, index, now).await?;
                spawned += 1;
            }
        }
        Ok(spawned)
    }

    /// 全部收摊（关闭项目 / 退出宿主）。
    ///
    /// 顺序：**先让内核摘掉实例与会话**（会话如实作废）→ 再逐个收进程。
    pub async fn shutdown_all(&mut self, now: Instant, grace: Duration) {
        let keys: Vec<InstanceKey> = self.instances.keys().cloned().collect();
        for (plugin_id, index) in keys {
            let events = self
                .registry
                .on_process_exit(&plugin_id, index, ExitCause::Shutdown, now);
            self.forget_pending_opens(&events);
            self.forward(events);
            self.kill_instance(&plugin_id, index, grace).await;
        }
    }

    // ==================== 查询 ====================

    /// 会话 → 连接（驱动桥用它发 `query.execute` / `meta.*`）。
    ///
    /// 排队中的会话**还没有连接**（`None`）：它要等队首轮到才会真的开出去。
    pub fn session_conn(&self, session_id: &str) -> Option<&SidecarConn> {
        let record = self.registry.session(session_id)?;
        self.instances
            .get(&(record.plugin_id.clone(), record.index))
            .map(|i| i.process.conn())
    }

    pub fn session(&self, session_id: &str) -> Option<&SessionRecord> {
        self.registry.session(session_id)
    }

    pub fn instances_of(&self, plugin_id: &str) -> Vec<&ProcessInstance> {
        self.registry.instances_of(plugin_id)
    }

    /// 某个实例的对端自述（`initialize` 的返回；诊断用）。
    pub fn greeting_of(&self, plugin_id: &str, index: usize) -> Option<&Value> {
        self.instances
            .get(&(plugin_id.to_string(), index))
            .map(|i| &i.greeting)
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// 活实例数（诊断）。
    pub fn live_instances(&self) -> usize {
        self.instances.len()
    }
}

/// 事件泵：把一条连接上的事件汇进总通道，直到连接结束。
fn spawn_event_pump(
    plugin_id: String,
    index: usize,
    generation: u64,
    mut events: ConnEvents,
    tx: mpsc::UnboundedSender<InstanceEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let mapped = match event {
                ConnEvent::Notification { method, params } => InstanceEvent::Notification {
                    plugin_id: plugin_id.clone(),
                    generation,
                    method,
                    params,
                },
                ConnEvent::Issue { detail } => InstanceEvent::Issue {
                    plugin_id: plugin_id.clone(),
                    generation,
                    detail,
                },
                ConnEvent::Disconnected { reason, .. } => InstanceEvent::Disconnected {
                    plugin_id: plugin_id.clone(),
                    index,
                    generation,
                    reason,
                },
            };
            if tx.send(mapped).is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIDECAR_MANIFEST: &str = r#"
[plugin]
id = "com.example.fixture"
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
id = "fixture"
display_name = "Fixture"

[capabilities.driver]
concurrency = "serial"
"#;

    #[test]
    fn deployment_comes_from_the_manifest() {
        let manifest: PluginManifest = toml::from_str(SIDECAR_MANIFEST).unwrap();
        let deployment =
            Deployment::from_manifest(&manifest, Path::new("plugins/com.example.fixture"))
                .expect("解析应当成功")
                .expect("sidecar 形态应当有部署信息");

        assert_eq!(deployment.plugin_id, "com.example.fixture");
        assert_eq!(deployment.spec.drivers, vec!["fixture".to_string()]);
        assert_eq!(deployment.spec.max_instances, 1);
        assert!(deployment.command.program.ends_with("bin/agent"));
        assert!(deployment.command.args.is_empty());
    }

    #[test]
    fn deployment_is_none_for_wasm_only_plugins() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.wasm"
name = "Wasm"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "wasm"

[capabilities.wasm]
entry = "plugin.wasm"
"#,
        )
        .unwrap();
        assert!(
            Deployment::from_manifest(&manifest, Path::new("."))
                .unwrap()
                .is_none()
        );
    }

    /// 清单层面的矛盾（比如 jsonl 传输）要在**部署期**报出来，不要拖到起进程才发现。
    #[test]
    fn deployment_reports_manifest_conflicts() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.jsonl"
name = "Jsonl"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/agent"
transport = "jsonl"
"#,
        )
        .unwrap();
        let err = Deployment::from_manifest(&manifest, Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("jsonl"), "{err}");
    }

    #[test]
    fn deploy_rejects_an_id_that_escapes_the_data_root() {
        let manifest: PluginManifest = toml::from_str(SIDECAR_MANIFEST).unwrap();
        let mut deployment = Deployment::from_manifest(&manifest, Path::new("."))
            .unwrap()
            .unwrap();
        deployment.plugin_id = "../../evil".to_string();

        let mut supervisor = SidecarSupervisor::new();
        assert!(matches!(
            supervisor.deploy(deployment),
            Err(SupervisorError::BadPluginId { .. })
        ));
    }

    /// 同形态重复登记是幂等的（安装流程与打开项目会各登记一次）。
    #[test]
    fn deploying_the_same_thing_twice_is_fine() {
        let manifest: PluginManifest = toml::from_str(SIDECAR_MANIFEST).unwrap();
        let mut supervisor = SidecarSupervisor::new();
        assert!(
            supervisor
                .deploy_manifest(&manifest, Path::new("."))
                .unwrap()
        );
        assert!(
            supervisor
                .deploy_manifest(&manifest, Path::new("."))
                .unwrap()
        );
        assert_eq!(supervisor.live_instances(), 0);
    }

    /// wasm / 纯前端插件不登记（`false`），而不是报错。
    #[test]
    fn deploy_manifest_reports_non_subprocess_plugins() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.ui"
name = "UI"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[capabilities.frontend]
entry = "main.js"
"#,
        )
        .unwrap();
        let mut supervisor = SidecarSupervisor::new();
        assert!(
            !supervisor
                .deploy_manifest(&manifest, Path::new("."))
                .unwrap()
        );
    }

    /// 旧代连接的遗留事件**不能**动当前实例。
    ///
    /// 这不是杞人忧天：实例被回收/重启之后，旧连接的 `Disconnected` 仍可能后到
    /// （事件泵转发与放行之间有窗口）；不加代次判定的话，它会把**新实例**判死。
    #[tokio::test]
    async fn a_stale_disconnect_does_not_touch_the_current_instance() {
        use crate::sidecar::lifecycle::ProcessState;

        let now = Instant::now();
        let manifest: PluginManifest = toml::from_str(SIDECAR_MANIFEST).unwrap();
        let mut supervisor = SidecarSupervisor::new();
        supervisor
            .deploy_manifest(&manifest, Path::new("."))
            .unwrap();

        // 不走真实进程：本用例只考察「旧代事件不该动新实例」这一条。
        // 摆一个 Ready 实例 + 一个活会话（与真实路径同口径：acquire → Spawn → on_process_ready）
        let decision = supervisor
            .registry
            .acquire("com.example.fixture", "fixture", "s1", now);
        assert!(
            matches!(decision, Decision::Spawn { index: 0, .. }),
            "{decision:?}"
        );
        let opened = supervisor
            .registry
            .on_process_ready("com.example.fixture", 0, Some(1), now);
        assert_eq!(opened.len(), 1, "就绪后应当放行排队的那一个会话");

        // 一条 generation 对不上的断开（旧连接留下来的）
        supervisor
            .handle_disconnect("com.example.fixture", 0, 99, "旧连接".into(), now)
            .await;

        let instances = supervisor.instances_of("com.example.fixture");
        assert_eq!(instances.len(), 1);
        assert_eq!(
            instances[0].state,
            ProcessState::Ready,
            "旧代事件把当前实例判死了"
        );
        assert!(
            supervisor.session("s1").is_some(),
            "旧代事件不该把当前会话作废"
        );
        assert!(supervisor.try_event().is_none(), "也不该往外报事件");
    }
}

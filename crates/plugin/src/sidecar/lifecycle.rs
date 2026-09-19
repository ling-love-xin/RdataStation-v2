//! 三层对象模型的决策内核（sans-io）：`PluginProcess → DriverInstance → Session`。
//!
//! 设计出处：`docs/architecture/plugin/plugin-dev-plan.md` §4.1。本模块**只做决策**，
//! 一行 I/O 都没有 —— 该起进程就返回 [`Action::Spawn`]，该判死就返回 [`Event`]，
//! 由 `manager` 那一层去执行。这样五条规则全部可单测（含"进程崩了会怎样"这类只能靠
//! 造场景验证的路径），而 I/O 层只剩"把动作发出去"这点薄逻辑。
//!
//! ## 与 §4.1 五条规则的对应
//!
//! | 规则 | 落点 |
//! | --- | --- |
//! | 1 | 进程按 `plugin_id` 去重、**不是每连接一进程**；`max_instances` 声明上限 | [`Registry::acquire`] 先复用 Ready 实例 |
//! | 2 | 一个插件 = 一个进程 = 一个或多个 driver | [`ProcessSpec::drivers`]；喂错 driver 会被 [`RejectReason::UnknownDriver`] 挡下 |
//! | 3 | `concurrency = serial` 时在该进程上排队 | [`Decision::Queued`] + [`Registry::release`] 放行队首 |
//! | 4 | 空闲回收与 `ConnectionManager` 对齐（30min）；进程退出 → session 全失效、**如实报错不静默重连** | [`IDLE_TIMEOUT`] + [`Registry::sweep_idle`]；[`Registry::on_process_exit`] |
//! | 5 | 崩溃检测走 ping：连续 2 次失败 → `Error` + 通知 + **允许手动重启** | [`PING_MAX_FAILURES`] + [`Registry::on_ping`] + [`Registry::manual_restart`] |
//!
//! 时间是**入参**（`now: Instant`）而不是内部 `Instant::now()`：否则"空闲 30 分钟"
//! 这类规则只能靠 sleep 测。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// 插件 id（= 一个 sidecar 可执行文件的运行实例的归属）。
pub type PluginId = String;
/// 驱动 id（一个插件可声明多个 driver）。
pub type DriverId = String;
/// 会话 id（一次连接 = 一个会话句柄）。
pub type SessionId = String;

/// 清单未声明 `max_instances` 时的默认值（§4.1 规则 1）。
pub const DEFAULT_MAX_INSTANCES: usize = 1;

/// 连续 ping 失败多少次判死（§4.1 规则 5）。
pub const PING_MAX_FAILURES: u32 = 2;

/// 空闲回收时长（§4.1 规则 4：与 `ConnectionManager` 的 30min 对齐）。
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 单个实例上排队上限。
///
/// 没有上限的队列就是一个不会报错的内存增长：宿主侧该做的是**如实拒绝**，
/// 让用户看到"这个驱动忙不过来"，而不是把请求默默堆着。
pub const QUEUE_MAX_LEN: usize = 16;

/// 并发策略（清单 `[capabilities.driver].concurrency`，见 `manifest.rs`）。
///
/// 值就在清单那一侧：本模块只是消费方，不要在 `[backend]` 里再声明一份
/// （参考实现就是因为契约重复两份而两边跑偏，见 `plugin-dev-plan.md` §3.5）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Concurrency {
    /// 串行（默认）：同一实例同时只服务一个会话（JDBC 单 `Connection` 属此类）。
    #[default]
    Serial,
    /// 并行：同一实例可同时服务多个会话。
    Parallel,
}

/// 一个插件的运行规格（来自清单）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// 该插件声明的 driver id 列表（喂错就拒绝 —— 这是"声明与实现不符"的护栏）。
    pub drivers: Vec<DriverId>,
    /// 实例数上限。
    pub max_instances: usize,
    /// 并发策略。
    pub concurrency: Concurrency,
}

impl ProcessSpec {
    pub fn new(
        drivers: impl IntoIterator<Item = impl Into<String>>,
        max_instances: usize,
        concurrency: Concurrency,
    ) -> Self {
        Self {
            drivers: drivers.into_iter().map(Into::into).collect(),
            max_instances: max_instances.max(1),
            concurrency,
        }
    }

    /// 单实例 + 串行（最常见：JDBC 类驱动）。
    pub fn serial_single(drivers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(drivers, DEFAULT_MAX_INSTANCES, Concurrency::Serial)
    }

    pub fn knows_driver(&self, driver_id: &str) -> bool {
        self.drivers.iter().any(|d| d == driver_id)
    }
}

/// 实例状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// 已下发 Spawn，等 `on_process_ready`。
    Starting,
    /// 就绪，可开会话。
    Ready,
    /// 判死（崩溃 / ping 连续失败）：**不再接新会话，直到手动重启**（§4.1 规则 5）。
    Error,
}

/// 一个进程实例（`PluginProcess` 的一次运行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInstance {
    pub plugin_id: PluginId,
    /// 同插件的实例序号（`0..max_instances`）。
    pub index: usize,
    pub state: ProcessState,
    pub pid: Option<u32>,
    /// 人工重启次数（诊断用）。
    pub restart_count: u32,
    /// 连续 ping 失败次数（成功即清零）。
    pub ping_failures: u32,
    last_active: Instant,
    sessions: BTreeSet<SessionId>,
    queue: VecDeque<SessionId>,
}

impl ProcessInstance {
    fn new(plugin_id: &str, index: usize, now: Instant) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            index,
            state: ProcessState::Starting,
            pid: None,
            restart_count: 0,
            ping_failures: 0,
            last_active: now,
            sessions: BTreeSet::new(),
            queue: VecDeque::new(),
        }
    }

    pub fn sessions(&self) -> impl Iterator<Item = &SessionId> {
        self.sessions.iter()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn queued(&self) -> impl Iterator<Item = &SessionId> {
        self.queue.iter()
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// 是否空闲可回收（就绪 / 无会话 / 无排队）。
    pub fn is_idle(&self) -> bool {
        self.state == ProcessState::Ready && self.sessions.is_empty() && self.queue.is_empty()
    }
}

/// 一个会话的位置（诊断与生命周期通知用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub plugin_id: PluginId,
    pub driver_id: DriverId,
    pub index: usize,
}

/// 进程退出的原因：**崩溃与正常关闭必须分开**（前者要标记 `Error` 并要求人工介入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCause {
    /// 我们让它退的（空闲回收 / 关闭项目）。
    Shutdown,
    /// 它自己死的（进程崩溃、管道断开）。
    Crash,
}

/// 会话该怎么开。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// 直接在实例 `index` 上开会话（I/O 层去发 `session.open`）。
    Open {
        plugin_id: PluginId,
        index: usize,
        session_id: SessionId,
    },
    /// 先起实例 `index`，就绪后（`on_process_ready`）会自动把会话放行。
    Spawn {
        plugin_id: PluginId,
        index: usize,
        session_id: SessionId,
    },
    /// 排队等待（`position` 从 1 开始）。
    Queued {
        plugin_id: PluginId,
        index: usize,
        session_id: SessionId,
        position: usize,
    },
    /// 明确拒绝（**不排队的理由必须说清**）。
    Rejected {
        session_id: SessionId,
        reason: RejectReason,
    },
}

/// 拒绝理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// 清单没登记过这个插件（未安装 / 未启用）。
    UnknownPlugin,
    /// 该插件没声明这个 driver（往"声明与实现不符"上兜底）。
    UnknownDriver { driver_id: DriverId },
    /// 同一个 session id 被重复开（调用方的 bug，不能静默覆盖）。
    SessionIdReused,
    /// 实例处于 `Error`，必须先手动重启（§4.1 规则 5：不静默重连）。
    NeedsManualRestart,
    /// 该实例的排队已满（`QUEUE_MAX_LEN`）。
    QueueFull { index: usize },
}

/// I/O 层要执行的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 起一个实例（`current_dir` / 日志 / 环境白名单都是 I/O 层的事）。
    Spawn { plugin_id: PluginId, index: usize },
    /// 收掉一个实例。
    Kill {
        plugin_id: PluginId,
        index: usize,
        reason: KillReason,
    },
    /// 发一次 `session.ping`。
    Ping { plugin_id: PluginId, index: usize },
}

/// 收进程的理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillReason {
    /// 空闲超时（§4.1 规则 4）。
    Idle,
}

/// 需要通知宿主的事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// 会话失效 —— **必须如实告诉用户**（"不静默重连"的另一半）。
    SessionInvalidated {
        session_id: SessionId,
        reason: String,
    },
    /// 实例判死，需要人工介入。
    ProcessError {
        plugin_id: PluginId,
        index: usize,
        reason: String,
    },
}

/// 三层对象模型的决策内核。
#[derive(Debug, Default)]
pub struct Registry {
    specs: BTreeMap<PluginId, ProcessSpec>,
    instances: BTreeMap<(PluginId, usize), ProcessInstance>,
    sessions: BTreeMap<SessionId, SessionRecord>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记（或覆盖）一个插件的运行规格。
    pub fn register_spec(&mut self, plugin_id: &str, spec: ProcessSpec) {
        self.specs.insert(plugin_id.to_string(), spec);
    }

    pub fn spec(&self, plugin_id: &str) -> Option<&ProcessSpec> {
        self.specs.get(plugin_id)
    }

    /// 某个插件的全部实例（按序号）。
    pub fn instances_of(&self, plugin_id: &str) -> Vec<&ProcessInstance> {
        self.instances
            .iter()
            .filter(|((p, _), _)| p == plugin_id)
            .map(|(_, i)| i)
            .collect()
    }

    pub fn session(&self, session_id: &str) -> Option<&SessionRecord> {
        self.sessions.get(session_id)
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// 请求开一个会话：返回"怎么开"的决策。
    pub fn acquire(
        &mut self,
        plugin_id: &str,
        driver_id: &str,
        session_id: &str,
        now: Instant,
    ) -> Decision {
        let reject = |session_id: &str, reason: RejectReason| Decision::Rejected {
            session_id: session_id.to_string(),
            reason,
        };

        let Some(spec) = self.specs.get(plugin_id).cloned() else {
            return reject(session_id, RejectReason::UnknownPlugin);
        };
        if !spec.knows_driver(driver_id) {
            return reject(
                session_id,
                RejectReason::UnknownDriver {
                    driver_id: driver_id.to_string(),
                },
            );
        }
        if self.sessions.contains_key(session_id) {
            return reject(session_id, RejectReason::SessionIdReused);
        }

        let indexes = self.indexes_of(plugin_id);

        // 判死的实例会挡住该插件的全部新会话（规则 5：先人工介入，不静默起来）
        if indexes
            .iter()
            .any(|i| self.instances[&(plugin_id.to_string(), *i)].state == ProcessState::Error)
        {
            return reject(session_id, RejectReason::NeedsManualRestart);
        }

        // ① 有 Ready 实例 → 按其并发策略决定"直接开"还是"排队"
        let ready: Vec<usize> = indexes
            .iter()
            .copied()
            .filter(|i| self.instances[&(plugin_id.to_string(), *i)].state == ProcessState::Ready)
            .collect();

        if !ready.is_empty() {
            let key = match spec.concurrency {
                // 并行：挑会话最少的实例（即文档说的"轮询分配"），同数取小序号
                Concurrency::Parallel => *ready
                    .iter()
                    .min_by_key(|i| {
                        let inst = &self.instances[&(plugin_id.to_string(), **i)];
                        (inst.session_count(), inst.index)
                    })
                    .expect("ready 非空"),
                // 串行：只有空着的实例能直接开
                Concurrency::Serial => match ready
                    .iter()
                    .find(|i| self.instances[&(plugin_id.to_string(), **i)].session_count() == 0)
                {
                    Some(i) => *i,
                    None => {
                        // 都忙着 → 排到最短的队
                        let target = *ready
                            .iter()
                            .min_by_key(|i| {
                                self.instances[&(plugin_id.to_string(), **i)].queue_len()
                            })
                            .expect("ready 非空");
                        return self.enqueue(plugin_id, target, session_id, now);
                    }
                },
            };

            self.open_session(plugin_id, driver_id, session_id, key, now);
            return Decision::Open {
                plugin_id: plugin_id.to_string(),
                index: key,
                session_id: session_id.to_string(),
            };
        }

        // ② 没有 Ready 实例：还有额度就起一个
        if indexes.len() < spec.max_instances {
            let index = (0..spec.max_instances)
                .find(|i| !indexes.contains(i))
                .unwrap_or(indexes.len());
            let mut inst = ProcessInstance::new(plugin_id, index, now);
            inst.queue.push_back(session_id.to_string());
            self.instances.insert((plugin_id.to_string(), index), inst);
            return Decision::Spawn {
                plugin_id: plugin_id.to_string(),
                index,
                session_id: session_id.to_string(),
            };
        }

        // ③ 额度用尽且都在启动中 → 排到最短的队（等 on_process_ready 放行）
        let target = *indexes
            .iter()
            .min_by_key(|i| self.instances[&(plugin_id.to_string(), **i)].queue_len())
            .expect("indexes 非空");
        self.enqueue(plugin_id, target, session_id, now)
    }

    fn indexes_of(&self, plugin_id: &str) -> Vec<usize> {
        self.instances
            .keys()
            .filter(|(p, _)| p == plugin_id)
            .map(|(_, i)| *i)
            .collect()
    }

    fn enqueue(
        &mut self,
        plugin_id: &str,
        index: usize,
        session_id: &str,
        now: Instant,
    ) -> Decision {
        let key = (plugin_id.to_string(), index);
        let inst = self.instances.get_mut(&key).expect("实例应存在");
        if inst.queue_len() >= QUEUE_MAX_LEN {
            return Decision::Rejected {
                session_id: session_id.to_string(),
                reason: RejectReason::QueueFull { index },
            };
        }
        inst.queue.push_back(session_id.to_string());
        inst.last_active = now;
        Decision::Queued {
            plugin_id: plugin_id.to_string(),
            index,
            session_id: session_id.to_string(),
            position: inst.queue_len(),
        }
    }

    fn open_session(
        &mut self,
        plugin_id: &str,
        driver_id: &str,
        session_id: &str,
        index: usize,
        now: Instant,
    ) {
        let key = (plugin_id.to_string(), index);
        if let Some(inst) = self.instances.get_mut(&key) {
            inst.sessions.insert(session_id.to_string());
            inst.last_active = now;
        }
        self.sessions.insert(
            session_id.to_string(),
            SessionRecord {
                plugin_id: plugin_id.to_string(),
                driver_id: driver_id.to_string(),
                index,
            },
        );
    }

    /// 会话结束（正常关闭 / 出错释放都走这里）。
    ///
    /// 返回该实例上被放行的排队会话（0 或 1 个）。
    pub fn release(&mut self, session_id: &str, now: Instant) -> Vec<Decision> {
        let Some(record) = self.sessions.remove(session_id) else {
            return Vec::new();
        };
        let key = (record.plugin_id.clone(), record.index);
        let Some(inst) = self.instances.get_mut(&key) else {
            return Vec::new();
        };
        inst.sessions.remove(session_id);
        inst.last_active = now;

        // 串行实例空出来了 → 放行队首
        self.drain_one(&record.plugin_id, record.index, now)
    }

    /// 实例就绪：登记 pid，并放行排队中的会话。
    pub fn on_process_ready(
        &mut self,
        plugin_id: &str,
        index: usize,
        pid: Option<u32>,
        now: Instant,
    ) -> Vec<Decision> {
        let key = (plugin_id.to_string(), index);
        let Some(inst) = self.instances.get_mut(&key) else {
            return Vec::new();
        };
        inst.state = ProcessState::Ready;
        inst.pid = pid;
        inst.ping_failures = 0;
        inst.last_active = now;
        self.drain_one(plugin_id, index, now)
    }

    /// 放行队首一个会话（若有），并登记它。
    fn drain_one(&mut self, plugin_id: &str, index: usize, now: Instant) -> Vec<Decision> {
        let Some(spec) = self.specs.get(plugin_id).cloned() else {
            return Vec::new();
        };
        let key = (plugin_id.to_string(), index);
        let Some(inst) = self.instances.get_mut(&key) else {
            return Vec::new();
        };
        if inst.state != ProcessState::Ready {
            return Vec::new();
        }

        // 并行实例不会被"占用"（可共享），故只有串行才需要放行队列
        if spec.concurrency == Concurrency::Parallel && inst.queue.is_empty() {
            return Vec::new();
        }
        // 串行：有会话在跑就别放
        if spec.concurrency == Concurrency::Serial && !inst.sessions.is_empty() {
            return Vec::new();
        }

        let Some(session_id) = inst.queue.pop_front() else {
            return Vec::new();
        };

        // 队首要拿哪个 driver？排队时没记，这里用清单的第一个（单 driver 插件是常态；
        // 多 driver 的排队放行需要更细的记录，留到接真实靶子时再补）。
        let driver_id = spec.drivers.first().cloned().unwrap_or_default();
        self.open_session(plugin_id, &driver_id, &session_id, index, now);
        vec![Decision::Open {
            plugin_id: plugin_id.to_string(),
            index,
            session_id,
        }]
    }

    /// 进程退出 / 管道断开。
    ///
    /// 无论哪种原因，该实例的**全部会话与排队会话都立刻失效**（如实报错，不静默重连）；
    /// 区别只在于：崩溃会把实例标记成 `Error`（要求人工重启），正常关闭则直接移除。
    pub fn on_process_exit(
        &mut self,
        plugin_id: &str,
        index: usize,
        cause: ExitCause,
        now: Instant,
    ) -> Vec<Event> {
        let key = (plugin_id.to_string(), index);
        let Some(mut inst) = self.instances.remove(&key) else {
            return Vec::new();
        };

        let mut events = Vec::new();
        let reason = match cause {
            ExitCause::Crash => format!("{plugin_id}#{index} 进程退出"),
            ExitCause::Shutdown => format!("{plugin_id}#{index} 已关闭"),
        };

        // 在跑的和排队的都要交还调用方
        for session_id in inst.sessions.iter().chain(inst.queue.iter()) {
            self.sessions.remove(session_id);
            events.push(Event::SessionInvalidated {
                session_id: session_id.clone(),
                reason: reason.clone(),
            });
        }
        inst.sessions.clear();
        inst.queue.clear();

        match cause {
            ExitCause::Crash => {
                inst.state = ProcessState::Error;
                inst.pid = None;
                inst.last_active = now;
                events.push(Event::ProcessError {
                    plugin_id: plugin_id.to_string(),
                    index,
                    reason: reason.clone(),
                });
                self.instances.insert(key, inst);
            }
            ExitCause::Shutdown => { /* 移除即可，上面已 remove */ }
        }

        events
    }

    /// ping 结果（成功清零失败计数；连续失败到顶判死）。
    pub fn on_ping(&mut self, plugin_id: &str, index: usize, ok: bool, now: Instant) -> Vec<Event> {
        let key = (plugin_id.to_string(), index);
        let Some(inst) = self.instances.get_mut(&key) else {
            return Vec::new();
        };
        inst.last_active = now;

        if ok {
            inst.ping_failures = 0;
            return Vec::new();
        }

        inst.ping_failures += 1;
        if inst.ping_failures < PING_MAX_FAILURES {
            return Vec::new();
        }

        // 判死：把该实例的会话与排队全部交还，并标记 Error
        let failures = inst.ping_failures;
        inst.state = ProcessState::Error;
        let mut events = Vec::new();
        let session_ids: Vec<SessionId> = inst
            .sessions
            .iter()
            .cloned()
            .chain(inst.queue.iter().cloned())
            .collect();
        inst.sessions.clear();
        inst.queue.clear();
        for session_id in session_ids {
            self.sessions.remove(&session_id);
            events.push(Event::SessionInvalidated {
                session_id,
                reason: format!("{plugin_id}#{index} 连续 {failures} 次心跳失败"),
            });
        }
        events.push(Event::ProcessError {
            plugin_id: plugin_id.to_string(),
            index,
            reason: format!("连续 {failures} 次心跳失败"),
        });
        events
    }

    /// 空闲回收：返回要收掉的实例。
    ///
    /// 只收「就绪 + 无会话 + 无排队 + 闲置超时」的；**正在启动的不动**（还没用就杀是抖动源）。
    pub fn sweep_idle(&mut self, now: Instant, idle_timeout: Duration) -> Vec<Action> {
        let mut actions = Vec::new();
        let stale: Vec<(PluginId, usize)> = self
            .instances
            .iter()
            .filter(|(_, inst)| inst.is_idle())
            .filter(|(_, inst)| now.duration_since(inst.last_active) >= idle_timeout)
            .map(|((p, i), _)| (p.clone(), *i))
            .collect();

        for (plugin_id, index) in stale {
            self.instances.remove(&(plugin_id.clone(), index));
            actions.push(Action::Kill {
                plugin_id,
                index,
                reason: KillReason::Idle,
            });
        }
        actions
    }

    /// 需要发心跳的实例（就绪的都要；判死/启动中的不用）。
    pub fn ping_targets(&self) -> Vec<Action> {
        self.instances
            .iter()
            .filter(|(_, inst)| inst.state == ProcessState::Ready)
            .map(|((p, i), _)| Action::Ping {
                plugin_id: p.clone(),
                index: *i,
            })
            .collect()
    }

    /// 人工重启（§4.1 规则 5 的"允许手动重启"）。
    ///
    /// 把 `Error` 实例清成 `Starting` 并重新下发 `Spawn`；若该插件一个实例都没有（已被
    /// 正常关闭过），也补一个。
    pub fn manual_restart(&mut self, plugin_id: &str, now: Instant) -> Vec<Action> {
        if !self.specs.contains_key(plugin_id) {
            return Vec::new();
        }

        let mut actions = Vec::new();
        let indexes = self.indexes_of(plugin_id);

        for index in indexes {
            let key = (plugin_id.to_string(), index);
            let Some(inst) = self.instances.get_mut(&key) else {
                continue;
            };
            if inst.state != ProcessState::Error {
                continue;
            }
            inst.state = ProcessState::Starting;
            inst.restart_count += 1;
            inst.ping_failures = 0;
            inst.last_active = now;
            actions.push(Action::Spawn {
                plugin_id: plugin_id.to_string(),
                index,
            });
        }

        if actions.is_empty() && self.instances_of(plugin_id).is_empty() {
            let index = 0;
            self.instances.insert(
                (plugin_id.to_string(), index),
                ProcessInstance::new(plugin_id, index, now),
            );
            actions.push(Action::Spawn {
                plugin_id: plugin_id.to_string(),
                index,
            });
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    fn registry(spec: ProcessSpec) -> Registry {
        let mut r = Registry::new();
        r.register_spec("oracle-jdbc", spec);
        r
    }

    fn parallel(n: usize) -> ProcessSpec {
        ProcessSpec::new(["oracle-jdbc"], n, Concurrency::Parallel)
    }

    /// 直接摆一个**已就绪**实例。
    ///
    /// 只用于"被测的不是启动流程"的场景（启动流程本身由
    /// `spawns_only_when_no_instance_is_ready` 覆盖）。这样每个用例只剩
    /// "摆好前提 → 断言被考察的那条规则"，不用每处重复一遍启动路径。
    fn arrange_ready(r: &mut Registry, index: usize, pid: u32, now: Instant) {
        let mut inst = ProcessInstance::new("oracle-jdbc", index, now);
        inst.state = ProcessState::Ready;
        inst.pid = Some(pid);
        r.instances.insert(("oracle-jdbc".into(), index), inst);
    }

    /// 规则 1：**进程按 plugin_id 去重，不是每连接一进程**。
    #[test]
    fn one_process_serves_many_sessions() {
        let now = t0();
        let mut r = registry(parallel(3));
        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now);
        assert!(matches!(d, Decision::Spawn { index: 0, .. }), "{d:?}");
        r.on_process_ready("oracle-jdbc", 0, Some(1234), now);

        for s in ["s2", "s3", "s4"] {
            let d = r.acquire("oracle-jdbc", "oracle-jdbc", s, now);
            assert!(
                matches!(d, Decision::Open { index: 0, .. }),
                "同一插件应复用进程：{d:?}"
            );
        }
        assert_eq!(r.instances_of("oracle-jdbc").len(), 1);
        assert_eq!(r.session_count(), 4);
    }

    /// 规则 3：`serial` 时第二个会话排队，释放后放行。
    #[test]
    fn serial_concurrency_queues_the_second_session() {
        let now = t0();
        let mut r = registry(ProcessSpec::serial_single(["oracle-jdbc"]));
        arrange_ready(&mut r, 0, 1, now);

        let d1 = r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now);
        assert!(matches!(d1, Decision::Open { .. }), "{d1:?}");

        let d2 = r.acquire("oracle-jdbc", "oracle-jdbc", "s2", now);
        match d2 {
            Decision::Queued {
                position, index, ..
            } => {
                assert_eq!((position, index), (1, 0));
            }
            other => panic!("串行第二个会话应排队：{other:?}"),
        }

        // 释放 s1 → s2 被放行
        let released = r.release("s1", now);
        assert_eq!(released.len(), 1);
        match &released[0] {
            Decision::Open {
                session_id, index, ..
            } => {
                assert_eq!((session_id.as_str(), *index), ("s2", 0));
            }
            other => panic!("应放行队首：{other:?}"),
        }
        assert_eq!(r.session("s2").map(|s| s.index), Some(0));
    }

    /// 没有 Ready 实例但还有额度 → 起新实例；**不抢跑**已有的 Ready。
    #[test]
    fn spawns_only_when_no_instance_is_ready() {
        let now = t0();
        let mut r = registry(parallel(2));

        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now);
        assert!(matches!(d, Decision::Spawn { index: 0, .. }), "{d:?}");

        // 还没就绪 → 再要会话就再起一个（额度允许）
        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "s2", now);
        assert!(matches!(d, Decision::Spawn { index: 1, .. }), "{d:?}");
        assert_eq!(r.instances_of("oracle-jdbc").len(), 2);

        // 双双就绪后：并行会把会话分给会话数少的那个（轮询）
        let d1 = r.on_process_ready("oracle-jdbc", 0, Some(11), now);
        let d2 = r.on_process_ready("oracle-jdbc", 1, Some(22), now);
        assert_eq!(d1.len(), 1, "实例 0 就绪应放行 s1");
        assert_eq!(d2.len(), 1, "实例 1 就绪应放行 s2");

        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "s3", now);
        match d {
            Decision::Open { index, .. } => assert_eq!(index, 0, "应挑会话最少的实例"),
            other => panic!("{other:?}"),
        }
    }

    /// 规则 2 的护栏：喂一个该插件没声明的 driver 要被挡下（"声明与实现不符"）。
    #[test]
    fn unknown_driver_is_rejected() {
        let mut r = registry(ProcessSpec::serial_single(["oracle-jdbc"]));
        let d = r.acquire("oracle-jdbc", "mysql-jdbc", "s1", t0());
        assert!(
            matches!(
                d,
                Decision::Rejected {
                    reason: RejectReason::UnknownDriver { .. },
                    ..
                }
            ),
            "{d:?}"
        );
    }

    #[test]
    fn unknown_plugin_and_reused_session_id_are_rejected() {
        let now = t0();
        let mut r = registry(parallel(1));
        assert!(matches!(
            r.acquire("nope", "oracle-jdbc", "s1", now),
            Decision::Rejected {
                reason: RejectReason::UnknownPlugin,
                ..
            }
        ));

        r.on_process_ready("oracle-jdbc", 0, None, now);
        arrange_ready(&mut r, 0, 1, now);
        assert!(matches!(
            r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now),
            Decision::Open { .. }
        ));
        assert!(
            matches!(
                r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now),
                Decision::Rejected {
                    reason: RejectReason::SessionIdReused,
                    ..
                }
            ),
            "重复的 session id 不能静默覆盖"
        );
    }

    /// 队列有上限：满了要**如实拒绝**，不能把请求默默堆着。
    #[test]
    fn queue_has_a_hard_limit() {
        let now = t0();
        let mut r = registry(ProcessSpec::serial_single(["oracle-jdbc"]));
        arrange_ready(&mut r, 0, 1, now);

        assert!(matches!(
            r.acquire("oracle-jdbc", "oracle-jdbc", "busy", now),
            Decision::Open { .. }
        ));
        for i in 0..QUEUE_MAX_LEN {
            let d = r.acquire("oracle-jdbc", "oracle-jdbc", &format!("q{i}"), now);
            assert!(
                matches!(d, Decision::Queued { .. }),
                "第 {i} 个应排队：{d:?}"
            );
        }
        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "overflow", now);
        assert!(
            matches!(
                d,
                Decision::Rejected {
                    reason: RejectReason::QueueFull { index: 0 },
                    ..
                }
            ),
            "{d:?}"
        );
    }

    /// 规则 4 的一半：进程崩溃 → 全部会话（含排队）失效、标记 `Error`、**不静默重连**。
    #[test]
    fn crash_invalidates_every_session_and_blocks_new_ones() {
        let now = t0();
        let mut r = registry(ProcessSpec::serial_single(["oracle-jdbc"]));
        arrange_ready(&mut r, 0, 77, now);
        r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now);
        r.acquire("oracle-jdbc", "oracle-jdbc", "s2", now); // 排队

        let events = r.on_process_exit("oracle-jdbc", 0, ExitCause::Crash, now);

        let invalidated: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                Event::SessionInvalidated { session_id, .. } => Some(session_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            invalidated,
            vec!["s1", "s2"],
            "在跑的和排队的都要交还：{events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ProcessError { index: 0, .. })),
            "崩溃要报 ProcessError：{events:?}"
        );
        assert_eq!(r.session_count(), 0);

        // 不静默重连：新会话被挡，直到手动重启
        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "s3", now);
        assert!(
            matches!(
                d,
                Decision::Rejected {
                    reason: RejectReason::NeedsManualRestart,
                    ..
                }
            ),
            "{d:?}"
        );

        let actions = r.manual_restart("oracle-jdbc", now);
        assert_eq!(
            actions,
            vec![Action::Spawn {
                plugin_id: "oracle-jdbc".into(),
                index: 0
            }],
            "手动重启要重新下发 Spawn"
        );
        r.on_process_ready("oracle-jdbc", 0, Some(78), now);
        assert!(matches!(
            r.acquire("oracle-jdbc", "oracle-jdbc", "s4", now),
            Decision::Spawn { .. } | Decision::Open { .. } | Decision::Queued { .. }
        ));
        assert!(
            !matches!(
                r.acquire("oracle-jdbc", "oracle-jdbc", "s5", now),
                Decision::Rejected { .. }
            ),
            "重启后应恢复可用"
        );
    }

    /// 正常关闭与崩溃**要分开**：前者不该把插件标成错误。
    #[test]
    fn graceful_shutdown_does_not_mark_the_plugin_as_errored() {
        let now = t0();
        let mut r = registry(parallel(1));
        arrange_ready(&mut r, 0, 1, now);
        r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now);

        let events = r.on_process_exit("oracle-jdbc", 0, ExitCause::Shutdown, now);
        assert_eq!(events.len(), 1, "只该有一条会话失效：{events:?}");
        assert!(matches!(events[0], Event::SessionInvalidated { .. }));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ProcessError { .. })),
            "正常关闭不该报错"
        );

        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "s2", now);
        assert!(matches!(d, Decision::Spawn { .. }), "应能重新起进程：{d:?}");
    }

    /// 规则 5：连续 2 次 ping 失败判死；中间成功一次就清零。
    #[test]
    fn ping_failures_mark_error_only_after_two_consecutive_failures() {
        let now = t0();
        let mut r = registry(parallel(1));
        arrange_ready(&mut r, 0, 1, now);
        r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now);

        assert!(
            r.on_ping("oracle-jdbc", 0, false, now).is_empty(),
            "一次失败不判死"
        );
        assert!(r.on_ping("oracle-jdbc", 0, true, now).is_empty());
        assert_eq!(
            r.instances_of("oracle-jdbc")[0].ping_failures,
            0,
            "成功应清零计数"
        );

        assert!(r.on_ping("oracle-jdbc", 0, false, now).is_empty());
        let events = r.on_ping("oracle-jdbc", 0, false, now);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ProcessError { index: 0, .. })),
            "{events:?}"
        );
        assert!(
            events.iter().any(
                |e| matches!(e, Event::SessionInvalidated { session_id, .. } if session_id == "s1")
            ),
            "判死要把会话交还：{events:?}"
        );
        assert_eq!(r.instances_of("oracle-jdbc")[0].state, ProcessState::Error);
    }

    /// 规则 4 的另一半：空闲回收只收"真空闲"的，且不早收。
    #[test]
    fn idle_sweep_reclaims_only_truly_idle_instances() {
        let now = t0();
        let mut r = registry(parallel(2));
        arrange_ready(&mut r, 0, 0, now);
        arrange_ready(&mut r, 1, 1, now);
        r.acquire("oracle-jdbc", "oracle-jdbc", "busy", now); // 落在 index 0

        // 未到时限：一个都不收
        assert!(
            r.sweep_idle(now + Duration::from_secs(60), IDLE_TIMEOUT)
                .is_empty(),
            "没到 30 分钟不该收"
        );

        // 到时限：有会话的不收，空闲的收
        let actions = r.sweep_idle(now + IDLE_TIMEOUT + Duration::from_secs(1), IDLE_TIMEOUT);
        assert_eq!(actions.len(), 1, "{actions:?}");
        match &actions[0] {
            Action::Kill { index, reason, .. } => {
                assert_eq!((*index, *reason), (1, KillReason::Idle))
            }
            other => panic!("{other:?}"),
        }
        let left = r.instances_of("oracle-jdbc");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].session_count(), 1, "有会话的实例必须留着");
    }

    /// 启动中的实例不参与空闲回收（还没用就杀是抖动源）。
    #[test]
    fn starting_instances_are_never_swept() {
        let now = t0();
        let mut r = registry(parallel(1));
        r.acquire("oracle-jdbc", "oracle-jdbc", "s1", now); // → Spawn，状态 Starting
        assert!(
            r.sweep_idle(now + IDLE_TIMEOUT * 10, IDLE_TIMEOUT)
                .is_empty(),
            "启动中不该被回收"
        );
    }

    #[test]
    fn ping_targets_are_the_ready_instances_only() {
        let now = t0();
        let mut r = registry(parallel(2));
        // index 0 还在启动（acquire 只下发 Spawn），index 1 已就绪
        let d = r.acquire("oracle-jdbc", "oracle-jdbc", "queued", now);
        assert!(matches!(d, Decision::Spawn { index: 0, .. }), "{d:?}");
        arrange_ready(&mut r, 1, 42, now);

        assert_eq!(
            r.ping_targets(),
            vec![Action::Ping {
                plugin_id: "oracle-jdbc".into(),
                index: 1
            }],
            "启动中的实例不该被心跳"
        );
    }

    /// 正常关闭后再"手动重启"：没有实例了也要补一个。
    #[test]
    fn manual_restart_recreates_an_instance_after_graceful_shutdown() {
        let now = t0();
        let mut r = registry(parallel(1));
        arrange_ready(&mut r, 0, 1, now);
        r.on_process_exit("oracle-jdbc", 0, ExitCause::Shutdown, now);
        assert!(r.instances_of("oracle-jdbc").is_empty());

        let actions = r.manual_restart("oracle-jdbc", now);
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert_eq!(r.instances_of("oracle-jdbc").len(), 1);
    }

    /// 没登记的插件不该被"重启"出一个实例来。
    #[test]
    fn manual_restart_of_unknown_plugin_is_a_noop() {
        let mut r = Registry::new();
        assert!(r.manual_restart("nope", t0()).is_empty());
    }

    /// 退出消息可能重复（我们主动 Kill 之后进程又报了一次退出）：不能把别人带走。
    #[test]
    fn duplicate_exit_notifications_are_tolerated() {
        let now = t0();
        let mut r = registry(parallel(1));
        arrange_ready(&mut r, 0, 1, now);

        assert!(
            r.on_process_exit("oracle-jdbc", 0, ExitCause::Shutdown, now)
                .is_empty()
        );
        assert!(
            r.on_process_exit("oracle-jdbc", 0, ExitCause::Crash, now)
                .is_empty(),
            "重复的退出通知应被忽略（实例已不在）"
        );
    }
}

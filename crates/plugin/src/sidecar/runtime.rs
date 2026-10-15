//! 插件运行时循环（宿主接线的主干）。
//!
//! 职责：周期驱动 [`SidecarSupervisor`]（事件落地 / 空闲回收 / 心跳判死），把
//! [`SupervisorEvent::InboundRequest`]（反向 `host.*` 请求）接到
//! [`HostApiHandler`] 并回帧，其余事件如实打日志。
//!
//! ## 为什么必须有一个循环
//!
//! supervisor 的设计是「时间是入参」（见 `supervisor.rs` 模块头）：`sweep_idle` /
//! `ping_all` / `drain_events` 全要宿主主动 tick。此前**只有测试在 tick** ——
//! 宿主不 tick 的话，进程崩溃没人知道、空闲进程永不回收、反向请求永远没人回。
//!
//! ## 接线口径（与 `host.rs` 模块头一致）
//!
//! - 权限 = 清单 `permissions.host`（点分 glob），经 [`HostApiHandler`] 门控，
//!   默认拒绝；插件未加载（查不到清单）按空权限处理 → `PermissionDenied`。
//! - 回帧 = 按 `(plugin_id, index)` 从 supervisor 拿连接句柄
//!   （[`SidecarSupervisor::instance_conn_handle`]），实例已收掉就丢弃
//!   （连接都没了，回帧也没人收）。
//! - 宿主实现 = [`DefaultHostProvider`]（storage 真落盘 / log 走 tracing，
//!   其余诚实 `Unsupported`）。
//!
//! ## 未接线（P4）
//!
//! 崩溃 / 会话失效 / 通知等事件目前只打日志；转成 UI 通知（通知中心 / 角标）
//! 是 P4 注册表与界面的活。

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;

use super::host::HostApiHandler;
use super::host_provider::DefaultHostProvider;
use super::supervisor::{SidecarSupervisor, SupervisorEvent};
use crate::manager::PluginManager;

/// 心跳 / 空闲回收 / 事件落地的拍频（每拍做全部三件事）。
pub const RUNTIME_TICK: Duration = Duration::from_secs(5);
/// 空闲实例回收超时（对齐 `lifecycle.rs` 的 30min 口径）。
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 插件运行时循环：持有 `PluginManager`，按固定拍频驱动 sidecar 运行时。
#[derive(Clone)]
pub struct PluginRuntimeLoop {
    manager: Arc<PluginManager>,
    tick: Duration,
}

impl PluginRuntimeLoop {
    pub fn new(manager: Arc<PluginManager>, tick: Duration) -> Self {
        Self { manager, tick }
    }

    /// 跑一拍：热插拔扫描 → 事件落地 → 分发 → 空闲回收 → 心跳判死。
    ///
    /// 同一次入口只用一个 `now`（supervisor 纪律：规则判定可重放）；
    /// 分发放在回收之前——先把反向请求答完，再收拾空闲实例，顺序上不会误杀。
    pub async fn tick_once(&self) {
        // 热插拔（目录即插件）：新目录自动加载/激活、目录消失自动停用、
        // 清单变化置 needs_reload（不热生效）。5s 一拍，无需 watcher。
        match self.manager.scan_and_diff_plugins().await {
            Ok(diff) => {
                if !diff.added.is_empty() || !diff.removed.is_empty() || !diff.changed.is_empty() {
                    tracing::info!(
                        added = ?diff.added,
                        removed = ?diff.removed,
                        changed = ?diff.changed,
                        "插件热插拔扫描：目录有变化"
                    );
                }
            }
            Err(e) => tracing::warn!(error = %e, "插件热插拔扫描失败"),
        }

        // 先绑定 Arc 再 lock：`sidecar_supervisor()` 返回临时 Arc，直接链式
        // `.lock()` 会在语句结束 drop 临时值 → 借用悬垂（E0716）。
        let sup_arc = self.manager.sidecar_supervisor();
        let mut sup = sup_arc.lock().await;
        let now = Instant::now();
        let _ = sup.drain_events(now).await;
        while let Some(event) = sup.try_event() {
            dispatch_event(&self.manager, &mut sup, event).await;
        }
        let _ = sup.sweep_idle(now, IDLE_TIMEOUT).await;
        let _ = sup.ping_all(now).await;
    }

    /// 后台循环（tokio 任务）。句柄被 drop 即 detach（任务生命周期随 runtime）。
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                self.tick_once().await;
                tokio::time::sleep(self.tick).await;
            }
        })
    }
}

/// 分发一个 supervisor 事件（`InboundRequest` 有响应，其余如实打日志）。
async fn dispatch_event(
    manager: &PluginManager,
    sup: &mut SidecarSupervisor,
    event: SupervisorEvent,
) {
    match event {
        SupervisorEvent::InboundRequest {
            plugin_id,
            index,
            id,
            method,
            params,
            ..
        } => {
            let Some(conn) = sup.instance_conn_handle(&plugin_id, index) else {
                // 实例已收掉：回帧也没人收（连接都没了），按丢弃处理
                tracing::warn!(plugin = %plugin_id, method = %method, "反向请求到达但实例已收掉，丢弃");
                return;
            };
            // 权限 = 清单 permissions.host；插件未加载 = 空权限 → 默认拒绝
            let allowed = manager
                .get_loaded_plugin(&plugin_id)
                .map(|p| p.manifest.permissions.host)
                .unwrap_or_default();
            let mut handler = HostApiHandler::new(DefaultHostProvider::new(&plugin_id), allowed);
            match handler.handle(&method, params) {
                Ok(result) => {
                    if let Err(e) = conn.reply(id, result).await {
                        tracing::debug!(plugin = %plugin_id, method = %method, error = ?e, "反向请求回帧失败");
                    }
                }
                Err(err) => {
                    let (code, message) = err.to_rpc_error();
                    if let Err(e) = conn.reply_error(id, code, message).await {
                        tracing::debug!(plugin = %plugin_id, method = %method, error = ?e, "反向请求错误回帧失败");
                    }
                }
            }
        }
        // 流数据（SSH shell stdout / 端口转发数据）：宿主侧暂没有消费者
        // （P4 终端面板 / 隧道管理面板会订阅），先打 debug 日志；
        // 注意只记元信息，**不记数据本体**（可能很大，且可能是敏感内容）。
        SupervisorEvent::StreamData {
            plugin_id,
            channel_id,
            data,
            ..
        } => {
            tracing::debug!(
                plugin = %plugin_id,
                channel = %channel_id,
                bytes = data.len(),
                "插件流数据上行"
            );
        }
        SupervisorEvent::StreamEof {
            plugin_id,
            channel_id,
            ..
        } => {
            tracing::debug!(plugin = %plugin_id, channel = %channel_id, "插件流 EOF");
        }
        // 其余事件（崩溃 / 会话失效 / 通知 / 线路问题）如实打日志；
        // 转 UI 通知是 P4 的活（那层要订阅并渲染成界面元素）。
        other => {
            tracing::debug!(?other, "插件运行时事件");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::conn::SidecarConn;
    use crate::sidecar::proto::read_frame;
    use serde_json::json;

    /// 空拍：没有实例 / 没有事件时 tick 不 panic、不卡住。
    #[tokio::test]
    async fn tick_once_with_nothing_is_a_noop() {
        let manager = Arc::new(PluginManager::new(std::path::PathBuf::new()));
        let loop_ = PluginRuntimeLoop::new(manager, Duration::from_millis(1));
        loop_.tick_once().await;
        // 没断言即通过：跑一拍不 panic 就是这条测试的全部
    }

    /// 反向请求端到端（duplex 对端）：插件发 host.log → 循环分发 → 对端收到
    /// 同一 id 的响应帧。用 log（不落盘）验证整条接线，权限路径见下一条。
    #[tokio::test]
    async fn inbound_log_request_gets_a_reply() {
        // 权限：插件未加载 → 空权限 → PermissionDenied（-32001），
        // 但请求仍然被处理并回帧（对端拿得到错误，而不是干等超时）。
        // 直接测 dispatch 的下层：HostApiHandler + reply 回帧（权限闸路径），
        // 上层实例表接线由 supervisor 的事件泵测试 + instance_conn_handle 覆盖。

        let (host, peer) = tokio::io::duplex(1 << 16);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, _events) = SidecarConn::spawn(host_r, host_w);
        let (mut pr, pw) = tokio::io::split(peer);

        let allowed: Vec<String> = vec![];
        let mut handler = HostApiHandler::new(DefaultHostProvider::new("com.example.ext"), allowed);
        let err = handler.handle("host.log", json!({"level": "info", "message": "hi"})).unwrap_err();
        let (code, message) = err.to_rpc_error();
        assert_eq!(code, -32001);

        // 模拟 dispatch 的回帧动作（与 dispatch_event 同一条代码路径）
        let _ = conn.reply_error(7, code, message).await.unwrap();

        // 对端读回响应帧
        let frame = read_frame(&mut pr).await.expect("应有响应帧");
        let v: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["error"]["code"], -32001);

        conn.shutdown().await;
        drop(pw);
    }

    /// dispatch_event 的权限通过路径（handler 收合法权限后真执行）：storage 落盘
    /// 已由 provider 单测覆盖，这里钉住「权限 glob 决定能不能进 handler」。
    #[tokio::test]
    async fn inbound_with_permission_reaches_provider() {
        let (host, peer) = tokio::io::duplex(1 << 16);
        let (host_r, host_w) = tokio::io::split(host);
        let (conn, _events) = SidecarConn::spawn(host_r, host_w);
        let (mut pr, pw) = tokio::io::split(peer);

        let allowed: Vec<String> = vec!["logs.write".into()];
        let mut handler = HostApiHandler::new(DefaultHostProvider::new("com.example.ext"), allowed);

        // logs.write 放行 → provider.log 成功 → 回帧 result: null
        let out = handler
            .handle("host.log", json!({"level": "info", "message": "hi", "fields": {}}))
            .unwrap();
        assert_eq!(out, serde_json::Value::Null);
        conn.reply(8, out).await.unwrap();
        let frame = read_frame(&mut pr).await.expect("应有响应帧");
        let v: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(v["id"], 8);
        assert!(v["result"].is_null());

        // storage.read 未声明（只给了 logs.write）→ storage.get 被拒（-32001）
        let err = handler
            .handle("host.storage.get", json!({"key": "k"}))
            .unwrap_err();
        let (code, message) = err.to_rpc_error();
        assert_eq!(code, -32001);
        conn.reply_error(9, code, message).await.unwrap();
        let frame = read_frame(&mut pr).await.expect("应有响应帧");
        let v: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(v["id"], 9);
        assert_eq!(v["error"]["code"], -32001);

        conn.shutdown().await;
        drop(pw);
        drop(pr);
    }

    /// 未知 host 方法：权限闸先于分发 → -32601（方法不存在）。
    #[tokio::test]
    async fn unknown_host_method_is_32601() {
        let allowed: Vec<String> = vec!["*".into()];
        let mut handler = HostApiHandler::new(DefaultHostProvider::new("x"), allowed);
        let err = handler.handle("host.nope", json!({})).unwrap_err();
        assert_eq!(err.to_rpc_error().0, -32601);
    }
}

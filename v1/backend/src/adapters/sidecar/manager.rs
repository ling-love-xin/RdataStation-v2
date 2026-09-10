//! Sidecar 进程管理器
//!
//! 负责 Go Sidecar 进程的生命周期管理：
//! - 启动 Sidecar 进程
//! - 监听端口发现
//! - 健康检查
//! - 优雅停止

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::{SidecarConfig, SidecarError, SidecarStatus};

/// Sidecar 进程管理器
pub struct SidecarManager {
    /// 配置
    config: SidecarConfig,
    /// 进程状态
    status: Arc<Mutex<SidecarStatus>>,
    /// 子进程句柄
    child: Arc<Mutex<Option<Child>>>,
    /// Sidecar 监听端口
    port: Arc<Mutex<Option<u16>>>,
    /// 健康检查任务
    health_check_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// 停止信号发送器
    stop_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl SidecarManager {
    /// 创建新的 Sidecar 管理器
    pub fn new(config: Option<SidecarConfig>) -> Self {
        Self {
            config: config.unwrap_or_default(),
            status: Arc::new(Mutex::new(SidecarStatus::Stopped)),
            child: Arc::new(Mutex::new(None)),
            port: Arc::new(Mutex::new(None)),
            health_check_task: Arc::new(Mutex::new(None)),
            stop_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// 获取状态锁（带错误处理）
    fn lock_status(&self) -> Result<std::sync::MutexGuard<SidecarStatus>, SidecarError> {
        self.status
            .lock()
            .map_err(|e| SidecarError::Internal(format!("Mutex poisoned: {}", e)))
    }

    /// 获取子进程锁（带错误处理）
    fn lock_child(&self) -> Result<std::sync::MutexGuard<Option<Child>>, SidecarError> {
        self.child
            .lock()
            .map_err(|e| SidecarError::Internal(format!("Mutex poisoned: {}", e)))
    }

    /// 获取端口锁（带错误处理）
    fn lock_port(&self) -> Result<std::sync::MutexGuard<Option<u16>>, SidecarError> {
        self.port
            .lock()
            .map_err(|e| SidecarError::Internal(format!("Mutex poisoned: {}", e)))
    }

    /// 获取停止信号锁（带错误处理）
    fn lock_stop_tx(&self) -> Result<std::sync::MutexGuard<Option<oneshot::Sender<()>>>, SidecarError> {
        self.stop_tx
            .lock()
            .map_err(|e| SidecarError::Internal(format!("Mutex poisoned: {}", e)))
    }

    /// 获取健康检查任务锁（带错误处理）
    fn lock_health_check_task(&self) -> Result<std::sync::MutexGuard<Option<JoinHandle<()>>>, SidecarError> {
        self.health_check_task
            .lock()
            .map_err(|e| SidecarError::Internal(format!("Mutex poisoned: {}", e)))
    }

    /// 尝试设置状态（忽略锁中毒错误，用于错误恢复路径）
    fn try_set_status(&self, new_status: SidecarStatus) {
        if let Ok(mut status) = self.status.lock() {
            *status = new_status;
        }
    }

    /// 启动 Sidecar 进程
    pub async fn start(&self) -> Result<u16, SidecarError> {
        // 检查当前状态
        {
            let status = self.lock_status()?;
            if *status != SidecarStatus::Stopped {
                return Err(SidecarError::ProcessStartError(
                    "Sidecar is already running or starting".to_string(),
                ));
            }
        }

        // 更新状态为正在启动
        *self.lock_status()? = SidecarStatus::Starting;

        // 构建命令
        let mut cmd = Command::new(&self.config.binary_path);
        if self.config.debug {
            cmd.arg("--debug");
        }

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        // 启动进程
        let mut child = cmd.spawn().map_err(|e| {
            // 尽可能更新状态，忽略锁错误
            let _ = self.try_set_status(SidecarStatus::Error);
            SidecarError::ProcessStartError(format!("Failed to start Sidecar: {}", e))
        })?;

        // 从 stdout 读取端口号
        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = self.try_set_status(SidecarStatus::Error);
            SidecarError::ProcessStartError("Failed to capture Sidecar stdout".to_string())
        })?;

        // 异步读取端口
        let port = tokio::task::spawn_blocking(move || {
            use std::io::{self, BufRead};

            let reader = io::BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(line) = line {
                    if let Ok(port) = line.trim().parse::<u16>() {
                return Ok(port);
            }
                }
            }
            Err(SidecarError::ProcessStartError(
                "Failed to read port from Sidecar stdout".to_string(),
            ))
        })
        .await
        .map_err(|e| {
            let _ = self.try_set_status(SidecarStatus::Error);
            SidecarError::ProcessStartError(format!("Task join error: {}", e))
        })??;

        // 保存进程句柄
        *self.lock_child()? = Some(child);
        *self.lock_port()? = Some(port);
        *self.lock_status()? = SidecarStatus::Running;

        // 启动健康检查
        self.start_health_check().await?;

        Ok(port)
    }

    /// 停止 Sidecar 进程
    pub async fn stop(&self) -> Result<(), SidecarError> {
        // 发送停止信号给健康检查任务
        if let Some(tx) = self.lock_stop_tx()?.take() {
            let _ = tx.send(());
        }

        // 等待健康检查任务结束
        if let Some(handle) = self.lock_health_check_task()?.take() {
            let _ = handle.await;
        }

        // 更新状态
        *self.lock_status()? = SidecarStatus::Stopping;

        // 停止进程
        if let Some(mut child) = self.lock_child()?.take() {
            // 尝试优雅停止
            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                if let Ok(pid) = child.id() {
                    let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                }
            }

            // 等待进程退出或强制停止
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                let _ = child.wait();
            })
            .await;

            // 如果还没停止，强制杀死
            let _ = child.kill();
            let _ = child.wait();
        }

        // 更新状态
        *self.lock_status()? = SidecarStatus::Stopped;
        *self.lock_port()? = None;

        Ok(())
    }

    /// 获取 Sidecar 当前状态
    pub fn status(&self) -> SidecarStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 获取 Sidecar 监听端口
    pub fn port(&self) -> Option<u16> {
        *self.port.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 启动健康检查
    async fn start_health_check(&self) -> Result<(), SidecarError> {
        let (stop_tx, stop_rx) = oneshot::channel();
        let status_clone = Arc::clone(&self.status);
        let port_clone = Arc::clone(&self.port);
        let child_clone = Arc::clone(&self.child);
        let interval = Duration::from_millis(self.config.health_check_interval_ms);
        let config_clone = self.config.clone();

        *self.lock_stop_tx()? = Some(stop_tx);

        let handle = tokio::spawn(async move {
            let mut stop_rx = stop_rx;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        break;
                    }
                    _ = tokio::time::sleep(interval) => {
                        if let Some(port) = *port_clone.lock().unwrap_or_else(|e| e.into_inner()) {
                            // 执行健康检查
                            let client = reqwest::Client::new();
                            let url = format!("http://localhost:{}/rpc", port);
                            let health_request = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": "connectors.list_available",
                                "id": 1
                            });

                            if let Err(e) = client.post(&url)
                                .json(&health_request)
                                .timeout(Duration::from_secs(5))
                                .send()
                                .await
                            {
                                tracing::warn!("Sidecar health check failed: {}", e);

                                // 检查进程是否还在运行
                                let mut child_lock = child_clone.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(child) = child_lock.as_mut() {
                                    if let Ok(Some(_)) = child.try_wait() {
                                        tracing::error!("Sidecar process died, marking as error");
                                        *status_clone.lock().unwrap_or_else(|e| e.into_inner()) = SidecarStatus::Error;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        *self.lock_health_check_task()? = Some(handle);

        Ok(())
    }

    /// 重启 Sidecar
    pub async fn restart(&self) -> Result<u16, SidecarError> {
        let _ = self.stop().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.start().await
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // 尝试停止进程
        if *self.status.lock().unwrap_or_else(|e| e.into_inner()) == SidecarStatus::Running {
            let _ = self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SidecarConfig::default();
        assert_eq!(config.binary_path, "./rdata-sidecar");
        assert_eq!(config.debug, false);
        assert_eq!(config.startup_timeout_ms, 10000);
    }

    #[test]
    fn test_manager_creation() {
        let manager = SidecarManager::new(None);
        assert_eq!(manager.status(), SidecarStatus::Stopped);
        assert!(manager.port().is_none());
    }
}

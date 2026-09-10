//! 端口自动协商模块
//!
//! 提供端口自动发现、协商和冲突解决功能
//! 支持动态端口分配和端口范围管理

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::sync::{Arc, Mutex};

use crate::core::error::{CommonError, CoreError, CoreResult};

/// 默认端口范围
pub const DEFAULT_PORT_RANGE: PortRange = PortRange {
    start: 10000,
    end: 20000,
};

/// 常用数据库端口
pub const COMMON_DB_PORTS: &[u16] = &[
    3306,  // MySQL
    5432,  // PostgreSQL
    1433,  // SQL Server
    1521,  // Oracle
    27017, // MongoDB
    6379,  // Redis
    9042,  // Cassandra
    8086,  // InfluxDB
    9200,  // Elasticsearch
    5433,  // PostgreSQL (alternate)
    3307,  // MySQL (alternate)
];

/// 端口范围
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    /// 创建新的端口范围
    pub const fn new(start: u16, end: u16) -> Self {
        Self { start, end }
    }

    /// 检查端口是否在范围内
    pub fn contains(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }

    /// 获取范围内的随机端口
    pub fn random_port(&self) -> u16 {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen_range(self.start..=self.end)
    }

    /// 获取范围迭代器
    pub fn iter(&self) -> impl Iterator<Item = u16> {
        self.start..=self.end
    }
}

impl Default for PortRange {
    fn default() -> Self {
        DEFAULT_PORT_RANGE
    }
}

/// 端口协商器
///
/// 管理端口分配和冲突解决
#[derive(Debug, Clone)]
pub struct PortNegotiator {
    /// 已分配的端口
    allocated_ports: Arc<Mutex<HashSet<u16>>>,
    /// 端口范围
    range: PortRange,
    /// 是否允许使用常用端口
    allow_common_ports: bool,
}

impl PortNegotiator {
    /// 创建新的端口协商器
    pub fn new() -> Self {
        Self {
            allocated_ports: Arc::new(Mutex::new(HashSet::new())),
            range: DEFAULT_PORT_RANGE,
            allow_common_ports: false,
        }
    }

    /// 创建带自定义范围的端口协商器
    pub fn with_range(range: PortRange) -> Self {
        Self {
            allocated_ports: Arc::new(Mutex::new(HashSet::new())),
            range,
            allow_common_ports: false,
        }
    }

    /// 允许使用常用端口
    pub fn allow_common_ports(mut self) -> Self {
        self.allow_common_ports = true;
        self
    }

    /// 协商端口
    ///
    /// 尝试获取一个可用端口，优先使用 preferred_port
    /// 如果 preferred_port 不可用，则在范围内寻找其他端口
    pub fn negotiate(&self, preferred_port: Option<u16>) -> CoreResult<u16> {
        // 1. 尝试使用首选端口
        if let Some(port) = preferred_port {
            if self.is_port_available(port)? {
                self.allocate_port(port)?;
                return Ok(port);
            }
        }

        // 2. 如果允许，尝试常用端口
        if self.allow_common_ports {
            for &port in COMMON_DB_PORTS {
                if self.is_port_available(port)? {
                    self.allocate_port(port)?;
                    return Ok(port);
                }
            }
        }

        // 3. 在范围内寻找可用端口
        self.find_available_port()
    }

    /// 协商本地端口（用于 SSH 隧道等）
    ///
    /// 优先使用 0（让系统自动分配），然后尝试特定范围
    pub fn negotiate_local_port(&self, preferred: Option<u16>) -> CoreResult<u16> {
        // 如果首选是 0，让系统自动分配
        if preferred == Some(0) {
            let port = self.bind_to_port(0)?;
            self.allocate_port(port)?;
            return Ok(port);
        }

        // 否则使用常规协商逻辑
        self.negotiate(preferred)
    }

    /// 检查端口是否可用
    pub fn is_port_available(&self, port: u16) -> CoreResult<bool> {
        // 检查是否已被分配
        if let Ok(allocated) = self.allocated_ports.lock() {
            if allocated.contains(&port) {
                return Ok(false);
            }
        }

        // 尝试绑定端口
        match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => {
                // 立即释放，只是测试
                drop(listener);
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// 查找可用端口
    fn find_available_port(&self) -> CoreResult<u16> {
        // 首先尝试范围内的随机端口（避免总是从 start 开始）
        for _ in 0..100 {
            let port = self.range.random_port();
            if self.is_port_available(port)? {
                self.allocate_port(port)?;
                return Ok(port);
            }
        }

        // 如果随机尝试失败，顺序扫描
        for port in self.range.iter() {
            if self.is_port_available(port)? {
                self.allocate_port(port)?;
                return Ok(port);
            }
        }

        Err(CoreError::Common(CommonError::General(format!(
            "No available port found in range {}-{}",
            self.range.start, self.range.end
        ))))
    }

    /// 绑定到指定端口并返回实际分配的端口
    fn bind_to_port(&self, port: u16) -> CoreResult<u16> {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let listener = TcpListener::bind(addr).map_err(|e| {
            CoreError::Common(CommonError::General(format!(
                "Failed to bind to port {}: {}",
                port, e
            )))
        })?;

        let actual_port = listener
            .local_addr()
            .map_err(|e| {
                CoreError::Common(CommonError::General(format!(
                    "Failed to get local address: {}",
                    e
                )))
            })?
            .port();

        // 立即释放监听器
        drop(listener);

        Ok(actual_port)
    }

    /// 分配端口
    fn allocate_port(&self, port: u16) -> CoreResult<()> {
        let mut allocated = self.allocated_ports.lock().map_err(|_| {
            CoreError::Common(CommonError::General(
                "Failed to lock allocated ports".to_string(),
            ))
        })?;

        if allocated.contains(&port) {
            return Err(CoreError::Common(CommonError::General(format!(
                "Port {} is already allocated",
                port
            ))));
        }

        allocated.insert(port);
        Ok(())
    }

    /// 释放端口
    pub fn release_port(&self, port: u16) -> CoreResult<()> {
        let mut allocated = self.allocated_ports.lock().map_err(|_| {
            CoreError::Common(CommonError::General(
                "Failed to lock allocated ports".to_string(),
            ))
        })?;

        allocated.remove(&port);
        Ok(())
    }

    /// 获取已分配端口列表
    pub fn get_allocated_ports(&self) -> CoreResult<Vec<u16>> {
        let allocated = self.allocated_ports.lock().map_err(|_| {
            CoreError::Common(CommonError::General(
                "Failed to lock allocated ports".to_string(),
            ))
        })?;

        Ok(allocated.iter().copied().collect())
    }

    /// 批量协商多个端口
    pub fn negotiate_multiple(&self, count: usize) -> CoreResult<Vec<u16>> {
        let mut ports = Vec::with_capacity(count);

        for _ in 0..count {
            ports.push(self.negotiate(None)?);
        }

        Ok(ports)
    }

    /// 协商连续端口范围
    pub fn negotiate_range(&self, start: u16, count: usize) -> CoreResult<Vec<u16>> {
        let mut ports = Vec::with_capacity(count);
        let mut current = start;

        for _ in 0..count {
            // 查找从 current 开始的下一个可用端口
            loop {
                if !self.range.contains(current) {
                    // 释放已分配的端口
                    for port in &ports {
                        let _ = self.release_port(*port);
                    }
                    return Err(CoreError::Common(CommonError::General(format!(
                        "Cannot find {} consecutive ports starting from {}",
                        count, start
                    ))));
                }

                if self.is_port_available(current)? {
                    self.allocate_port(current)?;
                    ports.push(current);
                    current += 1;
                    break;
                }

                current += 1;
            }
        }

        Ok(ports)
    }

    /// 清空所有分配的端口
    pub fn clear(&self) -> CoreResult<()> {
        let mut allocated = self.allocated_ports.lock().map_err(|_| {
            CoreError::Common(CommonError::General(
                "Failed to lock allocated ports".to_string(),
            ))
        })?;

        allocated.clear();
        Ok(())
    }
}

impl Default for PortNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

/// 端口协商结果
#[derive(Debug, Clone)]
pub struct PortNegotiationResult {
    /// 分配的端口
    pub port: u16,
    /// 是否是首选端口
    pub is_preferred: bool,
    /// 尝试次数
    pub attempts: u32,
}

/// 高级端口协商器
///
/// 支持更复杂的协商策略
#[derive(Debug)]
pub struct AdvancedPortNegotiator {
    /// 基础协商器
    base: PortNegotiator,
    /// 重试次数
    max_retries: u32,
    /// 重试间隔（毫秒）
    retry_delay_ms: u64,
}

impl AdvancedPortNegotiator {
    /// 创建新的高级协商器
    pub fn new() -> Self {
        Self {
            base: PortNegotiator::new(),
            max_retries: 3,
            retry_delay_ms: 100,
        }
    }

    /// 设置重试次数
    pub fn with_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    /// 带重试的端口协商
    pub async fn negotiate_with_retry(
        &self,
        preferred_port: Option<u16>,
    ) -> CoreResult<PortNegotiationResult> {
        for (attempt_count, retry) in (0..=self.max_retries).enumerate() {
            let _ = retry;

            match self.base.negotiate(preferred_port) {
                Ok(port) => {
                    return Ok(PortNegotiationResult {
                        port,
                        is_preferred: preferred_port == Some(port),
                        attempts: attempt_count as u32 + 1,
                    });
                }
                Err(e) => {
                    if retry < self.max_retries {
                        tokio::time::sleep(tokio::time::Duration::from_millis(self.retry_delay_ms))
                            .await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(CoreError::Common(CommonError::General(
            "Port negotiation failed after max retries".to_string(),
        )))
    }

    /// 释放端口
    pub fn release_port(&self, port: u16) -> CoreResult<()> {
        self.base.release_port(port)
    }
}

impl Default for AdvancedPortNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_range_contains() {
        let range = PortRange::new(10000, 20000);
        assert!(range.contains(15000));
        assert!(!range.contains(9999));
        assert!(!range.contains(20001));
    }

    #[test]
    fn test_port_range_iter() {
        let range = PortRange::new(10000, 10002);
        let ports: Vec<u16> = range.iter().collect();
        assert_eq!(ports, vec![10000, 10001, 10002]);
    }

    #[test]
    fn test_port_negotiator_basic() -> Result<(), CoreError> {
        let negotiator = PortNegotiator::new();

        // 协商一个端口
        let port = negotiator.negotiate(None)?;
        assert!((10000..=20000).contains(&port));

        // 检查端口已被分配
        let allocated = negotiator.get_allocated_ports()?;
        assert!(allocated.contains(&port));

        // 释放端口
        negotiator.release_port(port)?;
        let allocated = negotiator.get_allocated_ports()?;
        assert!(!allocated.contains(&port));
        Ok(())
    }

    #[test]
    fn test_port_negotiator_preferred() -> Result<(), CoreError> {
        let negotiator = PortNegotiator::with_range(PortRange::new(30000, 40000));

        // 尝试协商首选端口
        let preferred = 35000;
        let port = negotiator.negotiate(Some(preferred))?;

        // 如果 35000 可用，应该返回它
        if negotiator.is_port_available(preferred)? {
            assert_eq!(port, preferred);
        }
        Ok(())
    }

    #[test]
    fn test_port_negotiator_multiple() -> Result<(), CoreError> {
        let negotiator = PortNegotiator::new();

        let ports = negotiator.negotiate_multiple(5)?;
        assert_eq!(ports.len(), 5);

        // 确保所有端口都是唯一的
        let unique_ports: HashSet<_> = ports.iter().copied().collect();
        assert_eq!(unique_ports.len(), 5);
        Ok(())
    }

    #[test]
    fn test_is_port_available() -> Result<(), CoreError> {
        let negotiator = PortNegotiator::new();

        // 高位端口应该可用
        let port = negotiator.negotiate(None)?;
        assert!(negotiator.is_port_available(port)?);

        // 分配后应该不可用
        assert!(!negotiator.is_port_available(port)?);

        negotiator.release_port(port)?;
        Ok(())
    }
}

//! 系统内存压力感知
//!
//! 自 engine `cache::lru_cache` 下沉（2026-10-29，engine 瘦身第三刀前置小手术）：
//! `driver::smart_pool`（将随 rds-driver 迁出）与 `cache` 两侧共用，放 shared 避免
//! rds-driver → rds-engine 的依赖环。

/// 内存压力级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// 正常状态
    Normal,
    /// 中等压力（开始淘汰缓存）
    Moderate,
    /// 高压力（激进淘汰）
    Critical,
}

impl MemoryPressure {
    /// 根据系统内存使用情况评估压力级别
    #[cfg(target_os = "linux")]
    pub fn detect() -> Self {
        if let Ok(mem_info) = std::fs::read_to_string("/proc/meminfo") {
            for line in mem_info.lines() {
                if line.starts_with("MemAvailable:") {
                    if let Some(available_kb) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        let available_mb = available_kb / 1024;
                        return if available_mb < 512 {
                            MemoryPressure::Critical
                        } else if available_mb < 1024 {
                            MemoryPressure::Moderate
                        } else {
                            MemoryPressure::Normal
                        };
                    }
                }
            }
        }
        MemoryPressure::Normal
    }

    #[cfg(target_os = "macos")]
    pub fn detect() -> Self {
        use std::process::Command;
        let output = Command::new("vm_stat").output().ok();

        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("Pages free") {
                    if let Some(pages) = line
                        .split_whitespace()
                        .last()
                        .and_then(|s| s.trim_end_matches('.').parse::<u64>().ok())
                    {
                        let free_mb = (pages * 4096) / (1024 * 1024);
                        return if free_mb < 512 {
                            MemoryPressure::Critical
                        } else if free_mb < 1024 {
                            MemoryPressure::Moderate
                        } else {
                            MemoryPressure::Normal
                        };
                    }
                }
            }
        }
        MemoryPressure::Normal
    }

    #[cfg(target_os = "windows")]
    pub fn detect() -> Self {
        // Windows 平台使用简单启发式，默认正常状态
        // 后续可通过 sysinfo crate 获取更准确的内存信息
        MemoryPressure::Normal
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    pub fn detect() -> Self {
        MemoryPressure::Normal
    }

    /// 获取淘汰比例（根据压力级别）
    pub fn eviction_ratio(&self) -> f64 {
        match self {
            MemoryPressure::Normal => 0.0,
            MemoryPressure::Moderate => 0.25,
            MemoryPressure::Critical => 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryPressure;

    #[test]
    fn eviction_ratio_scales_with_pressure() {
        assert_eq!(MemoryPressure::Normal.eviction_ratio(), 0.0);
        assert_eq!(MemoryPressure::Moderate.eviction_ratio(), 0.25);
        assert_eq!(MemoryPressure::Critical.eviction_ratio(), 0.5);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_detect_returns_normal() {
        assert_eq!(MemoryPressure::detect(), MemoryPressure::Normal);
    }
}

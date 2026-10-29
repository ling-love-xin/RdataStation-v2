//! 日志配置
//!
//! 定义日志模块的配置结构，包括日志级别过滤、输出目标和保留策略。

use crate::logging::record::LogLevel;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// 全局最低日志级别
    pub min_level: LogLevel,
    /// 模块级级别覆盖（target前缀 → 级别），预留字段
    #[allow(dead_code)]
    pub module_levels: HashMap<String, LogLevel>,
    /// 是否输出到 stderr，预留字段
    #[allow(dead_code)]
    pub file_output: bool,
    /// 是否输出到数据库，预留字段
    #[allow(dead_code)]
    pub db_output: bool,
    /// 日志文件目录
    pub log_dir: PathBuf,
    /// 日志文件保留天数
    pub retention_days: u32,
    /// **单个日志文件上限**（字节）。超过后本进程不再往文件写（只补一行说明），
    /// 库与 stderr 照常——防的是日志风暴在一天内把盘写爆（按天滚动不能防这个）。
    pub max_file_bytes: u64,
    /// **日志目录总配额**（字节）。启动清理时在"过期删"之后再过一遍"超配额按最旧删"。
    pub max_dir_bytes: u64,
    /// 数据库最大记录数（超过后清理旧记录）
    pub max_db_records: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            min_level: LogLevel::Info,
            module_levels: HashMap::new(),
            file_output: true,
            db_output: true,
            // 日志目录跟随数据根（`<RDS_HOME>/logs`），不再由调用方各自传
            log_dir: paths::log_dir(),
            retention_days: 7,
            // 16 MiB / 文件、256 MiB / 目录：日常远用不到，只在日志风暴时兜底
            max_file_bytes: 16 * 1024 * 1024,
            max_dir_bytes: 256 * 1024 * 1024,
            max_db_records: 100_000,
        }
    }
}

impl LogConfig {
    /// 创建默认配置并设置日志目录
    pub fn with_log_dir(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            ..Default::default()
        }
    }

    /// 获取日志文件路径
    pub fn log_file_path(&self) -> PathBuf {
        self.log_dir.join("app.log")
    }

    /// 设置全局最低级别
    pub fn set_min_level(&mut self, level: LogLevel) {
        self.min_level = level;
    }

    /// 设置模块级别（预留，当前通过 reload_log_level 动态调整）
    #[allow(dead_code)]
    pub fn set_module_level(&mut self, target: String, level: LogLevel) {
        self.module_levels.insert(target, level);
    }
}

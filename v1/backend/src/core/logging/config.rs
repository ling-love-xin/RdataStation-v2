//! 日志配置
//!
//! 定义日志模块的配置结构，包括日志级别过滤、输出目标和保留策略。

use crate::core::logging::record::LogLevel;
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
            log_dir: PathBuf::from(""),
            retention_days: 7,
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

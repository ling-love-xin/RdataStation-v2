//! 日志设置 model（struct + 默认值 + 默认函数）。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    /// 最低级别：低于它的记录既不写文件也不落库（stderr 同样按此过滤）。默认 `Info`。
    /// 排障时临时调成 `DEBUG`，比改环境变量重启省事。
    #[serde(default = "default_log_min_level")]
    pub min_level: LogMinLevel,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            min_level: default_log_min_level(),
        }
    }
}

/// 日志最低级别（与 `engine::logging::LogLevel` **同词表**）。
///
/// 为什么在这里独立定义而不是复用它：设置层不依赖 engine——引一个 `LogLevel`
/// 会把双引擎（duckdb / sqlx / russh）整个拖进设置页的依赖图。两侧的对应关系
/// 由装配层做一次文本转换（`LogLevel::parse_level`），词表对不上会在那里暴露。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogMinLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogMinLevel {
    /// 全部取值（登记表的枚举项、测试的穷尽遍历用）。
    pub const ALL: [LogMinLevel; 5] = [
        LogMinLevel::Trace,
        LogMinLevel::Debug,
        LogMinLevel::Info,
        LogMinLevel::Warn,
        LogMinLevel::Error,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LogMinLevel::Trace => "TRACE",
            LogMinLevel::Debug => "DEBUG",
            LogMinLevel::Info => "INFO",
            LogMinLevel::Warn => "WARN",
            LogMinLevel::Error => "ERROR",
        }
    }

    /// 解析落盘值（大小写不敏感；未知值返回 `None`，由调用方拒绝而不是猜）。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

fn default_log_min_level() -> LogMinLevel {
    LogMinLevel::Info
}

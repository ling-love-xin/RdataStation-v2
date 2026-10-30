//! 启动期日志缓冲：接线前的错误不丢
//!
//! 背景（2026-10-30 治理）：`init_app_logging` 必须先于全局系统库建立（`app_logs` 表
//! 由迁移创建），所以**接线前没有任何订阅者**——那之前的 `tracing` 宏是 no-op，
//! 启动代码只能 `eprintln!` 走 stderr，事后翻文件 / 查库都看不到（mod.rs 的前置条件
//! 注释原样说明这一点）。
//!
//! 用法：
//! - 启动早期（接线前）的错误调用 [`record`] 记入有界缓冲（同时保留 `eprintln!` 给 stderr）；
//! - [`init_logging`] 接线成功后调用 [`replay_pending`]，把缓冲里的记录按级别重放到三层
//!   （stderr + 滚动文件 + `app_logs` 表）。
//!
//! 有界（上限 [`CAP`] 条，超限丢最旧）：启动期记录数量级是几十条，128 足够；
//! 丢了最旧的也只影响"最早的失败"，不阻断启动。

use std::sync::Mutex;

use tracing::Level;

/// 缓冲上限：超限丢最旧（启动期只有少量一次性错误，正常远达不到）。
const CAP: usize = 128;

/// 启动期记录（级别 + 消息文本）
static BUFFER: Mutex<Vec<(Level, String)>> = Mutex::new(Vec::new());

/// 接线前记录一条启动期日志（幂等、线程安全；无订阅者时仅入缓冲）。
pub fn record(level: Level, message: impl Into<String>) {
    let mut buf = match BUFFER.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if buf.len() >= CAP {
        buf.remove(0);
    }
    buf.push((level, message.into()));
}

/// 接线成功后重放：把缓冲里全部记录按级别发到 `tracing`（订阅者已就绪，落三层）。
///
/// 只应在 `init_logging` 成功之后调用一次；调用后缓冲清空。
pub fn replay_pending() {
    let records = {
        let mut buf = match BUFFER.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *buf)
    };
    for (level, message) in records {
        match level {
            Level::ERROR => tracing::error!("{message}"),
            Level::WARN => tracing::warn!("{message}"),
            Level::INFO => tracing::info!("{message}"),
            Level::DEBUG => tracing::debug!("{message}"),
            Level::TRACE => tracing::trace!("{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 启动缓冲的两个行为（顺序重放清空 + 有界丢最旧）串行在一个测试里验证：
    /// 二者共享全局静态 `BUFFER`，拆成两个并行测试会互相踩（Rust 测试默认多线程）。
    #[test]
    fn startup_buffer_records_replays_and_stays_bounded() {
        record(Level::ERROR, "全局系统库初始化失败: boom");
        record(Level::INFO, "数据目录就绪");
        record(Level::WARN, "临时目录回退");
        replay_pending();
        // 重放后缓冲清空（再次重放是空操作，无 panic）
        replay_pending();
        // 上面 replay 的 tracing 调用此刻无订阅者 = no-op，测试只验证不 panic、不丢顺序
        {
            let buf = BUFFER.lock().unwrap();
            assert!(buf.is_empty(), "重放后缓冲必须清空");
        }

        for i in 0..(CAP + 40) {
            record(Level::INFO, format!("startup-{i}"));
        }
        let buf = BUFFER.lock().unwrap();
        assert_eq!(buf.len(), CAP, "超限只保留最近 CAP 条");
        // 最旧的被丢：第一条是 i=40 而不是 i=0
        assert_eq!(buf.first().unwrap().1, format!("startup-{}", 40));
        assert_eq!(buf.last().unwrap().1, format!("startup-{}", CAP + 39));
    }
}

//! 日志设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::{LogMinLevel, Settings};
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::SettingsService;

use std::sync::RwLock;

/// 日志级别变更的副作用出口（由装配层注册一次，见 `crates/app/src/main.rs`）。
static LOG_LEVEL_SINK: RwLock<Option<fn(LogMinLevel)>> = RwLock::new(None);
pub fn install_log_level_sink(sink: fn(LogMinLevel)) {
    if let Ok(mut guard) = LOG_LEVEL_SINK.write() { *guard = Some(sink); }
}
fn notify_log_level(level: LogMinLevel) {
    if let Some(sink) = LOG_LEVEL_SINK.read().ok().and_then(|guard| *guard) { sink(level); }
}


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
            Slot::LogMinLevel => {
                let Some(level) = value.as_text().and_then(LogMinLevel::parse) else {
                    return false;
                };
                SettingsService::set_log_min_level(level, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 当前日志最低级别。
    pub fn log_min_level(cx: &App) -> LogMinLevel {
        cx.global::<Settings>().logging.min_level
    }
    /// 设置并持久化日志最低级别，并通知日志系统即时 reload。
    ///
    /// 通知走 [`install_log_level_sink`] 注册的出口（装配层把 `engine` 的
    /// `reload_log_level` 接在那里）；没有出口时只落盘，不报错。
    pub fn set_log_min_level(level: LogMinLevel, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.logging.min_level = level;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_log_level(level);
    }
}

//! 洞察设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::SettingsService;

use std::sync::RwLock;

/// 分析引擎内存上限变更的副作用出口（由装配层注册一次，见 `crates/app/src/main.rs`）。
static INSIGHT_MEMORY_LIMIT_SINK: RwLock<Option<fn(i64)>> = RwLock::new(None);
pub fn install_insight_memory_limit_sink(sink: fn(i64)) {
    if let Ok(mut guard) = INSIGHT_MEMORY_LIMIT_SINK.write() { *guard = Some(sink); }
}
fn notify_insight_memory_limit(mb: i64) {
    if let Some(sink) = INSIGHT_MEMORY_LIMIT_SINK.read().ok().and_then(|guard| *guard) { sink(mb); }
}


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
            Slot::InsightMemoryLimitMb => {
                let Some(mb) = value.as_number() else {
                    return false;
                };
                // 只认 256–16384 MB（页面只给预设档；设置文件被手改坏时拒绝，不猜）。
                if !(256.0..=16384.0).contains(&mb) {
                    return false;
                }
                SettingsService::set_memory_limit_mb(mb.round() as i64, cx);
            }
            Slot::InsightFullScanConfirm => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_full_scan_confirm(on, cx);
            }
            Slot::InsightWorkSetLimit => {
                let Some(n) = value.as_number() else {
                    return false;
                };
                // 0 = 不提示；只认 0–200（页面只给预设档；手改坏时拒绝）。
                if !(0.0..=200.0).contains(&n) {
                    return false;
                }
                SettingsService::set_work_set_limit(n.round() as i64, cx);
            }
            Slot::InsightStatusUsage => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_status_usage(on, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 分析引擎内存上限（MB，默认 2048 = 2GB）。
    pub fn memory_limit_mb(cx: &App) -> i64 {
        cx.global::<Settings>().insight.memory_limit_mb
    }
    /// 设置并持久化分析引擎内存上限，并通知引擎即时应用（`SET memory_limit`）。
    ///
    /// 通知走 [`install_insight_memory_limit_sink`] 注册的出口（装配层把
    /// `engine` 的 `DuckDBManager::set_memory_limit_mb` 接在那里）；没有出口时只落盘。
    pub fn set_memory_limit_mb(mb: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.insight.memory_limit_mb = mb;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
        notify_insight_memory_limit(mb);
    }
    /// 全量统计前是否弹确认（默认开启）。
    pub fn full_scan_confirm(cx: &App) -> bool {
        cx.global::<Settings>().insight.full_scan_confirm
    }
    /// 设置并持久化全量确认开关（宿主在收到全量请求时读它决定弹窗）。
    pub fn set_full_scan_confirm(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.insight.full_scan_confirm = on;
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 洞察集合工作集提示阈值（表数超过该值提示先清理；0 = 不提示，默认 30）。
    pub fn work_set_limit(cx: &App) -> i64 {
        cx.global::<Settings>().insight.work_set_limit
    }
    /// 设置并持久化工作集提示阈值。
    pub fn set_work_set_limit(n: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.insight.work_set_limit = n;
        }
        persist(&cx.global::<Settings>().clone());
    }
    /// 洞察中间页工具栏是否显示分析引擎内存占用（默认开启）。
    pub fn status_usage(cx: &App) -> bool {
        cx.global::<Settings>().insight.status_usage
    }
    /// 设置并持久化内存占用显示开关。
    pub fn set_status_usage(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.insight.status_usage = on;
        }
        persist(&cx.global::<Settings>().clone());
    }
}

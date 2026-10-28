//! 布局设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use crate::SettingsService;


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    cx: &mut App,
) -> bool {
    match slot {
            Slot::LayoutLocked => {
                let Some(on) = value.as_bool() else {
                    return false;
                };
                SettingsService::set_layout_locked(on, cx);
            }
            Slot::LayoutLeftSidebarWidth => {
                let Some(n) = value.as_number() else {
                    return false;
                };
                if !(100.0..=2000.0).contains(&n) {
                    return false;
                }
                SettingsService::set_layout_left_sidebar_width(n as i64, cx);
            }
            Slot::LayoutRightSidebarWidth => {
                let Some(n) = value.as_number() else {
                    return false;
                };
                if !(100.0..=2000.0).contains(&n) {
                    return false;
                }
                SettingsService::set_layout_right_sidebar_width(n as i64, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 读取布局锁定设置（默认 false）。
    pub fn layout_locked(cx: &App) -> bool {
        cx.global::<Settings>().layout.locked
    }
    /// 读取左侧侧边栏宽度（默认 250px）。
    pub fn layout_left_sidebar_width(cx: &App) -> i64 {
        cx.global::<Settings>().layout.left_sidebar_width
    }
    /// 读取右侧侧边栏宽度（默认 300px）。
    pub fn layout_right_sidebar_width(cx: &App) -> i64 {
        cx.global::<Settings>().layout.right_sidebar_width
    }
    /// 【并行线补充】布局锁定开关：更新 global → 持久化 → 立即重绘。
    pub fn set_layout_locked(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.layout.locked = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 【并行线补充】左侧侧边栏宽度：更新 global → 持久化 → 立即重绘。
    pub fn set_layout_left_sidebar_width(px: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.layout.left_sidebar_width = px.clamp(100, 2000);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 【并行线补充】右侧侧边栏宽度：更新 global → 持久化 → 立即重绘。
    pub fn set_layout_right_sidebar_width(px: i64, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.layout.right_sidebar_width = px.clamp(100, 2000);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
}

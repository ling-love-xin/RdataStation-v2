//! 布局设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::SettingsService;
use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};
use workbench_shell::ui;

/// 侧边栏宽度写入的合法区间（基准字号下的 px）。
///
/// 上下界与 Dock 起步宽**同一来源**（`ui::*_DOCK_MIN/MAX × BASE_FONT_SIZE`）：
/// 过去写死 100..2000，等于允许把侧栏拖成 100px 的废条，也允许它吃掉整个窗口。
fn width_range(min_rem: f32, max_rem: f32) -> (i64, i64) {
    (
        (min_rem * ui::BASE_FONT_SIZE) as i64,
        (max_rem * ui::BASE_FONT_SIZE) as i64,
    )
}

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
        Slot::LayoutDashboardEnabled => {
            let Some(on) = value.as_bool() else {
                return false;
            };
            SettingsService::set_dashboard_enabled(on, cx);
        }
        Slot::LayoutLeftSidebarWidth => {
            let Some(n) = value.as_number() else {
                return false;
            };
            let (min, max) = width_range(ui::LEFT_DOCK_MIN, ui::LEFT_DOCK_MAX);
            if !(min as f64..=max as f64).contains(&n) {
                return false;
            }
            SettingsService::set_layout_left_sidebar_width(n as i64, cx);
        }
        Slot::LayoutRightSidebarWidth => {
            let Some(n) = value.as_number() else {
                return false;
            };
            let (min, max) = width_range(ui::RIGHT_DOCK_MIN, ui::RIGHT_DOCK_MAX);
            if !(min as f64..=max as f64).contains(&n) {
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
    /// 读取统计看板显隐开关（默认 true）。
    pub fn dashboard_enabled(cx: &App) -> bool {
        cx.global::<Settings>().layout.dashboard_enabled
    }
    /// 读取左侧侧边栏宽度（基准字号下的 px，默认 320）。
    pub fn layout_left_sidebar_width(cx: &App) -> i64 {
        cx.global::<Settings>().layout.left_sidebar_width
    }
    /// 读取右侧侧边栏宽度（基准字号下的 px，默认 352）。
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
    /// 统计看板开关：更新 global → 持久化 → 立即重绘。
    pub fn set_dashboard_enabled(on: bool, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.layout.dashboard_enabled = on;
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 【并行线补充】左侧侧边栏宽度：更新 global → 持久化 → 立即重绘。
    pub fn set_layout_left_sidebar_width(px: i64, cx: &mut App) {
        let (min, max) = width_range(ui::LEFT_DOCK_MIN, ui::LEFT_DOCK_MAX);
        {
            let settings = cx.global_mut::<Settings>();
            settings.layout.left_sidebar_width = px.clamp(min, max);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
    /// 【并行线补充】右侧侧边栏宽度：更新 global → 持久化 → 立即重绘。
    pub fn set_layout_right_sidebar_width(px: i64, cx: &mut App) {
        let (min, max) = width_range(ui::RIGHT_DOCK_MIN, ui::RIGHT_DOCK_MAX);
        {
            let settings = cx.global_mut::<Settings>();
            settings.layout.right_sidebar_width = px.clamp(min, max);
        }
        persist(&cx.global::<Settings>().clone());
        cx.refresh_windows();
    }
}

//! 项目设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
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
            Slot::ProjectSortMode => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                SettingsService::set_project_sort_mode(text, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 读取项目列表排序方式（`last_opened` / `name` / `created`）。
    pub fn project_sort_mode(cx: &App) -> String {
        cx.global::<Settings>().projects.sort_mode.clone()
    }
    /// 设置并持久化项目列表排序方式（更新 global → 写 settings.json）。
    pub fn set_project_sort_mode(mode: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.projects.sort_mode = mode.to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
}

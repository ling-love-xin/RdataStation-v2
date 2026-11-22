//! 项目设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::SettingsService;
use crate::model::Settings;
use crate::persistence::persist;
use crate::registry::{SettingValue, Slot};

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
        // 内部记忆：页面不显示，唯一写入方是新建项目对话框的成功分支。
        Slot::ProjectLastCreateLocation => {
            let Some(text) = value.as_text() else {
                return false;
            };
            SettingsService::set_project_last_create_location(text, cx);
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

    /// 上次新建项目用的父目录（新建对话框的默认「位置」）。
    ///
    /// 空串 = 没记过 → `None`（调用方回退用户主目录）。
    pub fn project_last_create_location(cx: &App) -> Option<String> {
        let path = cx
            .global::<Settings>()
            .projects
            .last_create_location
            .trim()
            .to_string();
        (!path.is_empty()).then_some(path)
    }

    /// 记住本次新建用的父目录（创建成功后调用；空串视为清除）。
    ///
    /// 值未变时不重复写盘——与 `project` crate 记录构建标识的口径一致，
    /// 免得每次新建都无谓改一次 settings.json 的 mtime。
    pub fn set_project_last_create_location(path: &str, cx: &mut App) {
        let next = path.trim().to_string();
        {
            let settings = cx.global_mut::<Settings>();
            if settings.projects.last_create_location == next {
                return;
            }
            settings.projects.last_create_location = next;
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
}

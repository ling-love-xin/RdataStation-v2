//! 启动设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
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
            Slot::StartupDefaultLeftPanel => {
                let Some(text) = value.as_text() else {
                    return false;
                };
                // 落盘值只认登记表枚举里的四档；未知值拒绝（设置文件被手改坏时不让写入）。
                if !matches!(text, "database" | "draft" | "resources" | "plugin") {
                    return false;
                }
                SettingsService::set_startup_default_left_panel(text, cx);
            }
        _ => return false,
    }
    true
}

impl SettingsService {
    /// 读取启动时左侧栏默认显示的模块（`database` / `draft` / `resources` / `plugin`）。
    ///
    /// 未配置时返回 `database`（与 `Shared::new` 的构造默认一致，见 `model::Startup`）。
    pub fn startup_default_left_panel(cx: &App) -> String {
        cx.global::<Settings>().startup.default_left_panel.clone()
    }
    /// 设置并持久化启动时左侧栏默认显示的模块（更新 global → 写 settings.json）。
    pub fn set_startup_default_left_panel(key: &str, cx: &mut App) {
        {
            let settings = cx.global_mut::<Settings>();
            settings.startup.default_left_panel = key.to_string();
        }
        let settings = cx.global::<Settings>().clone();
        persist(&settings);
    }
}

//! rds-settings — 值桥：登记表 ↔ `SettingsService` 的读写闭包与恢复默认。
//!
//! gpui-kit `setting` 组件的字段用闭包取值 / 写值（`Fn(&App) -> T` /
//! `Fn(T, &mut App)`），但它的**写闭包拿不到 `Window`**。本模块把这些
//! 闭包统一接到 `SettingsService::apply_by_key`（唯一写入路径），并把
//! "写后通知页面刷新 + 通知宿主重应用外部消费方"收敛到一处（`write_scalar`）。
//!
//! 依赖只向下：本模块只依赖 `settings` 自身 + gpui-kit，不引 engine / 具体对话框。
//! 不直接依赖 `settings_page`（`notify` / `host_changed` 以回调闭包传入）。

use std::rc::Rc;

use gpui_kit::{App, Window};

use crate::model::Settings;
use crate::registry::{SettingSpec, SettingValue};
use crate::{SettingsService, value_by_key};

/// 当前值（读取路径；页面渲染 / 偏离默认判定 / 契约测试共用）。
///
/// 从 GPUI global 读（`SettingsService` 是唯一持有者），不维护第二份快照。
pub fn current_value(cx: &App, key: &'static str) -> Option<SettingValue> {
    value_by_key(&SettingsService::get(cx), key)
}

/// 某项当前值是否偏离登记表默认值（"恢复默认"按钮出现的判据）。
///
/// 默认值只取自登记表（`spec.default_value()`），页面不另存一份，避免漂移。
pub fn is_changed(settings: &Settings, spec: &SettingSpec) -> bool {
    match (value_by_key(settings, spec.key), spec.default_value()) {
        (Some(current), Some(default)) => current != default,
        _ => false,
    }
}

/// 写入一个标量值（唯一写入路径的闭包版）。
///
/// `window`：只有需要即时窗口副作用的设置（主题 / 字号）由调用方传 `Some`；
/// 其余传 `None`（`SettingsService` 的 setter 都接受 `Option<&mut Window>`，
/// `None` 时自行遍历现有窗口，见 `set_font_size`）。
///
/// 写成功 → 通知页面重读（重渲染）→ 通知宿主重应用外部消费方（DockSkin 等）。
/// 写失败（未登记 / 形态不符）返回 `false`，调用方不假装成功。
pub fn write_scalar(
    key: &'static str,
    value: SettingValue,
    window: Option<&mut Window>,
    notify: &Rc<dyn Fn(&mut App)>,
    host_changed: &Rc<dyn Fn(&mut App)>,
    cx: &mut App,
) -> bool {
    if !SettingsService::apply_by_key(key, value, window, cx) {
        return false;
    }
    notify(cx);
    host_changed(cx);
    true
}

/// 恢复默认：把登记表默认值写回（返回是否成功）。
///
/// 复合值（无标量默认值）返回 `false`；页面只对可恢复的行调用。
pub fn reset_to_default(
    spec: &SettingSpec,
    window: Option<&mut Window>,
    notify: &Rc<dyn Fn(&mut App)>,
    host_changed: &Rc<dyn Fn(&mut App)>,
    cx: &mut App,
) -> bool {
    let Some(default) = spec.default_value() else {
        return false;
    };
    write_scalar(spec.key, default, window, notify, host_changed, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Settings;
    use crate::registry::{registry, KindTag};

    #[test]
    fn is_changed_tracks_registry_default() {
        let spec = registry()
            .iter()
            .find(|s| s.key == "editor.show_line_numbers")
            .expect("登记表必须有 show_line_numbers");
        assert_eq!(spec.kind.tag(), KindTag::Bool, "测试用例选的是布尔项");
        let mut settings = Settings::default();
        // 默认值与登记表一致 → 未偏离（登记表契约测试已保证二者相等）。
        assert!(!is_changed(&settings, spec));
        // 偏离默认 → changed。
        settings.editor.show_line_numbers = !settings.editor.show_line_numbers;
        assert!(is_changed(&settings, spec));
        // 改回默认 → 不再偏离。
        settings.editor.show_line_numbers = !settings.editor.show_line_numbers;
        assert!(!is_changed(&settings, spec));
    }

    #[test]
    fn composite_has_no_scalar_default_so_is_not_changed() {
        let spec = registry()
            .iter()
            .find(|s| s.composite)
            .expect("登记表必须有复合值项");
        let settings = Settings::default();
        // 复合值没有标量默认值（`default_value()` 返回 None）→ 恒不判 changed。
        assert!(spec.default_value().is_none());
        assert!(!is_changed(&settings, spec));
    }
}

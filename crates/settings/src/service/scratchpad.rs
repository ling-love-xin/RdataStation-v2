//! 草稿箱设置项的写入处理（`apply_slot`，由 `SettingsService::apply_by_key` 分发）与读写访问器。
use gpui_kit::App;

use crate::registry::{SettingValue, Slot};


/// 本域 slot 的标量写入处理；返回 `false` 表示拒绝（值形态不符 / 复合值 / 无法解析）。
pub(crate) fn apply_slot(
    slot: Slot,
    _value: SettingValue,
    _window: Option<&mut gpui_kit::Window>,
    _cx: &mut App,
) -> bool {
    match slot {
            Slot::ScratchpadGroupMaxH => return false,
            Slot::ScratchpadToolbar => return false,
        _ => return false,
    }
}

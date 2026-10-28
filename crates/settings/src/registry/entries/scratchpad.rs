//! 草稿箱设置项登记。
use crate::registry::{SettingEffect, SettingEntry, SettingKind, SettingSpec};

/// 本域全部登记项（`registry::registry()` 惰性合并，顺序 = 登记顺序）。
pub fn entries() -> Vec<SettingSpec> {
    vec![
        SettingSpec {
        key: "scratchpad.toolbar",
        section: "scratchpad",
        section_label: "草稿箱",
        group: "scratchpad",
        group_label: None,
        label: "工具栏按钮（显示/隐藏与位置）",
        hint: "⋯ 更多设置：每个按钮可切换显示/隐藏与左/右位置；空配置 = 全部显示 + 默认位置（左 5 右 2）；改完立即生效并持久化",
        hint_inline: true,
        kind: SettingKind::Composite,
        presets: &[],
        default_json: "{}",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "crates/scratchpad/src/scratchpad_view/chrome.rs::（⋯ 菜单读写）+ scratchpad_view.rs::ScratchpadView::new（启动读入）",
        composite: true,
        disabled: false,
        },
        SettingSpec {
        key: "scratchpad.group_max_h",
        section: "scratchpad",
        section_label: "草稿箱",
        group: "scratchpad",
        group_label: None,
        label: "分区高度上限",
        hint: "草稿/引用/回收站可拖拽分区的高度上限（rems，默认 7.5 = 面板默认）；拖拽 handle 写回（B3 新增，2026-10-28 补登记）。不上设置页，读写走分区拖拽 handle",
        hint_inline: true,
        kind: SettingKind::Number,
        presets: &[],
        default_json: "7.5",
        effect: SettingEffect::Immediate,
        entry: SettingEntry::Module,
        consumer: "crates/scratchpad/src/scratchpad_view.rs::ScratchpadView::new（启动读入）+ chrome.rs::persist_group_max_h（拖拽写回）",
        composite: false,
        disabled: false,
        },
    ]
}

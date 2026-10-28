//! 启动设置项登记。
use crate::registry::{SettingEffect, SettingEntry, SettingKind, SettingSpec};

/// 本域全部登记项（`registry::registry()` 惰性合并，顺序 = 登记顺序）。
pub fn entries() -> Vec<SettingSpec> {
    vec![
        SettingSpec {
        key: "startup.default_left_panel",
        section: "startup",
        section_label: "启动",
        group: "startup",
        group_label: None,
        label: "启动时默认显示的模块",
        hint: "应用启动时左侧栏默认停在哪一个模块（活动栏也可以随时切换）",
        hint_inline: false,
        kind: SettingKind::Enum(&[
        ("database", "数据库导航"),
        ("draft", "草稿箱"),
        ("resources", "资产库"),
        ("plugin", "插件"),
        ]),
        presets: &[],
        default_json: "\"database\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Page,
        consumer: "workbench/src/view.rs::WorkbenchView::init_workspace（读设置 → Shared.active_left）",
        composite: false,
        disabled: false,
        },
    ]
}

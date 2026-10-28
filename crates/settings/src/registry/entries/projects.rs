//! 项目设置项登记。
use crate::registry::{SettingEffect, SettingEntry, SettingKind, SettingSpec};

/// 本域全部登记项（`registry::registry()` 惰性合并，顺序 = 登记顺序）。
pub fn entries() -> Vec<SettingSpec> {
    vec![
        SettingSpec {
        key: "projects.sort_mode",
        section: "projects",
        section_label: "项目",
        group: "projects",
        group_label: None,
        label: "列表排序",
        hint: "项目选择器的排序方式（选择器内也有循环按钮）",
        hint_inline: false,
        kind: SettingKind::Enum(&[
        ("last_opened", "最近打开"),
        ("name", "名称"),
        ("created", "创建时间"),
        ]),
        presets: &[],
        default_json: "\"last_opened\"",
        effect: SettingEffect::NextUse,
        entry: SettingEntry::Both,
        consumer: "workbench/src/view.rs::WorkbenchView::new（读）+ components/project_host.rs（写）",
        composite: false,
        disabled: false,
        },
    ]
}

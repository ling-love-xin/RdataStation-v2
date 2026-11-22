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
        // 内部记忆（上次新建用的父目录）：`Module` 不上设置页，读写都发生在
        // 新建项目对话框里（`open_create_dialog` 读、`submit_create` 成功后写）。
        // 登记它不是为了页面，而是为了让「模型叶子必须登记」这条护栏继续成立。
        SettingSpec {
            key: "projects.last_create_location",
            section: "projects",
            section_label: "项目",
            group: "projects",
            group_label: None,
            label: "上次新建位置",
            hint: "新建项目对话框的默认父目录（内部记忆，不在本页出现）",
            hint_inline: false,
            kind: SettingKind::Text,
            presets: &[],
            default_json: "\"\"",
            effect: SettingEffect::NextUse,
            entry: SettingEntry::Module,
            consumer: "project/src/ui.rs::open_create_dialog（读）+ submit_create（写）",
            composite: false,
            disabled: false,
        },
    ]
}

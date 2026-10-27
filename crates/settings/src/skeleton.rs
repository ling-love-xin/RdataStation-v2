//! rds-settings — 骨架构造：把 `registry` 翻译成 gpui-kit `setting` 组件的
//! `Settings`（节 → 页，行 → 分组项），并挂载动作行 / 布局预览。
//!
//! 这是**构造期纯函数**（无 I/O、不改状态）：值读写经 `fields` 生成的闭包
//! 在 render / 事件期接到 global，本节只负责把登记表排进骨架。
//! 侧栏导航 / 搜索过滤 / 页面级 reset-all 由 gpui-kit `setting` 组件自带。

use std::rc::Rc;

use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::setting::{SelectIndex, SettingGroup, SettingPage, Settings};
use gpui_kit::App;

use crate::fields;
use crate::registry::{self, SettingSpec};
use crate::settings_page::SettingsHost;

/// 构造设置页骨架。
///
/// `notify`：任一设置项写入后要通知页面实体重读的闭包（在 `SettingsPage::new`
/// 里构造并传入）。
pub fn build(host: &SettingsHost, notify: &Rc<dyn Fn(&mut App)>) -> Settings {
    let mut settings = Settings::new("rds-settings")
        .with_group_variant(GroupBoxVariant::Normal)
        .default_selected_index(SelectIndex {
            page_ix: 0,
            group_ix: None,
        });

    for (section, section_label) in registry::sections() {
        let mut group = SettingGroup::new();
        // 布局分节：顶部先放左右侧边栏占比预览示意图。
        if section == "layout" {
            group = group.item(fields::layout_preview());
        }
        let rows: Vec<&'static SettingSpec> = registry::page_rows()
            .filter(|s| s.section == section)
            .collect();
        for spec in rows {
            group = group.item(fields::value_item(spec, host, notify));
        }
        // 动作行（不是设置项）：按节挂宿主提供的副作用。
        group = group.items(action_rows(section, host));
        settings = settings.page(SettingPage::new(section_label).group(group));
    }
    settings
}

/// 动作行（非设置项，见 `fields::action_row`）：按节从宿主桥取回调。
fn action_rows(section: &'static str, host: &SettingsHost) -> Vec<gpui_kit::component::setting::SettingItem> {
    let mut items = Vec::new();
    match section {
        "navigator" => {
            items.push(fields::action_row(
                "settings-open-cache",
                "缓存",
                "动作行（不是设置项）：打开缓存对话框，清理元数据缓存",
                "缓存管理…",
                host.on_open_cache.clone(),
            ));
            items.push(fields::action_row(
                "settings-open-type-short-map",
                "类型简称映射",
                "维护「数据库类型 → 简称」映射表（连接名称 `[简称]` 与徽标缩写共用；默认同徽标缩写，未登记类型回退类型名前 2 字母）",
                "维护映射表…",
                host.on_open_type_short_map.clone(),
            ));
        }
        "logging" => {
            items.push(fields::action_row(
                "settings-open-log-view",
                "查看日志",
                "动作行（不是设置项）：打开日志查看对话框（最近 500 条，可按级别与关键字筛）",
                "查看日志…",
                host.on_open_log_view.clone(),
            ));
            items.push(fields::action_row(
                "settings-open-log-dir",
                "日志目录",
                "动作行（不是设置项）：在系统文件管理器里打开日志所在目录",
                "打开日志目录",
                host.on_open_log_dir.clone(),
            ));
        }
        _ => {}
    }
    items
}

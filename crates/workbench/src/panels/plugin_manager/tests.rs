//! 插件管理页纯函数单测（不渲染窗口，不需要 gpui 测试上下文）。

use plugin::ContributionCounts;
use plugin::PluginInfo;
use plugin::PluginKind;
use plugin::wasm::{PluginMetadata, PluginState, PluginType};

use super::widgets::{
    PluginFilter, contribution_summary, filtered_plugins, plugin_kind_label,
    plugin_state_is_active, plugin_state_label, plugin_stats,
};

fn info(name: &str, id: &str, state: PluginState, description: &str) -> PluginInfo {
    PluginInfo {
        id: id.to_string(),
        name: name.to_string(),
        version: "1.0.0".to_string(),
        kind: PluginKind::Sidecar,
        metadata: PluginMetadata {
            id: id.to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            plugin_type: PluginType::Driver,
            description: description.to_string(),
            author: "RDS".to_string(),
            entry_point: "run".to_string(),
        },
        state,
        error: (state == PluginState::Error).then(|| "清单缺入口".to_string()),
    }
}

fn plugins() -> Vec<PluginInfo> {
    vec![
        info(
            "Oracle 驱动",
            "rds.driver.oracle",
            PluginState::Active,
            "Oracle 连接",
        ),
        info(
            "表大小看板",
            "rds.panel.table_size",
            PluginState::Inactive,
            "表体积统计",
        ),
        info("坏包", "rds.bad", PluginState::Error, "清单损坏"),
    ]
}

#[test]
fn filter_matches_state_buckets() {
    let all = plugins();
    assert_eq!(filtered_plugins(&all, PluginFilter::All, "").len(), 3);
    assert_eq!(filtered_plugins(&all, PluginFilter::Enabled, "").len(), 1);
    assert_eq!(filtered_plugins(&all, PluginFilter::Disabled, "").len(), 1);
    assert_eq!(filtered_plugins(&all, PluginFilter::Error, "").len(), 1);
}

#[test]
fn search_matches_name_id_and_description() {
    let all = plugins();
    assert_eq!(filtered_plugins(&all, PluginFilter::All, "oracle").len(), 1);
    assert_eq!(
        filtered_plugins(&all, PluginFilter::All, "table_size").len(),
        1
    );
    assert_eq!(filtered_plugins(&all, PluginFilter::All, "体积").len(), 1);
    assert_eq!(filtered_plugins(&all, PluginFilter::All, "不存在").len(), 0);
}

#[test]
fn results_are_sorted_by_name() {
    let all = plugins();
    let out = filtered_plugins(&all, PluginFilter::All, "");
    let names: Vec<&str> = out.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Oracle 驱动", "坏包", "表大小看板"]);
}

#[test]
fn stats_count_each_state() {
    let stats = plugin_stats(&plugins());
    assert_eq!(stats.total, 3);
    assert_eq!(stats.enabled, 1);
    assert_eq!(stats.disabled, 1);
    assert_eq!(stats.error, 1);
}

#[test]
fn state_labels_and_active_boundary() {
    assert_eq!(plugin_state_label(PluginState::Active), "已启用");
    assert_eq!(plugin_state_label(PluginState::Loaded), "已加载");
    assert_eq!(plugin_state_label(PluginState::Inactive), "已停用");
    assert_eq!(plugin_state_label(PluginState::Error), "出错");
    assert!(plugin_state_is_active(PluginState::Active));
    assert!(plugin_state_is_active(PluginState::Loaded));
    assert!(!plugin_state_is_active(PluginState::Inactive));
    assert!(!plugin_state_is_active(PluginState::Error));
}

#[test]
fn contribution_summary_handles_empty_and_counts() {
    assert_eq!(contribution_summary(None), "贡献点：无");
    assert_eq!(
        contribution_summary(Some(&ContributionCounts::default())),
        "贡献点：无"
    );
    let counts = ContributionCounts {
        commands: 2,
        panels: 1,
        settings: 0,
        drivers: 3,
    };
    assert_eq!(
        contribution_summary(Some(&counts)),
        "命令 2 · 面板 1 · 设置 0 · 驱动 3"
    );
}

#[test]
fn kind_labels_cover_current_variants() {
    assert_eq!(plugin_kind_label(PluginKind::Sidecar), "侧车进程");
    assert_eq!(plugin_kind_label(PluginKind::Frontend), "前端扩展");
    assert_eq!(plugin_kind_label(PluginKind::Unknown), "清单损坏");
}

use super::plugin_tab::{
    bulk_retry_includes, bulk_toggle_includes, contribution_breakdown, event_touches_plugin_list,
    install_receipt, panel_busy, plugin_row_subtitle, uninstall_confirm_description,
};
use plugin::events::PluginEvent;
use plugin::wasm::PluginState;

/// 贡献点明细只列非零类别；驱动也算贡献（驱动不在注册表里，但仍是插件声明）。
#[test]
fn contribution_breakdown_lists_only_nonzero_kinds() {
    let counts = plugin::ContributionCounts {
        commands: 2,
        panels: 0,
        settings: 1,
        drivers: 1,
    };
    assert_eq!(contribution_breakdown(counts), "命令 2 · 设置 1 · 驱动 1");
    assert_eq!(
        contribution_breakdown(plugin::ContributionCounts::default()),
        ""
    );
}

/// 忙碌标记只看 progress 文本是否在：空 = 可操作，有文案 = 其它入口全部拒绝。
/// 安装 / 重载 / 启停 / 卸载共用这一个标记，不能各自维护一套。
#[test]
fn pending_operation_marks_the_panel_busy() {
    let progress = std::rc::Rc::new(std::cell::RefCell::new(None));
    assert!(!panel_busy(&progress));
    *progress.borrow_mut() = Some("正在安装 demo.zip…".to_string());
    assert!(panel_busy(&progress));
    *progress.borrow_mut() = None;
    assert!(!panel_busy(&progress));
}

/// 出错行必须把 id 和原因同时留下：只显示原因会分不清是哪个插件，
/// 只显示 id 则又把「为什么出错」藏回日志里。
#[test]
fn plugin_row_subtitle_keeps_the_id_and_the_error_reason() {
    assert_eq!(
        plugin_row_subtitle("rds.driver.oracle", None),
        "rds.driver.oracle"
    );
    let failed = plugin_row_subtitle("rds.driver.oracle", Some("进程启动失败：超时"));
    assert!(failed.contains("rds.driver.oracle"), "{failed}");
    assert!(failed.contains("原因："), "{failed}");
    assert!(failed.contains("进程启动失败：超时"), "{failed}");
}

/// 事件订阅只应把「会改清单 / 状态」的生命周期事件翻成刷新；自定义事件不触发。
#[test]
fn only_lifecycle_events_trigger_a_list_refresh() {
    assert!(event_touches_plugin_list(&PluginEvent::PluginLoaded {
        plugin_id: "a".into()
    }));
    assert!(event_touches_plugin_list(&PluginEvent::PluginInstalled {
        plugin_id: "a".into(),
        code: "a".into(),
        version: "1.0".into(),
    }));
    assert!(event_touches_plugin_list(&PluginEvent::PluginDisabled {
        plugin_id: "a".into()
    }));
    assert!(event_touches_plugin_list(&PluginEvent::PluginChanged {
        plugin_id: "a".into()
    }));
    assert!(!event_touches_plugin_list(&PluginEvent::Custom {
        plugin_id: "a".into(),
        event_name: "progress".into(),
        payload: serde_json::json!({"pct": 10}),
    }));
}

/// 回执必须如实反映库里的启用状态与运行时结果：只说「已安装」会把停用 / 激活失败藏起来。
#[test]
fn install_receipt_tells_the_truth_about_enabled_state() {
    let active = install_receipt(
        "Oracle JDBC",
        "1.2.0",
        "rds.driver.oracle",
        true,
        Some(PluginState::Active),
    );
    assert!(active.contains("已安装并启用"), "{active}");

    let kept_disabled = install_receipt(
        "Oracle JDBC",
        "1.2.0",
        "rds.driver.oracle",
        false,
        Some(PluginState::Inactive),
    );
    assert!(kept_disabled.contains("保持停用"), "{kept_disabled}");

    let failed = install_receipt(
        "Oracle JDBC",
        "1.2.0",
        "rds.driver.oracle",
        true,
        Some(PluginState::Error),
    );
    assert!(failed.contains("激活失败"), "{failed}");
    assert!(failed.contains("日志"), "必须指出去哪看原因：{failed}");
}

/// 批量启停只碰该碰的状态：启用全部不能把出错插件也硬拉起来，停用全部不能
/// 把本就停用 / 出错的插件再算一遍。`Loaded` 是「已加载未激活」，属于可停用面。
#[test]
fn bulk_actions_skip_states_they_cannot_change() {
    for state in [
        PluginState::Active,
        PluginState::Loaded,
        PluginState::Inactive,
        PluginState::Error,
    ] {
        assert_eq!(
            bulk_toggle_includes(state, true),
            state == PluginState::Inactive,
            "启用全部：{state:?}"
        );
        assert_eq!(
            bulk_toggle_includes(state, false),
            matches!(state, PluginState::Active | PluginState::Loaded),
            "停用全部：{state:?}"
        );
        assert_eq!(
            bulk_retry_includes(state),
            state == PluginState::Error,
            "重试全部：{state:?}"
        );
    }
}

/// 卸载确认框必须把「删什么 / 留什么 / 什么情况被拒」说清楚——
/// 这是全仓唯一会删插件目录的入口，用户只在这一步看到后果。
#[test]
fn uninstall_prompt_states_what_is_deleted_kept_and_blocked() {
    let text = uninstall_confirm_description("Oracle JDBC", "rds.driver.oracle");
    assert!(text.contains("Oracle JDBC"), "{text}");
    assert!(text.contains("rds.driver.oracle"), "{text}");
    assert!(text.contains("安装目录"), "{text}");
    assert!(
        text.contains("plugin-data/rds.driver.oracle"),
        "必须说明用户数据保留：{text}"
    );
    assert!(text.contains("依赖"), "必须说明被依赖时会被拒：{text}");
}

//! 插件管理页纯函数单测（不渲染窗口，不需要 gpui 测试上下文）。

use plugin::manifest::{BackendRuntime, PluginPermissions};
use plugin::wasm::{PluginMetadata, PluginState, PluginType};
use plugin::{
    ContributionCounts, PluginDriverRegistration, PluginDriverSummary, PluginInfo, PluginKind,
};

use super::widgets::{
    contribution_summary, driver_id_conflicts, driver_registry_rows, filtered_plugins,
    plugin_backend_label, plugin_diagnostics, plugin_driver_chain,
    plugin_driver_registration_label, plugin_driver_stats, plugin_driver_stats_label,
    plugin_kind_label, plugin_receives_database_credentials, plugin_state_is_active,
    plugin_state_label, plugin_stats, plugin_uninstall_block_reason, DriverChainLevel,
    DriverRegistryStatus, PluginFilter,
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
        drivers: Vec::new(),
        registered_drivers: None,
        backend_kind: Some(plugin::manifest::BackendKind::Sidecar),
        runtime: Some(plugin::manifest::BackendRuntime::Rust),
        permissions: PluginPermissions::default(),
        contribution_items: Vec::new(),
        install_path: None,
        dependencies: Vec::new(),
        dependents: None,
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
fn driver_filter_keeps_only_plugins_that_declare_drivers() {
    let mut oracle = info("Oracle 驱动", "rds.driver.oracle-rust", PluginState::Active, "");
    oracle.drivers = vec![PluginDriverSummary {
        id: "oracle".to_string(),
        display_name: "Oracle Database (Rust)".to_string(),
        default_port: Some(1521),
        category: None,
    }];
    let panel = info(
        "表大小看板",
        "rds.panel.table_size",
        PluginState::Active,
        "",
    );

    let all = vec![oracle, panel];
    assert_eq!(filtered_plugins(&all, PluginFilter::Drivers, "").len(), 1);
    assert_eq!(
        filtered_plugins(&all, PluginFilter::Drivers, "SQL Server").len(),
        0
    );
    // 驱动显示名与 id 都进搜索：用户找的是驱动，不是插件包名。
    assert_eq!(
        filtered_plugins(&all, PluginFilter::All, "database (rust)").len(),
        1
    );
    assert_eq!(filtered_plugins(&all, PluginFilter::All, "1521").len(), 0);
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

#[test]
fn backend_label_distinguishes_native_from_interpreted() {
    let mut plugin = info(
        "Oracle 驱动",
        "rds.driver.oracle",
        PluginState::Active,
        "Oracle 连接",
    );
    assert_eq!(plugin_backend_label(&plugin), "侧车进程 · 原生 rust");
    plugin.runtime = Some(BackendRuntime::Python);
    assert_eq!(plugin_backend_label(&plugin), "侧车进程 · python 解释器");
    plugin.runtime = Some(BackendRuntime::Java);
    assert_eq!(plugin_backend_label(&plugin), "侧车进程 · java 解释器");
    plugin.runtime = None;
    assert_eq!(plugin_backend_label(&plugin), "侧车进程");
}

#[test]
fn database_credentials_are_flagged_only_for_database_scope() {
    let mut plugin = info(
        "Oracle 驱动",
        "rds.driver.oracle",
        PluginState::Active,
        "Oracle 连接",
    );
    assert!(!plugin_receives_database_credentials(&plugin));
    plugin.permissions.credentials = Some("database".to_string());
    assert!(plugin_receives_database_credentials(&plugin));
    plugin.permissions.credentials = Some("network".to_string());
    assert!(!plugin_receives_database_credentials(&plugin));
}

#[test]
fn driver_registration_label_separates_unknown_from_unregistered() {
    let mut plugin = info(
        "Oracle 驱动",
        "rds.driver.oracle",
        PluginState::Active,
        "Oracle 连接",
    );
    // 没读到全局库：不能说「未登记」
    assert_eq!(
        plugin_driver_registration_label(&plugin, "oracle"),
        "登记状态未知"
    );
    // 读到了、但没有这一行：确实未登记
    plugin.registered_drivers = Some(Vec::new());
    assert_eq!(
        plugin_driver_registration_label(&plugin, "oracle"),
        "未登记"
    );
    // 已登记但插件停用：行仍在，连接对话框会置灰
    plugin.registered_drivers = Some(vec![plugin::PluginDriverRegistration {
        id: "oracle".to_string(),
        enabled: false,
    }]);
    assert_eq!(
        plugin_driver_registration_label(&plugin, "oracle"),
        "已登记 · 已停用"
    );
    plugin.registered_drivers = Some(vec![plugin::PluginDriverRegistration {
        id: "oracle".to_string(),
        enabled: true,
    }]);
    assert_eq!(
        plugin_driver_registration_label(&plugin, "oracle"),
        "已登记"
    );
}

#[test]
fn driver_stats_separate_missing_registration_from_unknown() {
    let mut plugin = info("Oracle", "rds.driver.oracle", PluginState::Active, "");
    plugin.drivers = vec![
        PluginDriverSummary {
            id: "oracle".to_string(),
            display_name: "Oracle".to_string(),
            default_port: Some(1521),
            category: None,
        },
        PluginDriverSummary {
            id: "mssql".to_string(),
            display_name: "SQL Server".to_string(),
            default_port: Some(1433),
            category: None,
        },
    ];
    plugin.registered_drivers = Some(vec![
        PluginDriverRegistration {
            id: "oracle".to_string(),
            enabled: true,
        },
        PluginDriverRegistration {
            id: "mssql".to_string(),
            enabled: false,
        },
    ]);
    let stats = plugin_driver_stats(std::slice::from_ref(&plugin));
    assert_eq!(stats.declared, 2);
    assert_eq!(stats.registered, 1);
    assert!(!stats.unknown);
    assert_eq!(plugin_driver_stats_label(stats), "驱动 1/2 已登记");

    // 读不到登记表 = 未知，不能显示成「0/2 已登记」
    plugin.registered_drivers = None;
    let stats = plugin_driver_stats(std::slice::from_ref(&plugin));
    assert!(stats.unknown);
    assert_eq!(
        plugin_driver_stats_label(stats),
        "驱动 0/2 已登记 · 状态未知"
    );

    // 没有驱动声明的插件不该把「未知」带出来
    let no_drivers = info(
        "表大小看板",
        "rds.panel.table_size",
        PluginState::Active,
        "",
    );
    let stats = plugin_driver_stats(std::slice::from_ref(&no_drivers));
    assert!(!stats.unknown);
    assert_eq!(plugin_driver_stats_label(stats), "驱动 0");
}

#[test]
fn duplicate_driver_ids_across_enabled_plugins_are_reported() {
    let mut jdbc = info("Oracle JDBC", "rds.driver.oracle", PluginState::Active, "");
    jdbc.drivers = vec![PluginDriverSummary {
        id: "oracle".to_string(),
        display_name: "Oracle Database".to_string(),
        default_port: Some(1521),
        category: None,
    }];
    // 登记表读到了、但两行都没写进去：这里只验冲突，不让 `unknown` 干扰文案。
    jdbc.registered_drivers = Some(Vec::new());
    let mut rust = info(
        "Oracle Rust",
        "rds.driver.oracle-rust",
        PluginState::Active,
        "",
    );
    rust.drivers = vec![PluginDriverSummary {
        id: "oracle".to_string(),
        display_name: "Oracle Database (Rust)".to_string(),
        default_port: Some(1521),
        category: None,
    }];
    rust.registered_drivers = Some(Vec::new());

    let both = vec![jdbc.clone(), rust.clone()];
    let conflicts = driver_id_conflicts(&both);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].driver_id, "oracle");
    assert_eq!(
        conflicts[0].plugins,
        vec!["rds.driver.oracle", "rds.driver.oracle-rust"]
    );

    let stats = plugin_driver_stats(&both);
    assert_eq!(stats.conflicts, 1);
    assert_eq!(
        plugin_driver_stats_label(stats),
        "驱动 0/2 已登记 · 1 个驱动 id 冲突"
    );

    let chain = plugin_driver_chain(&both);
    assert_eq!(chain.conflicts.len(), 1);
    assert_eq!(chain.steps[1].level, DriverChainLevel::Bad);
    assert_eq!(
        chain.bottleneck.as_deref(),
        Some(
            "卡在：驱动 id 冲突（oracle 由 rds.driver.oracle、rds.driver.oracle-rust 同时声明），先停用或卸载其中一个插件。"
        )
    );

    // 停用一侧后不再冲突：停用插件不参与驱动注册。
    let mut off = rust;
    off.state = PluginState::Inactive;
    let conflicts = driver_id_conflicts(&[jdbc, off]);
    assert!(conflicts.is_empty());
}

#[test]
fn driver_chain_points_at_the_first_break() {
    let mut plugin = info("Oracle", "rds.driver.oracle", PluginState::Active, "");
    plugin.drivers = vec![
        PluginDriverSummary {
            id: "oracle".to_string(),
            display_name: "Oracle".to_string(),
            default_port: Some(1521),
            category: None,
        },
        PluginDriverSummary {
            id: "mssql".to_string(),
            display_name: "SQL Server".to_string(),
            default_port: Some(1433),
            category: None,
        },
    ];
    plugin.registered_drivers = Some(vec![
        PluginDriverRegistration {
            id: "oracle".to_string(),
            enabled: true,
        },
        PluginDriverRegistration {
            id: "mssql".to_string(),
            enabled: true,
        },
    ]);

    // 全通：四步全绿，无卡点
    let chain = plugin_driver_chain(std::slice::from_ref(&plugin));
    assert!(
        chain
            .steps
            .iter()
            .all(|step| step.level == DriverChainLevel::Ok)
    );
    assert_eq!(chain.bottleneck, None);

    // 插件停用：卡在第二步，后续两步也随之为空
    plugin.state = PluginState::Inactive;
    let chain = plugin_driver_chain(std::slice::from_ref(&plugin));
    assert_eq!(chain.steps[1].level, DriverChainLevel::Bad);
    assert_eq!(
        chain.bottleneck.as_deref(),
        Some("卡在：声明了驱动的插件都未启用。")
    );

    // 读不到登记表：第三步是警告，不是「未写入」
    plugin.state = PluginState::Active;
    plugin.registered_drivers = None;
    let chain = plugin_driver_chain(std::slice::from_ref(&plugin));
    assert_eq!(chain.steps[2].level, DriverChainLevel::Warn);
    assert_eq!(
        chain.bottleneck.as_deref(),
        Some("卡在：本次没读到 global.db 的 drivers 表。")
    );

    // 部分登记：链路能跑通，但要提示差额
    plugin.registered_drivers = Some(vec![PluginDriverRegistration {
        id: "oracle".to_string(),
        enabled: true,
    }]);
    let chain = plugin_driver_chain(std::slice::from_ref(&plugin));
    assert_eq!(chain.steps[3].level, DriverChainLevel::Ok);
    assert_eq!(
        chain.bottleneck.as_deref(),
        Some("注意：1 个驱动不可用（0 行停用、1 个未登记）。")
    );

    // 行已写入但全部停用：不能报成「没写入」，卡点要指向插件激活
    plugin.registered_drivers = Some(vec![
        PluginDriverRegistration {
            id: "oracle".to_string(),
            enabled: false,
        },
        PluginDriverRegistration {
            id: "mssql".to_string(),
            enabled: false,
        },
    ]);
    let chain = plugin_driver_chain(std::slice::from_ref(&plugin));
    assert_eq!(chain.steps[2].level, DriverChainLevel::Warn);
    assert_eq!(chain.steps[2].detail, "2 行已写入 · 2 行停用");
    assert_eq!(
        chain.bottleneck.as_deref(),
        Some("卡在：驱动行已写入但处于停用状态（插件激活没成功，看插件错误与 sidecar.log）。")
    );
}

#[test]
fn uninstall_blocks_only_when_dependents_are_known() {
    let mut plugin = info(
        "Oracle 驱动",
        "rds.driver.oracle",
        PluginState::Active,
        "Oracle 连接",
    );
    // 没读到依赖表：不拦，交给卸载时实时查库
    assert_eq!(plugin_uninstall_block_reason(&plugin), None);
    // 读到了、确实没人依赖
    plugin.dependents = Some(Vec::new());
    assert_eq!(plugin_uninstall_block_reason(&plugin), None);
    // 已知有依赖者：前置拦住，并列出名字
    plugin.dependents = Some(vec![
        "rds.insight.sales".to_string(),
        "rds.mock.orders".to_string(),
    ]);
    assert_eq!(
        plugin_uninstall_block_reason(&plugin).as_deref(),
        Some("被 rds.insight.sales、rds.mock.orders 依赖，需先卸载它们")
    );
}

#[test]
fn driver_registry_rows_join_declarations_with_registration_state() {
    let mut oracle = info(
        "Oracle 驱动",
        "rds.driver.oracle-rust",
        PluginState::Active,
        "",
    );
    oracle.drivers = vec![PluginDriverSummary {
        id: "oracle".to_string(),
        display_name: "Oracle Database (Rust)".to_string(),
        default_port: Some(1521),
        category: None,
    }];
    oracle.registered_drivers = Some(vec![PluginDriverRegistration {
        id: "oracle".to_string(),
        enabled: true,
    }]);

    // 同一驱动 id 的第二个启用插件：总表要显示两行，并给两行都标冲突。
    let mut duplicate = info(
        "Oracle 旧版",
        "rds.driver.oracle-copy",
        PluginState::Active,
        "",
    );
    duplicate.drivers = vec![PluginDriverSummary {
        id: "oracle".to_string(),
        display_name: "Oracle Database (JDBC)".to_string(),
        default_port: Some(1521),
        category: None,
    }];
    duplicate.registered_drivers = Some(Vec::new());

    let mut mssql = info(
        "SQL Server",
        "rds.driver.mssql-rust",
        PluginState::Loaded,
        "",
    );
    mssql.drivers = vec![PluginDriverSummary {
        id: "mssql".to_string(),
        display_name: "SQL Server (Rust)".to_string(),
        default_port: Some(1433),
        category: None,
    }];
    mssql.registered_drivers = Some(vec![PluginDriverRegistration {
        id: "mssql".to_string(),
        enabled: false,
    }]);

    let mut postgres = info(
        "PostgreSQL",
        "rds.driver.postgres-py",
        PluginState::Inactive,
        "",
    );
    postgres.drivers = vec![PluginDriverSummary {
        id: "postgresql".to_string(),
        display_name: "PostgreSQL".to_string(),
        default_port: Some(5432),
        category: None,
    }];
    // 没读到全局库：状态是未知，不是未登记
    postgres.registered_drivers = None;

    let rows = driver_registry_rows(&[oracle, duplicate, mssql, postgres]);
    assert_eq!(rows.len(), 4);
    // 先按驱动 id、再按插件 id 排序
    assert_eq!(rows[0].driver_id, "mssql");
    assert_eq!(rows[1].driver_id, "oracle");
    assert_eq!(rows[1].plugin_id, "rds.driver.oracle-copy");
    assert_eq!(rows[2].plugin_id, "rds.driver.oracle-rust");
    assert_eq!(rows[3].driver_id, "postgresql");

    let oracle_row = &rows[2];
    assert_eq!(oracle_row.status, DriverRegistryStatus::Registered);
    assert!(oracle_row.conflict);
    let duplicate_row = &rows[1];
    assert_eq!(duplicate_row.status, DriverRegistryStatus::Unregistered);
    assert!(duplicate_row.conflict);
    assert_eq!(rows[0].status, DriverRegistryStatus::Disabled);
    assert!(!rows[0].conflict);
    assert_eq!(rows[3].status, DriverRegistryStatus::Unknown);
    assert!(!rows[3].conflict);
}

#[test]
fn diagnostics_report_carries_state_paths_and_driver_registration() {
    let mut plugin = info(
        "Oracle 驱动",
        "rds.driver.oracle",
        PluginState::Error,
        "Oracle 连接",
    );
    plugin.error = Some("解释器未解析".to_string());
    plugin.registered_drivers = Some(vec![plugin::PluginDriverRegistration {
        id: "oracle".to_string(),
        enabled: true,
    }]);
    plugin.drivers = vec![plugin::PluginDriverSummary {
        id: "oracle".to_string(),
        display_name: "Oracle Database".to_string(),
        default_port: Some(1521),
        category: None,
    }];

    let report = plugin_diagnostics(&plugin);
    assert!(report.contains("rds.driver.oracle"), "{report}");
    assert!(report.contains("状态：出错"), "{report}");
    assert!(report.contains("错误：解释器未解析"), "{report}");
    assert!(report.contains("安装目录：未上报"), "{report}");
    assert!(report.contains("sidecar.log"), "{report}");
    assert!(
        report.contains("Oracle Database（oracle）· 已登记 · 默认端口 1521"),
        "{report}"
    );
}

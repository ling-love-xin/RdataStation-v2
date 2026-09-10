// RdataStation Core Library
//
// 架构分层：
// - api/: 对外 DTO / Error（前后端共享）
// - core/: 纯业务逻辑（无框架依赖）
// - adapters/: 适配器层（Tauri/CLI/HTTP/WASM）
// - commands/: Tauri 命令（统一入口）

#![allow(dependency_on_unit_never_type_fallback)]

pub mod adapters;
pub mod api;
pub mod commands;
pub mod core;
pub mod mock;

// 重新导出常用类型
pub use api::{ErrorResponse, QueryResult, Row, Value};

// 统一从 commands 模块导入所有 Tauri 命令
use commands::*;

// tauri-specta: TypeScript 绑定生成
use tauri_specta::{collect_commands, Builder};

// 项目状态管理
use commands::project_commands::ProjectState;
// 分析资源状态管理
use crate::core::scratchpad::ScratchpadState;
use commands::analytics_resource_commands::AnalyticsResourceState;

use crate::core::logging::record::LogLevel;
use crate::core::persistence::log_store::LogStore;
use std::sync::Arc;

fn register_drivers() {
    use core::driver::auto_register::AutoDriverRegistrar;
    AutoDriverRegistrar::auto_register();
}

/// 获取日志目录路径
fn get_log_dir() -> std::path::PathBuf {
    let base = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("RdataStation")
        .join("logs");
    let _ = std::fs::create_dir_all(&base);
    base
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_dir = get_log_dir();

    // 阶段1: 初始化 tracing subscriber（stderr + 文件 + DB channel）
    // DatabaseLogLayer 开始捕获日志到 channel，consumer 稍后启动
    let log_rx = match core::logging::subscriber::init_tracing_with_db(&log_dir, LogLevel::Info, 7)
    {
        Ok(rx) => rx,
        Err(e) => {
            eprintln!("FATAL: Failed to initialize tracing subscriber: {}", e);
            std::process::exit(1);
        }
    };

    register_drivers();

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("FATAL: Failed to create Tokio runtime: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = rt.block_on(core::migration::initialize_global_system()) {
        tracing::error!("Global system database initialization failed: {}", e);
        eprintln!("ERROR: Global system database initialization failed: {}", e);
    }

    if let Err(e) = rt.block_on(core::driver::init_driver_manager()) {
        tracing::error!("Driver manager initialization failed: {}", e);
        eprintln!("ERROR: Driver manager initialization failed: {}", e);
    }

    if let Err(e) = rt.block_on(core::plugin::init_plugin_manager()) {
        tracing::error!("Plugin manager initialization failed: {}", e);
        eprintln!("ERROR: Plugin manager initialization failed: {}", e);
    }

    // 初始化插件加载器
    let plugin_install_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("RdataStation")
        .join("plugins");
    if let Err(e) = rt.block_on(core::plugin::init_plugin_loader(plugin_install_dir.clone())) {
        tracing::error!("Plugin loader initialization failed: {}", e);
        eprintln!("ERROR: Plugin loader initialization failed: {}", e);
    }

    tracing::info!(
        "Plugin system initialized with install dir: {:?}",
        plugin_install_dir
    );

    // 阶段2: 数据库就绪后，创建 LogStore 并启动日志消费任务
    // 必须在 Tokio runtime 上下文中调用 spawn_log_consumer（内部使用 tokio::spawn）
    let _log_store = rt.block_on(async {
        let db_manager = core::migration::get_global_db_manager();
        match db_manager {
            Some(manager) => {
                let store = Arc::new(LogStore::new(manager.sqlite_pool()));
                core::logging::set_log_store(store.clone());
                let _consumer =
                    core::logging::subscriber::spawn_log_consumer(log_rx, store.clone());
                tracing::info!("Log store initialized with database persistence");
                Some(store)
            }
            None => {
                tracing::warn!("Global database manager not available, log persistence disabled");
                None
            }
        }
    });

    // ===== tauri-specta: 收集命令并导出 TypeScript 绑定 =====
    // 注意：返回 serde_json::Value / Vec<serde_json::Value> 的命令无法进入 collect_commands!
    // (specta 无法为递归类型生成 TypeScript)，这些命令仅在 all_handler 中注册。
    let specta_builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        // 连接命令
        connect_database,
        get_connections,
        switch_connection,
        close_connection,
        close_all_connections,
        get_active_connection,
        get_recent_connections,
        remove_recent_connection,
        test_connection,
        test_connection_config,
        create_database_file,
        convert_connection_type,
        detect_global_connections_in_project,
        get_global_connections,
        // 数据源命令
        get_data_source_types,
        get_available_drivers,
        get_driver_detail,
        install_driver,
        list_driver_files,
        list_environments,
        create_environment,
        update_environment,
        delete_environment,
        list_environment_policies,
        create_environment_policy,
        update_environment_policy,
        delete_environment_policy,
        list_auth_configs,
        create_auth_config,
        update_auth_config,
        delete_auth_config,
        list_network_configs,
        create_network_config,
        update_network_config,
        delete_network_config,
        test_network_config,
        get_all_drivers_catalog,
        enable_driver_for_project,
        disable_driver_for_project,
        list_enabled_project_drivers,
        project_create_environment,
        project_list_environments,
        project_update_environment,
        project_delete_environment,
        project_create_environment_policy,
        project_list_environment_policies,
        project_update_environment_policy,
        project_delete_environment_policy,
        project_create_auth_config,
        project_list_auth_configs,
        project_delete_auth_config,
        project_update_auth_config,
        project_create_network_config,
        project_list_network_configs,
        project_update_network_config,
        project_delete_network_config,
        // 快照命令
        snapshot_global_env,
        snapshot_global_auth,
        snapshot_global_network,
        validate_connection_config,
        // SQL 命令
        execute_sql,
        execute_transaction,
        begin_transaction,
        commit_transaction,
        rollback_transaction,
        get_transaction_status,
        get_sql_history,
        search_sql_history,
        clear_sql_history,
        remove_sql_history,
        // 驱动命令
        get_drivers,
        get_driver_info,
        create_connection,
        create_connection_with_config,
        update_connection,
        // 导航器状态命令
        save_navigator_state,
        load_navigator_state,
        // 元数据浏览命令
        load_databases,
        load_catalogs,
        load_schemas,
        load_tables,
        load_views,
        load_columns,
        load_indexes,
        load_constraints,
        load_procedures,
        load_functions,
        load_sequences,
        load_triggers,
        load_routine_source,
        invalidate_metadata_cache,
        get_cache_stats,
        reset_cache_stats,
        set_introspection_level,
        get_introspection_level,
        remove_introspection_level,
        // 项目命令
        create_project,
        get_project_config,
        update_project_config,
        get_recent_projects,
        add_recent_project,
        open_project_by_id,
        open_project_by_path,
        create_and_save_project,
        validate_project,
        validate_project_full,
        delete_project,
        update_project,
        rename_project,
        get_all_projects,
        remove_from_recent,
        delete_project_disk,
        // 端口协商命令
        negotiate_port,
        negotiate_local_port,
        is_port_available,
        release_port,
        negotiate_multiple_ports,
        negotiate_port_range,
        get_common_db_ports,
        get_port_range_info,
        // 联邦查询命令
        register_external_database,
        create_external_table,
        // DuckDB 加速查询
        execute_duckdb_accelerated,
        execute_sql_paginated,
        // 元数据缓存命令（排除返回 Vec<serde_json::Value> 的 get_tables_from_cache / get_columns_from_cache）
        metadata_cache_commands::get_metadata_cache_status,
        metadata_cache_commands::refresh_metadata_cache,
        metadata_cache_commands::clear_metadata_cache,
        metadata_cache_commands::get_sync_status,
        metadata_cache_commands::save_table_metadata_to_cache,
        metadata_cache_commands::save_tables_batch_to_cache,
        metadata_cache_commands::save_columns_batch_to_cache,
        metadata_cache_commands::cancel_sync,
        metadata_cache_commands::notify_ddl_event,
        // 缓存预热命令
        cache_warming_commands::start_cache_warming,
        cache_warming_commands::check_cache_version,
        cache_warming_commands::execute_cache_migration,
        // get_cache_migration_history 返回 Vec<serde_json::Value>，排除
        cache_warming_commands::get_introspect_level_suggestion,
        cache_warming_commands::get_schema_object_counts,
        // SQL 解析与转译命令
        parse_sql,
        format_sql,
        transpile_sql,
        validate_sql,
        split_sql,
        cancel_sql_query,
        ping_connection,
        // 结果集分析命令（排除返回 serde_json::Value 的 execute_insight_rule/get_column_insights/get_column_insight_full/list_insight_rules/list_rules_for_column）
        re_execute_with_filter,
        profile_column_from_table,
        execute_duckdb_analysis,
        create_duckdb_temp_table,
        save_cell_update,
        reload_insight_rules,
        get_table_profile,
        get_column_quality,
        batch_evaluate_columns,
        get_schema_insight,
        // 数据导出命令
        export_result_to_file,
        // 模拟数据生成 Mock
        mock_generate,
        mock_preview,
        mock_export,
        mock_map_column,
        mock_map_columns_batch,
        mock_list_templates,
        mock_import_schema,
        mock_apply_template,
        mock_save_to_scratchpad,
        mock_persist_as_asset,
        mock_cancel_generate,
        mock_generate_scenario,
        mock_resolve_dependencies,
        // 模拟数据生成 Mock 持久化
        save_mock_generation_task,
        get_mock_generation_history,
        get_mock_generation_detail,
        delete_mock_generation_task,
        save_mock_template,
        get_mock_templates,
        get_mock_template_detail,
        // 日志命令
        get_logs,
        search_logs,
        get_log_stats,
        clear_logs,
        get_log_session_id,
        export_logs,
        set_log_level,
        // 系统信息命令
        get_api_version,
    ]);

    // 全量命令处理器：collect_commands! 仅用于 specta 类型导出（上方），实际 dispatch 由此 handler 负责。
    // 插件命令含 State 参数，#[specta::specta] 不支持，因此不能放入 collect_commands!。
    // 使用函数 + impl Trait 返回类型包裹，避免 Tauri 2.x 大型 generate_handler! 的类型推断失败。
    fn make_handler1() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
        tauri::generate_handler![
            // ===== 连接命令 =====
            connect_database,
            get_connections,
            switch_connection,
            close_connection,
            close_all_connections,
            get_active_connection,
            get_recent_connections,
            remove_recent_connection,
            test_connection,
            test_connection_config,
            create_database_file,
            convert_connection_type,
            detect_global_connections_in_project,
            get_global_connections,
            // ===== 数据源命令 =====
            get_data_source_types,
            get_available_drivers,
            get_driver_detail,
            install_driver,
            list_driver_files,
            list_environments,
            create_environment,
            update_environment,
            delete_environment,
            list_environment_policies,
            create_environment_policy,
            update_environment_policy,
            delete_environment_policy,
            list_auth_configs,
            create_auth_config,
            update_auth_config,
            delete_auth_config,
            list_network_configs,
            create_network_config,
            update_network_config,
            delete_network_config,
            test_network_config,
            get_all_drivers_catalog,
            enable_driver_for_project,
            disable_driver_for_project,
            list_enabled_project_drivers,
            project_create_environment,
            project_list_environments,
            project_update_environment,
            project_delete_environment,
            project_create_environment_policy,
            project_list_environment_policies,
            project_update_environment_policy,
            project_delete_environment_policy,
            project_create_auth_config,
            project_list_auth_configs,
            project_delete_auth_config,
            project_update_auth_config,
            project_create_network_config,
            project_list_network_configs,
            project_update_network_config,
            project_delete_network_config,
            // ===== 快照命令 =====
            snapshot_global_env,
            snapshot_global_auth,
            snapshot_global_network,
            validate_connection_config,
            // ===== SQL 命令 =====
            execute_sql,
            execute_transaction,
            begin_transaction,
            commit_transaction,
            rollback_transaction,
            get_transaction_status,
            get_sql_history,
            search_sql_history,
            clear_sql_history,
            remove_sql_history,
            // ===== 驱动命令 =====
            get_drivers,
            get_driver_info,
            create_connection,
            create_connection_with_config,
            update_connection,
            // ===== 导航器状态命令 =====
            save_navigator_state,
            load_navigator_state,
            // ===== 元数据浏览命令 =====
            load_databases,
            load_catalogs,
            load_schemas,
            load_tables,
            load_views,
            load_columns,
            load_indexes,
            load_constraints,
            load_procedures,
            load_functions,
            load_sequences,
            load_triggers,
            load_routine_source,
            invalidate_metadata_cache,
            get_cache_stats,
            reset_cache_stats,
            set_introspection_level,
            get_introspection_level,
            remove_introspection_level,
            // ===== 项目命令 =====
            create_project,
            get_project_config,
            update_project_config,
            get_recent_projects,
            add_recent_project,
            open_project_by_id,
            open_project_by_path,
            create_and_save_project,
            validate_project,
            validate_project_full,
            delete_project,
            update_project,
            rename_project,
            get_all_projects,
            remove_from_recent,
            delete_project_disk,
            // ===== 端口协商命令 =====
            negotiate_port,
            negotiate_local_port,
            is_port_available,
            release_port,
            negotiate_multiple_ports,
            negotiate_port_range,
            get_common_db_ports,
            get_port_range_info,
        ]
    }

    fn make_handler2() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
        tauri::generate_handler![
            // ===== 项目连接命令 =====
            create_project_connection,
            get_project_connections,
            get_project_connection,
            update_project_connection,
            update_project_connection_status,
            delete_project_connection,
            search_project_connections,
            // ===== 项目存储命令 =====
            init_project_store,
            close_project_store,
            save_project_store_connection,
            get_project_store_connections,
            get_project_store_connection,
            delete_project_store_connection,
            save_project_store_sql_history,
            get_project_store_sql_history,
            save_project_store_workbench_state,
            get_project_store_workbench_state,
            // ===== 联邦查询命令 =====
            register_external_database,
            create_external_table,
            // ===== 元数据缓存命令 =====
            metadata_cache_commands::get_metadata_cache_status,
            metadata_cache_commands::refresh_metadata_cache,
            metadata_cache_commands::clear_metadata_cache,
            metadata_cache_commands::get_sync_status,
            metadata_cache_commands::save_table_metadata_to_cache,
            // REMOVED: too many params for specta (12 > 10 limit)
            // metadata_cache_commands::save_column_metadata_to_cache,
            metadata_cache_commands::save_tables_batch_to_cache,
            metadata_cache_commands::save_columns_batch_to_cache,
            metadata_cache_commands::get_tables_from_cache,
            metadata_cache_commands::get_columns_from_cache,
            metadata_cache_commands::cancel_sync,
            metadata_cache_commands::notify_ddl_event,
            // ===== 缓存预热命令 =====
            cache_warming_commands::start_cache_warming,
            cache_warming_commands::cancel_cache_warming,
            cache_warming_commands::get_warming_progress,
            cache_warming_commands::check_cache_version,
            cache_warming_commands::execute_cache_migration,
            cache_warming_commands::get_cache_migration_history,
            cache_warming_commands::get_introspect_level_suggestion,
            cache_warming_commands::get_schema_object_counts,
            cache_warming_commands::build_cache_index,
            // ===== SQL 解析与转译命令 =====
            parse_sql,
            format_sql,
            transpile_sql,
            validate_sql,
            split_sql,
            cancel_sql_query,
            execute_duckdb_accelerated,
            ping_connection,
            // ===== 结果集分析命令 =====
            re_execute_with_filter,
            profile_column_from_table,
            get_insight_version_detail,
            execute_duckdb_analysis,
            get_column_insights,
            get_column_insight_full,
            create_duckdb_temp_table,
            save_cell_update,
            save_column_insight_snapshot,
            get_column_insight_history,
            cleanup_insight_snapshots,
            get_insight_storage_stats,
            execute_insight_rule,
            list_insight_rules,
            list_rules_for_column,
            reload_insight_rules,
            get_table_profile,
            get_column_quality,
            batch_evaluate_columns,
            get_schema_insight,
            // ===== 数据导出命令 =====
            export_result_to_file,
            // ===== 分析资源管理命令 =====
            create_analytics_resource,
            update_analytics_resource,
            get_analytics_resource,
            list_analytics_resources,
            list_analytics_resources_paginated,
            delete_analytics_resource,
            batch_delete_analytics_resources,
            clone_analytics_resource,
            create_analytics_folder,
            get_analytics_folder,
            list_analytics_folders,
            add_analytics_resource_to_folder,
            remove_analytics_resource_from_folder,
            create_analytics_tag,
            list_analytics_tags,
            get_analytics_tag,
            add_analytics_tag_to_resource,
            remove_analytics_tag_from_resource,
            get_analytics_recycle_bin,
            restore_analytics_resource_from_recycle,
            permanent_delete_analytics_resource,
            init_analytics_resource_store,
            get_resource_versions,
            get_tags_for_resource,
            get_resources_by_tag,
            // ===== 草稿箱命令 =====
            list_scratchpad_files,
            list_scratchpad_directory,
            create_scratchpad_entry,
            delete_scratchpad_entry,
            rename_scratchpad_entry,
            read_scratchpad_file,
            save_scratchpad_file,
            import_external_file,
            add_external_reference,
            remove_external_reference,
            open_scratchpad_in_explorer,
            check_scratchpad_file_size,
            get_scratchpad_entry,
            init_scratchpad_store,
            list_scratchpad_trash,
            restore_scratchpad_from_trash,
            empty_scratchpad_trash,
            get_analyzable_files,
            update_scratchpad_file_meta,
            search_scratchpad_content,
            watch_scratchpad,
            unwatch_scratchpad,
            promote_scratchpad_to_resource,
            move_scratchpad_entry,
            replace_scratchpad_content,
            diff_scratchpad_with_content,
            // ===== 模拟数据生成命令 =====
            mock_generate,
            mock_preview,
            mock_export,
            mock_map_column,
            mock_map_columns_batch,
            mock_list_templates,
            mock_import_schema,
            mock_apply_template,
            mock_save_to_scratchpad,
            mock_persist_as_asset,
            mock_cancel_generate,
            mock_generate_scenario,
            mock_resolve_dependencies,
            // ===== 模拟数据持久化命令 =====
            save_mock_generation_task,
            get_mock_generation_history,
            get_mock_generation_detail,
            delete_mock_generation_task,
            save_mock_template,
            get_mock_templates,
            get_mock_template_detail,
            // ===== 日志命令 =====
            get_logs,
            search_logs,
            get_log_stats,
            clear_logs,
            get_log_session_id,
            export_logs,
            set_log_level,
            // ===== 系统信息命令 =====
            get_api_version,
            // ===== 插件系统命令 (含 State 参数，无 #[specta::specta]) =====
            plugin_db_query,
            plugin_db_metadata,
            plugin_list,
            plugin_load,
            plugin_activate,
            plugin_deactivate,
            plugin_unload,
            plugin_get_status,
            plugin_add_directory,
            plugin_get_all_installed,
            plugin_get_with_status,
            plugin_get,
            plugin_install,
            plugin_enable,
            plugin_disable,
            plugin_uninstall,
            project_plugin_enable,
            project_plugin_disable,
            project_plugin_remove,
            project_plugin_list,
            project_plugin_set_config,
            project_plugin_get_configs,
            plugin_load_enabled_on_startup,
        ]
    }

    let h1 = make_handler1();
    let h2 = make_handler2();
    let all_handler = move |invoke: tauri::ipc::Invoke<tauri::Wry>| -> bool {
        let cmd = invoke.message.command().to_string();
        // 根据命令字符串判断属于哪个 handler
        // handler1 覆盖到 get_port_range_info 之前的所有命令
        match cmd.as_str() {
            // handler1 范围的命令（从 connect_database 到 get_port_range_info）
            "connect_database"
            | "get_connections"
            | "switch_connection"
            | "close_connection"
            | "close_all_connections"
            | "get_active_connection"
            | "get_recent_connections"
            | "remove_recent_connection"
            | "test_connection"
            | "test_connection_config"
            | "create_database_file"
            | "convert_connection_type"
            | "detect_global_connections_in_project"
            | "get_global_connections"
            | "get_data_source_types"
            | "get_available_drivers"
            | "get_driver_detail"
            | "install_driver"
            | "list_driver_files"
            | "list_environments"
            | "create_environment"
            | "update_environment"
            | "delete_environment"
            | "list_environment_policies"
            | "create_environment_policy"
            | "update_environment_policy"
            | "delete_environment_policy"
            | "list_auth_configs"
            | "create_auth_config"
            | "update_auth_config"
            | "delete_auth_config"
            | "list_network_configs"
            | "create_network_config"
            | "update_network_config"
            | "delete_network_config"
            | "test_network_config"
            | "get_all_drivers_catalog"
            | "enable_driver_for_project"
            | "disable_driver_for_project"
            | "list_enabled_project_drivers"
            | "project_create_environment"
            | "project_list_environments"
            | "project_update_environment"
            | "project_delete_environment"
            | "project_create_environment_policy"
            | "project_list_environment_policies"
            | "project_update_environment_policy"
            | "project_delete_environment_policy"
            | "project_create_auth_config"
            | "project_list_auth_configs"
            | "project_delete_auth_config"
            | "project_update_auth_config"
            | "project_create_network_config"
            | "project_list_network_configs"
            | "project_update_network_config"
            | "project_delete_network_config"
            | "snapshot_global_env"
            | "snapshot_global_auth"
            | "snapshot_global_network"
            | "validate_connection_config"
            | "execute_sql"
            | "execute_transaction"
            | "begin_transaction"
            | "commit_transaction"
            | "rollback_transaction"
            | "get_transaction_status"
            | "get_sql_history"
            | "search_sql_history"
            | "clear_sql_history"
            | "remove_sql_history"
            | "get_drivers"
            | "get_driver_info"
            | "create_connection"
            | "create_connection_with_config"
            | "update_connection"
            | "save_navigator_state"
            | "load_navigator_state"
            | "load_databases"
            | "load_catalogs"
            | "load_schemas"
            | "load_tables"
            | "load_views"
            | "load_columns"
            | "load_indexes"
            | "load_constraints"
            | "load_procedures"
            | "load_functions"
            | "load_sequences"
            | "load_triggers"
            | "load_routine_source"
            | "invalidate_metadata_cache"
            | "get_cache_stats"
            | "reset_cache_stats"
            | "set_introspection_level"
            | "get_introspection_level"
            | "remove_introspection_level"
            | "create_project"
            | "get_project_config"
            | "update_project_config"
            | "get_recent_projects"
            | "add_recent_project"
            | "open_project_by_id"
            | "open_project_by_path"
            | "create_and_save_project"
            | "validate_project"
            | "validate_project_full"
            | "delete_project"
            | "update_project"
            | "rename_project"
            | "get_all_projects"
            | "remove_from_recent"
            | "delete_project_disk"
            | "negotiate_port"
            | "negotiate_local_port"
            | "is_port_available"
            | "release_port"
            | "negotiate_multiple_ports"
            | "negotiate_port_range"
            | "get_common_db_ports"
            | "get_port_range_info" => h1(invoke),
            // 所有其他命令由 handler2 处理
            _ => h2(invoke),
        }
    };

    // 仅在调试模式下导出 TypeScript 绑定文件（仅用于类型生成，不用于 dispatch）
    #[cfg(debug_assertions)]
    {
        // 使用大栈线程避免 specta 类型图递归导致栈溢出
        match std::thread::Builder::new()
            .name("specta-export".into())
            .stack_size(32 * 1024 * 1024) // 32 MiB
            .spawn(move || {
                specta_builder.export(
                    specta_typescript::Typescript::default(),
                    "../src/generated/specta/bindings.ts",
                )
            }) {
            Ok(handle) => {
                match handle.join() {
                    Ok(Ok(())) => { /* bindings.ts 导出成功 */ }
                    Ok(Err(e)) => {
                        eprintln!("[specta] WARNING: Failed to export bindings.ts: {e}");
                        eprintln!("[specta] The app will continue without updated bindings.ts");
                    }
                    Err(_panic) => {
                        eprintln!(
                            "[specta] WARNING: Export thread panicked (likely stack overflow)"
                        );
                        eprintln!("[specta] The app will continue without updated bindings.ts");
                        eprintln!("[specta] Try reducing the number of commands in specta_builder");
                    }
                }
            }
            Err(e) => {
                eprintln!("[specta] WARNING: Failed to spawn export thread: {e}");
                eprintln!("[specta] The app will continue without updated bindings.ts");
            }
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(ProjectState::new())
        .manage(AnalyticsResourceState::new())
        .manage(ScratchpadState::new())
        .manage(core::plugin::get_plugin_manager().cloned())
        .invoke_handler(all_handler)
        .setup(|_app| Ok(()))
        .run(tauri::generate_context!())
        .map_err(|e| {
            eprintln!("FATAL: Failed to run Tauri application: {}", e);
            e
        })
        .ok();
}

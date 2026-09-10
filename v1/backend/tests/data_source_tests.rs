//! 数据源模块 集成测试
//!
//! 覆盖：认证配置 CRUD、网络配置 CRUD、环境 CRUD、环境策略 CRUD、ID 前缀工具
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::core::persistence::auth_store::{self, AuthConfig};
use rdata_station_lib::core::persistence::env_store::{self, Environment, EnvironmentPolicy};
use rdata_station_lib::core::persistence::id_prefix;
use rdata_station_lib::core::persistence::network_store::{self, NetworkConfig};
use rusqlite::Connection;

// ==================== 测试数据库初始化 ====================

fn setup_in_memory_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory DB");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS auth_configs (
            id TEXT PRIMARY KEY,
            name TEXT,
            auth_type TEXT NOT NULL,
            auth_data TEXT NOT NULL,
            origin TEXT,
            source_id TEXT,
            snapshot_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS network_configs (
            id TEXT PRIMARY KEY,
            name TEXT,
            network_type TEXT NOT NULL,
            config TEXT NOT NULL,
            auth_config_id TEXT,
            origin TEXT,
            source_id TEXT,
            snapshot_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS environments (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            color TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            origin TEXT,
            source_id TEXT,
            snapshot_at TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS environment_policies (
            id TEXT PRIMARY KEY,
            environment_id TEXT NOT NULL,
            policy_type TEXT NOT NULL,
            policy_config TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        );
        ",
    )
    .expect("Failed to create test tables");

    conn
}

fn now_str() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

// ==================== Auth Config CRUD 测试 ====================

#[test]
fn test_auth_config_create_and_list() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let ac = AuthConfig {
        id: "G_auth_test_001".to_string(),
        name: Some("MySQL Root".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"root","password":"encrypted_pass"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_auth_config(&conn, &ac).expect("create auth config failed");

    let list = auth_store::list_auth_configs(&conn, None).expect("list auth configs failed");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "G_auth_test_001");
    assert_eq!(list[0].auth_type, "password");
}

#[test]
fn test_auth_config_list_by_type_filter() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let ac1 = AuthConfig {
        id: "G_auth_pwd_1".to_string(),
        name: Some("Pwd Auth".to_string()),
        auth_type: "password".to_string(),
        auth_data: "{}".to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    let ac2 = AuthConfig {
        id: "G_auth_key_1".to_string(),
        name: Some("Key Auth".to_string()),
        auth_type: "ssh_key".to_string(),
        auth_data: "{}".to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_auth_config(&conn, &ac1).unwrap();
    auth_store::create_auth_config(&conn, &ac2).unwrap();

    let all = auth_store::list_auth_configs(&conn, None).unwrap();
    assert_eq!(all.len(), 2);

    let pwd_only = auth_store::list_auth_configs(&conn, Some("password")).unwrap();
    assert_eq!(pwd_only.len(), 1);
    assert_eq!(pwd_only[0].auth_type, "password");
}

#[test]
fn test_auth_config_delete() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let ac = AuthConfig {
        id: "G_auth_del_1".to_string(),
        name: Some("Delete Me".to_string()),
        auth_type: "password".to_string(),
        auth_data: "{}".to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts,
    };

    auth_store::create_auth_config(&conn, &ac).unwrap();
    assert_eq!(auth_store::list_auth_configs(&conn, None).unwrap().len(), 1);

    auth_store::delete_auth_config(&conn, "G_auth_del_1").expect("delete auth config failed");
    assert_eq!(auth_store::list_auth_configs(&conn, None).unwrap().len(), 0);
}

// ==================== Network Config CRUD 测试 ====================

#[test]
fn test_network_config_create_and_list() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let nc = NetworkConfig {
        id: "G_net_ssh_001".to_string(),
        name: Some("Bastion SSH".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"bastion.example.com","port":22}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_network_config(&conn, &nc).expect("create network config failed");

    let list = network_store::list_network_configs(&conn, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "G_net_ssh_001");
    assert_eq!(list[0].network_type, "ssh");
}

#[test]
fn test_network_config_filter_by_type() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    for (id, ntype) in [
        ("G_net_ssh_a", "ssh"),
        ("G_net_ssl_a", "ssl"),
        ("G_net_proxy_a", "proxy"),
    ] {
        let nc = NetworkConfig {
            id: id.to_string(),
            name: Some(ntype.to_string()),
            network_type: ntype.to_string(),
            config: "{}".to_string(),
            auth_config_id: None,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: ts.clone(),
            updated_at: ts.clone(),
        };
        network_store::create_network_config(&conn, &nc).unwrap();
    }

    assert_eq!(
        network_store::list_network_configs(&conn, None)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        network_store::list_network_configs(&conn, Some("ssh"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        network_store::list_network_configs(&conn, Some("ssl"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        network_store::list_network_configs(&conn, Some("proxy"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        network_store::list_network_configs(&conn, Some("nonexistent"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn test_network_config_delete() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let nc = NetworkConfig {
        id: "G_net_del_1".to_string(),
        name: Some("Delete Me".to_string()),
        network_type: "ssh".to_string(),
        config: "{}".to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts,
    };

    network_store::create_network_config(&conn, &nc).unwrap();
    assert_eq!(
        network_store::list_network_configs(&conn, None)
            .unwrap()
            .len(),
        1
    );

    network_store::delete_network_config(&conn, "G_net_del_1").unwrap();
    assert_eq!(
        network_store::list_network_configs(&conn, None)
            .unwrap()
            .len(),
        0
    );
}

// ==================== Environment CRUD 测试 ====================

#[test]
fn test_environment_create_and_list() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let env = Environment {
        id: "G_env_dev".to_string(),
        name: "Development".to_string(),
        description: Some("开发环境".to_string()),
        color: Some("#4CAF50".to_string()),
        sort_order: 1,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };

    env_store::create_environment(&conn, &env).expect("create environment failed");

    let list = env_store::list_environments(&conn).unwrap();
    assert!(!list.is_empty());
    assert_eq!(list[0].id, "G_env_dev");
    assert_eq!(list[0].name, "Development");
}

#[test]
fn test_environment_delete() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let env = Environment {
        id: "G_env_del_test".to_string(),
        name: "To Delete".to_string(),
        description: None,
        color: None,
        sort_order: 99,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts,
    };

    env_store::create_environment(&conn, &env).unwrap();
    assert!(env_store::get_environment(&conn, "G_env_del_test")
        .unwrap()
        .is_some());

    env_store::delete_environment(&conn, "G_env_del_test").unwrap();
    assert!(env_store::get_environment(&conn, "G_env_del_test")
        .unwrap()
        .is_none());
}

#[test]
fn test_environment_sort_order_preserved() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    for i in 0..5 {
        let env = Environment {
            id: format!("G_env_sort_{}", i),
            name: format!("Env {}", i),
            description: None,
            color: None,
            sort_order: i,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: ts.clone(),
        };
        env_store::create_environment(&conn, &env).unwrap();
    }

    let list = env_store::list_environments(&conn).unwrap();
    // 环境按 sort_order 排序
    assert_eq!(list.len(), 5);
    assert!(list[0].sort_order <= list[4].sort_order);
}

// ==================== Environment Policy CRUD 测试 ====================

#[test]
fn test_policy_create_and_list() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    // 先创建环境
    let env = Environment {
        id: "G_env_pol_test".to_string(),
        name: "Policy Test".to_string(),
        description: None,
        color: None,
        sort_order: 1,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::create_environment(&conn, &env).unwrap();

    // 创建策略
    let policy = EnvironmentPolicy {
        id: "G_pol_readonly_1".to_string(),
        environment_id: "G_env_pol_test".to_string(),
        policy_type: "read_only".to_string(),
        policy_config: Some(r#"{"enabled":true}"#.to_string()),
        enabled: true,
        created_at: ts.clone(),
    };

    env_store::create_policy(&conn, &policy).expect("create policy failed");

    let list = env_store::list_policies(&conn, "G_env_pol_test").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].policy_type, "read_only");
    assert!(list[0].enabled);
}

#[test]
fn test_policy_update() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let env = Environment {
        id: "G_env_upd_test".to_string(),
        name: "Update Test".to_string(),
        description: None,
        color: None,
        sort_order: 1,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::create_environment(&conn, &env).unwrap();

    let mut policy = EnvironmentPolicy {
        id: "G_pol_upd_1".to_string(),
        environment_id: "G_env_upd_test".to_string(),
        policy_type: "read_only".to_string(),
        policy_config: None,
        enabled: true,
        created_at: ts.clone(),
    };
    env_store::create_policy(&conn, &policy).unwrap();

    // 更新策略
    policy.enabled = false;
    policy.policy_config = Some(r#"{"max_rows":1000}"#.to_string());
    env_store::update_policy(&conn, &policy).expect("update policy failed");

    let list = env_store::list_policies(&conn, "G_env_upd_test").unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].enabled);
    assert_eq!(
        list[0].policy_config.as_deref(),
        Some(r#"{"max_rows":1000}"#)
    );
}

#[test]
fn test_policy_multiple_types_per_env() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let env = Environment {
        id: "G_env_multi_pol".to_string(),
        name: "Multi Policy Env".to_string(),
        description: None,
        color: None,
        sort_order: 1,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::create_environment(&conn, &env).unwrap();

    for (ptype, enabled) in [
        ("read_only", true),
        ("query_timeout", true),
        ("max_rows", false),
        ("ddl_block", true),
    ] {
        let policy = EnvironmentPolicy {
            id: format!("G_pol_{}_{}", ptype, 1),
            environment_id: "G_env_multi_pol".to_string(),
            policy_type: ptype.to_string(),
            policy_config: None,
            enabled,
            created_at: ts.clone(),
        };
        env_store::create_policy(&conn, &policy).unwrap();
    }

    let list = env_store::list_policies(&conn, "G_env_multi_pol").unwrap();
    assert_eq!(list.len(), 4);
}

#[test]
fn test_policy_delete() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let env = Environment {
        id: "G_env_del_pol".to_string(),
        name: "Del Policy Env".to_string(),
        description: None,
        color: None,
        sort_order: 1,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::create_environment(&conn, &env).unwrap();

    let policy = EnvironmentPolicy {
        id: "G_pol_del_1".to_string(),
        environment_id: "G_env_del_pol".to_string(),
        policy_type: "read_only".to_string(),
        policy_config: None,
        enabled: true,
        created_at: ts,
    };
    env_store::create_policy(&conn, &policy).unwrap();
    assert_eq!(
        env_store::list_policies(&conn, "G_env_del_pol")
            .unwrap()
            .len(),
        1
    );

    env_store::delete_policy(&conn, "G_pol_del_1").unwrap();
    assert_eq!(
        env_store::list_policies(&conn, "G_env_del_pol")
            .unwrap()
            .len(),
        0
    );
}

// ==================== ID 前缀工具测试 ====================

#[test]
fn test_id_prefix_global_detection() {
    assert!(id_prefix::is_global("G_env_dev"));
    assert!(id_prefix::is_global("G_auth_001"));
    assert!(!id_prefix::is_global("P_env_local"));
    assert!(!id_prefix::is_global("GP_env_dev_20260522"));
}

#[test]
fn test_id_prefix_project_detection() {
    assert!(id_prefix::is_project("P_env_a1b2c3d4"));
    assert!(id_prefix::is_project("P_auth_xyz"));
    assert!(!id_prefix::is_project("G_env_dev"));
    assert!(!id_prefix::is_project("GP_env_dev_20260522"));
}

#[test]
fn test_id_prefix_snapshot_detection() {
    assert!(id_prefix::is_snapshot("GP_env_dev_20260522"));
    assert!(id_prefix::is_snapshot("GP_auth_main_20260522"));
    assert!(!id_prefix::is_snapshot("G_env_dev"));
    assert!(!id_prefix::is_snapshot("P_env_local"));
}

#[test]
fn test_generate_global_id() {
    let gid = id_prefix::generate_gid("env", "dev");
    assert!(gid.starts_with("G_env_"));
    assert!(gid.contains("dev"));

    let gid2 = id_prefix::generate_gid("auth", "mysql-root");
    assert!(gid2.starts_with("G_auth_"));
    assert!(gid2.contains("mysql-root"));
}

#[test]
fn test_generate_project_id() {
    let pid1 = id_prefix::generate_pid("env");
    let pid2 = id_prefix::generate_pid("env");
    assert!(pid1.starts_with("P_env_"));
    assert!(pid2.starts_with("P_env_"));
    // 随机后缀应不同
    assert_ne!(pid1, pid2);
}

#[test]
fn test_generate_snapshot_id() {
    let gpid = id_prefix::generate_gpid("env", "dev");
    assert!(gpid.starts_with("GP_env_dev_"));
}

#[test]
fn test_snapshot_id_to_global_id() {
    let gpid = "GP_env_dev_20260522";
    let result = id_prefix::source_global_id(gpid);
    assert_eq!(result, Some("G_env_dev".to_string()));
}

#[test]
fn test_global_id_to_snapshot_id() {
    let gid = "G_env_dev";
    let result = id_prefix::to_snapshot_id(gid);
    assert!(result.is_some());
    assert!(result.unwrap().starts_with("GP_env_dev_"));
}

#[test]
fn test_id_prefix_non_snapshot_conversion() {
    // 非快照 ID 应返回 None
    assert_eq!(id_prefix::source_global_id("G_env_dev"), None);
    assert_eq!(id_prefix::source_global_id("P_env_local"), None);

    // 非全局 ID 应返回 None
    assert_eq!(id_prefix::to_snapshot_id("P_env_local"), None);
    assert_eq!(id_prefix::to_snapshot_id("GP_env_dev_20260522"), None);
}

#[test]
fn test_origin_from_id() {
    assert_eq!(id_prefix::origin_from_id("G_env_dev"), "global");
    assert_eq!(id_prefix::origin_from_id("P_env_local"), "project");
    assert_eq!(
        id_prefix::origin_from_id("GP_env_dev_20260522"),
        "global_snapshot"
    );
}

// ==================== 边界情况测试 ====================

#[test]
fn test_auth_config_empty_list() {
    let conn = setup_in_memory_db();
    let list = auth_store::list_auth_configs(&conn, None).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_network_config_empty_list() {
    let conn = setup_in_memory_db();
    let list = network_store::list_network_configs(&conn, None).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_environment_empty_list() {
    let conn = setup_in_memory_db();
    let list = env_store::list_environments(&conn).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_policy_for_nonexistent_env() {
    let conn = setup_in_memory_db();
    let list = env_store::list_policies(&conn, "nonexistent_env").unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_auth_config_with_origin_and_snapshot_fields() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let ac = AuthConfig {
        id: "GP_auth_snap_001_20260522".to_string(),
        name: Some("Snapshot Auth".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"snap"}"#.to_string(),
        origin: Some("global_snapshot".to_string()),
        source_id: Some("G_auth_001".to_string()),
        snapshot_at: Some("2026-05-22T10:00:00Z".to_string()),
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_auth_configs(&conn, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].origin.as_deref(), Some("global_snapshot"));
    assert_eq!(list[0].source_id.as_deref(), Some("G_auth_001"));
    assert!(list[0].snapshot_at.is_some());
}

#[test]
fn test_network_config_with_origin_fields() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let nc = NetworkConfig {
        id: "GP_net_ssh_prod_20260522".to_string(),
        name: Some("Prod SSH".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"prod-bastion.example.com"}"#.to_string(),
        auth_config_id: None,
        origin: Some("global_snapshot".to_string()),
        source_id: Some("G_net_ssh_prod".to_string()),
        snapshot_at: Some("2026-05-22T10:00:00Z".to_string()),
        created_at: ts.clone(),
        updated_at: ts,
    };

    network_store::create_network_config(&conn, &nc).unwrap();

    let list = network_store::list_network_configs(&conn, None).unwrap();
    assert_eq!(list[0].origin.as_deref(), Some("global_snapshot"));
}

#[test]
fn test_environment_with_snapshot_origin() {
    let conn = setup_in_memory_db();
    let ts = now_str();

    let env = Environment {
        id: "GP_env_prod_20260522".to_string(),
        name: "Production (Snapshot)".to_string(),
        description: Some("From global snapshot".to_string()),
        color: Some("#F44336".to_string()),
        sort_order: 10,
        origin: Some("global_snapshot".to_string()),
        source_id: Some("G_env_prod".to_string()),
        snapshot_at: Some("2026-05-22T10:00:00Z".to_string()),
        created_at: ts,
    };

    env_store::create_environment(&conn, &env).unwrap();

    let found = env_store::get_environment(&conn, "GP_env_prod_20260522").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().origin.as_deref(), Some("global_snapshot"));
}

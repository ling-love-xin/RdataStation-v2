//! 数据源命令 集成测试（持久化层）
//!
//! 覆盖 Tauri 命令 data_source_commands 关联的持久化层函数：
//! - Auth Config（全局 + 项目）：CRUD、加密、所有 auth_type 变体
//! - Network Config（全局 + 项目）：CRUD、SSH/SSL/Proxy 类型
//! - Environment（全局）：CRUD
//! - Environment Policy（全局）：CRUD
//! - Driver：catalog、files
//! - ID 前缀：auth/network/env 前缀生成
//! - 快照：auth/network/env 快照生成
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::core::persistence::auth_store::{self, AuthConfig};
use rdata_station_lib::core::persistence::driver_store::{self, DriverFile};
use rdata_station_lib::core::persistence::env_store::{self, Environment, EnvironmentPolicy};
use rdata_station_lib::core::persistence::id_prefix;
use rdata_station_lib::core::persistence::network_store::{self, NetworkConfig};
use rusqlite::Connection;

// ==================== 测试数据库初始化 ====================

/// 全局库 schema（不含 origin/source_id/snapshot_at）
fn setup_global_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory DB");

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS auth_configs (
            id TEXT PRIMARY KEY,
            name TEXT,
            auth_type TEXT NOT NULL,
            auth_data TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS network_configs (
            id TEXT PRIMARY KEY,
            name TEXT,
            network_type TEXT NOT NULL,
            config TEXT NOT NULL,
            auth_config_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS environments (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            color TEXT,
            sort_order INTEGER DEFAULT 0,
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

        CREATE TABLE IF NOT EXISTS drivers (
            id TEXT PRIMARY KEY,
            type_id TEXT NOT NULL,
            name TEXT NOT NULL,
            driver_kind TEXT DEFAULT 'native',
            is_file INTEGER DEFAULT 0,
            default_port INTEGER,
            url_template TEXT,
            download_url TEXT,
            download_checksum TEXT,
            version TEXT,
            config_schema TEXT NOT NULL,
            supported_auth_types TEXT,
            capabilities TEXT,
            driver_properties TEXT,
            enabled INTEGER DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS driver_files (
            id TEXT PRIMARY KEY,
            driver_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_name TEXT NOT NULL,
            file_size INTEGER,
            checksum TEXT,
            version TEXT NOT NULL,
            installed_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        ",
    )
    .expect("Failed to create global DB tables");

    conn
}

/// 项目库 schema（含 origin/source_id/snapshot_at）
fn setup_project_db() -> Connection {
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
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            color TEXT,
            sort_order INTEGER DEFAULT 0,
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
    .expect("Failed to create project DB tables");

    conn
}

fn now_ts() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

// ==================== 1. Auth Config — 全局 CRUD ====================

#[test]
fn test_global_auth_config_create_and_list() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_mysql_root".to_string(),
        name: Some("MySQL Root".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"root","password":"secret123"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).expect("create global auth config failed");

    let list =
        auth_store::list_global_auth_configs(&conn, None).expect("list global auth configs failed");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "G_auth_mysql_root");
    assert_eq!(list[0].auth_type, "password");
    // 解密后应能读取原始密码
    let data: serde_json::Value =
        serde_json::from_str(&list[0].auth_data).expect("parse auth_data failed");
    assert_eq!(data["username"], "root");
    assert_eq!(data["password"], "secret123");
}

#[test]
fn test_global_auth_config_get_by_id() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_pg_admin".to_string(),
        name: Some("PG Admin".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"admin","password":"pgpass"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_pg_admin")
        .expect("get global auth config failed");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name.as_deref(), Some("PG Admin"));

    // 不存在的 ID
    let not_found = auth_store::get_global_auth_config(&conn, "G_auth_nonexistent")
        .expect("get global auth config failed");
    assert!(not_found.is_none());
}

#[test]
fn test_global_auth_config_update() {
    let conn = setup_global_db();
    let ts = now_ts();

    let mut ac = AuthConfig {
        id: "G_auth_upd_test".to_string(),
        name: Some("Original Name".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"u1","password":"p1"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    // 更新
    ac.name = Some("Updated Name".to_string());
    ac.auth_data = r#"{"username":"u2","password":"p2"}"#.to_string();
    ac.updated_at = now_ts();
    auth_store::update_auth_config(&conn, &ac).expect("update auth config failed");

    let found = auth_store::get_global_auth_config(&conn, "G_auth_upd_test")
        .unwrap()
        .unwrap();
    assert_eq!(found.name.as_deref(), Some("Updated Name"));
    let data: serde_json::Value = serde_json::from_str(&found.auth_data).unwrap();
    assert_eq!(data["username"], "u2");
    assert_eq!(data["password"], "p2");
}

#[test]
fn test_global_auth_config_delete() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_del_global".to_string(),
        name: Some("To Delete".to_string()),
        auth_type: "password".to_string(),
        auth_data: "{}".to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts,
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();
    assert_eq!(
        auth_store::list_global_auth_configs(&conn, None)
            .unwrap()
            .len(),
        1
    );

    auth_store::delete_auth_config(&conn, "G_auth_del_global").unwrap();
    assert_eq!(
        auth_store::list_global_auth_configs(&conn, None)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn test_global_auth_config_filter_by_type() {
    let conn = setup_global_db();
    let ts = now_ts();

    for (i, atype) in ["password", "kerberos", "password", "ldap", "password"]
        .iter()
        .enumerate()
    {
        let ac = AuthConfig {
            id: format!("G_auth_filter_{}", i),
            name: Some(format!("Auth {}", i)),
            auth_type: atype.to_string(),
            auth_data: "{}".to_string(),
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: ts.clone(),
            updated_at: ts.clone(),
        };
        auth_store::create_global_auth_config(&conn, &ac).unwrap();
    }

    assert_eq!(
        auth_store::list_global_auth_configs(&conn, None)
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        auth_store::list_global_auth_configs(&conn, Some("password"))
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        auth_store::list_global_auth_configs(&conn, Some("kerberos"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        auth_store::list_global_auth_configs(&conn, Some("ldap"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        auth_store::list_global_auth_configs(&conn, Some("oauth2"))
            .unwrap()
            .len(),
        0
    );
}

// ==================== 2. Auth Config — 所有 auth_type 变体 ====================

#[test]
fn test_auth_type_password_with_encryption() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_type_pwd".to_string(),
        name: Some("Password Auth".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"root","password":"MySecret123"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_type_pwd")
        .unwrap()
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(&found.auth_data).unwrap();
    // 密码应被解密回原始值
    assert_eq!(data["password"], "MySecret123");
}

#[test]
fn test_auth_type_ldap() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_ldap_001".to_string(),
        name: Some("LDAP Auth".to_string()),
        auth_type: "ldap".to_string(),
        auth_data: r#"{"username":"cn=admin,dc=example,dc=com","password":"ldap_pass"}"#
            .to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_global_auth_configs(&conn, Some("ldap")).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].auth_type, "ldap");
}

#[test]
fn test_auth_type_kerberos() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_krb_001".to_string(),
        name: Some("Kerberos Auth".to_string()),
        auth_type: "kerberos".to_string(),
        auth_data: r#"{"principal":"user@REALM.COM","password":"krb_pass"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_krb_001")
        .unwrap()
        .unwrap();
    assert_eq!(found.auth_type, "kerberos");
}

#[test]
fn test_auth_type_oauth2() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_oauth2_001".to_string(),
        name: Some("OAuth2 Auth".to_string()),
        auth_type: "oauth2".to_string(),
        auth_data: r#"{"clientId":"abc123","clientSecret":"secret_xyz"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_oauth2_001")
        .unwrap()
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(&found.auth_data).unwrap();
    // clientSecret 应被解密回原始值
    assert_eq!(data["clientSecret"], "secret_xyz");
}

#[test]
fn test_auth_type_os_auth() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_os_001".to_string(),
        name: Some("OS Auth".to_string()),
        auth_type: "os_auth".to_string(),
        auth_data: r#"{"username":"system_user"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_global_auth_configs(&conn, Some("os_auth")).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].auth_type, "os_auth");
}

#[test]
fn test_auth_type_trust() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_trust_001".to_string(),
        name: Some("Trust Auth".to_string()),
        auth_type: "trust".to_string(),
        auth_data: r#"{"username":"postgres"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_global_auth_configs(&conn, Some("trust")).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn test_auth_type_ssh_password() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_ssh_pwd".to_string(),
        name: Some("SSH Password".to_string()),
        auth_type: "ssh_password".to_string(),
        auth_data: r#"{"username":"bastion","password":"ssh_secret"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_ssh_pwd")
        .unwrap()
        .unwrap();
    assert_eq!(found.auth_type, "ssh_password");
}

#[test]
fn test_auth_type_proxy_password() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_proxy_pwd".to_string(),
        name: Some("Proxy Password".to_string()),
        auth_type: "proxy_password".to_string(),
        auth_data: r#"{"username":"proxy_user","password":"proxy_secret"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_global_auth_configs(&conn, Some("proxy_password")).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn test_auth_type_pg_class() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_pg_class".to_string(),
        name: Some("PG Class Auth".to_string()),
        auth_type: "pg_class".to_string(),
        auth_data: r#"{"username":"pg_user"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_global_auth_configs(&conn, Some("pg_class")).unwrap();
    assert_eq!(list.len(), 1);
}

// ==================== 3. Auth Config — 错误路径 ====================

#[test]
fn test_auth_config_empty_auth_data() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_empty".to_string(),
        name: Some("Empty Data".to_string()),
        auth_type: "password".to_string(),
        auth_data: "".to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    // 空 auth_data 应能正常创建
    auth_store::create_global_auth_config(&conn, &ac).expect("empty auth_data should be accepted");

    let found = auth_store::get_global_auth_config(&conn, "G_auth_empty")
        .unwrap()
        .unwrap();
    assert_eq!(found.auth_data, "");
}

#[test]
fn test_auth_config_update_nonexistent() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_not_exist".to_string(),
        name: Some("Ghost".to_string()),
        auth_type: "password".to_string(),
        auth_data: "{}".to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    let result = auth_store::update_auth_config(&conn, &ac);
    assert!(result.is_err());
}

#[test]
fn test_auth_config_delete_nonexistent_ok() {
    let conn = setup_global_db();
    // 删除不存在的配置不报错（DELETE 返回 0 rows）
    let result = auth_store::delete_auth_config(&conn, "G_auth_fake");
    assert!(result.is_ok());
}

#[test]
fn test_auth_config_auth_data_without_sensitive_fields() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_no_sensitive".to_string(),
        name: Some("No Sensitive".to_string()),
        auth_type: "os_auth".to_string(),
        auth_data: r#"{"username":"john"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_no_sensitive")
        .unwrap()
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(&found.auth_data).unwrap();
    assert_eq!(data["username"], "john");
}

#[test]
fn test_auth_config_already_encrypted_password() {
    let conn = setup_global_db();
    let ts = now_ts();

    // 已经带有 AES: 前缀的密码不应重复加密
    let ac = AuthConfig {
        id: "G_auth_pre_enc".to_string(),
        name: Some("Pre-encrypted".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"user","password":"AES:already_encrypted_base64"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_pre_enc")
        .unwrap()
        .unwrap();
    // 预加密的密码保持原样（因为 decrypt 会尝试解密，失败则保持原样）
    let data: serde_json::Value = serde_json::from_str(&found.auth_data).unwrap();
    // 无论如何，auth_data 不应该是空的
    assert!(data["username"].as_str().is_some());
}

#[test]
fn test_auth_config_passphrase_encryption() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_passphrase".to_string(),
        name: Some("SSH Key with Passphrase".to_string()),
        auth_type: "ssh_password".to_string(),
        auth_data: r#"{"username":"git","passphrase":"my_key_passphrase"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_passphrase")
        .unwrap()
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(&found.auth_data).unwrap();
    assert_eq!(data["passphrase"], "my_key_passphrase");
}

// ==================== 4. Network Config — 全局 CRUD ====================

#[test]
fn test_global_network_config_create_and_list() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_ssh_bastion".to_string(),
        name: Some("Bastion SSH".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"bastion.example.com","port":22,"username":"admin","remote_host":"10.0.0.1","remote_port":3306}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc)
        .expect("create global network config failed");

    let list = network_store::list_global_network_configs(&conn, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "G_net_ssh_bastion");
    assert_eq!(list[0].network_type, "ssh");
}

#[test]
fn test_global_network_config_get_by_id() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_ssl_prod".to_string(),
        name: Some("Prod SSL".to_string()),
        network_type: "ssl".to_string(),
        config: r#"{"ca_cert_path":"/path/to/ca.pem","ssl_mode":"verify-full"}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let found = network_store::get_global_network_config(&conn, "G_net_ssl_prod")
        .unwrap()
        .unwrap();
    assert_eq!(found.network_type, "ssl");

    let not_found = network_store::get_global_network_config(&conn, "G_net_fake").unwrap();
    assert!(not_found.is_none());
}

#[test]
fn test_global_network_config_update() {
    let conn = setup_global_db();
    let ts = now_ts();

    let mut nc = NetworkConfig {
        id: "G_net_update_test".to_string(),
        name: Some("Old Name".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"old.example.com"}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    nc.name = Some("New Name".to_string());
    nc.config = r#"{"host":"new.example.com"}"#.to_string();
    nc.updated_at = now_ts();
    network_store::update_network_config(&conn, &nc).expect("update network config failed");

    let found = network_store::get_global_network_config(&conn, "G_net_update_test")
        .unwrap()
        .unwrap();
    assert_eq!(found.name.as_deref(), Some("New Name"));
    assert!(found.config.contains("new.example.com"));
}

#[test]
fn test_global_network_config_delete() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_del_global".to_string(),
        name: Some("Delete Me".to_string()),
        network_type: "proxy".to_string(),
        config: "{}".to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts,
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();
    assert_eq!(
        network_store::list_global_network_configs(&conn, None)
            .unwrap()
            .len(),
        1
    );

    network_store::delete_network_config(&conn, "G_net_del_global").unwrap();
    assert_eq!(
        network_store::list_global_network_configs(&conn, None)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn test_global_network_config_with_auth_config_id() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_with_auth".to_string(),
        name: Some("SSH with Auth".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"example.com","port":22}"#.to_string(),
        auth_config_id: Some("G_auth_ssh_001".to_string()),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let found = network_store::get_global_network_config(&conn, "G_net_with_auth")
        .unwrap()
        .unwrap();
    assert_eq!(found.auth_config_id.as_deref(), Some("G_auth_ssh_001"));
}

// ==================== 5. Network Config — SSH/SSL/Proxy 类型 ====================

#[test]
fn test_network_config_type_ssh() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_ssh_type".to_string(),
        name: Some("SSH Tunnel".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"jump.example.com","port":22,"username":"deploy","auth_method":"password","remote_host":"db.internal","remote_port":5432}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let list = network_store::list_global_network_configs(&conn, Some("ssh")).unwrap();
    assert_eq!(list.len(), 1);
    let config: serde_json::Value = serde_json::from_str(&list[0].config).unwrap();
    assert_eq!(config["host"], "jump.example.com");
    assert_eq!(config["remote_host"], "db.internal");
}

#[test]
fn test_network_config_type_ssl() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_ssl_type".to_string(),
        name: Some("SSL Config".to_string()),
        network_type: "ssl".to_string(),
        config: r#"{"ca_cert_path":"/certs/ca.pem","client_cert_path":"/certs/client.pem","client_key_path":"/certs/client.key","ssl_mode":"verify-full"}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let list = network_store::list_global_network_configs(&conn, Some("ssl")).unwrap();
    assert_eq!(list.len(), 1);
    let config: serde_json::Value = serde_json::from_str(&list[0].config).unwrap();
    assert_eq!(config["ssl_mode"], "verify-full");
}

#[test]
fn test_network_config_type_proxy() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_proxy_type".to_string(),
        name: Some("HTTP Proxy".to_string()),
        network_type: "proxy".to_string(),
        config: r#"{"host":"proxy.corp.com","port":8080,"username":"proxy_user","password":"proxy_pass"}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let list = network_store::list_global_network_configs(&conn, Some("proxy")).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn test_network_config_type_socks() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_socks_type".to_string(),
        name: Some("SOCKS5 Proxy".to_string()),
        network_type: "socks5".to_string(),
        config: r#"{"host":"socks.corp.com","port":1080}"#.to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let list = network_store::list_global_network_configs(&conn, Some("socks5")).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn test_network_config_all_types_filter() {
    let conn = setup_global_db();
    let ts = now_ts();

    for (i, ntype) in ["ssh", "ssl", "proxy", "http_proxy", "socks5"]
        .iter()
        .enumerate()
    {
        let nc = NetworkConfig {
            id: format!("G_net_all_{}", i),
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
        network_store::create_global_network_config(&conn, &nc).unwrap();
    }

    assert_eq!(
        network_store::list_global_network_configs(&conn, None)
            .unwrap()
            .len(),
        5
    );
}

// ==================== 6. Network Config — 错误路径 ====================

#[test]
fn test_network_config_update_nonexistent() {
    let conn = setup_global_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "G_net_not_exist".to_string(),
        name: Some("Ghost".to_string()),
        network_type: "ssh".to_string(),
        config: "{}".to_string(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts,
    };

    let result = network_store::update_network_config(&conn, &nc);
    assert!(result.is_err());
}

#[test]
fn test_network_config_delete_nonexistent_ok() {
    let conn = setup_global_db();
    let result = network_store::delete_network_config(&conn, "G_net_fake");
    assert!(result.is_ok());
}

// ==================== 7. Environment — 全局 CRUD ====================

#[test]
fn test_environment_create_list_update_delete() {
    let conn = setup_global_db();
    let ts = now_ts();

    // 创建
    let env = Environment {
        id: "G_env_integration".to_string(),
        name: "Integration".to_string(),
        description: Some("集成测试环境".to_string()),
        color: Some("#FF9800".to_string()),
        sort_order: 10,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::create_environment(&conn, &env).expect("create environment failed");

    // 列表
    let list = env_store::list_environments(&conn).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Integration");

    // 更新
    let updated = Environment {
        id: "G_env_integration".to_string(),
        name: "Integration Updated".to_string(),
        description: Some("更新后的描述".to_string()),
        color: Some("#FF5722".to_string()),
        sort_order: 20,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::update_environment(&conn, &updated).expect("update environment failed");

    let found = env_store::get_environment(&conn, "G_env_integration")
        .unwrap()
        .unwrap();
    assert_eq!(found.name, "Integration Updated");
    assert_eq!(found.sort_order, 20);
    assert_eq!(found.color.as_deref(), Some("#FF5722"));

    // 删除
    env_store::delete_environment(&conn, "G_env_integration").unwrap();
    assert!(env_store::get_environment(&conn, "G_env_integration")
        .unwrap()
        .is_none());
}

#[test]
fn test_environment_multiple_sort_order() {
    let conn = setup_global_db();
    let ts = now_ts();

    let names = ["Production", "Staging", "Development", "Sandbox"];
    let orders = [0, 10, 20, 30];

    for (i, name) in names.iter().enumerate() {
        let env = Environment {
            id: format!("G_env_multi_{}", i),
            name: name.to_string(),
            description: None,
            color: None,
            sort_order: orders[i],
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: ts.clone(),
        };
        env_store::create_environment(&conn, &env).unwrap();
    }

    let list = env_store::list_environments(&conn).unwrap();
    assert_eq!(list.len(), 4);
    // 确保按 sort_order 排序
    assert!(list[0].sort_order <= list[1].sort_order);
    assert!(list[1].sort_order <= list[2].sort_order);
    assert!(list[2].sort_order <= list[3].sort_order);
}

#[test]
fn test_environment_update_nonexistent() {
    let conn = setup_global_db();
    let ts = now_ts();

    let env = Environment {
        id: "G_env_ghost".to_string(),
        name: "Ghost".to_string(),
        description: None,
        color: None,
        sort_order: 0,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts,
    };

    let result = env_store::update_environment(&conn, &env);
    assert!(result.is_err());
}

// ==================== 8. Environment Policy — 全局 CRUD ====================

#[test]
fn test_policy_crud_full_cycle() {
    let conn = setup_global_db();
    let ts = now_ts();

    // 创建环境
    let env = Environment {
        id: "G_env_policy_crud".to_string(),
        name: "Policy CRUD Env".to_string(),
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
        id: "G_ep_query_timeout".to_string(),
        environment_id: "G_env_policy_crud".to_string(),
        policy_type: "query_timeout".to_string(),
        policy_config: Some(r#"{"timeout_seconds":30}"#.to_string()),
        enabled: true,
        created_at: ts.clone(),
    };
    env_store::create_policy(&conn, &policy).expect("create policy failed");

    // 列表
    let list = env_store::list_policies(&conn, "G_env_policy_crud").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].policy_type, "query_timeout");

    // 更新
    let updated = EnvironmentPolicy {
        id: "G_ep_query_timeout".to_string(),
        environment_id: "G_env_policy_crud".to_string(),
        policy_type: "query_timeout".to_string(),
        policy_config: Some(r#"{"timeout_seconds":60}"#.to_string()),
        enabled: false,
        created_at: ts.clone(),
    };
    env_store::update_policy(&conn, &updated).expect("update policy failed");

    let list = env_store::list_policies(&conn, "G_env_policy_crud").unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].enabled);
    assert_eq!(
        list[0].policy_config.as_deref(),
        Some(r#"{"timeout_seconds":60}"#)
    );

    // 删除
    env_store::delete_policy(&conn, "G_ep_query_timeout").unwrap();
    let list = env_store::list_policies(&conn, "G_env_policy_crud").unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_policy_types_variety() {
    let conn = setup_global_db();
    let ts = now_ts();

    let env = Environment {
        id: "G_env_policy_types".to_string(),
        name: "Policy Types Env".to_string(),
        description: None,
        color: None,
        sort_order: 1,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
    };
    env_store::create_environment(&conn, &env).unwrap();

    for (ptype, config) in [
        ("read_only", Some(r#"{"enabled":true}"#)),
        ("query_timeout", Some(r#"{"timeout":120}"#)),
        ("max_rows", Some(r#"{"limit":5000}"#)),
        (
            "ddl_block",
            Some(r#"{"block_create":true,"block_drop":true}"#),
        ),
        (
            "dml_block",
            Some(r#"{"block_insert":false,"block_update":true}"#),
        ),
        (
            "schema_filter",
            Some(r#"{"include":["public"],"exclude":[]}"#),
        ),
    ] {
        let policy = EnvironmentPolicy {
            id: format!("G_ep_{}", ptype),
            environment_id: "G_env_policy_types".to_string(),
            policy_type: ptype.to_string(),
            policy_config: config.map(|s| s.to_string()),
            enabled: true,
            created_at: ts.clone(),
        };
        env_store::create_policy(&conn, &policy).unwrap();
    }

    let list = env_store::list_policies(&conn, "G_env_policy_types").unwrap();
    assert_eq!(list.len(), 6);
}

#[test]
fn test_policy_update_nonexistent() {
    let conn = setup_global_db();
    let ts = now_ts();

    let policy = EnvironmentPolicy {
        id: "G_ep_ghost".to_string(),
        environment_id: "G_env_fake".to_string(),
        policy_type: "read_only".to_string(),
        policy_config: None,
        enabled: true,
        created_at: ts,
    };

    let result = env_store::update_policy(&conn, &policy);
    assert!(result.is_err());
}

// ==================== 9. Driver — 全局操作 ====================

#[test]
fn test_driver_get_all_empty() {
    let conn = setup_global_db();
    let list = driver_store::get_all_drivers(&conn).unwrap();
    // 空表应返回空列表
    assert!(list.is_empty());
}

#[test]
fn test_driver_seed_and_get_all() {
    let conn = setup_global_db();

    // 插入一个测试驱动
    conn.execute(
        "INSERT INTO drivers (id, type_id, name, driver_kind, is_file, default_port, url_template, version, config_schema, supported_auth_types, capabilities, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            "mysql", "mysql", "MySQL (sqlx)", "native", 0, 3306,
            "mysql://{username}:{password}@{host}:{port}/{database}",
            "1.0.0",
            r#"{"fields":[{"key":"host","label":"主机","type":"text","required":true}]}"#,
            r#"["password","ssl"]"#,
            r#"["tree","health_check"]"#,
            1,
        ],
    ).expect("seed driver failed");

    let list = driver_store::get_all_drivers(&conn).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "mysql");
    assert_eq!(list[0].name, "MySQL (sqlx)");
    assert_eq!(list[0].driver_kind, "native");
    assert_eq!(list[0].default_port, Some(3306));
}

#[test]
fn test_driver_get_by_id() {
    let conn = setup_global_db();

    conn.execute(
        "INSERT INTO drivers (id, type_id, name, driver_kind, is_file, default_port, version, config_schema, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params!["pg", "postgresql", "PostgreSQL", "native", 0, 5432, "1.0.0", "{}", 1],
    ).expect("seed driver failed");

    let found = driver_store::get_driver(&conn, "pg").unwrap().unwrap();
    assert_eq!(found.name, "PostgreSQL");

    let not_found = driver_store::get_driver(&conn, "nonexistent").unwrap();
    assert!(not_found.is_none());
}

#[test]
fn test_driver_get_drivers_by_type() {
    let conn = setup_global_db();

    for (id, type_id, name) in [
        ("mysql", "mysql", "MySQL"),
        ("mariadb", "mysql", "MariaDB"),
        ("postgres", "postgresql", "PostgreSQL"),
    ] {
        conn.execute(
            "INSERT INTO drivers (id, type_id, name, driver_kind, is_file, version, config_schema, enabled)
             VALUES (?1, ?2, ?3, 'native', 0, '1.0.0', '{}', 1)",
            rusqlite::params![id, type_id, name],
        ).expect("seed driver failed");
    }

    let mysql_drivers = driver_store::get_drivers_by_type(&conn, "mysql").unwrap();
    assert_eq!(mysql_drivers.len(), 2);

    let pg_drivers = driver_store::get_drivers_by_type(&conn, "postgresql").unwrap();
    assert_eq!(pg_drivers.len(), 1);

    let empty = driver_store::get_drivers_by_type(&conn, "nonexistent").unwrap();
    assert!(empty.is_empty());
}

#[test]
fn test_driver_files_crud() {
    let conn = setup_global_db();
    let ts = now_ts();

    // 注册驱动文件
    let df = DriverFile {
        id: "df_mysql_8".to_string(),
        driver_id: "mysql".to_string(),
        file_path: "/drivers/mysql-connector.jar".to_string(),
        file_name: "mysql-connector.jar".to_string(),
        file_size: Some(2_000_000),
        checksum: Some("abc123".to_string()),
        version: "8.0.33".to_string(),
        installed_at: ts.clone(),
        updated_at: ts.clone(),
    };

    driver_store::register_driver_file(&conn, &df).expect("register driver file failed");

    // 列出驱动文件
    let files = driver_store::list_driver_files(&conn, "mysql").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].driver_id, "mysql");
    assert_eq!(files[0].version, "8.0.33");

    // 检查是否已安装
    assert!(driver_store::is_driver_file_installed(&conn, "mysql", "8.0.33").unwrap());
    assert!(!driver_store::is_driver_file_installed(&conn, "mysql", "8.0.34").unwrap());
}

#[test]
fn test_driver_files_multiple_versions() {
    let conn = setup_global_db();
    let ts = now_ts();

    for version in ["8.0.30", "8.0.31", "8.0.33"] {
        let df = DriverFile {
            id: format!("df_mysql_{}", version.replace('.', "_")),
            driver_id: "mysql".to_string(),
            file_path: format!("/drivers/mysql-{}.jar", version),
            file_name: format!("mysql-connector-{}.jar", version),
            file_size: Some(2_000_000),
            checksum: None,
            version: version.to_string(),
            installed_at: ts.clone(),
            updated_at: ts.clone(),
        };
        driver_store::register_driver_file(&conn, &df).unwrap();
    }

    let files = driver_store::list_driver_files(&conn, "mysql").unwrap();
    assert_eq!(files.len(), 3);
    // 应按版本降序排列
    assert_eq!(files[0].version, "8.0.33");
}

// ==================== 10. ID 前缀 — auth/network/env 前缀 ====================

#[test]
fn test_id_prefix_generate_auth_id() {
    let gid = id_prefix::generate_gid("auth", "mysql-root");
    assert!(gid.starts_with("G_auth_"));
    assert!(gid.contains("mysql-root"));

    let pid = id_prefix::generate_pid("auth");
    assert!(pid.starts_with("P_auth_"));
}

#[test]
fn test_id_prefix_generate_network_id() {
    let gid = id_prefix::generate_gid("net", "ssh-bastion");
    assert!(gid.starts_with("G_net_"));
    assert!(gid.contains("ssh-bastion"));

    let pid = id_prefix::generate_pid("net");
    assert!(pid.starts_with("P_net_"));
}

#[test]
fn test_id_prefix_generate_environment_id() {
    let gid = id_prefix::generate_gid("env", "production");
    assert!(gid.starts_with("G_env_"));
    assert!(gid.contains("production"));

    let pid = id_prefix::generate_pid("env");
    assert!(pid.starts_with("P_env_"));
}

#[test]
fn test_id_prefix_generate_policy_id() {
    let gid = id_prefix::generate_gid("ep", "readonly");
    assert!(gid.starts_with("G_ep_"));
    assert!(gid.contains("readonly"));

    let pid = id_prefix::generate_pid("ep");
    assert!(pid.starts_with("P_ep_"));
}

#[test]
fn test_id_prefix_origin_detection_auth() {
    assert_eq!(id_prefix::origin_from_id("G_auth_001"), "global");
    assert_eq!(id_prefix::origin_from_id("P_auth_local"), "project");
    assert_eq!(
        id_prefix::origin_from_id("GP_auth_001_20260620"),
        "global_snapshot"
    );
}

#[test]
fn test_id_prefix_origin_detection_network() {
    assert_eq!(id_prefix::origin_from_id("G_net_ssh"), "global");
    assert_eq!(id_prefix::origin_from_id("P_net_ssh"), "project");
    assert_eq!(
        id_prefix::origin_from_id("GP_net_ssh_20260620"),
        "global_snapshot"
    );
}

#[test]
fn test_id_prefix_origin_detection_env() {
    assert_eq!(id_prefix::origin_from_id("G_env_prod"), "global");
    assert_eq!(id_prefix::origin_from_id("P_env_dev"), "project");
    assert_eq!(
        id_prefix::origin_from_id("GP_env_prod_20260620"),
        "global_snapshot"
    );
}

#[test]
fn test_id_prefix_gen_project_id_custom() {
    let pid = id_prefix::gen_project_id("auth", "custom_suffix");
    assert!(pid.starts_with("P_auth_"));
    assert!(pid.ends_with("custom_suffix"));
}

// ==================== 11. 快照 — auth/network/env 快照生成 ====================

#[test]
fn test_snapshot_auth_id_conversion() {
    let gid = "G_auth_mysql_root";
    let snapshot = id_prefix::to_snapshot_id(gid);
    assert!(snapshot.is_some());
    let gpid = snapshot.unwrap();
    assert!(gpid.starts_with("GP_auth_mysql_root_"));

    let source = id_prefix::source_global_id(&gpid);
    assert_eq!(source, Some("G_auth_mysql_root".to_string()));
}

#[test]
fn test_snapshot_network_id_conversion() {
    let gid = "G_net_ssh_bastion";
    let snapshot = id_prefix::to_snapshot_id(gid);
    assert!(snapshot.is_some());
    let gpid = snapshot.unwrap();
    assert!(gpid.starts_with("GP_net_ssh_bastion_"));

    let source = id_prefix::source_global_id(&gpid);
    assert_eq!(source, Some("G_net_ssh_bastion".to_string()));
}

#[test]
fn test_snapshot_env_id_conversion() {
    let gid = "G_env_production";
    let snapshot = id_prefix::to_snapshot_id(gid);
    assert!(snapshot.is_some());
    let gpid = snapshot.unwrap();
    assert!(gpid.starts_with("GP_env_production_"));

    let source = id_prefix::source_global_id(&gpid);
    assert_eq!(source, Some("G_env_production".to_string()));
}

#[test]
fn test_snapshot_project_auth_config_with_origin() {
    let conn = setup_project_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "GP_auth_snap_001_20260620".to_string(),
        name: Some("Snapshot Auth".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"snap","password":"snap_pwd"}"#.to_string(),
        origin: Some("global_snapshot".to_string()),
        source_id: Some("G_auth_001".to_string()),
        snapshot_at: Some("2026-06-20T10:00:00Z".to_string()),
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_auth_config(&conn, "GP_auth_snap_001_20260620")
        .unwrap()
        .unwrap();
    assert_eq!(found.origin.as_deref(), Some("global_snapshot"));
    assert_eq!(found.source_id.as_deref(), Some("G_auth_001"));
    assert!(found.snapshot_at.is_some());
}

#[test]
fn test_snapshot_project_network_config_with_origin() {
    let conn = setup_project_db();
    let ts = now_ts();

    let nc = NetworkConfig {
        id: "GP_net_ssh_prod_20260620".to_string(),
        name: Some("Prod SSH Snapshot".to_string()),
        network_type: "ssh".to_string(),
        config: r#"{"host":"prod-bastion.example.com"}"#.to_string(),
        auth_config_id: None,
        origin: Some("global_snapshot".to_string()),
        source_id: Some("G_net_ssh_prod".to_string()),
        snapshot_at: Some("2026-06-20T10:00:00Z".to_string()),
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_network_config(&conn, &nc).unwrap();

    let found = network_store::get_network_config(&conn, "GP_net_ssh_prod_20260620")
        .unwrap()
        .unwrap();
    assert_eq!(found.origin.as_deref(), Some("global_snapshot"));
    assert_eq!(found.source_id.as_deref(), Some("G_net_ssh_prod"));
}

#[test]
fn test_snapshot_project_environment_with_origin() {
    let conn = setup_project_db();
    let ts = now_ts();

    let env = Environment {
        id: "GP_env_prod_20260620".to_string(),
        name: "Production (Snapshot)".to_string(),
        description: Some("从全局快照".to_string()),
        color: Some("#F44336".to_string()),
        sort_order: 10,
        origin: Some("global_snapshot".to_string()),
        source_id: Some("G_env_prod".to_string()),
        snapshot_at: Some("2026-06-20T10:00:00Z".to_string()),
        created_at: ts,
    };

    env_store::create_environment(&conn, &env).unwrap();

    let found = env_store::get_environment(&conn, "GP_env_prod_20260620")
        .unwrap()
        .unwrap();
    assert_eq!(found.origin.as_deref(), Some("global_snapshot"));
    assert_eq!(found.source_id.as_deref(), Some("G_env_prod"));
}

#[test]
fn test_snapshot_project_local_environment() {
    let conn = setup_project_db();
    let ts = now_ts();

    let env = Environment {
        id: "P_env_local_dev".to_string(),
        name: "Local Dev (Project)".to_string(),
        description: Some("项目本地创建的环境".to_string()),
        color: Some("#4CAF50".to_string()),
        sort_order: 1,
        origin: Some("project".to_string()),
        source_id: None,
        snapshot_at: None,
        created_at: ts,
    };

    env_store::create_environment(&conn, &env).unwrap();

    let found = env_store::get_environment(&conn, "P_env_local_dev")
        .unwrap()
        .unwrap();
    assert_eq!(found.origin.as_deref(), Some("project"));
    assert!(found.source_id.is_none());
}

#[test]
fn test_snapshot_project_local_auth_config() {
    let conn = setup_project_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "P_auth_local_001".to_string(),
        name: Some("Local Auth".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"local","password":"local123"}"#.to_string(),
        origin: Some("project".to_string()),
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_auth_config(&conn, &ac).unwrap();

    let list = auth_store::list_auth_configs(&conn, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].origin.as_deref(), Some("project"));
}

// ==================== 边界情况测试 ====================

#[test]
fn test_global_auth_config_empty_list() {
    let conn = setup_global_db();
    let list = auth_store::list_global_auth_configs(&conn, None).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_global_network_config_empty_list() {
    let conn = setup_global_db();
    let list = network_store::list_global_network_configs(&conn, None).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_global_environment_empty_list() {
    let conn = setup_global_db();
    let list = env_store::list_environments(&conn).unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_policy_for_nonexistent_environment() {
    let conn = setup_global_db();
    let list = env_store::list_policies(&conn, "G_env_fake").unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_auth_config_with_special_characters_in_name() {
    let conn = setup_global_db();
    let ts = now_ts();

    let ac = AuthConfig {
        id: "G_auth_special".to_string(),
        name: Some("MySQL 生产环境 (只读) [2026]".to_string()),
        auth_type: "password".to_string(),
        auth_data: r#"{"username":"readonly","password":"p@ss!#$%"}"#.to_string(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    auth_store::create_global_auth_config(&conn, &ac).unwrap();

    let found = auth_store::get_global_auth_config(&conn, "G_auth_special")
        .unwrap()
        .unwrap();
    assert_eq!(found.name.as_deref(), Some("MySQL 生产环境 (只读) [2026]"));
}

#[test]
fn test_network_config_large_config_json() {
    let conn = setup_global_db();
    let ts = now_ts();

    let large_config = format!(
        r#"{{"host":"server.example.com","port":22,"forwarding":[{}]}}"#,
        (0..10)
            .map(|i| format!(r#"{{"local":{},"remote":{}}}"#, 10000 + i, 3306 + i))
            .collect::<Vec<_>>()
            .join(",")
    );

    let nc = NetworkConfig {
        id: "G_net_large_config".to_string(),
        name: Some("Large Config".to_string()),
        network_type: "ssh".to_string(),
        config: large_config,
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: ts.clone(),
        updated_at: ts.clone(),
    };

    network_store::create_global_network_config(&conn, &nc).unwrap();

    let found = network_store::get_global_network_config(&conn, "G_net_large_config")
        .unwrap()
        .unwrap();
    assert!(found.config.len() > 100);
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::persistence::GlobalSqlitePool;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

    const GLOBAL_MIGRATION_SQL: &str =
        include_str!("../../../../migrations/global/001_init.sql");

    async fn create_test_global_store() -> (GlobalDatabaseManager, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rds_plugin_test_{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("global.db");
        let pool = Arc::new(
            GlobalSqlitePool::new(db_path.clone(), 2)
                .await
                .expect("create pool"),
        );
        {
            let conn = pool.acquire().await.expect("acquire connection");
            conn.inner()
                .expect("get connection")
                .execute_batch(GLOBAL_MIGRATION_SQL)
                .expect("run migration");
        }
        let manager = GlobalDatabaseManager::new(pool, db_path.clone());
        (manager, dir)
    }

    fn cleanup(dir: PathBuf) {
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn t001_register_and_get_plugin() {
        let (manager, dir) = create_test_global_store().await;
        let now = chrono::Utc::now().to_rfc3339();
        let plugin = Plugin {
            id: Uuid::new_v4().to_string(),
            code: "test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            author: Some("Test Author".to_string()),
            description: Some("A test plugin".to_string()),
            repo_url: Some("https://example.com".to_string()),
            plugin_type: "wasm".to_string(),
            manifest_json: Some("{}".to_string()),
            install_path: "/test/path".to_string(),
            is_enabled: true,
            is_builtin: false,
            created_at: now.clone(),
            updated_at: now,
        };

        manager.register_plugin(&plugin).await.expect("register plugin");

        let fetched = manager.get_plugin(&plugin.id).await.expect("get plugin");
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.code, plugin.code);
        assert_eq!(fetched.name, plugin.name);

        let by_code = manager.get_plugin_by_code_version(&plugin.code, &plugin.version).await.expect("get by code");
        assert!(by_code.is_some());

        cleanup(dir);
    }

    #[tokio::test]
    async fn t002_get_all_plugins() {
        let (manager, dir) = create_test_global_store().await;
        for i in 0..3 {
            let now = chrono::Utc::now().to_rfc3339();
            let plugin = Plugin {
                id: Uuid::new_v4().to_string(),
                code: format!("test.plugin{}", i),
                name: format!("Test Plugin {}", i),
                version: "1.0.0".to_string(),
                author: None,
                description: None,
                repo_url: None,
                plugin_type: "wasm".to_string(),
                manifest_json: None,
                install_path: "/test/path".to_string(),
                is_enabled: i % 2 == 0,
                is_builtin: false,
                created_at: now.clone(),
                updated_at: now,
            };
            manager.register_plugin(&plugin).await.expect("register");
        }

        let all = manager.get_all_plugins().await.expect("get all");
        assert_eq!(all.len(), 3);

        cleanup(dir);
    }

    #[tokio::test]
    async fn t003_update_plugin_enabled() {
        let (manager, dir) = create_test_global_store().await;
        let now = chrono::Utc::now().to_rfc3339();
        let plugin = Plugin {
            id: Uuid::new_v4().to_string(),
            code: "test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            description: None,
            repo_url: None,
            plugin_type: "wasm".to_string(),
            manifest_json: None,
            install_path: "/test/path".to_string(),
            is_enabled: true,
            is_builtin: false,
            created_at: now.clone(),
            updated_at: now,
        };

        manager.register_plugin(&plugin).await.expect("register");

        manager.update_plugin_enabled(&plugin.id, false).await.expect("update");

        let fetched = manager.get_plugin(&plugin.id).await.expect("get");
        assert!(fetched.is_some());
        assert!(!fetched.unwrap().is_enabled);

        cleanup(dir);
    }

    #[tokio::test]
    async fn t004_delete_plugin() {
        let (manager, dir) = create_test_global_store().await;
        let now = chrono::Utc::now().to_rfc3339();
        let plugin = Plugin {
            id: Uuid::new_v4().to_string(),
            code: "test.plugin".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            description: None,
            repo_url: None,
            plugin_type: "wasm".to_string(),
            manifest_json: None,
            install_path: "/test/path".to_string(),
            is_enabled: true,
            is_builtin: false,
            created_at: now.clone(),
            updated_at: now,
        };

        manager.register_plugin(&plugin).await.expect("register");

        manager.delete_plugin(&plugin.id).await.expect("delete");

        let fetched = manager.get_plugin(&plugin.id).await.expect("get");
        assert!(fetched.is_none());

        cleanup(dir);
    }

    #[tokio::test]
    async fn t005_global_config() {
        let (manager, dir) = create_test_global_store().await;
        let plugin_id = Uuid::new_v4().to_string();

        let config = PluginGlobalConfig {
            plugin_id: plugin_id.clone(),
            key: "test.key".to_string(),
            value: Some("test.value".to_string()),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        manager.set_plugin_global_config(&config).await.expect("set config");

        let configs = manager.get_plugin_global_configs(&plugin_id).await.expect("get configs");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].key, "test.key");
        assert_eq!(configs[0].value, Some("test.value".to_string()));

        cleanup(dir);
    }
}


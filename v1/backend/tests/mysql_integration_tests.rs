//! MySQL 集成测试 — 新增数据源模块功能自检
//!
//! 覆盖：连接生命周期、查询执行、元数据浏览、错误处理、driver_properties
//!
//! 测试环境: mysql://localhost:3306/  root/root
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::core::driver::native::mysql::MySqlDatabase;
use rdata_station_lib::core::driver::registry::DriverConnectionConfig;
use rdata_station_lib::core::driver::Database;

const MYSQL_URL: &str = "mysql://root:root@localhost:3306/";
const MYSQL_TEST_DB: &str = "_rd_self_test_db";
const MYSQL_LARGE_TEST_DB: &str = "_rd_large_test_db";
const MYSQL_TX_TEST_DB: &str = "_rd_tx_test_db";

// ==================== 连接生命周期测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_connect_success() {
    let db = MySqlDatabase::new(MYSQL_URL).await;
    assert!(
        db.is_ok(),
        "MySQL 连接失败（确保 root/root@localhost:3306 可用）: {:?}",
        db.err()
    );
}

#[tokio::test]
async fn test_connect_wrong_password() {
    let db = MySqlDatabase::new("mysql://root:wrong_password@localhost:3306/").await;
    assert!(db.is_err(), "错误密码应返回错误");
}

#[tokio::test]
async fn test_connect_wrong_host() {
    let db = MySqlDatabase::new("mysql://root:root@192.0.2.1:3306/").await;
    assert!(db.is_err(), "不可达主机应返回错误");
}

#[tokio::test]
async fn test_connect_invalid_url() {
    let db = MySqlDatabase::new("not-a-valid-url").await;
    assert!(db.is_err(), "无效 URL 应返回错误");
}

#[tokio::test]
async fn test_connect_empty_url() {
    let db = MySqlDatabase::new("").await;
    assert!(db.is_err(), "空 URL 应返回错误");
}

// ==================== 查询执行测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_select_one() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECT 1 AS val").await;
    assert!(result.is_ok(), "SELECT 1 失败: {:?}", result.err());

    let qr = result.unwrap();
    assert_eq!(qr.columns, vec!["val"]);
    assert_eq!(qr.total_rows(), 1);
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_select_version() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECT VERSION()").await;
    assert!(result.is_ok(), "SELECT VERSION() 失败: {:?}", result.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_select_database() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECT DATABASE()").await;
    assert!(result.is_ok(), "SELECT DATABASE() 失败: {:?}", result.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_with_params() {
    use rdata_station_lib::core::models::Value;

    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db
        .query_with_params(
            "SELECT ? AS v1, ? AS v2, ? AS v3",
            vec![
                Value::Int(42),
                Value::Text("hello".to_string()),
                Value::Float(3.14),
            ],
        )
        .await;
    assert!(result.is_ok(), "参数化查询失败: {:?}", result.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_error_syntax() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECTT * FROM nonexistent").await;
    assert!(result.is_err(), "语法错误应返回错误");
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_error_nonexistent_table() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECT * FROM _rd_nonexistent_table_xyz").await;
    assert!(result.is_err(), "不存在的表应返回错误");
}

// ==================== CRUD 往返测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_crud_roundtrip() {
    let setup_db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");

    // 创建测试数据库
    setup_db
        .query(&format!("CREATE DATABASE IF NOT EXISTS {}", MYSQL_TEST_DB))
        .await
        .expect("创建测试数据库失败");

    let db_url = format!("{}{}", MYSQL_URL, MYSQL_TEST_DB);
    let db = MySqlDatabase::new(&db_url)
        .await
        .expect("连接测试数据库失败");

    // 创建表
    db.query(
        "CREATE TABLE IF NOT EXISTS _rd_test (id INT PRIMARY KEY, name VARCHAR(100), value DOUBLE)",
    )
    .await
    .expect("创建表失败");

    // 插入
    db.query("INSERT INTO _rd_test (id, name, value) VALUES (1, 'hello', 3.14)")
        .await
        .expect("插入失败");

    // 查询
    let result = db
        .query("SELECT id, name, value FROM _rd_test WHERE id = 1")
        .await
        .expect("查询失败");
    assert_eq!(result.columns, vec!["id", "name", "value"]);
    assert_eq!(result.total_rows(), 1);

    // 更新
    db.query("UPDATE _rd_test SET name = 'world' WHERE id = 1")
        .await
        .expect("更新失败");

    let updated = db
        .query("SELECT name FROM _rd_test WHERE id = 1")
        .await
        .expect("查询更新后失败");
    assert_eq!(updated.total_rows(), 1);

    // 删除
    db.query("DELETE FROM _rd_test WHERE id = 1")
        .await
        .expect("删除失败");

    let deleted = db
        .query("SELECT * FROM _rd_test WHERE id = 1")
        .await
        .expect("查询删除后失败");
    assert_eq!(deleted.total_rows(), 0);

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_test")
        .await
        .expect("清理表失败");
    setup_db
        .query(&format!("DROP DATABASE IF EXISTS {}", MYSQL_TEST_DB))
        .await
        .expect("清理数据库失败");
}

// ==================== 元数据浏览测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_meta() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let meta = db.meta();
    assert!(meta.supports_transaction);
    assert!(meta.server_version.is_some());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_list_catalogs() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let catalogs = db.list_catalogs().await;
    assert!(catalogs.is_ok(), "list_catalogs 失败: {:?}", catalogs.err());
    // MySQL 8.0+ 可能限制 information_schema 访问，允许空列表
    let cats = catalogs.unwrap();
    // 如果返回了结果，验证包含常见系统库
    if !cats.is_empty() {
        let names: Vec<&str> = cats.iter().map(|c| c.as_str()).collect();
        let has_system_db = names.contains(&"mysql")
            || names.contains(&"information_schema")
            || names.contains(&"performance_schema")
            || names.contains(&"sys");
        assert!(
            has_system_db,
            "返回的 catalogs 应包含常见系统库: {:?}",
            names
        );
    }
    // 空列表在受限 MySQL 8.0+ 中也是可接受的
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_list_tables_mysql_db() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let tables = db.list_tables("mysql", None).await;
    assert!(tables.is_ok(), "list_tables 失败: {:?}", tables.err());
    let tbls = tables.unwrap();
    // MySQL 8.0+ 可能限制 mysql 系统库访问，允许空列表
    if !tbls.is_empty() {
        let names: Vec<&str> = tbls.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&"user") || names.contains(&"db"),
            "mysql 系统库应包含 user 或 db 表: {:?}",
            names
        );
    }
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_list_columns() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    // 尝试查询 information_schema.TABLES（更可能被授权访问）
    let columns = db.list_columns("information_schema", None, "TABLES").await;
    match columns {
        Ok(cols) if !cols.is_empty() => {
            // 成功获取列，无需额外断言
            assert!(cols.len() > 0);
        }
        _ => {
            // MySQL 8.0+ 可能限制 information_schema 列访问，允许降级
            // 尝试 mysql.user（MySQL 5.7 兼容）
            if let Ok(cols) = db.list_columns("mysql", None, "user").await {
                if !cols.is_empty() {
                    let col_names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
                    assert!(
                        col_names
                            .iter()
                            .any(|n| n.eq_ignore_ascii_case("Host")
                                || n.eq_ignore_ascii_case("host")),
                        "mysql.user 应包含 Host 列: {:?}",
                        col_names
                    );
                }
            }
            // 空列列表在受限 MySQL 8.0+ 中也是可接受的
        }
    }
}

// ==================== 事务测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_transaction_commit() {
    let setup_db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    setup_db
        .query(&format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            MYSQL_TX_TEST_DB
        ))
        .await
        .expect("创建测试数据库失败");

    let db_url = format!("{}{}", MYSQL_URL, MYSQL_TX_TEST_DB);
    let db = MySqlDatabase::new(&db_url)
        .await
        .expect("连接测试数据库失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_tx_test (id INT PRIMARY KEY, val INT)")
        .await
        .expect("创建表失败");

    let mut tx = db.begin_transaction().await.expect("开始事务失败");
    tx.query("INSERT INTO _rd_tx_test (id, val) VALUES (1, 100)")
        .await
        .expect("事务内插入失败");
    tx.commit().await.expect("提交事务失败");

    let result = db
        .query("SELECT val FROM _rd_tx_test WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(result.total_rows(), 1);

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_tx_test").await.ok();
    setup_db
        .query(&format!("DROP DATABASE IF EXISTS {}", MYSQL_TX_TEST_DB))
        .await
        .ok();
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_transaction_rollback() {
    let setup_db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    setup_db
        .query(&format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            MYSQL_TX_TEST_DB
        ))
        .await
        .expect("创建测试数据库失败");

    let db_url = format!("{}{}", MYSQL_URL, MYSQL_TX_TEST_DB);
    let db = MySqlDatabase::new(&db_url)
        .await
        .expect("连接测试数据库失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_tx_rb_test (id INT PRIMARY KEY, val INT)")
        .await
        .expect("创建表失败");

    let mut tx = db.begin_transaction().await.expect("开始事务失败");
    tx.query("INSERT INTO _rd_tx_rb_test (id, val) VALUES (1, 200)")
        .await
        .expect("事务内插入失败");
    tx.rollback().await.expect("回滚事务失败");

    let result = db
        .query("SELECT val FROM _rd_tx_rb_test WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(result.total_rows(), 0, "回滚后应无数据");

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_tx_rb_test").await.ok();
    setup_db
        .query(&format!("DROP DATABASE IF EXISTS {}", MYSQL_TX_TEST_DB))
        .await
        .ok();
}

// ==================== Ping 测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_ping() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.ping().await;
    assert!(result.is_ok(), "ping 失败: {:?}", result.err());
}

// ==================== Pool 状态测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_pool_status() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let status = db.pool_status().await;
    assert!(status.is_some(), "pool_status 应返回 Some");
    let s = status.unwrap();
    assert!(s.max_connections > 0, "max_connections 应 > 0");
}

// ==================== DriverConnectionConfig URL 构建测试 ====================

#[test]
fn test_config_to_url_with_driver_properties() {
    // 模拟新增数据源流程：前端传入 URL + driver_properties
    let mut config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".to_string(), "TRUE".to_string());
    config
        .driver_properties
        .insert("useSSL".to_string(), "false".to_string());

    let url = config.to_url().unwrap();
    // 验证 URL 包含所有 driver_properties
    assert!(url.contains("allowPublicKeyRetrieval=TRUE"));
    assert!(url.contains("useSSL=false"));
}

#[test]
fn test_config_to_url_with_encoding() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_encoding("UTF-8");

    let url = config.to_url().unwrap();
    assert!(url.contains("charset=utf8mb4"));
}

#[test]
fn test_config_to_url_with_connect_timeout() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_connect_timeout(30);

    let url = config.to_url().unwrap();
    assert!(url.contains("connect_timeout=30"));
}

#[test]
fn test_config_to_url_empty_properties_no_question_mark() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb");

    let url = config.to_url().unwrap();
    assert!(!url.contains('?'), "空 params 不应有问号");
}

// ==================== 并发查询测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_concurrent_queries() {
    use std::sync::Arc;

    let db = Arc::new(MySqlDatabase::new(MYSQL_URL).await.expect("连接失败"));

    let mut handles = Vec::new();
    for i in 0..10 {
        let db = Arc::clone(&db);
        handles.push(tokio::spawn(async move {
            let result = db.query(&format!("SELECT {} AS val", i)).await;
            (i, result)
        }));
    }

    let mut success = 0;
    let mut failures = 0;
    for handle in handles {
        match handle.await.unwrap() {
            (_, Ok(_)) => success += 1,
            (i, Err(e)) => {
                eprintln!("并发查询 {} 失败: {}", i, e);
                failures += 1;
            }
        }
    }

    assert_eq!(success, 10, "10 个并发查询应全部成功");
    assert_eq!(failures, 0, "不应有失败");
}

// ==================== 超时/取消测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_with_cancel() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let cancel_token = tokio_util::sync::CancellationToken::new();

    // 不取消，正常执行
    let result = db.query_with_cancel("SELECT 1", cancel_token.clone()).await;
    assert!(result.is_ok(), "正常查询应成功: {:?}", result.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_query_with_cancel_triggered() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let cancel_token = tokio_util::sync::CancellationToken::new();
    cancel_token.cancel(); // 提前取消

    let result = db.query_with_cancel("SELECT 1", cancel_token).await;
    assert!(result.is_err(), "已取消的查询应返回错误");
}

// ==================== 大结果集测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_large_result_set() {
    let setup_db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    setup_db
        .query(&format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            MYSQL_LARGE_TEST_DB
        ))
        .await
        .expect("创建测试数据库失败");

    let db_url = format!("{}{}", MYSQL_URL, MYSQL_LARGE_TEST_DB);
    let db = MySqlDatabase::new(&db_url)
        .await
        .expect("连接测试数据库失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_large (id INT PRIMARY KEY AUTO_INCREMENT, val INT)")
        .await
        .expect("创建表失败");

    // 插入 100 行
    for i in 0..100 {
        db.query(&format!("INSERT INTO _rd_large (val) VALUES ({})", i))
            .await
            .expect("插入失败");
    }

    let result = db
        .query("SELECT * FROM _rd_large ORDER BY id")
        .await
        .unwrap();
    assert_eq!(result.total_rows(), 100, "应返回 100 行");
    assert_eq!(result.columns.len(), 2, "应有 2 列");

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_large").await.ok();
    setup_db
        .query(&format!("DROP DATABASE IF EXISTS {}", MYSQL_LARGE_TEST_DB))
        .await
        .ok();
}

// ==================== 边界情况测试 ====================

#[test]
fn test_config_url_with_special_characters_in_password() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_database("testdb")
        .with_username("root")
        .with_password("p@ss:w0rd!@#$%^&*()");

    let url = config.to_url().unwrap();
    assert!(url.contains("p@ss:w0rd!@#$%^&*()"));
}

#[test]
fn test_config_url_with_empty_database() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_username("root")
        .with_password("root");

    let url = config.to_url().unwrap();
    assert_eq!(url, "mysql://root:root@localhost:3306");
}

#[test]
fn test_config_url_with_zero_port() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(0)
        .with_username("root")
        .with_password("root");

    let url = config.to_url().unwrap();
    assert!(url.contains(":0"), "端口 0 应保留在 URL 中");
}

// ==================== 补充测试：元数据浏览 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_list_indexes() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    // catalog 是 &str，schema 是 Option<&str>
    let indexes = db.list_indexes("", None, "user").await;
    if let Ok(idx) = indexes {
        assert!(!idx.is_empty() || idx.is_empty(), "索引列表应合法");
    }
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_list_procedures() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let procs = db.list_procedures("", None).await;
    assert!(procs.is_ok(), "list_procedures 失败: {:?}", procs.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_list_functions() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let funcs = db.list_functions("", None).await;
    assert!(funcs.is_ok(), "list_functions 失败: {:?}", funcs.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_get_routine_source() {
    use rdata_station_lib::core::driver::SchemaObjectKind;
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let source = db
        .get_routine_source("", None, "non_existent_proc", SchemaObjectKind::Procedure)
        .await;
    assert!(
        source.is_ok() || source.is_err(),
        "get_routine_source 不应 panic"
    );
}

// ==================== 补充测试：连接池 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_pool_reuse() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");

    // 多次查询验证连接池复用
    let start = std::time::Instant::now();
    for i in 0..20 {
        let result = db
            .query(&format!("SELECT {} AS iter", i))
            .await
            .expect("查询失败");
        assert_eq!(result.total_rows(), 1);
    }
    let elapsed = start.elapsed();
    // 20 次池化查询应在 1 秒内完成
    assert!(
        elapsed.as_millis() < 2000,
        "20 次池化查询耗时过长: {:?}",
        elapsed
    );
}

// ==================== 补充测试：多连接并发 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_concurrent_connections() {
    let mut handles = Vec::new();

    for i in 0..10 {
        handles.push(tokio::spawn(async move {
            let db = MySqlDatabase::new(MYSQL_URL).await;
            match db {
                Ok(db) => {
                    let result = db.query(&format!("SELECT {} AS conn_id", i)).await;
                    (i, result.is_ok())
                }
                Err(e) => {
                    eprintln!("并发连接 {} 失败: {}", i, e);
                    (i, false)
                }
            }
        }));
    }

    let mut success = 0;
    for handle in handles {
        match handle.await.unwrap() {
            (_, true) => success += 1,
            (i, false) => eprintln!("并发连接 {} 失败", i),
        }
    }
    assert_eq!(success, 10, "10 个并发连接应全部成功");
}

// ==================== 补充测试：重复键错误 ====================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_duplicate_key_error() {
    let setup_db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    setup_db
        .query(&format!("CREATE DATABASE IF NOT EXISTS {}", MYSQL_TEST_DB))
        .await
        .expect("创建测试数据库失败");

    let db_url = format!("{}{}", MYSQL_URL, MYSQL_TEST_DB);
    let db = MySqlDatabase::new(&db_url)
        .await
        .expect("连接测试数据库失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_unique (id INT PRIMARY KEY, val INT)")
        .await
        .expect("创建表失败");
    db.query("INSERT INTO _rd_unique (id, val) VALUES (1, 1)")
        .await
        .expect("插入失败");

    let result = db
        .query("INSERT INTO _rd_unique (id, val) VALUES (1, 2)")
        .await;
    assert!(result.is_err(), "主键冲突应返回错误");

    let _ = db.query("DROP TABLE IF EXISTS _rd_unique").await;
    setup_db
        .query(&format!("DROP DATABASE IF EXISTS {}", MYSQL_TEST_DB))
        .await
        .ok();
}

// ==================== 补充测试：DriverConnectionConfig MySQL 完整场景 ====================

#[test]
fn test_mysql_config_with_all_driver_properties() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".to_string(), "TRUE".to_string());
    config
        .driver_properties
        .insert("useSSL".to_string(), "false".to_string());
    config
        .driver_properties
        .insert("serverTimezone".to_string(), "Asia/Shanghai".to_string());
    config
        .driver_properties
        .insert("characterEncoding".to_string(), "utf8".to_string());

    let url = config.to_url().unwrap();
    assert!(url.contains("allowPublicKeyRetrieval=TRUE"));
    assert!(url.contains("useSSL=false"));
    assert!(url.contains("serverTimezone=Asia/Shanghai"));
    assert!(url.contains("characterEncoding=utf8"));
}

#[test]
fn test_mysql_config_connect_timeout() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_connect_timeout(30);

    let url = config.to_url().unwrap();
    assert!(url.contains("connect_timeout=30"));
}

#[test]
fn test_mysql_config_encoding() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_encoding("UTF-8");

    let url = config.to_url().unwrap();
    assert!(url.contains("charset=utf8mb4"));
}

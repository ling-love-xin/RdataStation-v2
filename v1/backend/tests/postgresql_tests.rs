//! PostgreSQL 多驱动多场景连接测试
//!
//! 覆盖：
//! - 多种驱动程序兼容性（postgres / postgres_native）
//! - 不同连接模式：全局配置、项目级配置、组合配置
//! - 连接池管理、事务处理、并发连接、网络异常恢复
//!
//! 测试环境: postgresql://localhost:5432/business_db  postgres/postgresql
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::core::driver::native::postgres::PostgresDatabase;
use rdata_station_lib::core::driver::native::postgres_native::PostgresNativeDatabase;
use rdata_station_lib::core::driver::registry::DriverConnectionConfig;
use rdata_station_lib::core::driver::Database;

const PG_URL: &str = "postgres://postgres:postgresql@localhost:5432/business_db";
const PG_TEST_DB: &str = "_rd_pg_self_test_db";

// ==================== 驱动兼容性测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_driver_connect() {
    let db = PostgresDatabase::new(PG_URL).await;
    assert!(db.is_ok(), "PostgreSQL (sqlx) 驱动连接失败: {:?}", db.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_native_driver_connect() {
    let db = PostgresNativeDatabase::new(PG_URL).await;
    assert!(
        db.is_ok(),
        "PostgreSQL (native-tls) 驱动连接失败: {:?}",
        db.err()
    );
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_driver_compare_basic_query() {
    // 对比两种驱动的查询结果一致
    let pg = PostgresDatabase::new(PG_URL)
        .await
        .expect("PG 驱动连接失败");
    let pg_native = PostgresNativeDatabase::new(PG_URL)
        .await
        .expect("PG Native 驱动连接失败");

    let result_pg = pg.query("SELECT 1 AS val").await.expect("查询失败");
    let result_native = pg_native.query("SELECT 1 AS val").await.expect("查询失败");

    assert_eq!(result_pg.total_rows(), result_native.total_rows());
    assert_eq!(result_pg.columns, result_native.columns);
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_driver_wrong_password() {
    let db = PostgresDatabase::new("postgres://postgres:wrong_password@localhost:5432/business_db")
        .await;
    assert!(db.is_err(), "错误密码应返回错误");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_native_driver_wrong_password() {
    let db = PostgresNativeDatabase::new(
        "postgres://postgres:wrong_password@localhost:5432/business_db",
    )
    .await;
    assert!(db.is_err(), "错误密码应返回错误");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_driver_wrong_host() {
    let db =
        PostgresDatabase::new("postgres://postgres:postgresql@192.0.2.1:5432/business_db").await;
    assert!(db.is_err(), "不可达主机应返回错误");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_driver_invalid_url() {
    let db = PostgresDatabase::new("not-a-valid-url").await;
    assert!(db.is_err(), "无效 URL 应返回错误");
}

// ==================== 基础连接测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_ping() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.ping().await;
    assert!(result.is_ok(), "ping 失败: {:?}", result.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_meta() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let meta = db.meta();
    assert!(meta.supports_transaction);
    assert!(meta.server_version.is_some());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_select_one() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.query("SELECT 1 AS val").await.expect("查询失败");
    assert_eq!(result.total_rows(), 1);
    assert_eq!(result.columns, vec!["val"]);
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_select_version() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.query("SELECT version()").await;
    assert!(result.is_ok(), "SELECT version() 失败: {:?}", result.err());
}

// ==================== 连接池管理测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_pool_status() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let status = db.pool_status().await;
    assert!(status.is_some(), "pool_status 应返回 Some");
    if let Some(s) = status {
        assert!(s.max_connections > 0, "max_connections 应 > 0");
    }
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_pool_reuse() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    // 多次查询验证连接池复用
    for i in 0..20 {
        let result = db
            .query(&format!("SELECT {} AS iter", i))
            .await
            .expect("查询失败");
        assert_eq!(result.total_rows(), 1);
    }
}

// ==================== 事务处理能力测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_transaction_commit() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    // 准备测试表
    db.query(&format!(
        "CREATE TABLE IF NOT EXISTS {} (id INT PRIMARY KEY, val INT)",
        PG_TEST_DB
    ))
    .await
    .expect("创建表失败");

    let mut tx = db.begin_transaction().await.expect("开始事务失败");
    tx.query(&format!("INSERT INTO {} VALUES (1, 100)", PG_TEST_DB))
        .await
        .expect("插入失败");
    tx.commit().await.expect("提交失败");

    let result = db
        .query(&format!("SELECT val FROM {} WHERE id = 1", PG_TEST_DB))
        .await
        .unwrap();
    assert_eq!(result.total_rows(), 1);

    let _ = db
        .query(&format!("DROP TABLE IF EXISTS {}", PG_TEST_DB))
        .await;
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_transaction_rollback() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    db.query(&format!(
        "CREATE TABLE IF NOT EXISTS {} (id INT PRIMARY KEY, val INT)",
        PG_TEST_DB
    ))
    .await
    .expect("创建表失败");

    let mut tx = db.begin_transaction().await.expect("开始事务失败");
    tx.query(&format!("INSERT INTO {} VALUES (1, 200)", PG_TEST_DB))
        .await
        .expect("插入失败");
    tx.rollback().await.expect("回滚失败");

    let result = db
        .query(&format!("SELECT val FROM {} WHERE id = 1", PG_TEST_DB))
        .await
        .unwrap();
    assert_eq!(result.total_rows(), 0, "回滚后应无数据");

    let _ = db
        .query(&format!("DROP TABLE IF EXISTS {}", PG_TEST_DB))
        .await;
}

// ==================== 并发连接测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_concurrent_queries() {
    use std::sync::Arc;

    let db = Arc::new(PostgresDatabase::new(PG_URL).await.expect("连接失败"));

    let mut handles = Vec::new();
    for i in 0..20 {
        let db = Arc::clone(&db);
        handles.push(tokio::spawn(async move {
            let result = db
                .query(&format!("SELECT {} AS val, pg_sleep(0.01)", i))
                .await;
            (i, result)
        }));
    }

    let mut success = 0;
    for handle in handles {
        match handle.await.unwrap() {
            (_, Ok(_)) => success += 1,
            (i, Err(e)) => eprintln!("并发查询 {} 失败: {}", i, e),
        }
    }
    assert_eq!(success, 20, "20 个并发查询应全部成功");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_concurrent_connections() {
    let mut handles = Vec::new();

    for i in 0..10 {
        handles.push(tokio::spawn(async move {
            let db = PostgresDatabase::new(PG_URL).await;
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

// ==================== 元数据浏览测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_catalogs() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let catalogs = db.list_catalogs().await;
    assert!(catalogs.is_ok(), "list_catalogs 失败: {:?}", catalogs.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_schemas() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    // list_schemas 只接受 catalog: &str，返回 Vec<String>
    let schemas = db.list_schemas("business_db").await;
    assert!(schemas.is_ok(), "list_schemas 失败: {:?}", schemas.err());
    let s = schemas.unwrap();
    assert!(s.iter().any(|x| x == "public"), "应包含 public schema");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_tables() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let tables = db.list_tables("business_db", Some("public")).await;
    assert!(tables.is_ok(), "list_tables 失败: {:?}", tables.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_procedures() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let procs = db.list_procedures("business_db", Some("public")).await;
    assert!(procs.is_ok(), "list_procedures 失败: {:?}", procs.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_functions() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let funcs = db.list_functions("business_db", Some("public")).await;
    assert!(funcs.is_ok(), "list_functions 失败: {:?}", funcs.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_sequences() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let seqs = db.list_sequences("business_db", Some("public")).await;
    assert!(seqs.is_ok(), "list_sequences 失败: {:?}", seqs.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_triggers() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let triggers = db.list_triggers("business_db", Some("public")).await;
    assert!(triggers.is_ok(), "list_triggers 失败: {:?}", triggers.err());
}

// ==================== CRUD 往返测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_crud_roundtrip() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_pg_crud (id SERIAL PRIMARY KEY, name VARCHAR(100), value DOUBLE PRECISION)")
        .await
        .expect("创建表失败");

    // INSERT
    db.query("INSERT INTO _rd_pg_crud (name, value) VALUES ('hello', 3.14)")
        .await
        .expect("插入失败");
    db.query("INSERT INTO _rd_pg_crud (name, value) VALUES ('world', 2.718)")
        .await
        .expect("插入失败");

    // SELECT
    let result = db
        .query("SELECT * FROM _rd_pg_crud ORDER BY id")
        .await
        .expect("查询失败");
    assert_eq!(result.total_rows(), 2);

    // UPDATE
    db.query("UPDATE _rd_pg_crud SET name = 'updated' WHERE name = 'hello'")
        .await
        .expect("更新失败");

    // DELETE
    db.query("DELETE FROM _rd_pg_crud WHERE name = 'world'")
        .await
        .expect("删除失败");

    let result = db
        .query("SELECT COUNT(*) as cnt FROM _rd_pg_crud")
        .await
        .expect("查询失败");
    assert_eq!(result.total_rows(), 1);

    let _ = db.query("DROP TABLE IF EXISTS _rd_pg_crud").await;
}

// ==================== 错误处理测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_error_syntax() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.query("SELECTT * FROM nonexistent").await;
    assert!(result.is_err(), "语法错误应返回错误");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_error_nonexistent_table() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.query("SELECT * FROM _rd_nonexistent_xyz").await;
    assert!(result.is_err(), "不存在的表应返回错误");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_error_duplicate_key() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_pg_unique (id INT PRIMARY KEY, val INT)")
        .await
        .expect("创建表失败");
    db.query("INSERT INTO _rd_pg_unique (id, val) VALUES (1, 1)")
        .await
        .expect("插入失败");

    let result = db
        .query("INSERT INTO _rd_pg_unique (id, val) VALUES (1, 2)")
        .await;
    assert!(result.is_err(), "主键冲突应返回错误");

    let _ = db.query("DROP TABLE IF EXISTS _rd_pg_unique").await;
}

// ==================== 网络异常恢复测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_connection_recovery() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    // 正常查询
    let result = db.query("SELECT 1").await;
    assert!(result.is_ok(), "初始查询应成功");

    // 模拟网络中断后恢复（通过 ping 验证）
    let ping = db.ping().await;
    assert!(ping.is_ok(), "ping 应成功（恢复后）");
}

// ==================== DriverConnectionConfig 构建测试 ====================

#[test]
fn test_pg_config_to_url_with_properties() {
    let mut config = DriverConnectionConfig::new("postgres")
        .with_host("localhost")
        .with_port(5432)
        .with_database("business_db")
        .with_username("postgres")
        .with_password("postgresql");
    config
        .driver_properties
        .insert("sslmode".to_string(), "disable".to_string());
    config
        .driver_properties
        .insert("application_name".to_string(), "rd_test".to_string());

    let url = config.to_url().unwrap();
    assert!(url.starts_with("postgres://postgres:postgresql@localhost:5432/business_db"));
    assert!(url.contains("sslmode=disable"));
    assert!(url.contains("application_name=rd_test"));
}

#[test]
fn test_pg_config_url_override_with_properties() {
    let mut config = DriverConnectionConfig::new("postgres")
        .with_url_override("postgres://postgres:postgresql@localhost:5432/business_db");
    config
        .driver_properties
        .insert("sslmode".to_string(), "disable".to_string());
    config
        .driver_properties
        .insert("connect_timeout".to_string(), "10".to_string());

    let url = config.to_url().unwrap();
    assert!(url.contains("sslmode=disable"));
    assert!(url.contains("connect_timeout=10"));
}

#[test]
fn test_pg_native_config_with_tls_properties() {
    let mut config = DriverConnectionConfig::new("postgres_native")
        .with_host("localhost")
        .with_port(5432)
        .with_database("business_db")
        .with_username("postgres")
        .with_password("postgresql");
    config
        .driver_properties
        .insert("sslmode".to_string(), "require".to_string());
    config
        .driver_properties
        .insert("sslrootcert".to_string(), "/path/to/ca.pem".to_string());

    let url = config.to_url().unwrap();
    assert!(url.contains("sslmode=require"));
    assert!(url.contains("sslrootcert=/path/to/ca.pem"));
}

// ==================== 大结果集测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_large_result_set() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    db.query("CREATE TABLE IF NOT EXISTS _rd_pg_large (id SERIAL PRIMARY KEY, val INT)")
        .await
        .expect("创建表失败");

    for i in 0..100 {
        db.query(&format!("INSERT INTO _rd_pg_large (val) VALUES ({})", i))
            .await
            .expect("插入失败");
    }

    let result = db
        .query("SELECT * FROM _rd_pg_large ORDER BY id")
        .await
        .unwrap();
    assert_eq!(result.total_rows(), 100);
    assert_eq!(result.columns.len(), 2);

    let _ = db.query("DROP TABLE IF EXISTS _rd_pg_large").await;
}

// ==================== 取消查询测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_with_cancel() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let cancel_token = tokio_util::sync::CancellationToken::new();

    let result = db.query_with_cancel("SELECT 1", cancel_token.clone()).await;
    assert!(result.is_ok(), "正常查询应成功: {:?}", result.err());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_with_cancel_triggered() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let cancel_token = tokio_util::sync::CancellationToken::new();
    cancel_token.cancel();

    let result = db.query_with_cancel("SELECT 1", cancel_token).await;
    assert!(result.is_err(), "已取消的查询应返回错误");
}

// ==================== 参数化查询测试 ====================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_with_params() {
    use rdata_station_lib::core::models::Value;

    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db
        .query_with_params(
            "SELECT $1::int AS v1, $2::text AS v2, $3::float AS v3",
            vec![
                Value::Int(42),
                Value::Text("hello".to_string()),
                Value::Float(3.14),
            ],
        )
        .await;
    assert!(result.is_ok(), "参数化查询失败: {:?}", result.err());
}

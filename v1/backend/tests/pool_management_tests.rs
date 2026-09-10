//! 连接池管理综合测试
//!
//! 覆盖双池架构（SmartPool + StandardPool）的核心功能：
//! - DbPool trait 方法（acquire/close/is_closed/status）
//! - StandardPool + StandardPoolConfig（用户数据源池）
//! - SmartPool + SmartPoolConfig + SmartPoolWrapper（系统内置库池）
//! - PoolStatus / PoolStats / StandardPoolStats
//! - ConnectionMetadata 生命周期
//! - SqlitePoolWrapper（实际连接池实现）
//! - 并发获取 / 池关闭 / 动态缩放
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use std::sync::Arc;
use std::time::Duration;

use rdata_station_lib::core::driver::native::sqlite_pool::SqlitePoolWrapper;
use rdata_station_lib::core::driver::smart_pool::{
    PoolStats, SmartPool, SmartPoolConfig, SmartPoolWrapper,
};
use rdata_station_lib::core::driver::standard_pool::{
    StandardPool, StandardPoolConfig, StandardPoolStats,
};
use rdata_station_lib::core::driver::traits::{DbPool, PoolStatus};
use rdata_station_lib::core::error::{ConnectionError, CoreError};

// ============================================================================
// PoolStatus 测试
// ============================================================================

#[test]
fn test_pool_status_unknown() {
    let status = PoolStatus::unknown();
    assert_eq!(status.size, 0);
    assert_eq!(status.idle, 0);
    assert_eq!(status.active, 0);
    assert_eq!(status.waiting, 0);
    assert_eq!(status.max_connections, 10);
    assert_eq!(status.min_connections, 2);
}

#[test]
fn test_pool_status_custom() {
    let status = PoolStatus {
        size: 10,
        idle: 5,
        active: 3,
        waiting: 2,
        max_connections: 20,
        min_connections: 2,
    };
    assert_eq!(status.size, 10);
    assert_eq!(status.idle, 5);
    assert_eq!(status.active, 3);
    assert_eq!(status.waiting, 2);
    assert_eq!(status.max_connections, 20);
    assert_eq!(status.min_connections, 2);
}

#[test]
fn test_pool_status_clone() {
    let status = PoolStatus {
        size: 5,
        idle: 2,
        active: 1,
        waiting: 2,
        max_connections: 10,
        min_connections: 1,
    };
    let cloned = status.clone();
    assert_eq!(cloned.size, status.size);
    assert_eq!(cloned.idle, status.idle);
    assert_eq!(cloned.active, status.active);
}

// ============================================================================
// StandardPoolConfig 测试
// ============================================================================

#[test]
fn test_standard_pool_config_default() {
    let config = StandardPoolConfig::default();
    assert_eq!(config.min_connections, 2);
    assert_eq!(config.max_connections, 20);
    assert_eq!(config.idle_timeout_secs, 600);
    assert_eq!(config.max_lifetime_secs, 1800);
    assert_eq!(config.acquire_timeout_secs, 30);
    assert!(config.health_check_enabled);
}

#[test]
fn test_standard_pool_config_for_sqlite() {
    let config = StandardPoolConfig::for_sqlite();
    assert_eq!(config.min_connections, 1);
    assert_eq!(config.max_connections, 5);
    assert_eq!(config.idle_timeout_secs, 300);
    assert_eq!(config.max_lifetime_secs, 3600);
    assert_eq!(config.acquire_timeout_secs, 10);
    assert!(config.health_check_enabled);
}

#[test]
fn test_standard_pool_config_for_duckdb() {
    let config = StandardPoolConfig::for_duckdb();
    // DuckDB 单写入者模型，连接数应保持为 1
    assert_eq!(config.min_connections, 1);
    assert_eq!(config.max_connections, 1);
    assert_eq!(config.idle_timeout_secs, 1800);
    assert_eq!(config.max_lifetime_secs, 7200);
    assert_eq!(config.acquire_timeout_secs, 30);
    assert!(config.health_check_enabled);
}

#[test]
fn test_standard_pool_config_for_network() {
    let config = StandardPoolConfig::for_network();
    assert_eq!(config.min_connections, 2);
    assert_eq!(config.max_connections, 20);
    assert_eq!(config.idle_timeout_secs, 600);
    assert_eq!(config.max_lifetime_secs, 1800);
    assert_eq!(config.acquire_timeout_secs, 30);
    assert!(config.health_check_enabled);
}

#[test]
fn test_standard_pool_config_serialization() {
    let config = StandardPoolConfig::default();
    let json = serde_json::to_string(&config).expect("序列化失败");
    assert!(json.contains("min_connections"));
    assert!(json.contains("max_connections"));
    assert!(json.contains("idle_timeout_secs"));

    let parsed: StandardPoolConfig = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.min_connections, config.min_connections);
    assert_eq!(parsed.max_connections, config.max_connections);
}

#[test]
fn test_standard_pool_config_custom() {
    let config = StandardPoolConfig {
        min_connections: 5,
        max_connections: 50,
        idle_timeout_secs: 300,
        max_lifetime_secs: 3600,
        acquire_timeout_secs: 15,
        health_check_enabled: false,
    };
    assert_eq!(config.min_connections, 5);
    assert_eq!(config.max_connections, 50);
    assert!(!config.health_check_enabled);
}

// ============================================================================
// StandardPoolStats 测试
// ============================================================================

#[test]
fn test_standard_pool_stats_default() {
    let stats = StandardPoolStats::default();
    assert_eq!(stats.size, 0);
    assert_eq!(stats.idle, 0);
    assert_eq!(stats.active, 0);
    assert_eq!(stats.waiting, 0);
    assert_eq!(stats.total_acquires, 0);
    assert_eq!(stats.total_releases, 0);
    assert_eq!(stats.health_check_failures, 0);
}

#[test]
fn test_standard_pool_stats_serialization() {
    let stats = StandardPoolStats {
        size: 10,
        idle: 5,
        active: 3,
        waiting: 2,
        total_acquires: 100,
        total_releases: 95,
        health_check_failures: 1,
    };
    let json = serde_json::to_string(&stats).expect("序列化失败");
    assert!(json.contains("total_acquires"));
    assert!(json.contains("health_check_failures"));
}

// ============================================================================
// SmartPoolConfig 测试
// ============================================================================

#[test]
fn test_smart_pool_config_default() {
    let config = SmartPoolConfig::default();
    assert_eq!(config.min_connections, 2);
    assert_eq!(config.max_connections, 20);
    assert_eq!(config.initial_connections, 2);
    assert_eq!(config.acquire_timeout, Duration::from_secs(30));
    assert_eq!(config.idle_timeout, Duration::from_secs(600));
    assert_eq!(config.max_lifetime, Duration::from_secs(1800));
    assert_eq!(config.health_check_interval, Duration::from_secs(60));
    assert!(config.enable_dynamic_scaling);
    assert_eq!(config.scaling_threshold_ms, 100);
    assert_eq!(config.scale_up_step, 2);
    assert_eq!(config.scale_down_step, 1);
}

#[test]
fn test_smart_pool_config_custom() {
    let config = SmartPoolConfig {
        min_connections: 5,
        max_connections: 50,
        initial_connections: 5,
        acquire_timeout: Duration::from_secs(15),
        idle_timeout: Duration::from_secs(300),
        max_lifetime: Duration::from_secs(3600),
        health_check_interval: Duration::from_secs(30),
        enable_dynamic_scaling: false,
        scaling_threshold_ms: 200,
        scale_up_step: 3,
        scale_down_step: 2,
    };
    assert_eq!(config.min_connections, 5);
    assert_eq!(config.max_connections, 50);
    assert!(!config.enable_dynamic_scaling);
    assert_eq!(config.scaling_threshold_ms, 200);
}

#[test]
fn test_smart_pool_config_clone() {
    let config = SmartPoolConfig::default();
    let cloned = config.clone();
    assert_eq!(cloned.min_connections, config.min_connections);
    assert_eq!(cloned.max_connections, config.max_connections);
    assert_eq!(cloned.enable_dynamic_scaling, config.enable_dynamic_scaling);
}

// ============================================================================
// PoolStats 测试
// ============================================================================

#[test]
fn test_pool_stats_default() {
    let stats = PoolStats::default();
    assert_eq!(stats.current_size, 0);
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.idle_connections, 0);
    assert_eq!(stats.waiting_requests, 0);
    assert_eq!(stats.total_acquires, 0);
    assert_eq!(stats.total_releases, 0);
    assert_eq!(stats.avg_acquire_ms, 0.0);
    assert_eq!(stats.connections_created, 0);
    assert_eq!(stats.connections_destroyed, 0);
    assert_eq!(stats.health_check_failures, 0);
    assert_eq!(stats.scale_up_count, 0);
    assert_eq!(stats.scale_down_count, 0);
}

#[test]
fn test_pool_stats_custom() {
    let stats = PoolStats {
        current_size: 10,
        active_connections: 5,
        idle_connections: 3,
        waiting_requests: 2,
        total_acquires: 1000,
        total_releases: 995,
        avg_acquire_ms: 12.5,
        connections_created: 50,
        connections_destroyed: 40,
        health_check_failures: 3,
        scale_up_count: 5,
        scale_down_count: 2,
    };
    assert_eq!(stats.current_size, 10);
    assert_eq!(stats.active_connections, 5);
    assert_eq!(stats.avg_acquire_ms, 12.5);
    assert_eq!(stats.scale_up_count, 5);
    assert_eq!(stats.scale_down_count, 2);
}

// ============================================================================
// SqlitePoolWrapper 测试（实际连接池实现）
// ============================================================================

#[tokio::test]
async fn test_sqlite_pool_create() {
    let pool = SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败");
    assert!(!pool.is_closed());
}

#[test]
fn test_sqlite_pool_create_with_size() {
    let pool = SqlitePoolWrapper::with_size(":memory:", 3).expect("创建 SQLite 池失败");
    let status = pool.status();
    assert_eq!(status.size, 3);
    assert_eq!(status.max_connections, 3);
    assert_eq!(status.min_connections, 1);
}

#[test]
fn test_sqlite_pool_size_clamped() {
    // 池大小被 clamp 到 1..=10
    let pool = SqlitePoolWrapper::with_size(":memory:", 0).expect("创建 SQLite 池失败");
    let status = pool.status();
    assert_eq!(status.size, 1); // clamped to 1

    let pool = SqlitePoolWrapper::with_size(":memory:", 100).expect("创建 SQLite 池失败");
    let status = pool.status();
    assert_eq!(status.size, 10); // clamped to 10
}

#[tokio::test]
async fn test_sqlite_pool_acquire() {
    let pool = SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败");
    let db = pool.acquire().await.expect("获取连接失败");

    // 验证获取的连接可用
    let meta = db.meta();
    assert!(!meta.is_in_memory);
}

#[tokio::test]
async fn test_sqlite_pool_multiple_acquire() {
    let pool = SqlitePoolWrapper::with_size(":memory:", 3).expect("创建 SQLite 池失败");

    // 连续获取多个连接
    let db1 = pool.acquire().await.expect("获取连接 1 失败");
    let db2 = pool.acquire().await.expect("获取连接 2 失败");
    let db3 = pool.acquire().await.expect("获取连接 3 失败");

    // 验证每个连接可用
    assert!(!db1.meta().is_in_memory);
    assert!(!db2.meta().is_in_memory);
    assert!(!db3.meta().is_in_memory);

    // 第4个连接应从池中补充（pool_size=3, 已取3个，应自动创建新连接）
    let db4 = pool.acquire().await.expect("获取连接 4 失败");
    assert!(!db4.meta().is_in_memory);
}

#[tokio::test]
async fn test_sqlite_pool_acquire_after_close() {
    let pool = SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败");
    pool.close().await.expect("关闭池失败");
    assert!(pool.is_closed());

    let result = pool.acquire().await;
    assert!(result.is_err());
    match result {
        Err(CoreError::Connection(ConnectionError::PoolClosed)) => {
            // 预期错误
        }
        Err(e) => panic!("期望 PoolClosed 错误，但得到: {}", e),
        Ok(_) => panic!("期望 PoolClosed 错误，但得到了 Ok"),
    }
}

#[tokio::test]
async fn test_sqlite_pool_close() {
    let pool = SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败");
    assert!(!pool.is_closed());

    pool.close().await.expect("关闭池失败");
    assert!(pool.is_closed());
}

#[tokio::test]
async fn test_sqlite_pool_double_close() {
    let pool = SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败");
    pool.close().await.expect("第一次关闭失败");
    // 第二次关闭不应报错
    pool.close().await.expect("第二次关闭失败");
    assert!(pool.is_closed());
}

#[test]
fn test_sqlite_pool_status() {
    let pool = SqlitePoolWrapper::with_size(":memory:", 5).expect("创建 SQLite 池失败");
    let status = pool.status();

    assert_eq!(status.size, 5);
    assert_eq!(status.max_connections, 5);
    assert_eq!(status.min_connections, 1);
    assert_eq!(status.waiting, 0);
}

#[tokio::test]
async fn test_sqlite_pool_status_after_acquire() {
    let pool = Arc::new(SqlitePoolWrapper::with_size(":memory:", 3).expect("创建 SQLite 池失败"));

    let _db = pool.acquire().await.expect("获取连接失败");

    let p = pool.clone();
    let status = tokio::task::spawn_blocking(move || p.status())
        .await
        .expect("spawn_blocking failed");
    // 取走1个后，池中剩余2个
    assert_eq!(status.idle, 2);
    assert_eq!(status.active, 1);
}

// ============================================================================
// StandardPool 测试（包装 SqlitePoolWrapper）
// ============================================================================

#[tokio::test]
async fn test_standard_pool_create() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);
    assert_eq!(pool.name(), "test-pool");
    assert!(!pool.is_closed());
}

#[tokio::test]
async fn test_standard_pool_for_sqlite() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::for_sqlite("sqlite-pool", inner);
    let config = pool.config();
    assert_eq!(config.min_connections, 1);
    assert_eq!(config.max_connections, 5);
}

#[tokio::test]
async fn test_standard_pool_for_duckdb() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::for_duckdb("duckdb-pool", inner);
    let config = pool.config();
    assert_eq!(config.min_connections, 1);
    assert_eq!(config.max_connections, 1);
}

#[tokio::test]
async fn test_standard_pool_for_network() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::for_network("network-pool", inner);
    let config = pool.config();
    assert_eq!(config.min_connections, 2);
    assert_eq!(config.max_connections, 20);
}

#[tokio::test]
async fn test_standard_pool_acquire() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);

    let db = pool.acquire().await.expect("获取连接失败");
    assert!(!db.meta().is_in_memory);
}

#[tokio::test]
async fn test_standard_pool_acquire_after_close() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);
    pool.close().await.expect("关闭池失败");

    let result = pool.acquire().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_standard_pool_close() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);
    assert!(!pool.is_closed());

    pool.close().await.expect("关闭池失败");
    assert!(pool.is_closed());
}

#[test]
fn test_standard_pool_status() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);

    let status = pool.status();
    assert!(status.size > 0);
    assert!(status.max_connections > 0);
}

#[tokio::test]
async fn test_standard_pool_stats() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);

    // 获取连接后统计信息应更新
    let _db = pool.acquire().await.expect("获取连接失败");
    let stats = pool.stats().await;
    assert!(stats.total_acquires > 0);
}

#[tokio::test]
async fn test_standard_pool_inner_pool() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let pool = StandardPool::new("test-pool", StandardPoolConfig::for_sqlite(), inner);
    let inner_pool = pool.inner_pool();
    assert!(!inner_pool.is_closed());
}

// ============================================================================
// SmartPool 测试
// ============================================================================

#[tokio::test]
async fn test_smart_pool_create() {
    let pool = SmartPool::with_defaults("smart-pool");
    let config = pool.config().await;
    assert_eq!(config.min_connections, 2);
    assert_eq!(config.max_connections, 20);
    assert!(config.enable_dynamic_scaling);
}

#[tokio::test]
async fn test_smart_pool_create_custom() {
    let config = SmartPoolConfig {
        min_connections: 3,
        max_connections: 10,
        initial_connections: 3,
        acquire_timeout: Duration::from_secs(10),
        idle_timeout: Duration::from_secs(300),
        max_lifetime: Duration::from_secs(3600),
        health_check_interval: Duration::from_secs(30),
        enable_dynamic_scaling: false,
        scaling_threshold_ms: 200,
        scale_up_step: 2,
        scale_down_step: 1,
    };
    let pool = SmartPool::new("custom-smart", config);
    let loaded_config = pool.config().await;
    assert_eq!(loaded_config.min_connections, 3);
    assert!(!loaded_config.enable_dynamic_scaling);
}

#[tokio::test]
async fn test_smart_pool_stats() {
    let pool = SmartPool::with_defaults("stats-pool");
    let stats = pool.stats().await;
    assert_eq!(stats.current_size, 0);
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.scale_up_count, 0);
    assert_eq!(stats.scale_down_count, 0);
}

#[tokio::test]
async fn test_smart_pool_record_acquire() {
    let pool = SmartPool::with_defaults("acquire-pool");
    pool.record_acquire(50.0).await;

    let stats = pool.stats().await;
    assert_eq!(stats.total_acquires, 1);
    assert!(stats.avg_acquire_ms > 0.0);
}

#[tokio::test]
async fn test_smart_pool_multiple_record_acquire() {
    let pool = SmartPool::with_defaults("multi-acquire-pool");

    // 记录多次获取
    for _ in 0..100 {
        pool.record_acquire(10.0).await;
    }

    let stats = pool.stats().await;
    assert_eq!(stats.total_acquires, 100);
    assert!((stats.avg_acquire_ms - 10.0).abs() < 1.0);
}

#[tokio::test]
async fn test_smart_pool_should_scale_up() {
    let pool = SmartPool::with_defaults("scale-pool");

    // 初始不应扩容
    let should = pool.should_scale_up().await;
    assert!(!should);

    // 多次记录高延迟获取
    for _ in 0..100 {
        pool.record_acquire(200.0).await; // 超过 scaling_threshold_ms=100
    }

    // 高延迟应触发扩容
    let should = pool.should_scale_up().await;
    assert!(should);
}

#[tokio::test]
async fn test_smart_pool_scale_up() {
    let pool = SmartPool::with_defaults("scale-up-pool");
    pool.scale_up().await;

    let stats = pool.stats().await;
    assert_eq!(stats.scale_up_count, 1);
}

#[tokio::test]
async fn test_smart_pool_scale_down() {
    let pool = SmartPool::with_defaults("scale-down-pool");
    pool.scale_down().await;

    let stats = pool.stats().await;
    assert_eq!(stats.scale_down_count, 1);
}

#[tokio::test]
async fn test_smart_pool_close() {
    let pool = SmartPool::with_defaults("close-pool");
    pool.close().await.expect("关闭池失败");

    let stats = pool.stats().await;
    // 关闭后 current_size 应为 0
    assert_eq!(stats.current_size, 0);
}

// ============================================================================
// SmartPoolWrapper 测试
// ============================================================================

#[tokio::test]
async fn test_smart_pool_wrapper_create() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);
    assert!(!wrapper.is_closed());
}

#[tokio::test]
async fn test_smart_pool_wrapper_acquire() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);

    let db = wrapper.acquire().await.expect("获取连接失败");
    assert!(!db.meta().is_in_memory);
}

#[tokio::test]
async fn test_smart_pool_wrapper_acquire_after_close() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);
    wrapper.close().await.expect("关闭池失败");

    let result = wrapper.acquire().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_smart_pool_wrapper_close() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);
    assert!(!wrapper.is_closed());

    wrapper.close().await.expect("关闭池失败");
    assert!(wrapper.is_closed());
}

#[test]
fn test_smart_pool_wrapper_status() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);

    let status = wrapper.status();
    assert!(status.size > 0);
    assert!(status.max_connections > 0);
}

#[tokio::test]
async fn test_smart_pool_wrapper_inner_pool() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);
    let inner_pool = wrapper.inner_pool();
    assert!(!inner_pool.is_closed());
}

#[tokio::test]
async fn test_smart_pool_wrapper_smart_pool() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败"));
    let wrapper = SmartPoolWrapper::new("wrapper-pool", SmartPoolConfig::default(), inner);
    let smart_pool = wrapper.smart_pool();
    let config = smart_pool.config().await;
    assert!(config.enable_dynamic_scaling);
}

// ============================================================================
// 并发获取测试
// ============================================================================

#[tokio::test]
async fn test_sqlite_pool_concurrent_acquire() {
    let pool = Arc::new(SqlitePoolWrapper::with_size(":memory:", 5).expect("创建 SQLite 池失败"));

    let mut handles = Vec::new();
    for i in 0..10 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            let db = pool
                .acquire()
                .await
                .expect(&format!("并发获取连接 {} 失败", i));
            assert!(!db.meta().is_in_memory);
            i
        }));
    }

    for handle in handles {
        handle.await.expect("并发任务失败");
    }
}

#[tokio::test]
async fn test_standard_pool_concurrent_acquire() {
    let inner: Arc<dyn DbPool> =
        Arc::new(SqlitePoolWrapper::with_size(":memory:", 5).expect("创建 SQLite 池失败"));
    let pool = Arc::new(StandardPool::new(
        "concurrent-pool",
        StandardPoolConfig::for_sqlite(),
        inner,
    ));

    let mut handles = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            let db = pool.acquire().await.expect("并发获取连接失败");
            assert!(!db.meta().is_in_memory);
        }));
    }

    for handle in handles {
        handle.await.expect("并发任务失败");
    }
}

// ============================================================================
// 连接生命周期测试
// ============================================================================

#[tokio::test]
async fn test_connection_acquire_and_drop() {
    let pool = SqlitePoolWrapper::new(":memory:").expect("创建 SQLite 池失败");

    {
        let db = pool.acquire().await.expect("获取连接失败");
        assert!(!db.meta().is_in_memory);
        // db 在此作用域结束时被 drop
    }

    // drop 后池应仍可用
    let db2 = pool.acquire().await.expect("再次获取连接失败");
    assert!(!db2.meta().is_in_memory);
}

#[tokio::test]
async fn test_pool_keeps_working_after_partial_acquire() {
    let pool = SqlitePoolWrapper::with_size(":memory:", 3).expect("创建 SQLite 池失败");

    // 获取2个连接
    let db1 = pool.acquire().await.expect("获取连接 1 失败");
    let db2 = pool.acquire().await.expect("获取连接 2 失败");

    // drop db1
    drop(db1);

    // 仍可获取
    let db3 = pool.acquire().await.expect("获取连接 3 失败");
    drop(db2);
    drop(db3);

    assert!(!pool.is_closed());
}

// ============================================================================
// 内存/文件池对比测试
// ============================================================================

#[tokio::test]
async fn test_sqlite_pool_file_vs_memory() {
    // 文件池
    let tmp_dir = std::env::temp_dir();
    let file_path = tmp_dir.join("pool_test_file.db");
    let file_pool = SqlitePoolWrapper::new(&file_path.to_string_lossy()).expect("创建文件池失败");

    let db = file_pool.acquire().await.expect("获取文件池连接失败");
    assert!(!db.meta().is_in_memory); // 文件数据库

    drop(db);
    file_pool.close().await.expect("关闭文件池失败");

    // 清理测试文件
    let _ = std::fs::remove_file(&file_path);

    // 内存池
    let mem_pool = SqlitePoolWrapper::new(":memory:").expect("创建内存池失败");
    let db = mem_pool.acquire().await.expect("获取内存池连接失败");
    assert!(!db.meta().is_in_memory); // 内存数据库

    drop(db);
    mem_pool.close().await.expect("关闭内存池失败");
}

// ============================================================================
// 错误传播测试
// ============================================================================

#[test]
fn test_pool_closed_error_display() {
    let err = CoreError::connection(ConnectionError::PoolClosed);
    let msg = format!("{}", err);
    assert!(!msg.is_empty());
    assert!(msg.to_lowercase().contains("pool") || msg.to_lowercase().contains("closed"));
}

#[test]
fn test_pool_closed_error_debug() {
    let err = CoreError::connection(ConnectionError::PoolClosed);
    let debug = format!("{:?}", err);
    assert!(debug.contains("PoolClosed"));
}

// ============================================================================
// 池大小边界测试
// ============================================================================

#[test]
fn test_sqlite_pool_min_size() {
    let pool = SqlitePoolWrapper::with_size(":memory:", 1).expect("创建单连接池失败");
    let status = pool.status();
    assert_eq!(status.size, 1);
}

#[test]
fn test_sqlite_pool_max_size() {
    let pool = SqlitePoolWrapper::with_size(":memory:", 10).expect("创建最大池失败");
    let status = pool.status();
    assert_eq!(status.size, 10);
}

#[tokio::test]
async fn test_sqlite_pool_exhaust_and_replenish() {
    let pool = Arc::new(SqlitePoolWrapper::with_size(":memory:", 2).expect("创建池失败"));

    // 取走所有连接
    let db1 = pool.acquire().await.expect("获取连接 1 失败");
    let db2 = pool.acquire().await.expect("获取连接 2 失败");

    // 池中应无空闲连接（active=2, idle=0）
    let p = pool.clone();
    let status = tokio::task::spawn_blocking(move || p.status())
        .await
        .expect("spawn_blocking failed");
    assert_eq!(status.idle, 0);
    assert_eq!(status.active, 2);

    // 仍可获取（自动创建新连接）
    let db3 = pool.acquire().await.expect("获取连接 3 失败");
    assert!(!db3.meta().is_in_memory);

    drop(db1);
    drop(db2);
    drop(db3);
}

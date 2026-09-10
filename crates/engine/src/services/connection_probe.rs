//! 连接探测服务（Phase A：测试连接）
//!
//! 独立会话测试连接：通过 DriverRegistry 创建一次性数据库连接，ping + 探测版本，
//! **不注册进 ConnectionManager**，不污染正式连接状态（dev-plan §4 风险对策）。
//!
//! 入参 `DriverConnectionConfig`（统一连接配置），出参 `connection::model::TestResult`。

use std::time::Instant;

use connection::model::TestResult;

use crate::driver::registry::{DriverConnectionConfig, DriverRegistry};
use shared::error::{ConnectionError, CoreError};

/// 按驱动返回版本探测 SQL（仅网络型；文件型通过 meta 探测）。
fn probe_sql(driver: &str) -> Option<&'static str> {
    match driver {
        "mysql" | "mysql_native" => Some("SELECT VERSION()"),
        "postgres" | "postgres_native" => Some("SELECT version()"),
        "sqlite" => Some("SELECT sqlite_version()"),
        "duckdb" => Some("SELECT version()"),
        _ => None,
    }
}

/// 对驱动配置执行独立测试连接。
///
/// 成功返回 `TestResult::ok`（含耗时与服务器版本）；失败返回 `Err(CoreError)`，
/// 由调用方（DataSourceService）转为 `TestResult::err`。
pub async fn test_connection(config: DriverConnectionConfig) -> Result<TestResult, CoreError> {
    let started = Instant::now();

    let factory = DriverRegistry::get(&config.driver).ok_or_else(|| {
        CoreError::connection(ConnectionError::DriverNotFound {
            driver: config.driver.clone(),
        })
    })?;

    // 独立会话：create 返回的 db 在函数退出即 drop，不会进入连接池/管理器。
    let db = factory.create(config.clone()).await?;

    // 健康检查（驱动可覆盖实现真正的 ping）。
    db.ping().await?;

    let latency_ms = started.elapsed().as_millis() as u64;

    // 版本探测：优先取连接器 meta，缺失时执行探测 SQL（仅第一行第一列）。
    let version = match db.meta().server_version.clone() {
        Some(v) => Some(v),
        None => {
            if let Some(sql) = probe_sql(&config.driver) {
                db.query(sql)
                    .await
                    .ok()
                    .and_then(|r| {
                        r.rows
                            .first()
                            .and_then(|row| row.first())
                            .map(|v| v.to_string())
                    })
            } else {
                None
            }
        }
    };

    tracing::info!(
        target: "connection_probe",
        driver = %config.driver,
        latency_ms,
        version = ?version,
        "测试连接成功"
    );

    Ok(TestResult::ok(
        format!("连接成功 · {} ms", latency_ms),
        latency_ms,
        version,
    ))
}

/// 测试连接（错误转换为可读消息）。
///
/// 对话框直接调用此函数拿 `TestResult`，无需处理 `CoreError`。
pub async fn test_connection_result(config: DriverConnectionConfig) -> TestResult {
    match test_connection(config).await {
        Ok(r) => r,
        Err(e) => TestResult::err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_sql_mapping() {
        assert_eq!(probe_sql("mysql"), Some("SELECT VERSION()"));
        assert_eq!(probe_sql("postgres"), Some("SELECT version()"));
        assert_eq!(probe_sql("sqlite"), Some("SELECT sqlite_version()"));
        assert_eq!(probe_sql("duckdb"), Some("SELECT version()"));
        assert_eq!(probe_sql("oracle"), None);
    }

    #[test]
    fn driver_not_found_is_error() {
        let config = DriverConnectionConfig::new("no-such-driver");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(test_connection(config));
        assert!(result.is_err());
    }
}

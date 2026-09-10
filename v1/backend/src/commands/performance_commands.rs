//! 性能监控相关命令
//!
//! 处理性能指标查询、监控配置等操作

use crate::core::error::CoreError;
use crate::core::performance::get_performance_monitor;

/// 获取性能指标
#[tauri::command]
#[specta::specta]
pub async fn get_performance_metrics() -> Result<serde_json::Value, CoreError> {
    let monitor = get_performance_monitor();
    let metrics = monitor.get_metrics().await;
    let uptime = monitor.uptime();

    let mut response = serde_json::to_value(metrics)
        .map_err(|e| CoreError::from(format!("序列化性能指标失败: {}", e)))?;

    if let Some(obj) = response.as_object_mut() {
        obj.insert(
            "uptime_secs".to_string(),
            serde_json::json!(uptime.as_secs()),
        );
    }

    Ok(response)
}

/// 重置性能指标
#[tauri::command]
#[specta::specta]
pub async fn reset_performance_metrics() -> Result<(), CoreError> {
    let monitor = get_performance_monitor();
    monitor.reset_metrics().await;
    Ok(())
}

/// 获取系统健康状态
#[tauri::command]
#[specta::specta]
pub async fn get_system_health() -> Result<serde_json::Value, CoreError> {
    let monitor = get_performance_monitor();
    let metrics = monitor.get_metrics().await;

    let cache_hit_rate = metrics.cache_hit_rate;
    let avg_response_time = metrics.avg_response_time_ms;
    let active_requests = metrics.active_requests;

    let health_status = if cache_hit_rate > 0.8 && avg_response_time < 100.0 && active_requests < 50
    {
        "healthy"
    } else if cache_hit_rate > 0.5 && avg_response_time < 500.0 && active_requests < 100 {
        "degraded"
    } else {
        "critical"
    };

    Ok(serde_json::json!({
        "status": health_status,
        "cache_hit_rate": cache_hit_rate,
        "avg_response_time_ms": avg_response_time,
        "active_requests": active_requests,
        "estimated_memory_mb": metrics.estimated_memory_mb,
    }))
}

//! 内存管理相关命令
//!
//! 处理内存监控、缓存清理、内存统计等操作

use crate::core::cache::{get_memory_guard, CacheManager};
use crate::core::error::CoreError;

/// 内存统计响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct MemoryStatsResponse {
    pub cache_entries: u32,
    pub pool_size: u32,
    pub estimated_memory_mb: f64,
    pub last_eviction_secs_ago: Option<u32>,
    pub total_evictions: u32,
    pub pressure_level: String,
    pub cache_hit_rate: f64,
    pub cache_size: u32,
}

/// 获取内存统计信息
#[tauri::command]
#[specta::specta]
pub async fn get_memory_stats() -> Result<MemoryStatsResponse, CoreError> {
    let guard = get_memory_guard().ok_or_else(|| "Memory guard not initialized".to_string())?;

    let stats = guard.get_stats().await;

    let last_eviction_secs_ago = stats
        .last_eviction
        .map(|instant| instant.elapsed().as_secs() as u32);

    // 获取缓存命中率
    let cache_manager = CacheManager::instance();
    let cache_hit_rate = {
        let manager = cache_manager
            .lock()
            .map_err(|e| CoreError::from(format!("Failed to lock cache: {}", e)))?;
        manager.stats().metadata.hit_rate()
    };

    let cache_size = {
        let manager = cache_manager
            .lock()
            .map_err(|e| CoreError::from(format!("Failed to lock cache: {}", e)))?;
        manager.stats().metadata.entry_count
    };

    Ok(MemoryStatsResponse {
        cache_entries: stats.cache_entries as u32,
        pool_size: stats.pool_size as u32,
        estimated_memory_mb: stats.estimated_memory_mb,
        last_eviction_secs_ago,
        total_evictions: stats.total_evictions as u32,
        pressure_level: stats.pressure_level,
        cache_hit_rate,
        cache_size: cache_size as u32,
    })
}

/// 清理过期缓存
#[tauri::command]
#[specta::specta]
pub async fn cleanup_expired_cache() -> Result<u32, CoreError> {
    let cache_manager = CacheManager::instance();
    let manager = cache_manager
        .lock()
        .map_err(|e| CoreError::from(format!("Failed to lock cache: {}", e)))?;

    let cleaned = manager.cleanup_expired() as u32;
    Ok(cleaned)
}

/// 强制淘汰缓存
#[tauri::command]
#[specta::specta]
pub async fn force_evict_cache(_ratio: f64) -> Result<u32, CoreError> {
    let guard = get_memory_guard().ok_or_else(|| "Memory guard not initialized".to_string())?;

    let evicted = guard.check_and_evict().await? as u32;
    Ok(evicted)
}

/// 清除指定连接的所有缓存
#[tauri::command]
#[specta::specta]
pub async fn clear_connection_cache(conn_id: String) -> Result<(), CoreError> {
    let cache_manager = CacheManager::instance();
    let manager = cache_manager
        .lock()
        .map_err(|e| CoreError::from(format!("Failed to lock cache: {}", e)))?;

    manager.invalidate_connection(&conn_id);
    Ok(())
}

/// 清除所有缓存
#[tauri::command]
#[specta::specta]
pub async fn clear_all_cache() -> Result<(), CoreError> {
    let cache_manager = CacheManager::instance();
    let manager = cache_manager
        .lock()
        .map_err(|e| CoreError::from(format!("Failed to lock cache: {}", e)))?;

    manager.clear_all();
    Ok(())
}

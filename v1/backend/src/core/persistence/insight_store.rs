//! 洞察持久化存储（DuckDB 侧）
//!
//! 管理洞察快照的 JSON 序列化存储。
//! 数据存储在项目的 `analytics.duckdb` 中。
//!
//! 版本化：每条记录包含 version_id / parent_version_id / checksum，
//! 支持历史版本链和快照对比。
//!
//! 存储防护：
//! - 每列最多保留 MAX_VERSIONS_PER_COLUMN (100) 个版本
//! - 支持按天数清理过期快照 (cleanup_older_than)
//! - 提供存储用量查询

use std::sync::Arc;
use uuid::Uuid;

use crate::core::error::{CommonError, CoreError, StorageError};
use crate::core::persistence::project_db::ProjectDuckdbConnection;

use super::super::services::result_service::ColumnInsightFull;

const MAX_VERSIONS_PER_COLUMN: usize = 100;

// ==================== 列洞察快照存储 ====================

pub struct InsightColumnStore {
    duckdb: Arc<ProjectDuckdbConnection>,
}

impl InsightColumnStore {
    pub fn new(duckdb: Arc<ProjectDuckdbConnection>) -> Self {
        Self { duckdb }
    }

    pub async fn save_snapshot(
        &self,
        insight: &ColumnInsightFull,
        parent_version_id: Option<&str>,
    ) -> Result<(String, String), CoreError> {
        let column_name = insight.stats.column_name.clone();
        let data_type = insight.stats.data_type.clone();

        let existing_count = self.count_versions(&column_name).await?;
        if existing_count >= MAX_VERSIONS_PER_COLUMN {
            self.evict_oldest_version(&column_name, existing_count - MAX_VERSIONS_PER_COLUMN + 1)
                .await?;
        }

        let snapshot_id = Uuid::new_v4().to_string();
        let version_id = Uuid::new_v4().to_string();
        let stats_json = serde_json::to_string(insight).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Serialize insight failed: {}",
                e
            )))
        })?;
        let checksum = sha256_hex(&stats_json);

        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        conn.execute(
            "INSERT INTO insight_column_snapshots (snapshot_id, column_name, data_type, stats_json, version_id, parent_version_id, checksum, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
            duckdb::params![
                &snapshot_id,
                &column_name,
                &data_type,
                &stats_json,
                &version_id,
                &parent_version_id.map(|s| s.to_string()),
                &checksum,
            ],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "duckdb".to_string(),
            operation: "save_insight_snapshot".to_string(),
            reason: e.to_string(),
        }))?;

        Ok((snapshot_id.clone(), version_id))
    }

    pub async fn get_latest_snapshot(
        &self,
        column_name: &str,
    ) -> Result<Option<ColumnInsightFull>, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        let result: Option<String> = conn.query_row(
            "SELECT stats_json FROM insight_column_snapshots WHERE column_name = ? ORDER BY created_at DESC LIMIT 1",
            duckdb::params![column_name],
            |row| row.get(0),
        ).ok().flatten();

        match result {
            Some(json) => {
                let insight: ColumnInsightFull = serde_json::from_str(&json).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Deserialize insight failed: {}",
                        e
                    )))
                })?;
                Ok(Some(insight))
            }
            None => Ok(None),
        }
    }

    pub async fn get_history(
        &self,
        column_name: &str,
        limit: Option<usize>,
    ) -> Result<Vec<InsightVersionEntry>, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        let limit_val = limit.unwrap_or(20) as i64;
        let mut stmt = conn.prepare(
            "SELECT snapshot_id, column_name, data_type, stats_json, version_id, parent_version_id, checksum, created_at
             FROM insight_column_snapshots WHERE column_name = ? ORDER BY created_at DESC LIMIT ?"
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "duckdb".to_string(),
            operation: "get_insight_history".to_string(),
            reason: e.to_string(),
        }))?;

        let entries: Vec<InsightVersionEntry> = stmt
            .query_map(duckdb::params![column_name, limit_val], |row| {
                Ok(InsightVersionEntry {
                    snapshot_id: row.get(0)?,
                    column_name: row.get(1)?,
                    data_type: row.get(2)?,
                    stats_json: row.get::<_, String>(3)?,
                    version_id: row.get(4)?,
                    parent_version_id: row.get(5)?,
                    checksum: row.get(6)?,
                    created_at: row.get::<_, String>(7)?,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "duckdb".to_string(),
                    operation: "query_insight_history".to_string(),
                    reason: e.to_string(),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(entries)
    }

    pub async fn get_snapshot_by_version(
        &self,
        version_id: &str,
    ) -> Result<Option<ColumnInsightFull>, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        let result: Option<String> = conn
            .query_row(
                "SELECT stats_json FROM insight_column_snapshots WHERE version_id = ?",
                duckdb::params![version_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        match result {
            Some(json) => {
                let insight: ColumnInsightFull = serde_json::from_str(&json).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Deserialize insight failed: {}",
                        e
                    )))
                })?;
                Ok(Some(insight))
            }
            None => Ok(None),
        }
    }

    pub async fn delete_snapshot(&self, snapshot_id: &str) -> Result<(), CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        conn.execute(
            "DELETE FROM insight_column_snapshots WHERE snapshot_id = ?",
            duckdb::params![snapshot_id],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "duckdb".to_string(),
                operation: "delete_insight_snapshot".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    // ═══════════════════ 存储防护 ═══════════════════

    /// 统计某列的版本总数
    pub async fn count_versions(&self, column_name: &str) -> Result<usize, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM insight_column_snapshots WHERE column_name = ?",
                duckdb::params![column_name],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "duckdb".to_string(),
                    operation: "count_versions".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(count as usize)
    }

    /// 淘汰指定列最旧的 N 个版本（版本链保持不变）
    async fn evict_oldest_version(&self, column_name: &str, count: usize) -> Result<(), CoreError> {
        if count == 0 {
            return Ok(());
        }

        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        conn.execute(
            "DELETE FROM insight_column_snapshots WHERE snapshot_id IN (
                SELECT snapshot_id FROM insight_column_snapshots
                WHERE column_name = ? ORDER BY created_at ASC LIMIT ?
            )",
            duckdb::params![column_name, count as i64],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "duckdb".to_string(),
                operation: "evict_oldest_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    /// 清理 N 天前的洞察快照
    ///
    /// 返回清理的条目数
    pub async fn cleanup_older_than(&self, days: i64) -> Result<i64, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        let deleted = conn
            .execute(
                "DELETE FROM insight_column_snapshots
             WHERE created_at < (CURRENT_TIMESTAMP - INTERVAL ? DAY)",
                duckdb::params![days],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "duckdb".to_string(),
                    operation: "cleanup_older_than".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(deleted as i64)
    }

    /// 获取存储用量统计
    pub async fn get_storage_stats(&self) -> Result<InsightStorageStats, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            ))
        })?;

        let total_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM insight_column_snapshots", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        let unique_columns: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT column_name) FROM insight_column_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total_size_approx: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(stats_json)), 0)::DOUBLE FROM insight_column_snapshots",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        Ok(InsightStorageStats {
            total_snapshots: total_count as usize,
            unique_columns: unique_columns as usize,
            total_size_bytes: total_size_approx,
            total_size_display: format_storage_size(total_size_approx),
        })
    }
}

// ==================== 历史版本条目 ====================

#[derive(Debug, Clone, serde::Serialize)]
pub struct InsightVersionEntry {
    pub snapshot_id: String,
    pub column_name: String,
    pub data_type: Option<String>,
    pub stats_json: String,
    pub version_id: String,
    pub parent_version_id: Option<String>,
    pub checksum: String,
    pub created_at: String,
}

impl InsightVersionEntry {
    pub fn parse_insight(&self) -> Result<ColumnInsightFull, CoreError> {
        serde_json::from_str(&self.stats_json).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Parse insight from version entry failed: {}",
                e
            )))
        })
    }
}

// ==================== 存储用量统计 ====================

#[derive(Debug, Clone, serde::Serialize)]
pub struct InsightStorageStats {
    pub total_snapshots: usize,
    pub unique_columns: usize,
    pub total_size_bytes: f64,
    pub total_size_display: String,
}

// ==================== 表探查报告存储（Phase 2 预留） ====================

pub struct InsightTableReportStore {
    duckdb: Arc<ProjectDuckdbConnection>,
}

impl InsightTableReportStore {
    pub fn new(duckdb: Arc<ProjectDuckdbConnection>) -> Self {
        Self { duckdb }
    }

    pub async fn save_table_quality(
        &self,
        table_name: &str,
        report_json: &str,
    ) -> Result<(), CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General("DuckDB connection is closed".into()))
        })?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS insight_table_reports \
             (table_name TEXT PRIMARY KEY, report_json TEXT, saved_at TEXT)",
            [],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::write(
                "insight_table_reports",
                format!("Failed to create insight_table_reports: {}", e),
            ))
        })?;
        conn.execute(
            "INSERT OR REPLACE INTO insight_table_reports \
             (table_name, report_json, saved_at) VALUES (?, ?, CURRENT_TIMESTAMP)",
            duckdb::params![table_name, report_json],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::write(
                "insight_table_reports",
                format!("Failed to save table quality: {}", e),
            ))
        })?;
        Ok(())
    }

    pub async fn load_table_quality(&self, table_name: &str) -> Result<Option<String>, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General("DuckDB connection is closed".into()))
        })?;
        let result: Option<String> = conn
            .query_row(
                "SELECT report_json FROM insight_table_reports WHERE table_name = ?",
                duckdb::params![table_name],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        Ok(result)
    }
}

// ==================== Schema 洞察报告存储（Phase 3 预留） ====================

pub struct InsightSchemaReportStore {
    duckdb: Arc<ProjectDuckdbConnection>,
}

impl InsightSchemaReportStore {
    pub fn new(duckdb: Arc<ProjectDuckdbConnection>) -> Self {
        Self { duckdb }
    }

    pub async fn save_schema_insight(
        &self,
        schema_name: &str,
        report_json: &str,
    ) -> Result<(), CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General("DuckDB connection is closed".into()))
        })?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS insight_schema_reports \
             (schema_name TEXT PRIMARY KEY, report_json TEXT, saved_at TEXT)",
            [],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::write(
                "insight_schema_reports",
                format!("Failed to create insight_schema_reports: {}", e),
            ))
        })?;
        conn.execute(
            "INSERT OR REPLACE INTO insight_schema_reports \
             (schema_name, report_json, saved_at) VALUES (?, ?, CURRENT_TIMESTAMP)",
            duckdb::params![schema_name, report_json],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::write(
                "insight_schema_reports",
                format!("Failed to save schema insight: {}", e),
            ))
        })?;
        Ok(())
    }

    pub async fn load_schema_insight(
        &self,
        schema_name: &str,
    ) -> Result<Option<String>, CoreError> {
        let duckdb_conn = self.duckdb.acquire().await?;
        let mut guard = duckdb_conn.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General("DuckDB connection is closed".into()))
        })?;
        let result: Option<String> = conn
            .query_row(
                "SELECT report_json FROM insight_schema_reports WHERE schema_name = ?",
                duckdb::params![schema_name],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        Ok(result)
    }
}

// ==================== 统一洞察存储门面 ====================

pub struct InsightStorage {
    pub columns: InsightColumnStore,
    pub tables: InsightTableReportStore,
    pub schemas: InsightSchemaReportStore,
}

impl InsightStorage {
    pub fn new(duckdb: Arc<ProjectDuckdbConnection>) -> Self {
        Self {
            columns: InsightColumnStore::new(duckdb.clone()),
            tables: InsightTableReportStore::new(duckdb.clone()),
            schemas: InsightSchemaReportStore::new(duckdb),
        }
    }
}

// ==================== 工具函数 ====================

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn format_storage_size(bytes: f64) -> String {
    if bytes < 1024.0 {
        format!("{} B", bytes as u64)
    } else if bytes < 1024.0 * 1024.0 {
        format!("{:.1} KB", bytes / 1024.0)
    } else if bytes < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", bytes / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes / (1024.0 * 1024.0 * 1024.0))
    }
}

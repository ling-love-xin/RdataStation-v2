//! 日志持久化存储
//!
//! 使用全局 SQLite 数据库存储应用日志，支持批量写入和分页查询。
//! 遵循 persistence 层设计模式，与 global_db.rs 保持一致。

use crate::core::error::{CoreError, StorageError};
use crate::core::logging::record::{
    LogLevel, LogLevelCounts, LogPage, LogQuery, LogRecord, LogStats, TargetStat, TIMESTAMP_FMT,
};
use crate::core::persistence::GlobalSqlitePool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const DEFAULT_MAX_RECORDS: usize = 100_000;
const DEFAULT_RETENTION_DAYS: u32 = 7;
const CLEANUP_CHECK_INTERVAL: usize = 10;

pub struct LogStore {
    pool: Arc<GlobalSqlitePool>,
    max_records: usize,
    retention_days: u32,
    flush_count: AtomicUsize,
}

impl LogStore {
    pub fn new(pool: Arc<GlobalSqlitePool>) -> Self {
        Self {
            pool,
            max_records: DEFAULT_MAX_RECORDS,
            retention_days: DEFAULT_RETENTION_DAYS,
            flush_count: AtomicUsize::new(0),
        }
    }

    pub fn with_config(
        pool: Arc<GlobalSqlitePool>,
        max_records: usize,
        retention_days: u32,
    ) -> Self {
        Self {
            pool,
            max_records,
            retention_days,
            flush_count: AtomicUsize::new(0),
        }
    }

    fn sqlite_err(op: &str, reason: String) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: op.to_string(),
            reason,
        })
    }

    /// 从外部批量写入日志记录（供 subscriber consumer 调用）
    pub async fn flush_records(&self, records: &[LogRecord]) -> Result<(), CoreError> {
        self.flush_batch(records).await
    }

    /// 批量写入日志记录到 SQLite（带事务包裹）
    async fn flush_batch(&self, records: &[LogRecord]) -> Result<(), CoreError> {
        if records.is_empty() {
            return Ok(());
        }

        let conn = self.pool.acquire().await?;

        let result = (|| -> Result<(), CoreError> {
            conn.inner()?
                .execute_batch("BEGIN TRANSACTION")
                .map_err(|e| Self::sqlite_err("begin_tx", e.to_string()))?;

            let sql = "INSERT INTO app_logs (timestamp, level, target, message, fields, file, line, session_id) \
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

            for record in records {
                conn.inner()?
                    .execute(
                        sql,
                        rusqlite::params![
                            record.timestamp,
                            record.level.as_str(),
                            record.target,
                            record.message,
                            record.fields,
                            record.file,
                            record.line,
                            record.session_id,
                        ],
                    )
                    .map_err(|e| {
                        // 回滚事务
                        let _ = conn.inner().ok().map(|c| c.execute_batch("ROLLBACK"));
                        Self::sqlite_err("insert_log", e.to_string())
                    })?;
            }

            conn.inner()?
                .execute_batch("COMMIT")
                .map_err(|e| Self::sqlite_err("commit_tx", e.to_string()))?;

            Ok(())
        })();

        result?;

        // 懒清理：每 CLEANUP_CHECK_INTERVAL 次写入检查一次
        let count = self.flush_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(CLEANUP_CHECK_INTERVAL) {
            self.cleanup_old_records().await?;
        }

        Ok(())
    }

    /// 分页查询日志
    pub async fn query_logs(&self, query: &LogQuery) -> Result<LogPage, CoreError> {
        let conn = self.pool.acquire().await?;

        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(50).clamp(1, 500);
        let offset = (page - 1) * page_size;

        let (where_clause, params) = self.build_where_clause(query);

        let count_sql = format!("SELECT COUNT(*) FROM app_logs {}", where_clause);
        let total: usize = {
            let mut stmt = conn
                .inner()?
                .prepare(&count_sql)
                .map_err(|e| Self::sqlite_err("count_logs", e.to_string()))?;
            let params_refs: Vec<&dyn rusqlite::types::ToSql> = params
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            stmt.query_row(params_refs.as_slice(), |row| row.get(0))
                .map_err(|e| Self::sqlite_err("count_logs_query", e.to_string()))?
        };

        let query_sql = format!(
            "SELECT id, timestamp, level, target, message, fields, file, line, session_id \
             FROM app_logs {} ORDER BY timestamp DESC LIMIT ?{} OFFSET ?{}",
            where_clause,
            params.len() + 1,
            params.len() + 2,
        );

        let mut all_params = params;
        all_params.push(page_size.to_string());
        all_params.push(offset.to_string());

        let records = {
            let mut stmt = conn
                .inner()?
                .prepare(&query_sql)
                .map_err(|e| Self::sqlite_err("query_logs", e.to_string()))?;

            let param_refs: Vec<&dyn rusqlite::types::ToSql> = all_params
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(LogRecord {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        level: {
                            let level_str = row.get::<_, String>(2)?;
                            LogLevel::parse_level(&level_str).unwrap_or_else(|| {
                                tracing::warn!(
                                    "Invalid log level '{}' in database record id={}, defaulting to INFO",
                                    level_str,
                                    row.get::<_, i64>(0).unwrap_or(-1)
                                );
                                LogLevel::Info
                            })
                        },
                        target: row.get(3)?,
                        message: row.get(4)?,
                        fields: row.get(5)?,
                        file: row.get(6)?,
                        line: row.get(7)?,
                        session_id: row.get(8)?,
                    })
                })
                .map_err(|e| Self::sqlite_err("query_logs_rows", e.to_string()))?;

            let mut result = Vec::with_capacity(page_size as usize);
            for record in rows.flatten() {
                result.push(record);
            }
            result
        };

        Ok(LogPage {
            records,
            total: total as u32,
            page,
            page_size,
            total_pages: total as u32 / page_size
                + u32::from(!(total as u32).is_multiple_of(page_size)),
        })
    }

    /// 构建 WHERE 子句和参数
    fn build_where_clause(&self, query: &LogQuery) -> (String, Vec<String>) {
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(ref level) = query.level {
            conditions.push(format!("level = ?{}", params.len() + 1));
            params.push(level.as_str().to_string());
        }

        if let Some(ref target) = query.target {
            conditions.push(format!("target LIKE ?{}", params.len() + 1));
            params.push(format!("%{}%", target));
        }

        if let Some(ref keyword) = query.keyword {
            conditions.push(format!(
                "(message LIKE ?{0} OR target LIKE ?{0})",
                params.len() + 1
            ));
            params.push(format!("%{}%", keyword));
        }

        if let Some(ref start) = query.start {
            conditions.push(format!("timestamp >= ?{}", params.len() + 1));
            params.push(start.clone());
        }

        if let Some(ref end) = query.end {
            conditions.push(format!("timestamp <= ?{}", params.len() + 1));
            params.push(end.clone());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        (where_clause, params)
    }

    /// 获取日志统计（单次连接复用）
    pub async fn get_stats(&self) -> Result<LogStats, CoreError> {
        let conn = self.pool.acquire().await?;

        let result = (|| -> Result<LogStats, CoreError> {
            let total: usize = conn
                .inner()?
                .query_row("SELECT COUNT(*) FROM app_logs", [], |row| row.get(0))
                .map_err(|e| Self::sqlite_err("stats_total", e.to_string()))?;

            let by_level = {
                let mut stmt = conn
                    .inner()?
                    .prepare("SELECT level, COUNT(*) FROM app_logs GROUP BY level")
                    .map_err(|e| Self::sqlite_err("stats_by_level", e.to_string()))?;

                let mut counts = LogLevelCounts {
                    trace: 0,
                    debug: 0,
                    info: 0,
                    warn: 0,
                    error: 0,
                };
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
                    })
                    .map_err(|e| Self::sqlite_err("stats_level_rows", e.to_string()))?;

                for (level, count) in rows.flatten() {
                    match level.as_str() {
                        "TRACE" => counts.trace = count,
                        "DEBUG" => counts.debug = count,
                        "INFO" => counts.info = count,
                        "WARN" => counts.warn = count,
                        "ERROR" => counts.error = count,
                        _ => {}
                    }
                }
                counts
            };

            let by_target = {
                let mut stmt = conn
                    .inner()?
                    .prepare(
                        "SELECT target, COUNT(*) as cnt FROM app_logs GROUP BY target ORDER BY cnt DESC LIMIT 20",
                    )
                    .map_err(|e| Self::sqlite_err("stats_by_target", e.to_string()))?;

                let rows = stmt
                    .query_map([], |row| {
                        Ok(TargetStat {
                            target: row.get(0)?,
                            count: row.get(1)?,
                        })
                    })
                    .map_err(|e| Self::sqlite_err("stats_target_rows", e.to_string()))?;

                let mut result = Vec::new();
                for stat in rows.flatten() {
                    result.push(stat);
                }
                result
            };

            let first_timestamp: Option<String> = conn
                .inner()?
                .query_row(
                    "SELECT timestamp FROM app_logs ORDER BY timestamp ASC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();

            let last_timestamp: Option<String> = conn
                .inner()?
                .query_row(
                    "SELECT timestamp FROM app_logs ORDER BY timestamp DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();

            Ok(LogStats {
                total: total as u32,
                by_level,
                by_target,
                first_timestamp,
                last_timestamp,
            })
        })();

        result
    }

    /// 清理旧日志记录
    pub async fn cleanup(&self, before_timestamp: Option<&str>) -> Result<usize, CoreError> {
        let conn = self.pool.acquire().await?;

        let deleted = if let Some(ts) = before_timestamp {
            conn.inner()?
                .execute("DELETE FROM app_logs WHERE timestamp < ?1", [ts])
                .map_err(|e| Self::sqlite_err("cleanup_by_time", e.to_string()))?
        } else {
            conn.inner()?
                .execute(
                    "DELETE FROM app_logs WHERE id NOT IN (\
                 SELECT id FROM app_logs ORDER BY timestamp DESC LIMIT ?1)",
                    [self.max_records as i64],
                )
                .map_err(|e| Self::sqlite_err("cleanup_by_count", e.to_string()))?
        };

        tracing::info!("Cleaned up {} old log records", deleted);
        Ok(deleted)
    }

    /// 懒清理：双重策略保证日志不无限堆积
    ///
    /// 策略1（时间维度）：删除超过 retention_days 天的日志
    /// 策略2（数量维度）：超过 max_records * 1.2 时删除最旧记录
    /// 每 CLEANUP_CHECK_INTERVAL 次 flush 触发一次
    async fn cleanup_old_records(&self) -> Result<(), CoreError> {
        let conn = self.pool.acquire().await?;

        let result = (|| -> Result<(), CoreError> {
            // 策略1: 时间维度 —— 删除过期日志
            let cutoff = (chrono::Utc::now() - chrono::Duration::days(self.retention_days as i64))
                .format(TIMESTAMP_FMT)
                .to_string();

            let time_deleted = conn
                .inner()?
                .execute("DELETE FROM app_logs WHERE timestamp < ?1", [&cutoff])
                .map_err(|e| Self::sqlite_err("cleanup_by_time", e.to_string()))?;

            if time_deleted > 0 {
                tracing::info!(
                    "Cleaned up {} log records older than {} days",
                    time_deleted,
                    self.retention_days
                );
            }

            // 策略2: 数量维度 —— 超过上限 20% 时裁剪
            let count: usize = conn
                .inner()?
                .query_row("SELECT COUNT(*) FROM app_logs", [], |row| row.get(0))
                .map_err(|e| Self::sqlite_err("check_count", e.to_string()))?;

            if count > self.max_records * 120 / 100 {
                let excess = count - self.max_records;
                conn.inner()?
                    .execute(
                        "DELETE FROM app_logs WHERE id IN (\
                     SELECT id FROM app_logs ORDER BY timestamp ASC LIMIT ?1)",
                        [excess as i64],
                    )
                    .map_err(|e| Self::sqlite_err("auto_cleanup", e.to_string()))?;

                tracing::info!(
                    "Trimmed {} log records (count {} exceeds max {})",
                    excess,
                    count,
                    self.max_records
                );
            }

            Ok(())
        })();

        result
    }
}

impl Clone for LogStore {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            max_records: self.max_records,
            retention_days: self.retention_days,
            flush_count: AtomicUsize::new(self.flush_count.load(Ordering::Relaxed)),
        }
    }
}

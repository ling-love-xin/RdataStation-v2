//! SQLite / DuckDB 连接助手。

use super::*;

impl GlobalInstanceStore {
    /// 全局 SQLite 操作（懒加载打开全局库；同步段完成即放锁）。
    pub(super) fn with_sqlite<T>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut guard = self.sqlite.lock().map_err(|_| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "lock_sqlite".to_string(),
                reason: "全局登记 SQLite 锁中毒".to_string(),
            })
        })?;
        if guard.is_none() {
            let path = engine::migration::get_global_db_path()?;
            *guard = Some(rusqlite::Connection::open(&path).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources_global".to_string(),
                    operation: "open_sqlite".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        f(guard.as_ref().expect("刚打开过，必为 Some"))
    }

    /// 全局 DuckDB 操作（懒加载仅测试路径；生产连接由 `production()` 注入）。
    pub(super) async fn with_duckdb<T>(
        &self,
        f: impl FnOnce(&mut duckdb::Connection) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut guard = self.duckdb.lock().await;
        if guard.is_none() {
            let path = engine::migration::get_global_duckdb_path()?;
            // 格式守门（2026-11-01）：同上——先看魔数再看能不能连。
            shared::duckdb_open::assert_native_format(&path).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_duckdb_global".to_string(),
                    operation: "assert_native_format".to_string(),
                    reason: e.to_string(),
                })
            })?;
            *guard = Some(duckdb::Connection::open(&path).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_duckdb_global".to_string(),
                    operation: "open".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        f(guard.as_mut().expect("刚打开过，必为 Some"))
    }

    /// 全局 DuckDB 是否存在同名表（information_schema，DuckDB 标识符规范化为小写）。
    pub(super) async fn table_exists(&self, name: &str) -> Result<bool, CoreError> {
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare("SELECT count(*) FROM information_schema.tables WHERE table_name = ?1")
                .map_err(|e| duckdb_err("prepare_table_exists", &e.to_string()))?;
            let count: i64 = stmt
                .query_row(duckdb::params![name.to_lowercase()], |row| row.get(0))
                .map_err(|e| duckdb_err("query_table_exists", &e.to_string()))?;
            Ok(count > 0)
        })
        .await
    }

    pub(super) async fn drop_global_table(&self, name: &str) -> Result<(), CoreError> {
        self.with_duckdb(|conn| {
            conn.execute_batch(&format!("DROP TABLE IF EXISTS {};", quote_ident(name)))
                .map_err(|e| duckdb_err("drop_global_table", &e.to_string()))?;
            Ok(())
        })
        .await
    }

    /// 统计全局表行数 / 列数（升级与分发共用）。
    pub(super) async fn table_stats(&self, name: &str) -> Result<(Option<i32>, Option<i32>), CoreError> {
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT count(*) FROM {};", quote_ident(name)))
                .map_err(|e| duckdb_err("prepare_count", &e.to_string()))?;
            let rows: i64 = stmt
                .query_row([], |row| row.get(0))
                .map_err(|e| duckdb_err("query_count", &e.to_string()))?;
            let mut stmt = conn
                .prepare(&format!("DESCRIBE {};", quote_ident(name)))
                .map_err(|e| duckdb_err("prepare_describe", &e.to_string()))?;
            let columns = stmt
                .query_map([], |_| Ok(()))
                .map_err(|e| duckdb_err("query_describe", &e.to_string()))?
                .count();
            Ok((
                Some(rows.clamp(0, i32::MAX as i64) as i32),
                Some(columns as i32),
            ))
        })
        .await
    }

}

/// 按扩展名选 DuckDB read 表达式（白名单与 `engine/duckdb/file_reader.rs` 一致）。
///
/// 返回可直接嵌进 SQL 的表达式（含路径字面量，`'` 转义为 `''`）。
pub(crate) fn file_read_expr(abs_path: &Path) -> Option<String> {
    let ext = abs_path.extension()?.to_str()?.to_ascii_lowercase();
    let path_lit = abs_path.to_string_lossy().replace('\\', "/");
    let quoted = path_lit.replace('\'', "''");
    let expr = match ext.as_str() {
        "csv" | "tsv" | "txt" => format!("read_csv_auto('{quoted}')"),
        "parquet" => format!("read_parquet('{quoted}')"),
        "xlsx" | "xls" => format!("read_excel_auto('{quoted}')"),
        "json" | "ndjson" => format!("read_json_auto('{quoted}')"),
        _ => return None,
    };
    Some(expr)
}

/// 标识符引用（双引号 + 内部转义）；DuckDB 把标识符规范化为小写，读写同口径。
pub(crate) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(crate) fn sqlite_err(op: &str) -> impl Fn(rusqlite::Error) -> CoreError + '_ {
    move |e| {
        CoreError::storage(StorageError::Persistence {
            store: "analytics_resources_global".to_string(),
            operation: op.to_string(),
            reason: e.to_string(),
        })
    }
}

pub(crate) fn duckdb_err(op: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_duckdb_global".to_string(),
        operation: op.to_string(),
        reason: reason.to_string(),
    })
}


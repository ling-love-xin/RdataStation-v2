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

    /// 全局「同名链」的下一版号。
    ///
    /// 不过滤 `deleted_at`：覆盖路径会先软删旧登记，只看未删行就让覆盖退化成
    /// 「从 v1 重开」（版本链断掉）。与项目侧 `register_project_table` 同口径。
    pub(super) fn next_global_version(&self, name: &str) -> Result<i32, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'global'",
                rusqlite::params![name],
                |row| row.get::<_, i32>(0),
            )
            .map_err(sqlite_err("next_global_version"))
            .map(|max: i32| max + 1)
        })
    }

    /// 全局 DuckDB 里是否存在这张表。
    ///
    /// **参数是实际表名，不是资产名**（G2 起两者分开：资产名 `orders`、表名
    /// `res_orders_20261116…`）。标识符按小写比对：DuckDB 对未加引号的标识符做规范化，
    /// 我们建表与读取一律加引号，而 `information_schema` 里存原样名。
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

    /// 删掉指定 schema 下的全局表（快照在 `rds_archive`）。
    pub(super) async fn drop_global_archive_table(
        &self,
        schema: &str,
        name: &str,
    ) -> Result<(), CoreError> {
        self.with_duckdb(|conn| {
            conn.execute_batch(&format!(
                "DROP TABLE IF EXISTS {}.{}.{};",
                quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                quote_ident(schema),
                quote_ident(name)
            ))
            .map_err(|e| duckdb_err("drop_global_archive_table", &e.to_string()))?;
            Ok(())
        })
        .await
    }

    /// 同名链最新一版的登记 id（分发写完项目行后，用来记 `derived_from` 的上游）。
    ///
    /// 只看未软删行（分发拿到的就是当前那一版）；软删行是覆盖升级留下的历史，不参与。
    pub async fn latest_global_id(&self, name: &str) -> Result<Option<String>, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT id FROM analytics_resources \
                 WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL \
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![name],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_err("latest_global_id"))
        })
    }

    /// 按 `KeepVersions` 裁剪全局表快照的**历史版本**（只删物理快照，不删版本索引）。
    ///
    /// `keep` 数的是历史版本份数：`MetadataOnly`（0）只留当前快照，`Keep(5)` 留当前 +
    /// 最近 5 份历史。当前 head 永远不删；被项目 pin 的版本跳过。
    pub(super) async fn prune_version_tables(
        &self,
        name: &str,
        keep: crate::model::KeepVersions,
    ) -> Result<usize, CoreError> {
        let Some(head) = self.global_table_head(name)? else {
            return Ok(0);
        };
        let snapshot_versions: i64 = self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT count(*) FROM table_asset_versions WHERE asset_id = ?1",
                rusqlite::params![head.asset_id],
                |row| row.get(0),
            )
            .map_err(sqlite_err("count_prune_snapshot_versions"))
        })?;
        if snapshot_versions == 0 {
            return Ok(0);
        }
        let Some(keep) = keep.limit() else {
            return Ok(0);
        };
        let versions: Vec<(i32, String, String)> = self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT tav.version, ts.physical_schema, ts.physical_table
                    FROM table_asset_versions tav
                    JOIN table_snapshots ts ON ts.snapshot_id = tav.snapshot_id
                    WHERE tav.asset_id = ?
                    ORDER BY tav.version DESC
                    "#,
                )
                .map_err(sqlite_err("prepare_prune_versions"))?;
            let rows = stmt
                .query_map(rusqlite::params![head.asset_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .map_err(sqlite_err("query_prune_versions"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_prune_versions"))
        })?;

        let referenced = self.active_ref_versions(name)?;
        let referenced: std::collections::HashSet<i32> = referenced.into_iter().collect();

        let mut removed = 0usize;
        for (index, (version, schema, table)) in versions.into_iter().enumerate() {
            if index <= keep as usize || version == head.version {
                continue;
            }
            if referenced.contains(&version) {
                tracing::debug!(name, version, "全局快照仍被项目引用，跳过裁剪");
                continue;
            }
            match self.drop_global_archive_table(&schema, &table).await {
                Ok(()) => removed += 1,
                Err(error) => tracing::warn!(
                    error = %error,
                    table,
                    name,
                    "全局快照裁剪失败（保留策略这一次没生效，下次升级会再裁）"
                ),
            }
        }
        Ok(removed)
    }

    /// 统计全局表行数 / 列数（升级与分发共用；参数是实际表名，可带 `schema.table`）。
    pub(super) async fn table_stats(
        &self,
        name: &str,
    ) -> Result<(Option<i32>, Option<i32>), CoreError> {
        let ref_sql = qualified_ident(name);
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT count(*) FROM {ref_sql};"))
                .map_err(|e| duckdb_err("prepare_count", &e.to_string()))?;
            let rows: i64 = stmt
                .query_row([], |row| row.get(0))
                .map_err(|e| duckdb_err("query_count", &e.to_string()))?;
            let mut stmt = conn
                .prepare(&format!("DESCRIBE {ref_sql};"))
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

/// 指定 schema + 表名是否存在（快照在 `rds_archive`，不能用只认表名的旧检查）。
pub(crate) fn table_exists_in(conn: &duckdb::Connection, schema: &str, table: &str) -> bool {
    conn.prepare(
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = ?1 AND table_name = ?2",
    )
    .and_then(|mut stmt| stmt.query_row(duckdb::params![schema, table], |row| row.get::<_, i64>(0)))
    .map(|count| count > 0)
    .unwrap_or(false)
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

/// 标识符引用（双引号 + 内部转义）；可带 `schema.table`，逐段加引号。
pub(crate) fn qualified_ident(name: &str) -> String {
    name.split('.')
        .map(quote_ident)
        .collect::<Vec<_>>()
        .join(".")
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

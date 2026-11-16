//! 项目 → 全局升级：一版一张表（G2）+ 登记。

use super::*;

impl GlobalInstanceStore {
    /// 升级（项目 → 全局）：数据文件映射出表 / 脚本内容表，表 + 版本信息进全局。
    ///
    /// 语义（§13.2 / §13.5，2026-11-16 按 G2 改）：
    /// 1. 冲突检测**只查登记层**——一版一张表、表名带时间戳，全局表同名不再可能，
    ///    剩下的真冲突只有「同名资产已经有一版」，由调用方决定覆盖还是改名；
    /// 2. 先算下一版号，再按 `res_<描述>_<时间戳>` 建表（撞名则追加版本号）；
    /// 3. 全局登记（`scope = 'global'`，`analytics_table` 记实际表名）+ 全局版本快照；
    /// 4. 返回结果。
    ///
    /// **历史版本的表不删**：`analytics_table` 逐版记着各自那张表，这是「回到 v(n-1)
    /// 看数据」的唯一凭据；裁剪由 `KeepVersions` 那套策略管，不在这里顺手删。
    pub async fn promote(&self, req: &PromoteRequest) -> Result<PromoteOutcome, InstanceOpError> {
        // ---- 冲突检测（登记层；表层不再查）----
        let existing: Option<i32> = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT version FROM analytics_resources
                WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                ORDER BY version DESC LIMIT 1
                "#,
                rusqlite::params![req.name],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_err("check_global_conflict"))
        })?;
        if existing.is_some() {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Registry,
            });
        }

        // ---- 版本号 + 表名（一版一张表）----
        let next_version = self
            .next_global_version(&req.name)
            .map_err(InstanceOpError::Core)?;
        let now = Utc::now();
        let mut table = table_name::version_table_name(&req.name, now);
        if self.table_exists(&table).await? {
            // 同一秒内同名升级两次：时间戳撞了，按版本号区分（正常路径走不到）。
            table = table_name::version_table_name_with_version(&req.name, next_version, now);
        }

        // ---- 建全局表（表名 = `table`，不是资产名）----
        let (row_count, column_count) = match req.kind {
            PromoteKind::DataFile => {
                let read_expr = file_read_expr(&req.source_abs_path).ok_or_else(|| {
                    InstanceOpError::Core(CoreError::common(CommonError::InvalidArgument {
                        param: "source_abs_path".to_string(),
                        reason: format!(
                            "不支持的文件类型：{}（白名单：csv/tsv/txt/parquet/xlsx/xls/json/ndjson）",
                            req.source_abs_path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                        ),
                    }))
                })?;
                let create_sql = format!(
                    "CREATE TABLE {} AS SELECT * FROM {};",
                    quote_ident(&table),
                    read_expr
                );
                self.with_duckdb(|conn| {
                    conn.execute_batch(&create_sql)
                        .map_err(|e| duckdb_err("create_table_from_file", &e.to_string()))
                })
                .await?;
                self.table_stats(&table).await?
            }
            PromoteKind::Script => {
                // 先看体积再读内容：整段文本要进内存并落成全局库里的一条巨型行，
                // 超过上限时按「该当数据文件」处理，别把内存当缓存用。
                let size = std::fs::metadata(&req.source_abs_path)
                    .map_err(|e| {
                        InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                            "读取脚本信息失败（{}）：{}",
                            req.source_abs_path.display(),
                            e
                        ))))
                    })?
                    .len();
                if size > MAX_SCRIPT_BYTES {
                    return Err(InstanceOpError::Core(CoreError::common(
                        CommonError::General(format!(
                            "脚本 {} 有 {size} 字节，超过上限 {} MiB：全局库不承载大文本，请按数据文件归档",
                            req.rel_path.as_deref().unwrap_or(&req.name),
                            MAX_SCRIPT_BYTES / (1024 * 1024)
                        )),
                    )));
                }
                let content = std::fs::read_to_string(&req.source_abs_path).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "读取脚本内容失败（{}）：{}",
                        req.source_abs_path.display(),
                        e
                    ))))
                })?;
                let create_sql = format!(
                    "CREATE TABLE {} (\"filename\" TEXT, \"content\" TEXT, \"content_hash\" TEXT);",
                    quote_ident(&table)
                );
                let insert_sql = format!(
                    "INSERT INTO {} (\"filename\", \"content\", \"content_hash\") VALUES (?1, ?2, ?3);",
                    quote_ident(&table)
                );
                let write_result = self
                    .with_duckdb(|conn| {
                        conn.execute_batch(&create_sql)
                            .map_err(|e| duckdb_err("create_content_table", &e.to_string()))?;
                        conn.execute(
                            &insert_sql,
                            duckdb::params![
                                req.rel_path.as_deref().unwrap_or(""),
                                content,
                                req.content_hash
                            ],
                        )
                        .map_err(|e| duckdb_err("insert_content", &e.to_string()))?;
                        Ok(())
                    })
                    .await;
                // 半成品表要收掉：`CREATE` 成了、`INSERT` 败了会留一张**空内容表**，
                // 下次分发读到「应只有一行、实际 0 行」才暴露，排查成本很高。
                if write_result.is_err() {
                    let _ = self.drop_global_table(&table).await;
                }
                write_result?;
                (Some(1_i32), Some(3_i32))
            }
        };

        // ---- 全局登记（scope = 'global'，记实际表名）+ 版本快照 ----
        let outcome =
            self.register_global(&req.name, &table, next_version, req, row_count, column_count)?;
        // 一版一张表会随版本线性增长：按保留策略裁掉更早的表（只删内容表，
        // 登记与版本快照都留）。裁剪失败只记日志，不回滚已经成功的升级。
        if let Some(keep) = req.keep_versions {
            if let Err(error) = self.prune_version_tables(&req.name, keep).await {
                tracing::warn!(error = %error, name = %req.name, "全局历史表裁剪失败");
            }
        }
        Ok(outcome)
    }

    /// 覆盖式升级：软删旧登记，再走 [`promote`]。
    ///
    /// 覆盖语义（§13.2 冲突询问「覆盖」分支）：**旧表不删**。G2 起一版一张表，
    /// 旧版本那张表正是「回到上一版看数据」的凭据；删它就等于把历史版本清空，
    /// 与「可回溯」直接冲突。版本号接续同名链（`next_global_version` 不看 `deleted_at`）。
    pub async fn promote_overwrite(
        &self,
        req: &PromoteRequest,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        self.with_sqlite(|conn| {
            conn.execute(
                r#"
                UPDATE analytics_resources
                SET deleted_at = ?1
                WHERE name = ?2 AND scope = 'global' AND deleted_at IS NULL
                "#,
                rusqlite::params![Utc::now().to_rfc3339(), req.name],
            )
            .map_err(sqlite_err("soft_delete_global"))?;
            Ok(())
        })?;
        let mut outcome = self.promote(req).await?;
        outcome.overwritten = true;
        Ok(outcome)
    }
}

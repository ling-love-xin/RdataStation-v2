//! 项目 → 全局升级：冲突双查 + CREATE TABLE AS。

use super::*;

impl GlobalInstanceStore {
    /// 升级（项目 → 全局）：数据文件映射出表 / 脚本内容表，表 + 版本信息进全局。
    ///
    /// 语义（§13.2 / §13.5）：
    /// 1. 冲突检测（全局 DuckDB 表 + 全局登记行同名）；
    /// 2. 建全局表（数据文件 = `CREATE TABLE AS read_xxx`；脚本 = 内容表 + INSERT）；
    /// 3. 全局登记（scope = 'global'，kind = 'analysis'）+ 全局版本快照；
    /// 4. 返回结果（冲突未覆盖时返回 [`InstanceOpError::Conflict`]，由 UI 弹询问）。
    pub async fn promote(&self, req: &PromoteRequest) -> Result<PromoteOutcome, InstanceOpError> {
        tracing::info!(
            name = %req.name,
            kind = ?req.kind,
            rel_path = ?req.rel_path,
            "【探针】升级到全局 promote 进入"
        );
        // ---- 冲突检测（先查登记，再查 DuckDB 表，双保险）----
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

        let table_exists = self.table_exists(&req.name).await?;
        if table_exists && existing.is_none() {
            // 全局表在、登记不在：脏状态（索引修复同族）。不静默覆盖，按冲突走询问。
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Table,
            });
        }
        if existing.is_some() && !table_exists {
            // 登记在、表不在：同属脏状态，冲突路径覆盖时会重建表，安全。
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Registry,
            });
        }
        if existing.is_some() && table_exists {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::RegistryAndTable,
            });
        }

        // ---- 建全局表 ----
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
                    quote_ident(&req.name),
                    read_expr
                );
                self.with_duckdb(|conn| {
                    conn.execute_batch(&create_sql)
                        .map_err(|e| duckdb_err("create_table_from_file", &e.to_string()))
                })
                .await?;
                self.table_stats(&req.name).await?
            }
            PromoteKind::Script => {
                let content = std::fs::read_to_string(&req.source_abs_path).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "读取脚本内容失败（{}）：{}",
                        req.source_abs_path.display(),
                        e
                    ))))
                })?;
                let create_sql = format!(
                    "CREATE TABLE {} (\"filename\" TEXT, \"content\" TEXT, \"content_hash\" TEXT);",
                    quote_ident(&req.name)
                );
                let insert_sql = format!(
                    "INSERT INTO {} (\"filename\", \"content\", \"content_hash\") VALUES (?1, ?2, ?3);",
                    quote_ident(&req.name)
                );
                self.with_duckdb(|conn| {
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
                .await?;
                (Some(1_i32), Some(3_i32))
            }
        };

        // ---- 全局登记（scope = 'global'）+ 版本快照 ----
        let outcome = self.register_global(&req.name, req, row_count, column_count)?;
        Ok(outcome)
    }

    /// 覆盖式升级：删掉全局旧表 + 软删旧登记，再走 [`promote`]。
    ///
    /// 覆盖语义（§13.2 冲突询问「覆盖」分支）：旧表删除、登记行软删，新升级
    /// 成为新版本（版本号接续旧链）。
    pub async fn promote_overwrite(
        &self,
        req: &PromoteRequest,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        tracing::info!(name = %req.name, "【探针】升级到全局 promote_overwrite 进入（覆盖）");
        self.drop_global_table(&req.name).await?;
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

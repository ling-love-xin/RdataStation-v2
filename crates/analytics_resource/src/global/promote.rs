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

        // ---- 文件本体：先保旧版，再原子替换当前副本 ----
        //
        // 表型一版一行，但文件历史目录要跨版本稳定，所以目录键由资产名推（`payload::asset_key`），
        // 不用每次换了 id 的行 id。旧当前文件在替换前补进旧版本目录，失败不留「旧内容被覆盖、
        // 历史又没副本」的洞。
        let key = payload::asset_key(&req.name);
        let previous: Option<(i32, String)> = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT version, file_rel_path FROM analytics_resources
                WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                  AND file_rel_path IS NOT NULL
                ORDER BY version DESC LIMIT 1
                "#,
                rusqlite::params![req.name],
                |row| Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(sqlite_err("read_previous_file"))
        })?;
        if let Some((old_version, old_rel)) = previous {
            let old_abs = payload::resources_dir(&self.system_dir).join(&old_rel);
            payload::store_version_copy(&self.system_dir, &key, old_version, &old_abs, &old_rel)
                .await?;
        }
        let file = payload::stage_and_commit(
            &req.source_abs_path,
            &self.system_dir,
            req.rel_path.as_deref(),
            &req.name,
        )
        .await?;

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
        let outcome = self.register_global(
            &req.name,
            &table,
            next_version,
            req,
            row_count,
            column_count,
            None,
            Some(&file),
        )?;
        payload::store_version_copy(
            &self.system_dir,
            &key,
            next_version,
            &file.abs,
            &file.rel,
        )
        .await?;
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

    /// 从全局文件历史版本恢复：旧副本校验 hash → 生成新版本（不原地回滚）。
    ///
    /// 与项目文件型“从历史还原”同构：恢复 = 以旧字节为源再走一次升级（新表 / 新内容表、
    /// 新登记行、版本号 +1），旧版本与其副本一律保留。副本已被裁剪或 hash 不符时明确失败。
    pub async fn restore_global_file_version(
        &self,
        name: &str,
        version: i32,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let active: Option<(String, String, Option<i64>, Option<String>)> =
            self.with_sqlite(|conn| {
                conn.query_row(
                    r#"
                    SELECT kind, file_rel_path, file_size, origin_ref
                    FROM analytics_resources
                    WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                      AND file_rel_path IS NOT NULL
                    ORDER BY version DESC LIMIT 1
                    "#,
                    rusqlite::params![name],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(sqlite_err("read_restore_active"))
            })?;
        let Some((kind, rel, file_size, origin_ref)) = active else {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        };
        let (source_id, source_version) = origin_ref
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .map(|value| {
                (
                    value
                        .get("resource_id")
                        .and_then(|item| item.as_str())
                        .map(str::to_string),
                    value.get("version").and_then(|item| item.as_i64()).map(|v| v as i32),
                )
            })
            .unwrap_or((None, None));

        // 版本快照给 hash（恢复前必须确认副本字节就是那一版，不用当前内容冒充）。
        let snapshot: Option<(String,)> = self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT snapshot FROM analytics_resource_versions \
                 WHERE resource_id = (SELECT id FROM analytics_resources \
                    WHERE name = ?1 AND scope = 'global' AND version = ?2 LIMIT 1) \
                   AND version = ?2",
                rusqlite::params![name, version],
                |row| Ok((row.get::<_, String>(0)?,)),
            )
            .optional()
            .map_err(sqlite_err("read_restore_snapshot"))
        })?;
        let Some((snapshot_text,)) = snapshot else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{version} 的版本快照不存在，无法恢复"
                )),
            )));
        };
        let snapshot_json: serde_json::Value = serde_json::from_str(&snapshot_text).map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "全局「{name}」v{version} 的版本快照损坏：{e}"
            ))))
        })?;
        let expected_hash = snapshot_json
            .get("content_hash")
            .and_then(|value| value.as_str())
            .filter(|hash| !hash.is_empty())
            .map(str::to_string);

        let key = payload::asset_key(name);
        let copy = payload::version_copy(&self.system_dir, &key, version, &rel).ok_or_else(|| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "全局「{name}」v{version} 的内容副本已被裁剪或缺失，无法恢复"
            ))))
        })?;
        if let Some(expected) = expected_hash.as_deref() {
            let actual = crate::payload::PayloadStore::content_hash_sync(&copy)?;
            if actual != expected {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::General(format!(
                        "全局「{name}」v{version} 的内容副本指纹不符（登记 {expected}，实际 {actual}），拒绝恢复"
                    )),
                )));
            }
        }

        let req = PromoteRequest {
            name: name.to_string(),
            kind: if kind == "file" {
                PromoteKind::Script
            } else {
                PromoteKind::DataFile
            },
            source_abs_path: copy,
            rel_path: Some(rel),
            content_hash: expected_hash.unwrap_or_default(),
            file_size,
            source_resource_id: source_id,
            source_version,
            source_table: None,
            keep_versions: None,
        };
        self.promote_overwrite(&req).await
    }

    /// 表型升级：项目分析库里的表直接 CTAS 到全局库（不经 Parquet）。
    ///
    /// 与 [`Self::promote`] 的区别只在数据面：源是 `analytics.<源表>`，目标在同一根连接的
    /// `rds_global.<版本表>`。版本号 / 表名 / 登记 / 裁剪与文件型升级共用同一套规则。
    pub async fn promote_table_from_project(
        &self,
        project: &ProjectDuckConn,
        req: &PromoteRequest,
        overwrite: bool,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let Some(source_table) = req.source_table.as_deref() else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General("升级请求缺少源表名".to_string()),
            )));
        };

        // ---- 冲突 / 覆盖（与文件型升级同口径：只看登记层）----
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
        if overwrite {
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
        } else if existing.is_some() {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Registry,
            });
        }

        // ---- 版本号 + 表名 ----
        let next_version = self
            .next_global_version(&req.name)
            .map_err(InstanceOpError::Core)?;
        let now = Utc::now();
        let mut table = table_name::version_table_name(&req.name, now);
        if self.table_exists(&table).await? {
            table = table_name::version_table_name_with_version(&req.name, next_version, now);
        }

        // ---- 同根跨库 CTAS：项目会话直接读全局目标 ----
        let source_ref = format!(
            "{}.{}",
            quote_ident(shared::duckdb_root::PROJECT_ALIAS),
            quote_ident(source_table)
        );
        let create_sql = format!(
            "CREATE TABLE {}.{} AS SELECT * FROM {source_ref};",
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&table)
        );
        let write_result = {
            let mut guard = project.0.lock().await;
            match guard.as_mut() {
                Some(conn) => conn.execute_batch(&create_sql).map_err(|e| {
                    InstanceOpError::Core(duckdb_err(
                        "create_global_table_from_project",
                        &e.to_string(),
                    ))
                }),
                None => Err(InstanceOpError::Core(CoreError::common(
                    CommonError::General("项目 DuckDB 连接已关闭".to_string()),
                ))),
            }
        };
        if let Err(error) = write_result {
            // CTAS 半途失败可能留下目标表；尽力清掉，不让登记指向残表。
            let _ = self.drop_global_table(&table).await;
            return Err(error);
        }
        let (row_count, column_count) = self.table_stats(&table).await?;

        // 表型来源不是文件：重建定义直接写跨库 SELECT，避免登记成临时路径。
        let definition = Some(format!(
            "CREATE TABLE {} AS SELECT * FROM analytics.{};",
            quote_ident(&table),
            quote_ident(source_table)
        ));
        let mut outcome = self.register_global(
            &req.name,
            &table,
            next_version,
            req,
            row_count,
            column_count,
            definition,
            None,
        )?;
        outcome.overwritten = overwrite;
        if let Some(keep) = req.keep_versions {
            if let Err(error) = self.prune_version_tables(&req.name, keep).await {
                tracing::warn!(error = %error, name = %req.name, "全局历史表裁剪失败");
            }
        }
        Ok(outcome)
    }
}

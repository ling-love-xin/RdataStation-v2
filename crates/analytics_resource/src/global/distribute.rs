//! 全局 → 项目分发：COPY parquet 中转 → CTAS。

use super::*;

impl GlobalInstanceStore {
    /// 分发（全局 → 项目）：读全局登记 + 全局 DuckDB 表，落到当前项目库。
    ///
    /// 语义（§13.6）：跨项目唯一法定路径 = 项目 → 全局 → 项目，项目 ↔ 项目禁止直传。
    /// 形态判定：全局行 `file_rel_path` 非空 → 脚本内容表，分发**重建为文件**
    /// （`resources/<rel_path>`，File 档）；否则 → 建项目分析表（Analysis 档）。
    ///
    /// 连接纪律：项目 DuckDB 是单写者，本方法在调用方（后台 job 工作线程）已打开
    /// 项目写连接的前提下执行——打开动作由调用方负责（`ProjectDatabaseManager::open`），
    /// 避免与宿主主写连接并发。
    pub async fn distribute(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
        target: Option<&str>,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        tracing::info!(name = %name, target = ?target, "【探针】从全局分发 distribute 进入");
        // 项目侧落库名：未填改名 = 沿用全局名（§13.6 冲突询问「改名」分支才带新名）。
        let target_name = target.unwrap_or(name).to_string();

        // ---- 读全局登记行（最新版本）----
        let global_row = self.with_sqlite(|conn| {
            let row = conn
                .query_row(
                    r#"
                    SELECT id, version, kind, content_hash, file_rel_path, definition_sql,
                           row_count, column_count, file_size
                    FROM analytics_resources
                    WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                    ORDER BY version DESC LIMIT 1
                    "#,
                    rusqlite::params![name],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i32>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<i32>>(6)?,
                            row.get::<_, Option<i32>>(7)?,
                            row.get::<_, Option<i64>>(8)?,
                        ))
                    },
                )
                .optional()
                .map_err(sqlite_err("read_global_row"))?;
            Ok(row)
        })?;
        let (
            global_id,
            global_version,
            _kind,
            global_hash,
            rel_path,
            definition_sql,
            _global_rows,
            _global_cols,
            global_size,
        ) = global_row.ok_or_else(|| InstanceOpError::NotFound {
            name: name.to_string(),
        })?;

        // ---- 项目侧重名双保险探测（SQLite 未删登记 + DuckDB 表 + resources 文件）----
        // 与升级侧 `promote` 同口径：登记层与本体层任一命中都按冲突走询问，不静默覆盖。
        // 命中层名一并回传：冲突框要说清是登记、是表、还是文件。
        if let Some(field) =
            project_name_taken(project, project_root, &target_name, rel_path.as_deref()).await?
        {
            return Err(InstanceOpError::Conflict {
                name: target_name,
                field,
            });
        }

        // ---- 形态判定 + 落项目 ----
        let shape = if let Some(rel) = rel_path.as_deref() {
            // 脚本内容表 → 重建为文件（File 档）。
            let content = self
                .read_script_content(name)
                .await
                .map_err(InstanceOpError::Core)?;
            write_project_file(project_root, rel, &content, &target_name)
                .map_err(InstanceOpError::Core)?;
            let outcome = self
                .register_project_file(
                    project,
                    &target_name,
                    rel,
                    global_id.as_str(),
                    global_version,
                    global_hash.as_deref(),
                    global_size,
                )
                .await
                .map_err(InstanceOpError::Core)?;
            outcome
        } else {
            // 分析表 / 数据文件映射表 → 建项目表（Analysis 档）。
            // 数据搬运路径：全局连接 `COPY ... TO parquet` → 项目连接
            // `CREATE TABLE AS SELECT read_parquet(...)`。
            // 为什么不用 ATTACH：Windows 单句柄限制下，项目连接 ATTACH 全局 DuckDB
            // 文件会撞「文件被占用」（全局写连接由 GlobalDatabaseManager 持有，不能关）。
            // Parquet 是 DuckDB 原生格式，中转确定性、无锁冲突。
            let tmp = std::env::temp_dir().join(format!(
                "rds_dist_{}.parquet",
                uuid::Uuid::new_v4().simple()
            ));
            let copy_sql = format!(
                "COPY {} TO '{}' (FORMAT PARQUET);",
                quote_ident(name),
                tmp.to_string_lossy().replace('\\', "/")
            );
            let copy_result = self
                .with_duckdb(|conn| {
                    conn.execute_batch(&copy_sql)
                        .map_err(|e| duckdb_err("distribute_copy_out", &e.to_string()))?;
                    Ok(())
                })
                .await;
            // COPY 失败时 `?` 会提前返回，后面那句「用后即删」就执行不到——
            // 先把可能已生成的半截 parquet 收掉，别往系统临时目录漏文件。
            if copy_result.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            copy_result?;

            let create_sql = format!(
                "CREATE TABLE {} AS SELECT * FROM read_parquet('{}');",
                quote_ident(&target_name),
                tmp.to_string_lossy().replace('\\', "/")
            );
            let mut guard = project.0.lock().await;
            let conn = guard.as_mut().ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(
                    "项目 DuckDB 连接已关闭".to_string(),
                )))
            })?;
            let project_result = conn
                .execute_batch(&create_sql)
                .map_err(|e| duckdb_err("distribute_create_table", &e.to_string()));
            drop(guard);

            // 临时 parquet 用后即删（失败也尝试清理，不污染系统临时目录）。
            let _ = std::fs::remove_file(&tmp);
            project_result?;

            let (rows, cols) = project_table_stats(&project, &target_name).await?;
            self.register_project_table(
                project,
                &target_name,
                global_id.as_str(),
                global_version,
                global_hash.as_deref(),
                definition_sql.as_deref(),
                rows,
                cols,
            )
            .await
            .map_err(InstanceOpError::Core)?
        };

        Ok(shape)
    }

    /// 覆盖式分发：删项目旧表 / 旧文件 + 软删项目登记，再走 [`distribute`]。
    ///
    /// 覆盖语义（§13.2 冲突询问「覆盖」分支）：项目侧旧本体移除、登记软删，
    /// 新分发接续项目版本链（version = 旧 max + 1）。
    pub async fn distribute_overwrite(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
        target: Option<&str>,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        tracing::info!(name = %name, target = ?target, "【探针】从全局分发 distribute_overwrite 进入（覆盖）");
        let target_name = target.unwrap_or(name).to_string();

        // 项目侧旧本体移除（表或文件都删；文件在 resources/ 下，删前先探存在）。
        {
            let mut guard = project.0.lock().await;
            if let Some(conn) = guard.as_mut() {
                conn.execute_batch(&format!(
                    "DROP TABLE IF EXISTS {};",
                    quote_ident(&target_name)
                ))
                .map_err(|e| duckdb_err("drop_project_table", &e.to_string()))?;
            }
        }
        if let Some(rel) = self
            .with_sqlite(|conn| {
                Ok(conn
                    .query_row(
                        "SELECT file_rel_path FROM analytics_resources WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL ORDER BY version DESC LIMIT 1",
                        rusqlite::params![name],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map_err(sqlite_err("read_global_rel"))?)
            })?
            .flatten()
        {
            let path = project_root.join("resources").join(&rel);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "删除项目旧文件失败（{}）：{}",
                        path.display(),
                        e
                    ))))
                })?;
            }
        }
        // 软删项目旧登记（按目标名；不清掉旧链，下面 `distribute` 的重名探测会命中未删旧行）。
        {
            let pool = project.1.clone();
            let conn = pool.acquire().await.map_err(InstanceOpError::Core)?;
            let inner = conn.inner().map_err(InstanceOpError::Core)?;
            inner
                .execute(
                    "UPDATE analytics_resources SET deleted_at = ?1 \
                     WHERE name = ?2 AND scope = 'project' AND deleted_at IS NULL",
                    rusqlite::params![Utc::now().to_rfc3339(), target_name],
                )
                .map_err(sqlite_err("soft_delete_project"))?;
        }
        self.distribute(project, project_root, name, Some(&target_name))
            .await
    }

    // ==================== 分发内部 ====================

}

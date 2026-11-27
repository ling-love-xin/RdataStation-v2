//! 全局 → 项目分发：同根跨库 CTAS。

use super::*;

/// 分发需要的一条全局登记行（id / version / kind / hash / rel_path / definition /
/// row_count / column_count / file_size / analytics_table）。
pub(super) type DistributionRow = (
    String,
    i32,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<i64>,
    Option<String>,
    Option<String>,
    // 表型快照的物理 schema；文件型 / 旧数据文件表为 `None`。
    Option<String>,
);

fn map_distribution_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DistributionRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

impl GlobalInstanceStore {
    /// 读分发要用的全局登记行。
    ///
    /// `selected_version = None` 取当前存活最新版；`Some(v)` 取历史版本，
    /// **不过滤 `deleted_at`**——引用 pin 的旧版可能就是被覆盖升级软删掉的那一行。
    pub(super) fn read_global_row(
        &self,
        name: &str,
        selected_version: Option<i32>,
    ) -> Result<Option<DistributionRow>, CoreError> {
        self.with_sqlite(|conn| {
            let kind: Option<String> = conn
                .query_row(
                    r#"
                    SELECT kind FROM analytics_resources
                    WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                    ORDER BY version DESC LIMIT 1
                    "#,
                    rusqlite::params![name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sqlite_err("read_global_row_kind"))?;
            let Some(kind) = kind else {
                return Ok(None);
            };
            if kind == "analysis" {
                let row = conn
                    .query_row(
                        r#"
                        SELECT ar.id, tav.version, ar.kind, ar.content_hash, ar.file_rel_path,
                               ar.definition_sql, tm.row_count, tm.column_count, ar.file_size,
                               ts.physical_table, tav.manifest_id, ts.physical_schema
                        FROM analytics_resources ar
                        JOIN table_asset_versions tav
                          ON tav.asset_id = ar.id
                         AND tav.version = COALESCE(?2, ar.version)
                        JOIN table_snapshots ts ON ts.snapshot_id = tav.snapshot_id
                        JOIN table_manifests tm ON tm.manifest_id = tav.manifest_id
                        WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.deleted_at IS NULL
                        LIMIT 1
                        "#,
                        rusqlite::params![name, selected_version],
                        map_distribution_row,
                    )
                    .optional()
                    .map_err(sqlite_err("read_global_table_row"))?;
                // 表资产必须命中快照版本链；没有索引说明数据已丢失，不再回退旧单行。
                return Ok(row);
            }
            let row = match selected_version {
                Some(version) => conn
                    .query_row(
                        r#"
                        SELECT ar.id, ar.version, ar.kind, ar.content_hash, ar.file_rel_path,
                               ar.definition_sql, ar.row_count, ar.column_count, ar.file_size,
                               ar.analytics_table, fav.manifest_id, NULL
                        FROM analytics_resources ar
                        LEFT JOIN file_asset_versions fav
                          ON fav.resource_id = ar.id AND fav.version = ar.version
                        WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.version = ?2
                        LIMIT 1
                        "#,
                        rusqlite::params![name, version],
                        map_distribution_row,
                    )
                    .optional()
                    .map_err(sqlite_err("read_global_row_version"))?,
                None => conn
                    .query_row(
                        r#"
                        SELECT ar.id, ar.version, ar.kind, ar.content_hash, ar.file_rel_path,
                               ar.definition_sql, ar.row_count, ar.column_count, ar.file_size,
                               ar.analytics_table, fav.manifest_id, NULL
                        FROM analytics_resources ar
                        LEFT JOIN file_asset_versions fav
                          ON fav.resource_id = ar.id AND fav.version = ar.version
                        WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.deleted_at IS NULL
                        ORDER BY ar.version DESC LIMIT 1
                        "#,
                        rusqlite::params![name],
                        map_distribution_row,
                    )
                    .optional()
                    .map_err(sqlite_err("read_global_row"))?,
            };
            Ok(row)
        })
    }

    /// 分发（全局 → 项目）：读全局登记 + 全局 DuckDB 表，落到当前项目库。
    ///
    /// 语义（§13.6）：跨项目唯一法定路径 = 项目 → 全局 → 项目，项目 ↔ 项目禁止直传。
    /// 形态判定：全局行 `file_rel_path` 非空 → 脚本内容表，分发**重建为文件**
    /// （`resources/<rel_path>`，File 档）；否则 → 建项目表（`AnalyticsTable` 档）。
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
        self.distribute_inner(project, project_root, name, None, target)
            .await
    }

    /// 按**指定全局版本**分发到项目（固化引用版本用）。
    ///
    /// 与 [`Self::distribute`] 的唯一区别是版本来源：这里不取“当前最新”，
    /// 而是按 `name + global_version` 找登记行；目标名必须显式给出，避免覆盖项目同名资产。
    pub async fn distribute_version(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
        global_version: i32,
        target: &str,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        self.distribute_inner(
            project,
            project_root,
            name,
            Some(global_version),
            Some(target),
        )
        .await
    }

    async fn distribute_inner(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
        selected_version: Option<i32>,
        target: Option<&str>,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        // 项目侧落库名：未填改名 = 沿用全局名（§13.6 冲突询问「改名」分支才带新名）。
        let target_name = target.unwrap_or(name).to_string();

        // ---- 读全局登记行（最新版本，或调用方指定的历史版本）----
        let global_row = self.read_global_row(name, selected_version)?;
        let (
            global_id,
            global_version,
            global_kind,
            global_hash,
            rel_path,
            definition_sql,
            _global_rows,
            _global_cols,
            global_size,
            global_table,
            manifest_id,
            source_schema,
        ) = global_row.ok_or_else(|| InstanceOpError::NotFound {
            name: name.to_string(),
        })?;
        // 形态以登记 `kind` 为准：`file` 才按文件分发。数据文件升级后虽然也留了
        // 本体（用于版本恢复），它的共享形态仍是表，不能被 `file_rel_path` 误判。
        let is_file = global_kind == "file";

        // 源表：G2 起由登记行 `analytics_table` 给出实际表名；老行（迁移 036 之前）
        // 那列是空的，回退到资产名——那时表名就等于资产名。
        let source_table = global_table.unwrap_or_else(|| name.to_string());

        // 内容表可能已被 `KeepVersions` 裁掉（登记行还在、表没了）。
        // 不前置检查的话，后面的跨库 CTAS 会抛 DuckDB 的 Catalog Error，
        // 用户看不出这是「策略清掉的」而不是「数据坏了」。
        // 文件-first 的新行没有 analytics_table；只有旧脚本内容表 / 表型才查表。
        if !(is_file && manifest_id.is_some()) {
            let exists = if let Some(schema) = source_schema.as_deref() {
                self.with_duckdb(|conn| Ok(table_exists_in(conn, schema, &source_table)))
                    .await?
            } else {
                self.table_exists(&source_table).await?
            };
            if !exists {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::General(format!(
                        "全局实例「{name}」v{global_version} 的快照（{source_table}）已被保留策略清理：\
版本元数据仍在，可在全局面板查看版本链；要这一版的数据，在它的来源项目重新升级一版再分发"
                    )),
                )));
            }
        }

        // ---- 项目侧重名双保险探测（SQLite 未删登记 + DuckDB 表 + resources 文件）----
        // 与升级侧 `promote` 同口径：登记层与本体层任一命中都按冲突走询问，不静默覆盖。
        // 命中层名一并回传：冲突框要说清是登记、是表、还是文件。
        if let Some(field) = project_name_taken(
            project,
            project_root,
            &target_name,
            if is_file { rel_path.as_deref() } else { None },
        )
        .await?
        {
            return Err(InstanceOpError::Conflict {
                name: target_name,
                field,
            });
        }

        // ---- 形态判定 + 落项目 ----
        let shape = if is_file {
            let Some(rel) = rel_path.as_deref() else {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::General(format!(
                        "全局文件资产「{name}」没有登记本体相对路径，无法分发"
                    )),
                )));
            };
            // 文件-first：文件版本必须有 manifest，从全局 CAS 物化；没有 manifest
            // 的旧文件行不再支持分发。
            let Some(manifest_id) = manifest_id.as_deref() else {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::General(format!(
                        "全局文件资产「{name}」v{global_version} 没有 manifest：旧文件资产不再支持分发"
                    )),
                )));
            };
            let cas = crate::payload::cas::CasStore::at_root(self.system_dir.join("cas"));
            let manifest = cas
                .read_manifest(manifest_id)
                .await
                .map_err(InstanceOpError::Core)?;
            let entry = manifest.content_entry().ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "全局文件「{name}」v{global_version} 的 manifest 缺少 Content"
                ))))
            })?;
            let blob = cas.blob_path(&entry.hash).map_err(InstanceOpError::Core)?;
            let store = crate::payload::PayloadStore::new(project_root);
            let staged = store
                .stage_payload(&blob, rel)
                .await
                .map_err(InstanceOpError::Core)?;
            store
                .commit_staged(&staged, rel)
                .await
                .map_err(InstanceOpError::Core)?;
            let content_hash = global_hash.clone().unwrap_or(manifest.content_hash);
            let outcome = match self
                .register_project_file(
                    project,
                    &target_name,
                    rel,
                    global_id.as_str(),
                    global_version,
                    Some(content_hash.as_str()),
                    global_size,
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => {
                    // 本体已经从全局 CAS 物化到 `resources/`：登记失败必须把这份
                    // 无主文件收掉，否则项目里会留下“文件在、资产不在”的残留。
                    let dest = store
                        .resolve(rel)
                        .unwrap_or_else(|_| project_root.join(crate::payload::RESOURCES_DIR_NAME).join(rel));
                    if let Err(cleanup) = tokio::fs::remove_file(&dest).await {
                        if cleanup.kind() != std::io::ErrorKind::NotFound {
                            tracing::error!(
                                error = %cleanup,
                                path = %dest.display(),
                                "分发登记失败后清理已物化文件失败"
                            );
                        }
                    }
                    return Err(InstanceOpError::Core(error));
                }
            };
            outcome
        } else {
            // 分析表 / 数据文件映射表 → 建项目表（`AnalyticsTable` 档）。
            // 2026-11-26 所有权收口后全局库与项目库同处 `shared::duckdb_root` 一条根连接：
            // 项目会话直接读 `rds_global.<源表>` 建 `analytics.<目标表>`，不再 COPY 中转。
            let source_ref = match source_schema.as_deref() {
                Some(schema) => format!(
                    "{}.{}.{}",
                    quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                    quote_ident(schema),
                    quote_ident(&source_table)
                ),
                None => format!(
                    "{}.{}",
                    quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                    quote_ident(&source_table)
                ),
            };
            let create_sql = format!(
                "CREATE TABLE {}.{} AS SELECT * FROM {source_ref};",
                quote_ident(shared::duckdb_root::PROJECT_ALIAS),
                quote_ident(&target_name),
            );
            let mut guard = project.0.lock().await;
            let conn = guard.as_mut().ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(
                    "项目 DuckDB 连接已关闭".to_string(),
                )))
            })?;
            conn.execute_batch(&create_sql)
                .map_err(|e| duckdb_err("distribute_create_table", &e.to_string()))?;
            drop(guard);

            let (rows, cols) = match project_table_stats(&project, &target_name).await {
                Ok(stats) => stats,
                Err(error) => {
                    // 建表成功但统计失败：不留半成品表。
                    drop_project_table_if_exists(project, Some(&target_name)).await;
                    return Err(InstanceOpError::Core(error));
                }
            };
            match self
                .register_project_table(
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
            {
                Ok(shape) => shape,
                Err(error) => {
                    // 登记失败也回滚刚建的项目表，避免「表在、资产不在」。
                    drop_project_table_if_exists(project, Some(&target_name)).await;
                    return Err(InstanceOpError::Core(error));
                }
            }
        };

        // 落好了再记账：全局侧要看得到「我被哪些项目用过、用到第几版」。
        // 记失败不回滚分发（回执是附属记录，与履历事件同语义）。
        self.record_dispatch(
            &global_id,
            name,
            &target_name,
            project_root,
            shape.version,
            shape.shape,
        )
        .await;

        Ok(shape)
    }

    /// 覆盖式分发（§13.2 冲突询问「覆盖」分支）：**先把旧内容藏起来**，再走一次无冲突的分发。
    ///
    /// 覆盖要同时动三处介质——项目 DuckDB 表、`resources/` 文件、项目登记行——三处之间
    /// 没有共同事务可用。所以不"先删后建"（任何一步失败都会变成「旧内容已消失、新内容
    /// 没落」），改成每一步都可逆：
    ///
    /// 1. 同名表 `ALTER TABLE ... RENAME TO <备份名>`（同库内改名，单语句）；
    /// 2. 同名文件 `rename` 到 `.RSmeta/overwrite-backup/<uuid>/`（与 `resources/` 同卷）；
    /// 3. 同名未删登记行软删（记下 id，供回滚复活）；
    /// 4. 调 [`Self::distribute`]：三查都不再命中旧内容，走正常落库，版本号仍按同名链接续；
    /// 5. 成功 → 丢弃备份；任一步失败 → 按相反顺序搬回，再抛原始错误。
    ///
    /// 最坏情况只留下可清理的备份（备份表名带 `__rds_old_`、备份文件在 `.RSmeta` 下），
    /// 不会出现「旧内容没了、新内容也没落」。
    pub async fn distribute_overwrite(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
        target: Option<&str>,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        let target_name = target.unwrap_or(name).to_string();

        // 先确认全局登记在：不存在就不该动项目侧任何东西（藏起来是有代价的动作）。
        let Some(old_rel) = self.global_rel_for_overwrite(name).await? else {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        };

        let backup_table = hide_project_table(project, &target_name).await?;
        let backup_file = match old_rel.as_deref() {
            Some(rel) => hide_project_file(project_root, rel).await?,
            None => None,
        };
        let hidden_registry = soft_delete_project_registry(project, &target_name).await?;

        match self
            .distribute(project, project_root, name, Some(&target_name))
            .await
        {
            Ok(outcome) => {
                // 新内容已落库：备份只是清理工作，失败不影响正确性。
                drop_project_table_if_exists(project, backup_table.as_deref()).await;
                discard_backup_file(backup_file.as_deref()).await;
                Ok(outcome)
            }
            Err(error) => {
                // 回滚顺序与藏的顺序相反：登记先复活（成本最低，用户最先生效）。
                if !hidden_registry.is_empty() {
                    if let Err(rollback) = restore_project_registry(project, &hidden_registry).await
                    {
                        tracing::error!(error = %rollback, "覆盖分发回滚：复活登记行失败");
                    }
                }
                if let (Some(rel), Some(backup)) = (old_rel.as_deref(), backup_file.as_deref()) {
                    if let Err(rollback) = restore_project_file(project_root, rel, backup).await {
                        tracing::error!(error = %rollback, "覆盖分发回滚：旧文件搬回原位失败");
                    }
                }
                if let Some(backup) = backup_table.as_deref() {
                    if let Err(rollback) = rename_project_table(project, backup, &target_name).await
                    {
                        tracing::error!(error = %rollback, "覆盖分发回滚：旧表改名回原位失败");
                    }
                }
                Err(error)
            }
        }
    }

    /// 读全局登记的最新 `file_rel_path`（覆盖分发用）。
    ///
    /// 返回 `None` = 该名字没登记（覆盖前必须先判断，不然会白藏一遍项目侧内容）；
    /// `Some(None)` = 登记在但没有重建目标（表型资源）——与「没登记」是两件事。
    async fn global_rel_for_overwrite(
        &self,
        name: &str,
    ) -> Result<Option<Option<String>>, InstanceOpError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT file_rel_path FROM analytics_resources \
                 WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL \
                 ORDER BY version DESC LIMIT 1",
                rusqlite::params![name],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(sqlite_err("read_global_rel_for_overwrite"))
        })
        .map_err(InstanceOpError::Core)
    }

    // ==================== 分发内部 ====================

    /// 全部全局存档的使用面（按全局名聚合，一次查完）。
    ///
    /// 详情区要给每个全局面板行显示一行摘要，逐行查会变成 N+1；这里一次
    /// `GROUP BY global_name` 拿全表，调用方按名字取。
    pub async fn dispatch_usage_all(
        &self,
    ) -> Result<std::collections::HashMap<String, DispatchUsage>, CoreError> {
        self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT global_name,
                           count(*)                         AS project_count,
                           max(version)                     AS latest_version,
                           sum(dispatch_count)              AS total_dispatches,
                           max(last_distributed_at)         AS last_distributed_at
                    FROM analytics_resource_dispatch
                    GROUP BY global_name
                    "#,
                )
                .map_err(sqlite_err("prepare_dispatch_usage_all"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .map_err(sqlite_err("query_dispatch_usage_all"))?;
            let mut out = std::collections::HashMap::new();
            for row in rows {
                let (name, project_count, latest_version, total_dispatches, last) =
                    row.map_err(sqlite_err("read_dispatch_usage_all"))?;
                out.insert(
                    name,
                    DispatchUsage {
                        project_count: usize::try_from(project_count).unwrap_or(usize::MAX),
                        latest_version,
                        total_dispatches,
                        last_distributed_at: last,
                    },
                );
            }
            Ok(out)
        })
    }

    /// 某全局存档被哪些项目用过（按最近分发倒序）。
    ///
    /// 回答两个业务问题：「删掉这个全局资源之前，有项目依赖它吗？」「它被用到第几版？」
    /// 零行 = 只提升过、从没被分发（不是错误，是「全球库里有、没人用」这个事实）。
    pub async fn dispatched_projects(
        &self,
        global_name: &str,
    ) -> Result<Vec<DispatchRecord>, CoreError> {
        self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT project_root, target_name, version, shape, dispatch_count,
                           first_distributed_at, last_distributed_at
                    FROM analytics_resource_dispatch
                    WHERE global_name = ?1
                    ORDER BY last_distributed_at DESC
                    "#,
                )
                .map_err(sqlite_err("prepare_dispatched_projects"))?;
            let rows = stmt
                .query_map(rusqlite::params![global_name], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i32>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                })
                .map_err(sqlite_err("query_dispatched_projects"))?;
            let mut out = Vec::new();
            for row in rows {
                let (project_root, target_name, version, shape, dispatch_count, first, last) =
                    row.map_err(sqlite_err("read_dispatched_project"))?;
                out.push(DispatchRecord {
                    project_root,
                    target_name,
                    version,
                    // 写入端只会写 'table' / 'file'；脏值按表型处理而不是丢行。
                    shape: if shape == "file" {
                        DistributeShape::File
                    } else {
                        DistributeShape::Table
                    },
                    dispatch_count,
                    first_distributed_at: first,
                    last_distributed_at: last,
                });
            }
            Ok(out)
        })
    }

    /// 写一条分发回执（全局侧记账：这个全局存档被哪个项目用过、用到第几版）。
    ///
    /// 失败只记日志、不回滚分发：回执是附属记录（与 `record_timeline` 同语义），
    /// 少一条只影响「被谁用过」的可见性，不影响项目侧已经落好的内容。
    ///
    /// 项目身份取**项目根路径**（规范化 `\` → `/`）：`distribute` 的入参里只有它，
    /// 不为了记账去改一整串签名；同一项目的路径稳定，聚合键仍然成立。
    async fn record_dispatch(
        &self,
        global_id: &str,
        global_name: &str,
        target_name: &str,
        project_root: &Path,
        version: i32,
        shape: DistributeShape,
    ) {
        let root = project_root.to_string_lossy().replace('\\', "/");
        let shape = match shape {
            DistributeShape::Table => "table",
            DistributeShape::File => "file",
        };
        let result = self.with_sqlite(|conn| {
            conn.execute(
                r#"
                INSERT INTO analytics_resource_dispatch
                    (id, global_id, global_name, project_root, target_name, version, shape)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(global_id, project_root, target_name) DO UPDATE SET
                    global_name = excluded.global_name,
                    version = excluded.version,
                    shape = excluded.shape,
                    dispatch_count = analytics_resource_dispatch.dispatch_count + 1,
                    last_distributed_at = CURRENT_TIMESTAMP
                "#,
                rusqlite::params![
                    uuid::Uuid::new_v4().simple().to_string(),
                    global_id,
                    global_name,
                    root,
                    target_name,
                    version,
                    shape,
                ],
            )
            .map_err(sqlite_err("record_dispatch"))?;
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!(error = %error, global_name, "分发回执写入失败");
        }
    }
}

// ==================== 覆盖式分发的可逆备份（本文件私有）====================
//
// 三个介质（项目 DuckDB 表 / resources 文件 / 项目登记行）没有共同事务，
// 覆盖按「先藏、后换、失败搬回」做——每步都能单独撤回，最坏只留可清理的备份。

/// 备份目录：`{项目}/.RSmeta/overwrite-backup/<uuid>/`。
///
/// 必须与 `resources/` 同卷：`rename` 才是原子的，拷到别的盘再删源会留两截状态。
fn overwrite_backup_dir(project_root: &Path) -> PathBuf {
    project_root
        .join(crate::payload::META_DIR_NAME)
        .join("overwrite-backup")
}

fn short_uuid() -> String {
    let raw = uuid::Uuid::new_v4().simple().to_string();
    raw[..12].to_string()
}

fn fs_err(path: &Path, op: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources_payload".to_string(),
        operation: op.to_string(),
        reason: format!("{}：{}", path.display(), reason),
    })
}

fn project_conn_closed() -> InstanceOpError {
    InstanceOpError::Core(CoreError::common(CommonError::General(
        "项目 DuckDB 连接已关闭".to_string(),
    )))
}

async fn run_project_ddl(
    project: &ProjectDuckConn,
    sql: &str,
    op: &str,
) -> Result<(), InstanceOpError> {
    let mut guard = project.0.lock().await;
    let Some(conn) = guard.as_mut() else {
        return Err(project_conn_closed());
    };
    conn.execute_batch(sql)
        .map_err(|e| InstanceOpError::Core(duckdb_err(op, &e.to_string())))
}

async fn project_table_exists(
    project: &ProjectDuckConn,
    name: &str,
) -> Result<bool, InstanceOpError> {
    let mut guard = project.0.lock().await;
    let Some(conn) = guard.as_mut() else {
        return Err(project_conn_closed());
    };
    let count: i64 = conn
        .prepare("SELECT count(*) FROM information_schema.tables WHERE table_name = ?1")
        .map_err(|e| duckdb_err("prepare_overwrite_table_exists", &e.to_string()))?
        .query_row(duckdb::params![name.to_lowercase()], |row| row.get(0))
        .map_err(|e| duckdb_err("query_overwrite_table_exists", &e.to_string()))?;
    Ok(count > 0)
}

/// 把项目侧同名表改名藏起来（返回备份表名；表不存在 → `None`）。
async fn hide_project_table(
    project: &ProjectDuckConn,
    name: &str,
) -> Result<Option<String>, InstanceOpError> {
    if !project_table_exists(project, name).await? {
        return Ok(None);
    }
    let backup = format!("{}__rds_old_{}", name, short_uuid());
    let sql = format!(
        "ALTER TABLE {} RENAME TO {};",
        quote_ident(name),
        quote_ident(&backup)
    );
    run_project_ddl(project, &sql, "hide_project_table").await?;
    Ok(Some(backup))
}

async fn rename_project_table(
    project: &ProjectDuckConn,
    from: &str,
    to: &str,
) -> Result<(), InstanceOpError> {
    let sql = format!(
        "ALTER TABLE {} RENAME TO {};",
        quote_ident(from),
        quote_ident(to)
    );
    run_project_ddl(project, &sql, "rename_project_table").await
}

/// 清理备份表：新内容已落库，删备份失败只记日志（留着不影响正确性）。
async fn drop_project_table_if_exists(project: &ProjectDuckConn, backup: Option<&str>) {
    let Some(backup) = backup else {
        return;
    };
    let sql = format!("DROP TABLE IF EXISTS {};", quote_ident(backup));
    if let Err(error) = run_project_ddl(project, &sql, "drop_backup_table").await {
        tracing::error!(error = %error, table = %backup, "覆盖分发后清理备份表失败（可手工删除）");
    }
}

/// 把项目侧同名文件搬到备份目录（返回备份路径；文件不存在 → `None`）。
async fn hide_project_file(
    project_root: &Path,
    rel: &str,
) -> Result<Option<PathBuf>, InstanceOpError> {
    let source = project_root
        .join(crate::payload::RESOURCES_DIR_NAME)
        .join(rel);
    if !source.is_file() {
        return Ok(None);
    }
    let dir = overwrite_backup_dir(project_root).join(short_uuid());
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| InstanceOpError::Core(fs_err(&dir, "hide_create_dir", e)))?;
    let Some(file_name) = source.file_name() else {
        return Err(InstanceOpError::Core(fs_err(
            &source,
            "hide_file_name",
            "路径没有文件名",
        )));
    };
    let dest = dir.join(file_name);
    tokio::fs::rename(&source, &dest)
        .await
        .map_err(|e| InstanceOpError::Core(fs_err(&source, "hide_rename", e)))?;
    Ok(Some(dest))
}

async fn restore_project_file(
    project_root: &Path,
    rel: &str,
    backup: &Path,
) -> Result<(), InstanceOpError> {
    let dest = project_root
        .join(crate::payload::RESOURCES_DIR_NAME)
        .join(rel);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| InstanceOpError::Core(fs_err(parent, "restore_create_dir", e)))?;
    }
    tokio::fs::rename(backup, &dest)
        .await
        .map_err(|e| InstanceOpError::Core(fs_err(backup, "restore_rename", e)))?;
    // 搬回后把空掉的备份目录收掉（`remove_dir` 只删空目录，非空说明有别的残留，留着更好查）。
    discard_empty_backup_dirs(backup).await;
    Ok(())
}

/// 收掉空的备份目录：先删本批的 `<uuid>/`，再试删 `overwrite-backup/`
/// （非空（并发的另一批还在用）就留着，`remove_dir` 自己会失败，忽略即可）。
async fn discard_empty_backup_dirs(backup: &Path) {
    if let Some(dir) = backup.parent() {
        let _ = tokio::fs::remove_dir(dir).await;
        if let Some(root) = dir.parent() {
            let _ = tokio::fs::remove_dir(root).await;
        }
    }
}

/// 丢掉备份文件与它的备份目录（清理动作，失败只记日志）。
async fn discard_backup_file(backup: Option<&Path>) {
    let Some(backup) = backup else {
        return;
    };
    if let Err(error) = tokio::fs::remove_file(backup).await {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::error!(error = %error, path = %backup.display(), "覆盖分发后清理备份文件失败");
        }
    }
    discard_empty_backup_dirs(backup).await;
}

/// 软删项目侧同名未删登记行，返回被软删的 id（供失败回滚复活）。
///
/// 查 + 改同事务（`BEGIN IMMEDIATE`）：并发分发时不会出现「查到两条、只改到一条」。
async fn soft_delete_project_registry(
    project: &ProjectDuckConn,
    name: &str,
) -> Result<Vec<String>, InstanceOpError> {
    let pool = project.1.clone();
    let conn = pool.acquire().await.map_err(InstanceOpError::Core)?;
    let inner = conn.inner().map_err(InstanceOpError::Core)?;
    inner
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| InstanceOpError::Core(sqlite_err("begin_overwrite_registry")(e)))?;
    let result = (|| -> Result<Vec<String>, CoreError> {
        let ids: Vec<String> = {
            let mut stmt = inner
                .prepare(
                    "SELECT id FROM analytics_resources \
                     WHERE name = ?1 AND scope = 'project' AND deleted_at IS NULL",
                )
                .map_err(sqlite_err("prepare_overwrite_registry"))?;
            let rows = stmt
                .query_map(rusqlite::params![name], |row| row.get::<_, String>(0))
                .map_err(sqlite_err("query_overwrite_registry"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_overwrite_registry"))?
        };
        if !ids.is_empty() {
            let now = Utc::now().to_rfc3339();
            inner
                .execute(
                    "UPDATE analytics_resources SET deleted_at = ?1 \
                     WHERE name = ?2 AND scope = 'project' AND deleted_at IS NULL",
                    rusqlite::params![now, name],
                )
                .map_err(sqlite_err("soft_delete_overwrite_registry"))?;
        }
        Ok(ids)
    })();
    match result {
        Ok(ids) => {
            inner
                .execute_batch("COMMIT")
                .map_err(|e| InstanceOpError::Core(sqlite_err("commit_overwrite_registry")(e)))?;
            Ok(ids)
        }
        Err(error) => {
            let _ = inner.execute_batch("ROLLBACK");
            Err(InstanceOpError::Core(error))
        }
    }
}

/// 回滚：把刚软删的登记行复活（按 id 逐个改，不拼 SQL）。
async fn restore_project_registry(
    project: &ProjectDuckConn,
    ids: &[String],
) -> Result<(), InstanceOpError> {
    let pool = project.1.clone();
    let conn = pool.acquire().await.map_err(InstanceOpError::Core)?;
    let inner = conn.inner().map_err(InstanceOpError::Core)?;
    for id in ids {
        inner
            .execute(
                "UPDATE analytics_resources SET deleted_at = NULL \
                 WHERE id = ?1 AND scope = 'project'",
                rusqlite::params![id],
            )
            .map_err(|e| InstanceOpError::Core(sqlite_err("restore_overwrite_registry")(e)))?;
    }
    Ok(())
}

//! 全局 → 项目分发：COPY parquet 中转 → CTAS。

use super::*;

impl GlobalInstanceStore {
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
        
        // 项目侧落库名：未填改名 = 沿用全局名（§13.6 冲突询问「改名」分支才带新名）。
        let target_name = target.unwrap_or(name).to_string();

        // ---- 读全局登记行（最新版本）----
        let global_row = self.with_sqlite(|conn| {
            let row = conn
                .query_row(
                    r#"
                    SELECT id, version, kind, content_hash, file_rel_path, definition_sql,
                           row_count, column_count, file_size, analytics_table
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
                            row.get::<_, Option<String>>(9)?,
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
            global_table,
        ) = global_row.ok_or_else(|| InstanceOpError::NotFound {
            name: name.to_string(),
        })?;

        // 源表：G2 起由登记行 `analytics_table` 给出实际表名；老行（迁移 036 之前）
        // 那列是空的，回退到资产名——那时表名就等于资产名。
        let source_table = global_table.unwrap_or_else(|| name.to_string());

        // 内容表可能已被 `KeepVersions` 裁掉（登记行还在、表没了）。
        // 不前置检查的话，后面的 `COPY` 会抛 DuckDB 的 Catalog Error，
        // 用户看不出这是「策略清掉的」而不是「数据坏了」。
        if !self.table_exists(&source_table).await? {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局实例「{name}」v{global_version} 的内容表（{source_table}）已被保留策略清理：\
版本元数据仍在，可在全局面板查看版本链；要这一版的数据，在它的来源项目重新升级一版再分发"
                )),
            )));
        }

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
                .read_script_content(&source_table)
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
            // 分析表 / 数据文件映射表 → 建项目表（`AnalyticsTable` 档）。
            // 数据搬运路径：全局连接 `COPY ... TO parquet` → 项目连接
            // `CREATE TABLE AS SELECT read_parquet(...)`。
            // 为什么不用 ATTACH：Windows 单句柄限制下，项目连接 ATTACH 全局 DuckDB
            // 文件会撞「文件被占用」（全局写连接由 GlobalDatabaseManager 持有，不能关）。
            // Parquet 是 DuckDB 原生格式，中转确定性、无锁冲突。
            // 中转 parquet 落 `paths::temp_dir()`：运行时数据路径只有一个解析点
            // （rds-architecture），系统临时目录在只读盘 / 多用户 / 清理策略下不可控。
            let tmp_dir = paths::temp_dir();
            std::fs::create_dir_all(&tmp_dir).map_err(|e| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "创建临时目录失败（{}）：{}",
                    tmp_dir.display(),
                    e
                ))))
            })?;
            let tmp = tmp_dir.join(format!("rds_dist_{}.parquet", uuid::Uuid::new_v4().simple()));
            let copy_sql = format!(
                "COPY {} TO '{}' (FORMAT PARQUET);",
                quote_ident(&source_table),
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
                    if let Err(rollback) = restore_project_registry(project, &hidden_registry).await {
                        tracing::error!(error = %rollback, "覆盖分发回滚：复活登记行失败");
                    }
                }
                if let (Some(rel), Some(backup)) = (old_rel.as_deref(), backup_file.as_deref()) {
                    if let Err(rollback) = restore_project_file(project_root, rel, backup).await {
                        tracing::error!(error = %rollback, "覆盖分发回滚：旧文件搬回原位失败");
                    }
                }
                if let Some(backup) = backup_table.as_deref() {
                    if let Err(rollback) = rename_project_table(project, backup, &target_name).await {
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
    inner.execute_batch("BEGIN IMMEDIATE").map_err(|e| {
        InstanceOpError::Core(sqlite_err("begin_overwrite_registry")(e))
    })?;
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
            inner.execute_batch("COMMIT").map_err(|e| {
                InstanceOpError::Core(sqlite_err("commit_overwrite_registry")(e))
            })?;
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

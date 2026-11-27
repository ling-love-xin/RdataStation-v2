//! 项目 → 全局升级：表资产落不可变快照 + 登记。
//!
//! 表型走 [`GlobalInstanceStore::promote_table_from_project`]：源表冻结成
//! `rds_global.rds_archive.arc_*`，一条活跃登记行一个稳定 asset_id，版本链在
//! `table_asset_versions`。数据文件 / 文件资产仍走文件-first 的 `promote`。

use super::*;
use crate::service::table_asset::manifest_for_table;
use crate::store::table_asset::{NewTableSnapshot, SnapshotInstance};
use crate::table_asset::manifest::snapshot_table_name;

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
            PromoteKind::File => {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::InvalidArgument {
                        param: "kind".to_string(),
                        reason: "文件资产升级请走 publish_file（CAS），不能走表型 promote"
                            .to_string(),
                    },
                )));
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
        )?;
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

    /// 从全局文件历史版本恢复：从该版 manifest 生成新 head（不原地回滚）。
    ///
    /// 新版文件资产每一版都有 `file_asset_versions` 行；没有 manifest 的旧资产不再走
    /// 历史副本兼容路径，直接明确拒绝。
    pub async fn restore_global_file_version(
        &self,
        name: &str,
        version: i32,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let has_manifest = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT count(*) FROM analytics_resources ar
                JOIN file_asset_versions fav
                  ON fav.resource_id = ar.id AND fav.version = ar.version
                WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.version = ?2
                "#,
                rusqlite::params![name, version],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_err("check_restore_file_manifest"))
        })?;
        if has_manifest == 0 {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{version} 没有 manifest：旧文件资产不再支持版本恢复"
                )),
            )));
        }
        let outcome = self.restore_file_version(name, version).await?;
        Ok(PromoteOutcome {
            resource_id: outcome.resource_id,
            name: outcome.name,
            version: outcome.version,
            row_count: None,
            column_count: None,
            overwritten: true,
        })
    }

    /// 从全局表历史版本恢复：新版本直接指向旧快照，零复制（不原地回滚）。
    ///
    /// 快照不可变，恢复只是把 head 版本链再加一跳；旧版本索引与快照全部保留。
    /// 快照物理表已被清理时明确失败，不拿空表凑一版。
    pub async fn restore_global_table_version(
        &self,
        name: &str,
        version: i32,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let head = self
            .global_table_head(name)?
            .ok_or_else(|| InstanceOpError::NotFound {
                name: name.to_string(),
            })?;
        let target = self
            .global_table_version_row(&head.asset_id, version)?
            .ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "全局「{name}」v{version} 的版本索引不存在，无法恢复"
                ))))
            })?;
        let manifest = self
            .global_table_manifest(&target.manifest_id)?
            .ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "全局「{name}」v{version} 的 manifest 缺失，无法恢复"
                ))))
            })?;
        let live = self
            .with_duckdb(|conn| {
                Ok(table_exists_in(
                    conn,
                    &target.physical_schema,
                    &target.physical_table,
                ))
            })
            .await?;
        if !live {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{version} 的快照 {} 已不存在，无法恢复",
                    target.physical_table
                )),
            )));
        }

        let snapshot = NewTableSnapshot {
            snapshot_id: target.snapshot_id,
            instance: SnapshotInstance::Global,
            physical_schema: target.physical_schema,
            physical_table: target.physical_table.clone(),
        };
        let definition = Some(format!(
            "CREATE TABLE {}.{}.{} AS SELECT * FROM {}.{}.{};",
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&snapshot.physical_schema),
            quote_ident(&snapshot.physical_table),
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&snapshot.physical_schema),
            quote_ident(&snapshot.physical_table),
        ));
        let req = PromoteRequest {
            name: name.to_string(),
            kind: PromoteKind::DataFile,
            source_abs_path: std::path::PathBuf::from("rds_global.rds_archive"),
            rel_path: None,
            content_hash: target.manifest_id,
            file_size: None,
            source_resource_id: None,
            source_version: None,
            source_table: None,
            keep_versions: None,
        };
        self.append_global_table_version(
            &head,
            &req,
            definition.as_deref(),
            &manifest,
            &snapshot,
            true,
        )
    }

    /// 表型升级：项目分析库里的表冻结成全局不可变快照。
    ///
    /// - 首次升级：建一条活跃登记行 + v1；
    /// - 同名再升级：必须显式 `overwrite`，head 版本 +1，旧快照保留；
    /// - 内容完全相同：复用已有 manifest / 快照，不白复制一版；
    /// - 源表只读不搬走，项目侧仍保留原表。
    pub async fn promote_table_from_project(
        &self,
        _project: &ProjectDuckConn,
        req: &PromoteRequest,
        overwrite: bool,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let Some(source_ref_name) = req.source_table.as_deref() else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General("升级请求缺少源表名".to_string()),
            )));
        };
        // 项目表资产本体在 `analytics.rds_archive.arc_*`；普通表在 `analytics.<name>`。
        let (source_schema, source_table) = match source_ref_name.split_once('.') {
            Some((schema, table)) => (Some(schema), table),
            None => (None, source_ref_name),
        };
        let source_ref = match source_schema {
            Some(schema) => format!(
                "{}.{}.{}",
                quote_ident(shared::duckdb_root::PROJECT_ALIAS),
                quote_ident(schema),
                quote_ident(source_table)
            ),
            None => format!(
                "{}.{}",
                quote_ident(shared::duckdb_root::PROJECT_ALIAS),
                quote_ident(source_table)
            ),
        };
        let manifest_schema = match source_schema {
            Some(schema) => format!("{}.{schema}", shared::duckdb_root::PROJECT_ALIAS),
            None => shared::duckdb_root::PROJECT_ALIAS.to_string(),
        };

        // ---- 冲突 / 覆盖：只认一条活跃表资产 ----
        let head = self.global_table_head(&req.name)?;
        if head.is_none() {
            let active_name: Option<String> = self.with_sqlite(|conn| {
                conn.query_row(
                    "SELECT kind FROM analytics_resources \
                     WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL LIMIT 1",
                    rusqlite::params![req.name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sqlite_err("check_global_table_conflict"))
            })?;
            if active_name.is_some() {
                return Err(InstanceOpError::Conflict {
                    name: req.name.clone(),
                    field: ConflictField::Registry,
                });
            }
        } else if !overwrite {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Registry,
            });
        }

        let previous = match head.as_ref() {
            Some(head) => self.global_table_version_row(&head.asset_id, head.version)?,
            None => None,
        };

        // ---- 读 manifest + 需要时冻结快照 ----
        let (manifest, snapshot) = self
            .with_duckdb(|conn| {
                conn.execute_batch(&format!(
                    "CREATE SCHEMA IF NOT EXISTS {}.{};",
                    quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                    quote_ident(super::table_asset::GLOBAL_ARCHIVE_SCHEMA)
                ))
                .map_err(|e| duckdb_err("ensure_global_archive_schema", &e.to_string()))?;
                let manifest = manifest_for_table(conn, &manifest_schema, source_table)?;
                let existing_snapshot = self
                    .global_snapshot_for_manifest(&manifest.manifest_id)
                    .map_err(|error| CoreError::common(CommonError::General(error.to_string())))?;
                let reusable = existing_snapshot.filter(|snapshot| {
                    table_exists_in(conn, &snapshot.physical_schema, &snapshot.physical_table)
                });
                let snapshot = match reusable {
                    Some(snapshot) => NewTableSnapshot {
                        snapshot_id: snapshot.snapshot_id,
                        instance: SnapshotInstance::Global,
                        physical_schema: snapshot.physical_schema,
                        physical_table: snapshot.physical_table,
                    },
                    None => {
                        let snapshot_id = format!("tsnap_{}", uuid::Uuid::new_v4().simple());
                        let physical_table = snapshot_table_name(&snapshot_id);
                        let create_sql = format!(
                            "CREATE TABLE {}.{}.{} AS SELECT * FROM {source_ref};",
                            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                            quote_ident(super::table_asset::GLOBAL_ARCHIVE_SCHEMA),
                            quote_ident(&physical_table),
                        );
                        conn.execute_batch(&create_sql).map_err(|e| {
                            duckdb_err("create_global_table_snapshot", &e.to_string())
                        })?;
                        NewTableSnapshot {
                            snapshot_id,
                            instance: SnapshotInstance::Global,
                            physical_schema: super::table_asset::GLOBAL_ARCHIVE_SCHEMA.to_string(),
                            physical_table,
                        }
                    }
                };
                Ok((manifest, snapshot))
            })
            .await?;

        let definition = Some(format!(
            "CREATE TABLE {}.{}.{} AS SELECT * FROM {source_ref};",
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&snapshot.physical_schema),
            quote_ident(&snapshot.physical_table),
        ));

        // 内容与 head 相同：复用现有版本，不再追加空版本。
        if let (Some(head), Some(previous)) = (head.as_ref(), previous.as_ref()) {
            if previous.manifest_id == manifest.manifest_id {
                return Ok(PromoteOutcome {
                    resource_id: head.asset_id.clone(),
                    name: req.name.clone(),
                    version: head.version,
                    row_count: Some(previous.row_count as i32),
                    column_count: Some(previous.column_count as i32),
                    overwritten: overwrite,
                });
            }
        }

        let outcome = match head {
            Some(head) => self.append_global_table_version(
                &head,
                req,
                definition.as_deref(),
                &manifest,
                &snapshot,
                overwrite,
            )?,
            None => {
                let mut outcome = self.insert_global_table_asset(
                    &req.name,
                    req,
                    definition.as_deref(),
                    &manifest,
                    &snapshot,
                )?;
                outcome.overwritten = overwrite;
                outcome
            }
        };

        if let Some(keep) = req.keep_versions {
            if let Err(error) = self.prune_version_tables(&req.name, keep).await {
                tracing::warn!(error = %error, name = %req.name, "全局快照裁剪失败");
            }
        }
        Ok(outcome)
    }
}

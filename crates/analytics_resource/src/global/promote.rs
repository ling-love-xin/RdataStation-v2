//! 项目 → 全局升级：表资产落不可变快照 + 登记。
//!
//! 表型走 [`GlobalInstanceStore::promote_table_from_project`]：源表冻结成
//! `rds_global.rds_archive.arc_*`，一条活跃登记行一个稳定 asset_id，版本链在
//! `table_asset_versions`。数据文件走 [`GlobalInstanceStore::promote_file_as_table_asset`]，
//! 同样冻结成全局快照；文件资产走 `publish_file`。
use super::*;
use crate::service::table_asset::manifest_for_table;
use crate::store::table_asset::{NewTableSnapshot, SnapshotInstance};
use crate::table_asset::manifest::snapshot_table_name;

/// 发布基准校验：返回 true 表示本次是「接在已有 head 后追加」。
///
/// - head 不存在：只允许首次发布；带 base 说明发布者看的是别的实例，直接 `StaleBase`；
/// - head 存在 + 显式 base：必须等于 head，落后/超前都拒绝（只允许 fork）；
/// - head 存在 + 无 base：沿用旧口径，必须显式 `overwrite` 才追加。
fn check_publish_base(
    name: &str,
    head: Option<&super::table_asset::GlobalTableHead>,
    base: Option<i32>,
    overwrite: bool,
) -> Result<bool, InstanceOpError> {
    match (head, base) {
        (None, None) => Ok(false),
        (None, Some(base)) => Err(InstanceOpError::StaleBase {
            name: name.to_string(),
            base: Some(base),
            head: 0,
        }),
        (Some(head), Some(base)) if base == head.version => Ok(true),
        (Some(head), Some(base)) => Err(InstanceOpError::StaleBase {
            name: name.to_string(),
            base: Some(base),
            head: head.version,
        }),
        (Some(_), None) if overwrite => Ok(true),
        (Some(_), None) => Err(InstanceOpError::Conflict {
            name: name.to_string(),
            field: ConflictField::Registry,
        }),
    }
}

impl GlobalInstanceStore {
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
            base_global_version: None,
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

    /// 数据文件升级：文件直接冻结成全局不可变快照，不再建「一版一行」的旧表。
    ///
    /// 与表资产同构：一条活跃登记行 + `table_asset_versions`，物理表在
    /// `rds_global.rds_archive.arc_*`；同内容复用已有 manifest 快照。
    pub async fn promote_file_as_table_asset(
        &self,
        req: &PromoteRequest,
        overwrite: bool,
    ) -> Result<PromoteOutcome, InstanceOpError> {
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
                .map_err(sqlite_err("check_global_file_table_conflict"))
            })?;
            if active_name.is_some() {
                return Err(InstanceOpError::Conflict {
                    name: req.name.clone(),
                    field: ConflictField::Registry,
                });
            }
        } else if !overwrite && req.base_global_version.is_none() {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Registry,
            });
        }
        let overwrite =
            check_publish_base(&req.name, head.as_ref(), req.base_global_version, overwrite)?;

        let previous = match head.as_ref() {
            Some(head) => self.global_table_version_row(&head.asset_id, head.version)?,
            None => None,
        };

        let (manifest, snapshot) = self
            .with_duckdb(|conn| {
                conn.execute_batch(&format!(
                    "CREATE SCHEMA IF NOT EXISTS {}.{};",
                    quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                    quote_ident(super::table_asset::GLOBAL_ARCHIVE_SCHEMA)
                ))
                .map_err(|e| duckdb_err("ensure_global_archive_schema", &e.to_string()))?;
                let snapshot_id = format!("tsnap_{}", uuid::Uuid::new_v4().simple());
                let physical_table = snapshot_table_name(&snapshot_id);
                conn.execute_batch(&format!(
                    "CREATE TABLE {}.{}.{} AS SELECT * FROM {read_expr};",
                    quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                    quote_ident(super::table_asset::GLOBAL_ARCHIVE_SCHEMA),
                    quote_ident(&physical_table),
                ))
                .map_err(|e| duckdb_err("create_global_file_snapshot", &e.to_string()))?;
                let manifest = manifest_for_table(
                    conn,
                    &format!(
                        "{}.{}",
                        shared::duckdb_root::GLOBAL_ALIAS,
                        super::table_asset::GLOBAL_ARCHIVE_SCHEMA
                    ),
                    &physical_table,
                )?;
                let reusable = self
                    .global_snapshot_for_manifest(&manifest.manifest_id)
                    .map_err(|error| CoreError::common(CommonError::General(error.to_string())))?
                    .filter(|snapshot| {
                        table_exists_in(conn, &snapshot.physical_schema, &snapshot.physical_table)
                    });
                if let Some(snapshot) = reusable {
                    conn.execute_batch(&format!(
                        "DROP TABLE {}.{}.{};",
                        quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
                        quote_ident(super::table_asset::GLOBAL_ARCHIVE_SCHEMA),
                        quote_ident(&physical_table),
                    ))
                    .map_err(|e| duckdb_err("drop_duplicate_file_snapshot", &e.to_string()))?;
                    return Ok((
                        manifest,
                        NewTableSnapshot {
                            snapshot_id: snapshot.snapshot_id,
                            instance: SnapshotInstance::Global,
                            physical_schema: snapshot.physical_schema,
                            physical_table: snapshot.physical_table,
                        },
                    ));
                }
                Ok((
                    manifest,
                    NewTableSnapshot {
                        snapshot_id,
                        instance: SnapshotInstance::Global,
                        physical_schema: super::table_asset::GLOBAL_ARCHIVE_SCHEMA.to_string(),
                        physical_table,
                    },
                ))
            })
            .await?;

        let definition = Some(format!(
            "CREATE TABLE {}.{}.{} AS SELECT * FROM {read_expr};",
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&snapshot.physical_schema),
            quote_ident(&snapshot.physical_table),
        ));

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
                tracing::warn!(error = %error, name = %req.name, "全局文件快照裁剪失败");
            }
        }
        Ok(outcome)
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
        } else if !overwrite && req.base_global_version.is_none() {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
                field: ConflictField::Registry,
            });
        }
        let overwrite =
            check_publish_base(&req.name, head.as_ref(), req.base_global_version, overwrite)?;

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

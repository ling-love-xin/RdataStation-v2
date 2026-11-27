//! 全局表资产快照索引。
//!
//! 全局表资产和项目侧同构：一条活跃登记行一个稳定 `asset_id`，版本链在
//! `table_asset_versions`，每版指向 `rds_archive` 里一份不可变快照。全局侧只读，
//! 不提供 checkout 工作表。

use chrono::Utc;
use rusqlite::OptionalExtension;
use shared::error::CoreError;
use uuid::Uuid;

use super::{
    CommonError, GlobalInstanceStore, InstanceOpError, PromoteOutcome, PromoteRequest, quote_ident,
    sqlite_err, table_exists_in,
};
use crate::store::table_asset::{
    NewTableSnapshot, SnapshotInstance, TableAssetVersion, TableSnapshot,
};
use crate::table_asset::manifest::TableManifest;

/// 全局快照所在的 DuckDB schema。
pub(super) const GLOBAL_ARCHIVE_SCHEMA: &str = "rds_archive";

/// 全局表资产当前 head。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GlobalTableHead {
    pub asset_id: String,
    pub version: i32,
    pub analytics_table: String,
}

/// 一版全局表快照的读取结果（供分发 / 引用 / 版本浏览）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GlobalTableVersionRow {
    pub asset_id: String,
    pub version: i32,
    pub snapshot_id: String,
    pub manifest_id: String,
    pub physical_schema: String,
    pub physical_table: String,
    pub row_count: i64,
    pub column_count: i64,
    pub content_hash: String,
}

impl GlobalInstanceStore {
    /// 读全局表资产的活跃 head。
    pub(super) fn global_table_head(
        &self,
        name: &str,
    ) -> Result<Option<GlobalTableHead>, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT ar.id, ar.version, ar.analytics_table
                FROM analytics_resources ar
                WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.deleted_at IS NULL
                  AND ar.kind = 'analysis'
                ORDER BY ar.version DESC
                LIMIT 1
                "#,
                rusqlite::params![name],
                |row| {
                    let table: Option<String> = row.get(2)?;
                    Ok(table.map(|analytics_table| GlobalTableHead {
                        asset_id: row.get(0).unwrap_or_default(),
                        version: row.get(1).unwrap_or_default(),
                        analytics_table,
                    }))
                },
            )
            .optional()
            .map(|row| row.flatten())
            .map_err(sqlite_err("global_table_head"))
        })
    }

    /// 首次登记全局表资产：资源行 + v1 版本索引同一事务。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn insert_global_table_asset(
        &self,
        name: &str,
        req: &PromoteRequest,
        definition_sql: Option<&str>,
        manifest: &TableManifest,
        snapshot: &NewTableSnapshot,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let id = format!("arg_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        let origin_ref = req
            .source_resource_id
            .as_deref()
            .map(|source| crate::model::project_ref(source, req.source_version));
        let origin_kind = origin_ref
            .as_ref()
            .map(|_| crate::model::ORIGIN_KIND_PROJECT);
        self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_new_global_table"))?;
            tx.execute(
                r#"
                INSERT INTO analytics_resources (
                    id, resource_type, name, alias, config, scope, row_count, column_count,
                    file_size, version, parent_version_id, parent_resource_id, source_query,
                    created_at, updated_at, created_by, deleted_at,
                    kind, content_hash, file_rel_path, readonly, promoted_from,
                    source_connection_id, source_table, definition_sql, archived_at,
                    payload_mode, source_path, origin_kind, origin_ref, analytics_table,
                    structured
                ) VALUES (
                    ?1, 'analysis', ?2, NULL, 'null', 'global', ?3, ?4, NULL,
                    1, NULL, NULL, NULL, ?5, ?5, NULL, NULL,
                    'analysis', ?6, NULL, 1, NULL,
                    NULL, ?7, ?8, ?5,
                    'copy', ?9, ?10, ?11, ?12, 1
                )
                "#,
                rusqlite::params![
                    &id,
                    name,
                    manifest.row_count,
                    manifest.column_count,
                    &now,
                    &manifest.manifest_id,
                    req.source_table.as_deref(),
                    definition_sql,
                    req.source_abs_path.to_string_lossy().replace('\\', "/"),
                    origin_kind,
                    origin_ref,
                    &snapshot.physical_table,
                ],
            )
            .map_err(sqlite_err("insert_new_global_table"))?;
            ensure_manifest_on(&tx, manifest, &now)?;
            insert_snapshot_on(&tx, snapshot, &manifest.manifest_id, &now)?;
            insert_version_on(
                &tx,
                &id,
                1,
                None,
                &snapshot.snapshot_id,
                &manifest.manifest_id,
                &now,
            )?;
            tx.commit().map_err(sqlite_err("commit_new_global_table"))
        })?;
        Ok(PromoteOutcome {
            resource_id: id,
            name: name.to_string(),
            version: 1,
            row_count: Some(manifest.row_count as i32),
            column_count: Some(manifest.column_count as i32),
            overwritten: false,
        })
    }

    /// 追加一版并把 head 指过去；`parent_version` 是乐观锁。
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_global_table_version(
        &self,
        head: &GlobalTableHead,
        req: &PromoteRequest,
        definition_sql: Option<&str>,
        manifest: &TableManifest,
        snapshot: &NewTableSnapshot,
        overwrite: bool,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let version = head.version + 1;
        let now = Utc::now().to_rfc3339();
        let origin_ref = req
            .source_resource_id
            .as_deref()
            .map(|source| crate::model::project_ref(source, req.source_version));
        let origin_kind = origin_ref
            .as_ref()
            .map(|_| crate::model::ORIGIN_KIND_PROJECT);
        let updated = self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_append_global_table"))?;
            ensure_manifest_on(&tx, manifest, &now)?;
            insert_snapshot_on(&tx, snapshot, &manifest.manifest_id, &now)?;
            insert_version_on(
                &tx,
                &head.asset_id,
                version,
                Some(head.version),
                &snapshot.snapshot_id,
                &manifest.manifest_id,
                &now,
            )?;
            let affected = tx
                .execute(
                    r#"
                    UPDATE analytics_resources
                    SET version = ?1, analytics_table = ?2, content_hash = ?3,
                        row_count = ?4, column_count = ?5, definition_sql = ?6,
                        source_table = COALESCE(?7, source_table),
                        origin_kind = ?8, origin_ref = ?9,
                        updated_at = ?10
                    WHERE id = ?11 AND version = ?12 AND deleted_at IS NULL
                    "#,
                    rusqlite::params![
                        version,
                        &snapshot.physical_table,
                        &manifest.manifest_id,
                        manifest.row_count,
                        manifest.column_count,
                        definition_sql,
                        req.source_table.as_deref(),
                        origin_kind,
                        origin_ref,
                        &now,
                        &head.asset_id,
                        head.version,
                    ],
                )
                .map_err(sqlite_err("update_global_table_head"))?;
            if affected == 0 {
                return Err(CoreError::common(CommonError::General(format!(
                    "全局资产 {} 的 head 已不是 v{}，拒绝覆盖",
                    head.asset_id, head.version
                ))));
            }
            tx.commit()
                .map_err(sqlite_err("commit_append_global_table"))
        })?;
        let _ = updated;
        Ok(PromoteOutcome {
            resource_id: head.asset_id.clone(),
            name: req.name.clone(),
            version,
            row_count: Some(manifest.row_count as i32),
            column_count: Some(manifest.column_count as i32),
            overwritten: overwrite,
        })
    }

    /// 全局某资产的版本链（升序）。
    pub(crate) fn global_table_versions(
        &self,
        asset_id: &str,
    ) -> Result<Vec<TableAssetVersion>, InstanceOpError> {
        let rows = self.with_sqlite(|conn| {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT asset_id, version, snapshot_id, manifest_id, parent_version, created_at
                    FROM table_asset_versions
                    WHERE asset_id = ?
                    ORDER BY version ASC
                    "#,
                )
                .map_err(sqlite_err("prepare_global_table_versions"))?;
            let rows = stmt
                .query_map(rusqlite::params![asset_id], |row| {
                    let created_at: String = row.get(5)?;
                    Ok(TableAssetVersion {
                        asset_id: row.get(0)?,
                        version: row.get(1)?,
                        snapshot_id: row.get(2)?,
                        manifest_id: row.get(3)?,
                        parent_version: row.get(4)?,
                        created_at: parse_datetime(&created_at),
                    })
                })
                .map_err(sqlite_err("query_global_table_versions"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_err("collect_global_table_versions"))
        })?;
        Ok(rows)
    }

    /// 全局某资产某一版的快照信息。
    pub(super) fn global_table_version_row(
        &self,
        asset_id: &str,
        version: i32,
    ) -> Result<Option<GlobalTableVersionRow>, InstanceOpError> {
        let row = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT ar.id, tav.version, tav.snapshot_id, tav.manifest_id,
                       ts.physical_schema, ts.physical_table, tm.row_count, tm.column_count
                FROM table_asset_versions tav
                JOIN table_snapshots ts ON ts.snapshot_id = tav.snapshot_id
                JOIN table_manifests tm ON tm.manifest_id = tav.manifest_id
                JOIN analytics_resources ar ON ar.id = tav.asset_id
                WHERE tav.asset_id = ?1 AND tav.version = ?2
                "#,
                rusqlite::params![asset_id, version],
                |row| {
                    Ok(GlobalTableVersionRow {
                        asset_id: row.get(0)?,
                        version: row.get(1)?,
                        snapshot_id: row.get(2)?,
                        manifest_id: row.get(3)?,
                        physical_schema: row.get(4)?,
                        physical_table: row.get(5)?,
                        row_count: row.get(6)?,
                        column_count: row.get(7)?,
                        content_hash: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_err("global_table_version_row"))
        })?;
        Ok(row)
    }

    /// 读一份全局 manifest。
    pub(crate) fn global_table_manifest(
        &self,
        manifest_id: &str,
    ) -> Result<Option<TableManifest>, InstanceOpError> {
        let row = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT manifest_id, schema_hash, data_fingerprint, row_count, column_count, schema_json
                FROM table_manifests
                WHERE manifest_id = ?
                "#,
                rusqlite::params![manifest_id],
                |row| {
                    Ok(TableManifest {
                        manifest_id: row.get(0)?,
                        schema_hash: row.get(1)?,
                        data_fingerprint: row.get(2)?,
                        row_count: row.get(3)?,
                        column_count: row.get(4)?,
                        schema_json: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_err("global_table_manifest"))
        })?;
        Ok(row)
    }

    /// 读一份全局快照。
    pub(crate) fn global_table_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<TableSnapshot>, InstanceOpError> {
        let row = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT snapshot_id, manifest_id, instance, physical_schema, physical_table,
                       state, created_at, verified_at
                FROM table_snapshots
                WHERE snapshot_id = ?
                "#,
                rusqlite::params![snapshot_id],
                |row| {
                    let created_at: String = row.get(6)?;
                    Ok(TableSnapshot {
                        snapshot_id: row.get(0)?,
                        manifest_id: row.get(1)?,
                        instance: if row.get::<_, String>(2)? == "global" {
                            SnapshotInstance::Global
                        } else {
                            SnapshotInstance::Project
                        },
                        physical_schema: row.get(3)?,
                        physical_table: row.get(4)?,
                        state: if row.get::<_, String>(5)? == "live" {
                            crate::store::table_asset::SnapshotState::Live
                        } else {
                            crate::store::table_asset::SnapshotState::Missing
                        },
                        created_at: parse_datetime(&created_at),
                        verified_at: row
                            .get::<_, Option<String>>(7)?
                            .and_then(|text| chrono::DateTime::parse_from_rfc3339(&text).ok())
                            .map(|value| value.with_timezone(&Utc)),
                    })
                },
            )
            .optional()
            .map_err(sqlite_err("global_table_snapshot"))
        })?;
        Ok(row)
    }

    /// 从父资产某一版 fork 出新的全局表资产：新 `asset_id` 从 v1 起链。
    ///
    /// v1 **直接引用父版快照**（同实例零复制），之后再发布就在自己链上追加；
    /// `table_asset_forks` 记父子关系。父资产 head 已前进也不影响——fork 的语义
    /// 就是「从你看过的那一版分叉」，不是覆盖父 head。
    pub async fn fork_global_table_asset(
        &self,
        parent_name: &str,
        parent_version: i32,
        fork_name: &str,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        let head =
            self.global_table_head(parent_name)?
                .ok_or_else(|| InstanceOpError::NotFound {
                    name: parent_name.to_string(),
                })?;
        let target = self
            .global_table_version_row(&head.asset_id, parent_version)?
            .ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "全局「{parent_name}」v{parent_version} 的版本索引不存在，无法 fork"
                ))))
            })?;
        let manifest = self
            .global_table_manifest(&target.manifest_id)?
            .ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "全局「{parent_name}」v{parent_version} 的 manifest 缺失，无法 fork"
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
                    "全局「{parent_name}」v{parent_version} 的快照 {} 已不存在，无法 fork",
                    target.physical_table
                )),
            )));
        }

        let active: i64 = self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT count(*) FROM analytics_resources \
                 WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL",
                rusqlite::params![fork_name],
                |row| row.get(0),
            )
            .map_err(sqlite_err("check_fork_name"))
        })?;
        if active > 0 {
            return Err(InstanceOpError::Conflict {
                name: fork_name.to_string(),
                field: super::ConflictField::Registry,
            });
        }

        let id = format!("arg_{}", Uuid::new_v4().simple());
        let fork_id = format!("taf_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        let definition = Some(format!(
            "CREATE TABLE {}.{}.{} AS SELECT * FROM {}.{}.{};",
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&target.physical_schema),
            quote_ident(&target.physical_table),
            quote_ident(shared::duckdb_root::GLOBAL_ALIAS),
            quote_ident(&target.physical_schema),
            quote_ident(&target.physical_table),
        ));
        self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_global_table_fork"))?;
            let parent_source_table: Option<String> = tx
                .query_row(
                    "SELECT source_table FROM analytics_resources WHERE id = ?1",
                    rusqlite::params![&target.asset_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_err("read_fork_parent_row"))?;
            tx.execute(
                r#"
                INSERT INTO analytics_resources (
                    id, resource_type, name, alias, config, scope, row_count, column_count,
                    file_size, version, parent_version_id, parent_resource_id, source_query,
                    created_at, updated_at, created_by, deleted_at,
                    kind, content_hash, file_rel_path, readonly, promoted_from,
                    source_connection_id, source_table, definition_sql, archived_at,
                    payload_mode, source_path, origin_kind, origin_ref, analytics_table,
                    structured
                ) VALUES (
                    ?1, 'analysis', ?2, NULL, 'null', 'global', ?3, ?4, NULL,
                    1, NULL, NULL, NULL, ?5, ?5, NULL, NULL,
                    'analysis', ?6, NULL, 1, NULL,
                    NULL, ?7, ?8, ?5,
                    'copy', ?9, NULL, NULL, ?10, 1
                )
                "#,
                rusqlite::params![
                    &id,
                    fork_name,
                    manifest.row_count,
                    manifest.column_count,
                    &now,
                    &manifest.manifest_id,
                    parent_source_table.as_deref(),
                    definition.as_deref(),
                    format!(
                        "{}.{}.{}",
                        shared::duckdb_root::GLOBAL_ALIAS,
                        target.physical_schema,
                        target.physical_table
                    ),
                    &target.physical_table,
                ],
            )
            .map_err(sqlite_err("insert_global_table_fork"))?;
            insert_version_on(
                &tx,
                &id,
                1,
                None,
                &target.snapshot_id,
                &target.manifest_id,
                &now,
            )?;
            tx.execute(
                r#"
                INSERT INTO table_asset_forks (
                    fork_id, parent_asset_id, parent_version,
                    child_asset_id, child_version, created_at
                ) VALUES (?, ?, ?, ?, 1, ?)
                "#,
                rusqlite::params![&fork_id, &target.asset_id, parent_version, &id, &now,],
            )
            .map_err(sqlite_err("insert_table_asset_fork"))?;
            tx.commit().map_err(sqlite_err("commit_global_table_fork"))
        })?;
        Ok(PromoteOutcome {
            resource_id: id,
            name: fork_name.to_string(),
            version: 1,
            row_count: Some(manifest.row_count as i32),
            column_count: Some(manifest.column_count as i32),
            overwritten: false,
        })
    }

    /// 同内容的全局快照复用：按 manifest 找第一份 live。
    pub(super) fn global_snapshot_for_manifest(
        &self,
        manifest_id: &str,
    ) -> Result<Option<TableSnapshot>, InstanceOpError> {
        let row = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT snapshot_id FROM table_snapshots
                WHERE manifest_id = ?1 AND instance = 'global' AND state = 'live'
                  AND gc_candidate_at IS NULL
                ORDER BY created_at ASC LIMIT 1
                "#,
                rusqlite::params![manifest_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_err("global_snapshot_for_manifest"))
        })?;
        match row {
            Some(id) => self.global_table_snapshot(&id),
            None => Ok(None),
        }
    }
}

fn ensure_manifest_on(
    conn: &rusqlite::Connection,
    manifest: &TableManifest,
    now: &str,
) -> Result<(), CoreError> {
    conn.execute(
        r#"
        INSERT OR IGNORE INTO table_manifests (
            manifest_id, schema_hash, data_fingerprint,
            row_count, column_count, schema_json, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        rusqlite::params![
            &manifest.manifest_id,
            &manifest.schema_hash,
            &manifest.data_fingerprint,
            manifest.row_count,
            manifest.column_count,
            &manifest.schema_json,
            now,
        ],
    )
    .map_err(sqlite_err("global_insert_manifest"))?;
    let existing: (String, String, i64, i64) = conn
        .query_row(
            "SELECT schema_hash, data_fingerprint, row_count, column_count FROM table_manifests WHERE manifest_id = ?1",
            rusqlite::params![&manifest.manifest_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(sqlite_err("global_verify_manifest"))?;
    if existing.0 != manifest.schema_hash
        || existing.1 != manifest.data_fingerprint
        || existing.2 != manifest.row_count
        || existing.3 != manifest.column_count
    {
        return Err(CoreError::common(CommonError::General(format!(
            "manifest {} 已存在但内容不一致（哈希冲突或写入损坏）",
            manifest.manifest_id
        ))));
    }
    Ok(())
}

fn insert_snapshot_on(
    conn: &rusqlite::Connection,
    snapshot: &NewTableSnapshot,
    manifest_id: &str,
    now: &str,
) -> Result<(), CoreError> {
    conn.execute(
        r#"
        INSERT OR IGNORE INTO table_snapshots (
            snapshot_id, manifest_id, instance, physical_schema, physical_table,
            state, created_at
        ) VALUES (?, ?, ?, ?, ?, 'live', ?)
        "#,
        rusqlite::params![
            &snapshot.snapshot_id,
            manifest_id,
            snapshot.instance.as_str(),
            &snapshot.physical_schema,
            &snapshot.physical_table,
            now,
        ],
    )
    .map_err(sqlite_err("global_insert_snapshot"))?;
    let existing: (String, String, String, String) = conn
        .query_row(
            r#"
            SELECT manifest_id, instance, physical_schema, physical_table
            FROM table_snapshots WHERE snapshot_id = ?
            "#,
            rusqlite::params![&snapshot.snapshot_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(sqlite_err("global_verify_snapshot"))?;
    if existing.0 != manifest_id
        || existing.1 != snapshot.instance.as_str()
        || existing.2 != snapshot.physical_schema
        || existing.3 != snapshot.physical_table
    {
        return Err(CoreError::common(CommonError::General(format!(
            "快照 {} 已存在但物理定位或 manifest 不一致",
            snapshot.snapshot_id
        ))));
    }
    Ok(())
}

fn insert_version_on(
    conn: &rusqlite::Connection,
    asset_id: &str,
    version: i32,
    parent_version: Option<i32>,
    snapshot_id: &str,
    manifest_id: &str,
    now: &str,
) -> Result<(), CoreError> {
    conn.execute(
        r#"
        INSERT INTO table_asset_versions (
            asset_id, version, snapshot_id, manifest_id, parent_version, created_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
        rusqlite::params![
            asset_id,
            version,
            snapshot_id,
            manifest_id,
            parent_version,
            now,
        ],
    )
    .map_err(|error| {
        let text = error.to_string();
        if text.contains("UNIQUE") || text.contains("unique") {
            CoreError::common(CommonError::General(format!(
                "全局资产 {asset_id} 已有版本 {version}，不能重复提交"
            )))
        } else {
            sqlite_err("global_insert_version")(error)
        }
    })?;
    Ok(())
}

fn parse_datetime(text: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

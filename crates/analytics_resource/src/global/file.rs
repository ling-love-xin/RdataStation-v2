//! 全局文件资产：file-first 发布、fork、取回。
//!
//! 全局文件以 CAS manifest 为真相源；`analytics_resources` 只记头与来源，
//! `analytics_table` 一律为空。表只是可选投影，本模块不自动建表。

use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::OptionalExtension;
use tokio::fs;

use shared::error::{CommonError, CoreError, StorageError};

use crate::payload::cas::CasStore;
use crate::payload::manifest::FileManifest;
use crate::payload::PayloadStore;

use super::*;

impl GlobalInstanceStore {
    /// 全局 CAS：`{system}/cas/`（不是项目布局 `{root}/.RSmeta/cas`）。
    fn file_cas(&self) -> CasStore {
        CasStore::at_root(self.system_dir.join("cas"))
    }

    /// 发布项目文件到全局（file-first）。base 不匹配只报冲突，不覆盖。
    pub async fn publish_file(
        &self,
        req: &PublishFileRequest,
    ) -> Result<PublishFileOutcome, InstanceOpError> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::InvalidArgument {
                    param: "name".to_string(),
                    reason: "全局文件资产名不能为空".to_string(),
                },
            )));
        }

        let (manifest, manifest_id, source_abs) = self.resolve_manifest(req).await?;
        let content_class = self.content_class(manifest.content_entry()).await?;

        let head = self.current_global_file(name)?;
        let parent = match head {
            Some((head_id, head_version, head_rel)) => {
                if req.base_global_version != Some(head_version) {
                    return Err(InstanceOpError::StaleBase {
                        name: name.to_string(),
                        base: req.base_global_version,
                        head: head_version,
                    });
                }
                Some((head_id, head_version, head_rel))
            }
            None => {
                if req.base_global_version.is_some() {
                    return Err(InstanceOpError::StaleBase {
                        name: name.to_string(),
                        base: req.base_global_version,
                        head: 0,
                    });
                }
                None
            }
        };

        let version = self
            .next_global_version(name)
            .map_err(InstanceOpError::Core)?;
        let rel = req
            .rel_path
            .clone()
            .or_else(|| parent.as_ref().and_then(|(_, _, rel)| rel.clone()))
            .unwrap_or_else(|| fallback_rel(name, &source_abs));
        // 路径守门在落缓存前做一次：不允许绝对路径 / .. / 点前缀。
        PayloadStore::new(self.system_dir.clone())
            .resolve(&rel)
            .map_err(InstanceOpError::Core)?;

        let parent_id = parent.as_ref().map(|(id, _, _)| id.clone());
        let parent_version = parent.as_ref().map(|(_, version, _)| *version);
        let outcome = self.insert_global_file_row(
            name,
            version,
            parent_id,
            parent_version,
            &rel,
            &manifest,
            &manifest_id,
            &content_class,
            req,
            &source_abs,
        )?;

        // 挂当前物化缓存：失败不回滚版本，读路径仍可从 CAS 重建。
        if let Err(error) = self.materialize_global_current(&manifest, &rel).await {
            tracing::warn!(error = %error, name, rel, "全局文件当前缓存物化失败（版本已提交，可从 CAS 重建）");
        }

        Ok(outcome)
    }

    /// 从历史 manifest 恢复成新版本（不原地回滚）：旧版本行与 CAS 对象都保留。
    pub async fn restore_file_version(
        &self,
        name: &str,
        target_version: i32,
    ) -> Result<PublishFileOutcome, InstanceOpError> {
        let row: Option<(String, Option<String>, String, i64)> = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT fav.manifest_id, ar.file_rel_path, ar.content_hash, ar.file_size
                FROM analytics_resources ar
                JOIN file_asset_versions fav
                  ON fav.resource_id = ar.id AND fav.version = ar.version
                WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.version = ?2
                LIMIT 1
                "#,
                rusqlite::params![name, target_version],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    ))
                },
            )
            .optional()
            .map_err(sqlite_err("read_restore_file_version"))
        })?;
        let Some((manifest_id, rel, _hash, _size)) = row else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{target_version} 没有文件 manifest，无法按文件恢复"
                )),
            )));
        };
        let global_cas = self.file_cas();
        let manifest = global_cas
            .read_manifest(&manifest_id)
            .await
            .map_err(InstanceOpError::Core)?;
        let head = self.current_global_file(name)?;
        let parent_version = head.as_ref().map(|(_, version, _)| *version);
        let parent_id = head.as_ref().map(|(id, _, _)| id.clone());
        let version = self
            .next_global_version(name)
            .map_err(InstanceOpError::Core)?;
        let rel = rel.unwrap_or_else(|| fallback_rel(name, Path::new(&manifest_id)));
        let content_class = self.content_class(manifest.content_entry()).await?;
        let req = PublishFileRequest {
            name: name.to_string(),
            source_project_root: self.system_dir.clone(),
            manifest_id: Some(manifest_id.clone()),
            source_abs_path: None,
            rel_path: Some(rel.clone()),
            source_resource_id: None,
            source_version: None,
            base_global_version: parent_version,
        };
        let outcome = self.insert_global_file_row(
            name,
            version,
            parent_id,
            parent_version,
            &rel,
            &manifest,
            &manifest_id,
            &content_class,
            &req,
            &self.system_dir,
        )?;
        if let Err(error) = self.materialize_global_current(&manifest, &rel).await {
            tracing::warn!(error = %error, name, version, "恢复后当前缓存物化失败（可从 CAS 重建）");
        }
        Ok(outcome)
    }

    /// fork：复制同一 manifest 到新全局资产，原 head 不动。
    pub async fn fork_file(
        &self,
        name: &str,
        version: i32,
        fork_name: &str,
    ) -> Result<GlobalFileForkOutcome, InstanceOpError> {
        let source: Option<(String, Option<String>, Option<String>, i64, Option<String>)> =
            self.with_sqlite(|conn| {
                conn.query_row(
                    r#"
                    SELECT ar.id, ar.file_rel_path, ar.content_hash, ar.file_size, fav.manifest_id
                    FROM analytics_resources ar
                    LEFT JOIN file_asset_versions fav
                      ON fav.resource_id = ar.id AND fav.version = ar.version
                    WHERE ar.name = ?1 AND ar.scope = 'global' AND ar.version = ?2
                    LIMIT 1
                    "#,
                    rusqlite::params![name, version],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                            row.get(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(sqlite_err("fork_read_source"))
            })?;
        let Some((_source_id, rel, _hash, _file_size, manifest_id)) = source else {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        };
        let Some(manifest_id) = manifest_id else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{version} 没有 manifest，不能 fork；旧文件请先回填 CAS"
                )),
            )));
        };
        let global_cas = self.file_cas();
        let manifest = global_cas
            .read_manifest(&manifest_id)
            .await
            .map_err(InstanceOpError::Core)?;

        if self
            .with_sqlite(|conn| {
                conn.query_row(
                    "SELECT count(*) FROM analytics_resources \
                     WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL",
                    rusqlite::params![fork_name],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(sqlite_err("fork_check_name"))
            })?
            > 0
        {
            return Err(InstanceOpError::Conflict {
                name: fork_name.to_string(),
                field: ConflictField::Registry,
            });
        }

        let rel = rel.unwrap_or_else(|| format!("{fork_name}.dat"));
        let fork_rel = fork_rel_path(&rel, fork_name);
        let version = 1;
        let fork_id = format!("fork_{}", uuid::Uuid::new_v4().simple());
        let child_id = format!("arg_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        let content_hash = manifest.content_hash.clone();
        let file_size = manifest.file_size as i64;
        let content_class = self.content_class(manifest.content_entry()).await?;

        self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_fork_file"))?;
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
                    ?1, 'file', ?2, NULL, 'null', 'global', NULL, NULL, ?3,
                    ?4, NULL, NULL, NULL, ?5, ?5, NULL, NULL,
                    'file', ?6, ?7, 1, NULL, NULL, NULL, NULL, ?5,
                    'copy', NULL, ?8, ?9, NULL, ?10
                )
                "#,
                rusqlite::params![
                    &child_id,
                    fork_name,
                    file_size,
                    version,
                    now,
                    &content_hash,
                    &fork_rel,
                    crate::model::ORIGIN_KIND_GLOBAL,
                    crate::model::global_ref(name, version),
                    i32::from(content_class == "tabular"),
                ],
            )
            .map_err(sqlite_err("insert_fork_file"))?;
            let snapshot = serde_json::json!({
                "kind": "file",
                "content_class": content_class,
                "manifest_id": manifest_id,
                "content_hash": content_hash,
                "file_size": file_size,
                "file_rel_path": fork_rel,
                "forked_from": {"name": name, "version": version},
                "promoted_at": now,
            })
            .to_string();
            tx.execute(
                "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    format!("argv_{}", uuid::Uuid::new_v4().simple()),
                    &child_id,
                    version,
                    snapshot,
                    now
                ],
            )
            .map_err(sqlite_err("insert_fork_version"))?;
            insert_global_file_version_on(
                &tx,
                &child_id,
                version,
                &manifest_id,
                &content_hash,
                manifest.meta_hash.as_deref(),
                None,
                &now,
            )?;
            tx.execute(
                "INSERT INTO asset_forks \
                 (fork_id, parent_name, parent_version, child_name, child_version, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![&fork_id, name, version, fork_name, 1, now],
            )
            .map_err(sqlite_err("insert_asset_fork"))?;
            tx.commit().map_err(sqlite_err("commit_fork_file"))
        })?;

        if let Err(error) = self.materialize_global_current(&manifest, &fork_rel).await {
            tracing::warn!(error = %error, name = fork_name, "fork 后当前缓存物化失败（可从 CAS 重建）");
        }

        Ok(GlobalFileForkOutcome {
            fork_id,
            child_resource_id: child_id,
            child_name: fork_name.to_string(),
            child_version: 1,
            manifest_id,
        })
    }

    /// 全局文件取回：物化到项目草稿箱，并在项目库写 `base_scope=global` 的 lease。
    pub async fn checkout_file(
        &self,
        project: &crate::store::AnalyticsResourceStore,
        project_root: &Path,
        name: &str,
        selected_version: Option<i32>,
        dest_rel: &str,
    ) -> Result<GlobalFileCheckoutOutcome, InstanceOpError> {
        let row = self.read_global_row(name, selected_version)?;
        let Some((
            _global_id,
            version,
            kind,
            content_hash,
            _rel_path,
            _definition,
            _rows,
            _cols,
            _size,
            _table,
            manifest_id,
        )) = row
        else {
            return Err(InstanceOpError::NotFound {
                name: name.to_string(),
            });
        };
        if kind != "file" {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::InvalidArgument {
                    param: "name".to_string(),
                    reason: format!("全局「{name}」不是文件资产，不能按文件取回"),
                },
            )));
        }
        let Some(manifest_id) = manifest_id else {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!(
                    "全局「{name}」v{version} 没有 manifest，旧文件请先回填 CAS"
                )),
            )));
        };
        let lease_key = format!("global:{name}");
        if let Some(lease) = project.active_lease(&lease_key).await.map_err(InstanceOpError::Core)? {
            return Err(InstanceOpError::LeaseActive {
                name: name.to_string(),
                project_root: project_root.display().to_string(),
                dest_rel: lease.dest_rel,
            });
        }
        let dest = resolve_draft_path(project_root, dest_rel)?;
        if dest.exists() {
            return Err(InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "取回目标已存在，拒绝覆盖：{}",
                dest.display()
            )))));
        }

        let global_cas = self.file_cas();
        let manifest = global_cas
            .read_manifest(&manifest_id)
            .await
            .map_err(InstanceOpError::Core)?;
        let file_meta = global_cas
            .materialize_content(&manifest, &dest)
            .await
            .map_err(InstanceOpError::Core)?;

        // 全局侧先占租约（purge 守卫要看得见）；项目侧 lease 是再归档的本地凭据。
        if let Err(error) = self.create_global_lease(name, project_root, version, dest_rel) {
            let _ = fs::remove_file(&dest).await;
            return Err(error);
        }
        let lease = match project
            .create_lease(&lease_key, "global", version, dest_rel)
            .await
        {
            Ok(lease) => lease,
            Err(error) => {
                let _ = self.close_global_lease(name, project_root, "abandoned").await;
                let _ = fs::remove_file(&dest).await;
                return Err(InstanceOpError::Core(error));
            }
        };

        Ok(GlobalFileCheckoutOutcome {
            lease_id: lease.lease_id,
            name: name.to_string(),
            version,
            dest_rel: dest_rel.replace('\\', "/"),
            content_hash: content_hash.unwrap_or_default(),
            file_meta,
        })
    }

    async fn resolve_manifest(
        &self,
        req: &PublishFileRequest,
    ) -> Result<(FileManifest, String, PathBuf), InstanceOpError> {
        let global_cas = self.file_cas();
        if let Some(manifest_id) = req.manifest_id.as_deref() {
            let project_cas = CasStore::new(&req.source_project_root);
            let manifest = project_cas
                .read_manifest(manifest_id)
                .await
                .map_err(InstanceOpError::Core)?;
            let imported = global_cas
                .import_manifest(&project_cas, &manifest)
                .await
                .map_err(InstanceOpError::Core)?;
            let source_abs = req
                .source_abs_path
                .clone()
                .unwrap_or_else(|| req.source_project_root.clone());
            return Ok((manifest, imported, source_abs));
        }

        let source = req.source_abs_path.as_ref().ok_or_else(|| {
            InstanceOpError::Core(CoreError::common(CommonError::InvalidArgument {
                param: "source_abs_path".to_string(),
                reason: "没有 manifest 时必须给源文件".to_string(),
            }))
        })?;
        if !source.is_file() {
            return Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(format!("源文件不存在：{}", source.display())),
            )));
        }
        let object = global_cas
            .put_file(source)
            .await
            .map_err(InstanceOpError::Core)?;
        let rel = req
            .rel_path
            .clone()
            .unwrap_or_else(|| fallback_rel(&req.name, source));
        let manifest = FileManifest::single_file(&rel, &object.hash, object.size, None);
        let manifest_id = global_cas
            .put_manifest(&manifest)
            .await
            .map_err(InstanceOpError::Core)?;
        Ok((manifest, manifest_id, source.clone()))
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_global_file_row(
        &self,
        name: &str,
        version: i32,
        parent_id: Option<String>,
        parent_version: Option<i32>,
        rel: &str,
        manifest: &FileManifest,
        manifest_id: &str,
        content_class: &str,
        req: &PublishFileRequest,
        source_abs: &Path,
    ) -> Result<PublishFileOutcome, InstanceOpError> {
        let id = format!("arg_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        let content_hash = manifest.content_hash.clone();
        let file_size = manifest.file_size as i64;
        let structured = i32::from(content_class == "tabular");
        let origin_kind = req
            .source_resource_id
            .as_ref()
            .map(|_| crate::model::ORIGIN_KIND_PROJECT);
        let origin_ref = req
            .source_resource_id
            .as_deref()
            .map(|id| crate::model::project_ref(id, req.source_version));
        let parent_id = parent_id.as_deref();

        self.with_sqlite(|conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(sqlite_err("begin_publish_file"))?;
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
                    ?1, 'file', ?2, NULL, 'null', 'global', NULL, NULL, ?3,
                    ?4, ?5, ?5, NULL, ?6, ?6, NULL, NULL,
                    'file', ?7, ?8, 1, NULL, NULL, NULL, NULL, ?6,
                    'copy', ?9, ?10, ?11, NULL, ?12
                )
                "#,
                rusqlite::params![
                    &id,
                    name,
                    file_size,
                    version,
                    parent_id,
                    now,
                    &content_hash,
                    rel,
                    source_abs.to_string_lossy().replace('\\', "/"),
                    origin_kind,
                    origin_ref,
                    structured,
                ],
            )
            .map_err(sqlite_err("insert_global_file"))?;

            let snapshot = serde_json::json!({
                "kind": "file",
                "content_class": content_class,
                "manifest_id": manifest_id,
                "content_hash": content_hash,
                "file_size": file_size,
                "file_rel_path": rel,
                "duckdb_readable": content_class == "tabular",
                "parent_version": parent_version,
                "source": source_abs.to_string_lossy().to_string(),
                "promoted_at": now,
            })
            .to_string();
            tx.execute(
                "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    format!("argv_{}", uuid::Uuid::new_v4().simple()),
                    &id,
                    version,
                    snapshot,
                    now
                ],
            )
            .map_err(sqlite_err("insert_publish_version"))?;
            insert_global_file_version_on(
                &tx,
                &id,
                version,
                manifest_id,
                &content_hash,
                manifest.meta_hash.as_deref(),
                parent_version,
                &now,
            )?;
            tx.execute(
                "INSERT INTO global_publish_links \
                 (link_id, global_name, global_version, source_resource_id, source_scope, \
                  source_version, published_at) \
                 VALUES (?1, ?2, ?3, ?4, 'project', ?5, ?6)",
                rusqlite::params![
                    format!("gpl_{}", uuid::Uuid::new_v4().simple()),
                    name,
                    version,
                    req.source_resource_id,
                    req.source_version,
                    now
                ],
            )
            .map_err(sqlite_err("insert_publish_link"))?;
            tx.commit().map_err(sqlite_err("commit_publish_file"))
        })?;

        Ok(PublishFileOutcome {
            resource_id: id,
            name: name.to_string(),
            version,
            manifest_id: manifest_id.to_string(),
            content_hash,
            file_size,
            content_class: content_class.to_string(),
        })
    }

    async fn materialize_global_current(
        &self,
        manifest: &FileManifest,
        rel: &str,
    ) -> Result<(), CoreError> {
        let global_cas = self.file_cas();
        let entry = manifest.content_entry().ok_or_else(|| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "materialize_current".to_string(),
                reason: "manifest 缺少 Content 条目".to_string(),
            })
        })?;
        let blob = global_cas.blob_path(&entry.hash)?;
        let store = PayloadStore::new(self.system_dir.clone());
        let staged = store.stage_payload(&blob, rel).await?;
        store.commit_staged(&staged, rel).await?;
        Ok(())
    }

    async fn content_class(
        &self,
        entry: Option<&crate::payload::manifest::ManifestEntry>,
    ) -> Result<String, InstanceOpError> {
        let Some(entry) = entry else {
            return Ok("binary".to_string());
        };
        let ext = Path::new(&entry.path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "csv" | "tsv" | "parquet" | "xlsx" | "xls" | "json" | "ndjson"
        ) {
            return Ok("tabular".to_string());
        }
        let blob = self
            .file_cas()
            .blob_path(&entry.hash)
            .map_err(InstanceOpError::Core)?;
        let bytes = fs::read(&blob).await.map_err(|error| {
            InstanceOpError::Core(CoreError::storage(StorageError::io(
                blob.display().to_string(),
                "read_content_class",
                error.to_string(),
            )))
        })?;
        Ok(if std::str::from_utf8(&bytes).is_ok() {
            "text".to_string()
        } else {
            "binary".to_string()
        })
    }

    /// 当前 head 版本（0 表示没有活跃行）；发布冲突/覆盖确认用。
    pub(crate) fn current_file_head(&self, name: &str) -> Result<Option<i32>, CoreError> {
        Ok(self
            .current_global_file(name)
            .map_err(|error| match error {
                InstanceOpError::Core(error) => error,
                other => CoreError::common(CommonError::General(other.to_string())),
            })?
            .map(|(_, version, _)| version))
    }

    fn current_global_file(
        &self,
        name: &str,
    ) -> Result<Option<(String, i32, Option<String>)>, InstanceOpError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT id, version, file_rel_path
                FROM analytics_resources
                WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                ORDER BY version DESC LIMIT 1
                "#,
                rusqlite::params![name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(sqlite_err("read_global_file_head"))
        })
        .map_err(InstanceOpError::Core)
    }

    fn create_global_lease(
        &self,
        name: &str,
        project_root: &Path,
        version: i32,
        dest_rel: &str,
    ) -> Result<(), InstanceOpError> {
        let root = project_root.to_string_lossy().replace('\\', "/");
        if let Some((_, existing_dest)) = self
            .active_global_lease_for(name, &root)
            .map_err(InstanceOpError::Core)?
        {
            return Err(InstanceOpError::LeaseActive {
                name: name.to_string(),
                project_root: root,
                dest_rel: existing_dest,
            });
        }
        self.with_sqlite(|conn| {
            conn.execute(
                r#"
                INSERT INTO global_checkout_leases (
                    lease_id, resource_name, project_root, base_version,
                    dest_rel, state, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)
                "#,
                rusqlite::params![
                    format!("glease_{}", uuid::Uuid::new_v4().simple()),
                    name,
                    root,
                    version,
                    dest_rel,
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(sqlite_err("create_global_lease"))?;
            Ok(())
        })
        .map_err(InstanceOpError::Core)
    }

    fn active_global_lease_for(
        &self,
        name: &str,
        project_root: &str,
    ) -> Result<Option<(String, String)>, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT resource_name, dest_rel FROM global_checkout_leases \
                 WHERE resource_name = ?1 AND project_root = ?2 AND state = 'active' \
                 LIMIT 1",
                rusqlite::params![name, project_root],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sqlite_err("active_global_lease_for"))
        })
    }

    async fn close_global_lease(
        &self,
        name: &str,
        project_root: &Path,
        state: &str,
    ) -> Result<(), CoreError> {
        let root = project_root.to_string_lossy().replace('\\', "/");
        self.with_sqlite(|conn| {
            conn.execute(
                "UPDATE global_checkout_leases SET state = ?1, closed_at = ?2 \
                 WHERE resource_name = ?3 AND project_root = ?4 AND state = 'active'",
                rusqlite::params![state, Utc::now().to_rfc3339(), name, root],
            )
            .map_err(sqlite_err("close_global_lease"))?;
            Ok(())
        })
    }

    /// 永久删除前确认没有活跃全局取回；返回第一条用于报错。
    pub(super) fn active_global_lease(
        &self,
        name: &str,
    ) -> Result<Option<(String, String)>, CoreError> {
        self.with_sqlite(|conn| {
            conn.query_row(
                "SELECT project_root, dest_rel FROM global_checkout_leases \
                 WHERE resource_name = ?1 AND state = 'active' \
                 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sqlite_err("active_global_lease"))
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_global_file_version_on(
    conn: &rusqlite::Connection,
    resource_id: &str,
    version: i32,
    manifest_id: &str,
    content_hash: &str,
    meta_hash: Option<&str>,
    parent_version: Option<i32>,
    created_at: &str,
) -> Result<(), CoreError> {
    conn.execute(
        r#"
        INSERT INTO file_asset_versions (
            resource_id, version, manifest_id, content_hash, meta_hash,
            parent_version, content_pruned, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)
        "#,
        rusqlite::params![
            resource_id,
            version,
            manifest_id,
            content_hash,
            meta_hash,
            parent_version,
            created_at,
        ],
    )
    .map_err(sqlite_err("insert_global_file_version"))?;
    Ok(())
}

fn fallback_rel(name: &str, source: &Path) -> String {
    let ext = source
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    format!("{name}{ext}")
}

/// fork 不能和原资产共用同一个 `resources/<rel>`，否则当前缓存互相覆盖。
fn fork_rel_path(original: &str, fork_name: &str) -> String {
    let extension = Path::new(original)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    let dir = Path::new(original).parent().filter(|path| !path.as_os_str().is_empty());
    let file = format!("{fork_name}{extension}");
    match dir {
        Some(dir) => dir.join(file).to_string_lossy().replace('\\', "/"),
        None => file,
    }
}

fn resolve_draft_path(project_root: &Path, rel: &str) -> Result<PathBuf, InstanceOpError> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err(InstanceOpError::Core(CoreError::common(
            CommonError::InvalidArgument {
                param: "dest_rel".to_string(),
                reason: "取回路径不能为空".to_string(),
            },
        )));
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(InstanceOpError::Core(CoreError::common(
            CommonError::InvalidArgument {
                param: "dest_rel".to_string(),
                reason: "取回路径必须是相对路径".to_string(),
            },
        )));
    }
    let mut out = project_root.join("scratchpad");
    for component in candidate.components() {
        match component {
            std::path::Component::Normal(name) => {
                if name.to_string_lossy().starts_with('.') {
                    return Err(InstanceOpError::Core(CoreError::common(
                        CommonError::InvalidArgument {
                            param: "dest_rel".to_string(),
                            reason: "取回路径不允许点前缀".to_string(),
                        },
                    )));
                }
                out.push(name);
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(InstanceOpError::Core(CoreError::common(
                    CommonError::InvalidArgument {
                        param: "dest_rel".to_string(),
                        reason: "取回路径不允许 .. / 根组件".to_string(),
                    },
                )));
            }
        }
    }
    Ok(out)
}

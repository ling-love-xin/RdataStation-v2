//! 表资产快照闭环：显式归档、版本恢复、checkout 到工作表、再归档。
//!
//! 分工：
//! - 本模块负责 DuckDB 物理动作（DESCRIBE / 指纹 / CTAS / DROP）与编排；
//! - 元数据落库走 [`AnalyticsResourceStore`] 的 `table_asset` 方法；
//! - 调用方必须传入**项目写连接**（`ProjectDuckdbConnection`），不在这里另开文件连接。
//!
//! 旧「活表指针」路径不参与本闭环：一版一张 `arc_<snapshot>` 只读快照，
//! 编辑一律 checkout 到 `rds_work` 工作表。

use std::sync::Arc;

use engine::persistence::project_db::ProjectDuckdbConnection;
use engine::persistence::ProjectDatabaseManager;
use shared::error::{CommonError, CoreError, StorageError};

use crate::model::{ArchiveBinding, ArchiveKind, NewArchiveInput, PayloadMode, TimelineEventKind};
use crate::store::table_asset::{
    NewTableSnapshot, NewTableVersion, SnapshotInstance, SnapshotState, TableCheckoutLease,
};
use crate::store::AnalyticsResourceStore;
use crate::table_asset::manifest::{
    snapshot_table_name, work_table_name, DataFingerprint, TableColumn, TableManifest, TableSchema,
};

/// 归档一个项目 `analytics` 里的普通表。
#[derive(Debug, Clone)]
pub struct ArchiveTableRequest {
    /// 资产显示名（空 = 对已有资产沿用原名；新资产必须给）。
    pub name: String,
    /// 源表 schema；普通分析表一般是 `main`。
    pub source_schema: String,
    /// 源表名。
    pub source_table: String,
    /// 追加到已有资产；`None` 时按显示名找同名活跃资产。
    pub asset_id: Option<String>,
    /// 默认 `true`：显式归档消费源表（T8）。
    pub consume_source: bool,
}

/// 归档结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveTableOutcome {
    pub resource_id: String,
    pub name: String,
    pub version: i32,
    pub snapshot_id: String,
    pub manifest_id: String,
    pub created_new_version: bool,
    /// 源表是否已被消费（`false` 可能是幂等复用快照，或 DROP 失败）。
    pub source_consumed: bool,
}

/// checkout 到可写工作表的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCheckoutOutcome {
    pub resource_id: String,
    pub base_version: i32,
    pub lease_id: String,
    pub work_schema: String,
    pub work_table: String,
}

/// 再归档结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableConsumeOutcome {
    pub resource_id: String,
    pub version: i32,
    pub snapshot_id: String,
    pub manifest_id: String,
    pub created_new_version: bool,
    pub work_dropped: bool,
}

/// 显式归档：把普通表冻结成不可变快照，默认消费源表。
pub async fn archive_table(
    store: &AnalyticsResourceStore,
    duckdb: &Arc<ProjectDuckdbConnection>,
    request: &ArchiveTableRequest,
) -> Result<ArchiveTableOutcome, CoreError> {
    let source_schema = if request.source_schema.trim().is_empty() {
        "main".to_string()
    } else {
        request.source_schema.trim().to_string()
    };
    let source_table = request.source_table.trim().to_string();
    if source_table.is_empty() {
        return Err(user_err("归档表需要源表名"));
    }

    let existing = match request.asset_id.as_deref() {
        Some(id) => Some(store.get_resource_by_id(id).await?),
        None if !request.name.trim().is_empty() => store.latest_alive_by_name(request.name.trim()).await?,
        None => None,
    };
    if let Some(row) = &existing {
        if row.deleted_at.is_some() {
            return Err(user_err("资产已删除，不能继续追加版本"));
        }
        if ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref())
            != ArchiveKind::AnalyticsTable
        {
            return Err(user_err("目标资产不是表资产，不能写入表快照"));
        }
    }

    // 1) 读结构 + 数据指纹，并判断是否需要新快照。
    let (manifest, snapshot) = {
        let handle = duckdb.acquire().await?;
        let guard = handle.lock().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| user_err("项目 DuckDB 写连接未就绪"))?;
        if !table_exists(conn, &source_schema, &source_table) {
            return Err(user_err(&format!(
                "源表 {source_schema}.{source_table} 不存在"
            )));
        }
        let manifest = manifest_for_table(conn, &source_schema, &source_table)?;
        let reusable = match store
            .table_snapshot_for_manifest(&manifest.manifest_id, SnapshotInstance::Project)
            .await
        {
            Ok(snapshot) => snapshot.filter(|snapshot| {
                table_exists(conn, &snapshot.physical_schema, &snapshot.physical_table)
            }),
            Err(_) => None,
        };
        let snapshot = match reusable {
            Some(snapshot) => snapshot_draft(snapshot),
            None => {
                let snapshot_id = new_snapshot_id();
                let physical_table = snapshot_table_name(&snapshot_id);
                copy_table(conn, &source_schema, &source_table, "rds_archive", &physical_table)?;
                NewTableSnapshot {
                    snapshot_id,
                    instance: SnapshotInstance::Project,
                    physical_schema: "rds_archive".to_string(),
                    physical_table,
                }
            }
        };
        (manifest, snapshot)
    };

    // 2) 写元数据：首次建行 + v1，或追加新版本。
    let outcome = match existing {
        Some(current) => {
            let head = store.table_asset_version(&current.id, current.version).await?;
            let same_as_head = head
                .as_ref()
                .map(|version| version.manifest_id == manifest.manifest_id)
                .unwrap_or(false);
            if same_as_head
                && current.analytics_table.as_deref() == Some(snapshot.physical_table.as_str())
            {
                let consumed = consume_source_if_needed(duckdb, request.consume_source, &source_schema, &source_table, &snapshot.physical_table).await;
                return Ok(ArchiveTableOutcome {
                    resource_id: current.id,
                    name: current.name,
                    version: current.version,
                    snapshot_id: snapshot.snapshot_id,
                    manifest_id: manifest.manifest_id,
                    created_new_version: false,
                    source_consumed: consumed,
                });
            }
            let new_version = current.version + 1;
            let detail = format!("归档普通表 {source_schema}.{source_table} 为 v{new_version}");
            let version_input = NewTableVersion {
                asset_id: current.id.clone(),
                version: new_version,
                parent_version: Some(current.version),
                manifest: manifest.clone(),
                snapshot: snapshot.clone(),
            };
            let updated = store
                .commit_table_version_to_head(
                    &version_input,
                    TimelineEventKind::Updated,
                    Some(&detail),
                    None,
                )
                .await?;
            ArchiveTableOutcome {
                resource_id: updated.id,
                name: updated.name,
                version: updated.version,
                snapshot_id: snapshot.snapshot_id,
                manifest_id: manifest.manifest_id,
                created_new_version: true,
                source_consumed: false,
            }
        }
        None => {
            if request.name.trim().is_empty() {
                return Err(user_err("新表资产需要显示名"));
            }
            let detail = format!("归档普通表 {source_schema}.{source_table}");
            let input = NewArchiveInput {
                resource_type: "analysis".to_string(),
                name: request.name.trim().to_string(),
                alias: None,
                kind: ArchiveKind::AnalyticsTable,
                content_hash: manifest.manifest_id.clone(),
                definition_sql: None,
                row_count: Some(clamp_i32(manifest.row_count)),
                column_count: Some(clamp_i32(manifest.column_count)),
                file_rel_path: String::new(),
                payload_mode: PayloadMode::Copy,
                source_path: None,
                file_size: None,
                structured: true,
                binding: ArchiveBinding {
                    promoted_from: None,
                    source_connection_id: None,
                    source_table: Some(format!("{source_schema}.{source_table}")),
                },
                scope: "project".to_string(),
            };
            let created = store
                .insert_table_asset_with_version(
                    input,
                    &snapshot.physical_table,
                    &manifest,
                    &snapshot,
                    Some(&detail),
                )
                .await?;
            ArchiveTableOutcome {
                resource_id: created.id,
                name: created.name,
                version: created.version,
                snapshot_id: snapshot.snapshot_id,
                manifest_id: manifest.manifest_id,
                created_new_version: true,
                source_consumed: false,
            }
        }
    };

    let consumed = consume_source_if_needed(
        duckdb,
        request.consume_source,
        &source_schema,
        &source_table,
        &outcome.snapshot_id,
    )
    .await;
    Ok(ArchiveTableOutcome {
        source_consumed: consumed,
        ..outcome
    })
}

/// 宿主入口：按项目根打开项目库（带短重试）后归档。
///
/// 仅给后台 worker 用；UI 线程已有项目写连接时不要走这里，避免再开文件连接。
pub async fn archive_table_at(
    project_root: &std::path::Path,
    request: &ArchiveTableRequest,
) -> Result<ArchiveTableOutcome, CoreError> {
    const ATTEMPTS: usize = 3;
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        match ProjectDatabaseManager::open(project_root, 4).await {
            Ok(manager) => {
                let store = AnalyticsResourceStore::new(manager.sqlite_pool());
                let duckdb = manager
                    .duckdb_conn()
                    .ok_or_else(|| user_err("项目 DuckDB 写连接未就绪"))?;
                return archive_table(&store, &duckdb, request).await;
            }
            Err(error) => {
                let text = error.to_string();
                let busy =
                    text.contains("另一个程序正在使用此文件") || text.contains("already open");
                last = if busy {
                    format!("项目分析库正被别的操作占用（{text}）")
                } else {
                    format!("打开项目库失败：{text}")
                };
                if !busy {
                    return Err(user_err(&last));
                }
                if attempt + 1 < ATTEMPTS {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                }
            }
        }
    }
    Err(user_err(&format!("{last}；请稍后重试")))
}

/// 恢复历史版本：**新生成一版指向旧快照**，零复制，不改写历史版本号。
pub async fn restore_table_asset_version(
    store: &AnalyticsResourceStore,
    asset_id: &str,
    version: i32,
) -> Result<ArchiveTableOutcome, CoreError> {
    let current = store.get_resource_by_id(asset_id).await?;
    if current.deleted_at.is_some() {
        return Err(user_err("资产已删除，不能恢复"));
    }
    if ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
        != ArchiveKind::AnalyticsTable
    {
        return Err(user_err("该资产不是表资产，不能用表快照恢复"));
    }
    if current.version == version {
        return Ok(ArchiveTableOutcome {
            resource_id: current.id,
            name: current.name,
            version: current.version,
            snapshot_id: current.analytics_table.clone().unwrap_or_default(),
            manifest_id: current.content_hash.clone().unwrap_or_default(),
            created_new_version: false,
            source_consumed: false,
        });
    }
    let target = store
        .table_asset_version(asset_id, version)
        .await?
        .ok_or_else(|| user_err(&format!("资产 {asset_id} 没有 v{version} 快照")))?;
    let manifest = store
        .table_manifest(&target.manifest_id)
        .await?
        .ok_or_else(|| user_err(&format!("v{version} 的 manifest 不存在")))?;
    let snapshot = store
        .table_snapshot(&target.snapshot_id)
        .await?
        .ok_or_else(|| user_err(&format!("v{version} 的快照不存在")))?;
    let new_version = current.version + 1;
    let detail = format!("恢复 v{version} 的冻结快照为 v{new_version}");
    let input = NewTableVersion {
        asset_id: current.id.clone(),
        version: new_version,
        parent_version: Some(current.version),
        manifest,
        snapshot: NewTableSnapshot {
            snapshot_id: snapshot.snapshot_id,
            instance: snapshot.instance,
            physical_schema: snapshot.physical_schema,
            physical_table: snapshot.physical_table,
        },
    };
    let updated = store
        .commit_table_version_to_head(&input, TimelineEventKind::Restored, Some(&detail), None)
        .await?;
    Ok(ArchiveTableOutcome {
        resource_id: updated.id,
        name: updated.name,
        version: updated.version,
        snapshot_id: input.snapshot.snapshot_id,
        manifest_id: input.manifest.manifest_id,
        created_new_version: true,
        source_consumed: false,
    })
}

/// 把当前版本的不可变快照复制到 `rds_work` 工作表，并写 active 租约。
pub async fn checkout_table_asset(
    store: &AnalyticsResourceStore,
    duckdb: &Arc<ProjectDuckdbConnection>,
    asset_id: &str,
) -> Result<TableCheckoutOutcome, CoreError> {
    let current = store.get_resource_by_id(asset_id).await?;
    if current.deleted_at.is_some() {
        return Err(user_err("资产已删除，不能检出"));
    }
    if ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
        != ArchiveKind::AnalyticsTable
    {
        return Err(user_err("该资产不是表资产，不能检出工作表"));
    }
    if let Some(lease) = store.active_table_lease(asset_id).await? {
        return Err(user_err(&format!(
            "该资产已有活跃工作表 {}.{}（租约 {}）",
            lease.work_schema, lease.work_table, lease.lease_id
        )));
    }
    let version = store
        .table_asset_version(asset_id, current.version)
        .await?
        .ok_or_else(|| user_err("当前版本没有快照索引"))?;
    let snapshot = store
        .table_snapshot(&version.snapshot_id)
        .await?
        .ok_or_else(|| user_err("当前版本的快照不存在"))?;
    if snapshot.state != SnapshotState::Live {
        return Err(user_err("当前版本的快照已标记为丢失，不能检出"));
    }

    let token = uuid::Uuid::new_v4().simple().to_string();
    let work_schema = "rds_work".to_string();
    let work_table = work_table_name(asset_id, &token);
    {
        let handle = duckdb.acquire().await?;
        let guard = handle.lock().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| user_err("项目 DuckDB 写连接未就绪"))?;
        if !table_exists(conn, &snapshot.physical_schema, &snapshot.physical_table) {
            return Err(user_err("快照物理表不存在，无法检出"));
        }
        copy_table(
            conn,
            &snapshot.physical_schema,
            &snapshot.physical_table,
            &work_schema,
            &work_table,
        )?;
    }

    let lease = match store
        .create_table_lease(asset_id, current.version, &work_schema, &work_table)
        .await
    {
        Ok(lease) => lease,
        Err(error) => {
            drop_table_best_effort(duckdb, &work_schema, &work_table).await;
            return Err(error);
        }
    };
    Ok(TableCheckoutOutcome {
        resource_id: current.id,
        base_version: current.version,
        lease_id: lease.lease_id,
        work_schema,
        work_table,
    })
}

/// 再归档：把工作表冻结成新快照，消费租约并删除工作表。
pub async fn consume_table_checkout(
    store: &AnalyticsResourceStore,
    duckdb: &Arc<ProjectDuckdbConnection>,
    asset_id: &str,
) -> Result<TableConsumeOutcome, CoreError> {
    let current = store.get_resource_by_id(asset_id).await?;
    let lease: TableCheckoutLease = store
        .active_table_lease(asset_id)
        .await?
        .ok_or_else(|| user_err("该资产没有活跃工作表，不能再归档"))?;
    if current.version != lease.base_version {
        return Err(user_err(&format!(
            "资产当前是 v{}，与工作表基准 v{} 不一致；请重新检出",
            current.version, lease.base_version
        )));
    }

    let (manifest, snapshot, unchanged) = {
        let handle = duckdb.acquire().await?;
        let guard = handle.lock().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| user_err("项目 DuckDB 写连接未就绪"))?;
        if !table_exists(conn, &lease.work_schema, &lease.work_table) {
            return Err(user_err("工作表不存在，无法再归档"));
        }
        let manifest = manifest_for_table(conn, &lease.work_schema, &lease.work_table)?;
        let base = store
            .table_asset_version(asset_id, lease.base_version)
            .await?
            .ok_or_else(|| user_err("工作表基准版本没有快照索引"))?;
        if base.manifest_id == manifest.manifest_id {
            (manifest, None, true)
        } else {
            let reusable = store
                .table_snapshot_for_manifest(&manifest.manifest_id, SnapshotInstance::Project)
                .await
                .ok()
                .flatten()
                .filter(|snapshot| {
                    table_exists(conn, &snapshot.physical_schema, &snapshot.physical_table)
                });
            let snapshot = match reusable {
                Some(snapshot) => snapshot_draft(snapshot),
                None => {
                    let snapshot_id = new_snapshot_id();
                    let physical_table = snapshot_table_name(&snapshot_id);
                    copy_table(
                        conn,
                        &lease.work_schema,
                        &lease.work_table,
                        "rds_archive",
                        &physical_table,
                    )?;
                    NewTableSnapshot {
                        snapshot_id,
                        instance: SnapshotInstance::Project,
                        physical_schema: "rds_archive".to_string(),
                        physical_table,
                    }
                }
            };
            (manifest, Some(snapshot), false)
        }
    };

    if unchanged {
        store.consume_table_lease(&lease.lease_id, current.version).await?;
        let work_dropped =
            drop_table(duckdb, &lease.work_schema, &lease.work_table).await.is_ok();
        return Ok(TableConsumeOutcome {
            resource_id: current.id,
            version: current.version,
            snapshot_id: store
                .table_asset_version(asset_id, current.version)
                .await?
                .map(|version| version.snapshot_id)
                .unwrap_or_default(),
            manifest_id: manifest.manifest_id,
            created_new_version: false,
            work_dropped,
        });
    }

    let snapshot = snapshot.expect("变化时必须生成快照");
    let new_version = current.version + 1;
    let detail = format!("再归档工作表 {} 为 v{new_version}", lease.work_table);
    let input = NewTableVersion {
        asset_id: current.id.clone(),
        version: new_version,
        parent_version: Some(current.version),
        manifest: manifest.clone(),
        snapshot: snapshot.clone(),
    };
    let updated = match store
        .commit_table_version_to_head(
            &input,
            TimelineEventKind::Updated,
            Some(&detail),
            Some(&lease.lease_id),
        )
        .await
    {
        Ok(updated) => updated,
        Err(error) => {
            drop_table_best_effort(duckdb, &snapshot.physical_schema, &snapshot.physical_table)
                .await;
            return Err(error);
        }
    };
    let work_dropped = drop_table(duckdb, &lease.work_schema, &lease.work_table)
        .await
        .is_ok();
    Ok(TableConsumeOutcome {
        resource_id: updated.id,
        version: updated.version,
        snapshot_id: snapshot.snapshot_id,
        manifest_id: manifest.manifest_id,
        created_new_version: true,
        work_dropped,
    })
}

/// 放弃 checkout：租约标 abandoned，工作表删除。
pub async fn abandon_table_checkout(
    store: &AnalyticsResourceStore,
    duckdb: &Arc<ProjectDuckdbConnection>,
    asset_id: &str,
) -> Result<TableCheckoutLease, CoreError> {
    let lease = store
        .active_table_lease(asset_id)
        .await?
        .ok_or_else(|| user_err("该资产没有活跃工作表"))?;
    store.abandon_table_lease(&lease.lease_id).await?;
    let _ = drop_table(duckdb, &lease.work_schema, &lease.work_table).await;
    Ok(lease)
}

/// 表结构 + 数据指纹 → manifest。
pub fn manifest_for_table(
    conn: &duckdb::Connection,
    schema: &str,
    table: &str,
) -> Result<TableManifest, CoreError> {
    let qualified = qualified_name(schema, table);
    let mut stmt = conn
        .prepare(&format!("DESCRIBE {qualified}"))
        .map_err(|error| db_err("describe", error))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TableColumn::new(
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?.eq_ignore_ascii_case("YES"),
                row.get::<_, Option<String>>(4)?,
                0,
            ))
        })
        .map_err(|error| db_err("describe_rows", error))?;
    let mut columns = Vec::new();
    for (ordinal, row) in rows.enumerate() {
        let mut column = row.map_err(|error| db_err("describe_row", error))?;
        column.ordinal = i32::try_from(ordinal).unwrap_or(i32::MAX);
        columns.push(column);
    }
    let schema_model = TableSchema::new(columns)?;

    let hash_args = schema_model
        .columns
        .iter()
        .map(|column| quote_ident(&column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let fingerprint_sql = format!(
        "SELECT count(*) AS rows, \
         CAST(COALESCE(sum(hash({hash_args})),0) AS VARCHAR) AS sum_hash, \
         CAST(COALESCE(bit_xor(hash({hash_args})),0) AS VARCHAR) AS xor_hash, \
         CAST(min(hash({hash_args})) AS VARCHAR) AS min_hash, \
         CAST(max(hash({hash_args})) AS VARCHAR) AS max_hash \
         FROM {qualified}"
    );
    let (row_count, sum_hash, xor_hash, min_hash, max_hash) = conn
        .query_row(&fingerprint_sql, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|error| db_err("fingerprint", error))?;
    let data = DataFingerprint::new(
        row_count,
        i64::try_from(schema_model.columns.len()).unwrap_or(i64::MAX),
        sum_hash,
        xor_hash,
        min_hash,
        max_hash,
    );
    TableManifest::build(&schema_model, &data)
}

/// 物理表是否存在（当前 catalog 内）。
pub fn table_exists(conn: &duckdb::Connection, schema: &str, table: &str) -> bool {
    conn.prepare(&format!("SELECT * FROM {} LIMIT 0", qualified_name(schema, table)))
        .is_ok()
}

fn copy_table(
    conn: &duckdb::Connection,
    source_schema: &str,
    source_table: &str,
    dest_schema: &str,
    dest_table: &str,
) -> Result<(), CoreError> {
    conn.execute_batch(&format!(
        "CREATE SCHEMA IF NOT EXISTS {};\nCREATE TABLE {} AS SELECT * FROM {};",
        quote_ident(dest_schema),
        qualified_name(dest_schema, dest_table),
        qualified_name(source_schema, source_table),
    ))
    .map_err(|error| db_err("copy_table", error))
}

async fn drop_table(
    duckdb: &Arc<ProjectDuckdbConnection>,
    schema: &str,
    table: &str,
) -> Result<(), CoreError> {
    let handle = duckdb.acquire().await?;
    let guard = handle.lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| user_err("项目 DuckDB 写连接未就绪"))?;
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS {}",
        qualified_name(schema, table)
    ))
    .map_err(|error| db_err("drop_table", error))
}

async fn drop_table_best_effort(
    duckdb: &Arc<ProjectDuckdbConnection>,
    schema: &str,
    table: &str,
) {
    if let Err(error) = drop_table(duckdb, schema, table).await {
        tracing::warn!(schema, table, error = %error, "清理物理表失败");
    }
}

async fn consume_source_if_needed(
    duckdb: &Arc<ProjectDuckdbConnection>,
    consume_source: bool,
    source_schema: &str,
    source_table: &str,
    snapshot_table: &str,
) -> bool {
    if !consume_source || source_schema == "rds_archive" || source_table == snapshot_table {
        return false;
    }
    match drop_table(duckdb, source_schema, source_table).await {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                schema = source_schema,
                table = source_table,
                error = %error,
                "归档已提交，但消费源表失败"
            );
            false
        }
    }
}

fn new_snapshot_id() -> String {
    format!("tsnap_{}", uuid::Uuid::new_v4().simple())
}

fn snapshot_draft(snapshot: crate::store::table_asset::TableSnapshot) -> NewTableSnapshot {
    NewTableSnapshot {
        snapshot_id: snapshot.snapshot_id,
        instance: snapshot.instance,
        physical_schema: snapshot.physical_schema,
        physical_table: snapshot.physical_table,
    }
}

fn qualified_name(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

fn quote_ident(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn user_err(reason: &str) -> CoreError {
    CoreError::common(CommonError::General(reason.to_string()))
}

fn db_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "table_asset_snapshots".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::persistence::ProjectDatabaseManager;

    struct Env {
        dir: std::path::PathBuf,
        manager: ProjectDatabaseManager,
        store: AnalyticsResourceStore,
        duckdb: Arc<ProjectDuckdbConnection>,
    }

    async fn env(tag: &str) -> Env {
        let dir = crate::tests::temp_project_dir(tag);
        let manager = ProjectDatabaseManager::open(&dir, 2)
            .await
            .expect("open project");
        let store = AnalyticsResourceStore::new(manager.sqlite_pool());
        let duckdb = manager.duckdb_conn().expect("write handle");
        Env {
            dir,
            manager,
            store,
            duckdb,
        }
    }

    fn cleanup(env: Env) {
        let Env {
            dir,
            manager,
            store,
            duckdb,
        } = env;
        drop(store);
        drop(duckdb);
        drop(manager);
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn exec(duckdb: &Arc<ProjectDuckdbConnection>, sql: &str) {
        let handle = duckdb.acquire().await.expect("acquire");
        let guard = handle.lock().await;
        guard
            .as_ref()
            .expect("conn")
            .execute_batch(sql)
            .expect("exec");
    }

    async fn query_count(duckdb: &Arc<ProjectDuckdbConnection>, sql: &str) -> i64 {
        let handle = duckdb.acquire().await.expect("acquire");
        let guard = handle.lock().await;
        guard
            .as_ref()
            .expect("conn")
            .query_row(sql, [], |row| row.get(0))
            .expect("count")
    }

    async fn exists(duckdb: &Arc<ProjectDuckdbConnection>, schema: &str, table: &str) -> bool {
        let handle = duckdb.acquire().await.expect("acquire");
        let guard = handle.lock().await;
        table_exists(guard.as_ref().expect("conn"), schema, table)
    }

    fn archive_request(name: &str, table: &str) -> ArchiveTableRequest {
        ArchiveTableRequest {
            name: name.to_string(),
            source_schema: "main".to_string(),
            source_table: table.to_string(),
            asset_id: None,
            consume_source: true,
        }
    }

    #[tokio::test]
    async fn archive_freezes_an_immutable_snapshot_and_consumes_source() {
        let env = env("ta_archive").await;
        exec(
            &env.duckdb,
            "CREATE TABLE main.orders AS SELECT 1::BIGINT AS id, 'a'::VARCHAR AS side \
             UNION ALL SELECT 2, 'b';",
        )
        .await;

        let outcome = archive_table(&env.store, &env.duckdb, &archive_request("订单快照", "orders"))
            .await
            .expect("archive");

        assert_eq!(outcome.version, 1);
        assert!(outcome.source_consumed, "显式归档消费源表");
        let row = env
            .store
            .get_resource_by_id(&outcome.resource_id)
            .await
            .expect("row");
        assert_eq!(
            ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref()),
            ArchiveKind::AnalyticsTable
        );
        let snapshot_table = snapshot_table_name(&outcome.snapshot_id);
        assert_eq!(row.analytics_table.as_deref(), Some(snapshot_table.as_str()));
        assert!(
            !exists(&env.duckdb, "main", "orders").await,
            "源表已被消费"
        );
        assert_eq!(
            query_count(
                &env.duckdb,
                &format!(
                    "SELECT count(*) FROM {}",
                    qualified_name("rds_archive", &snapshot_table)
                )
            )
            .await,
            2
        );
        let version = env
            .store
            .table_asset_version(&outcome.resource_id, 1)
            .await
            .expect("version")
            .expect("version row");
        let manifest = env
            .store
            .table_manifest(&version.manifest_id)
            .await
            .expect("manifest")
            .expect("manifest row");
        assert_eq!(manifest.row_count, 2);
        cleanup(env);
    }

    #[tokio::test]
    async fn checkout_consume_and_restore_keep_snapshots() {
        let env = env("ta_flow").await;
        exec(
            &env.duckdb,
            "CREATE TABLE main.orders AS SELECT 1::BIGINT AS id, 10::DOUBLE AS amount;",
        )
        .await;
        let archived = archive_table(&env.store, &env.duckdb, &archive_request("订单", "orders"))
            .await
            .expect("archive");

        let checkout = checkout_table_asset(&env.store, &env.duckdb, &archived.resource_id)
            .await
            .expect("checkout");
        assert_eq!(checkout.base_version, 1);
        assert_eq!(
            query_count(
                &env.duckdb,
                &format!(
                    "SELECT count(*) FROM {}.{}",
                    quote_ident(&checkout.work_schema),
                    quote_ident(&checkout.work_table)
                )
            )
            .await,
            1
        );
        exec(
            &env.duckdb,
            &format!(
                "INSERT INTO {}.{} VALUES (2, 20.0)",
                quote_ident(&checkout.work_schema),
                quote_ident(&checkout.work_table)
            ),
        )
        .await;

        let consumed = consume_table_checkout(&env.store, &env.duckdb, &archived.resource_id)
            .await
            .expect("consume");
        assert_eq!(consumed.version, 2);
        assert!(consumed.created_new_version);
        assert!(consumed.work_dropped);
        assert!(env
            .store
            .active_table_lease(&archived.resource_id)
            .await
            .expect("lease")
            .is_none());

        let abandoned = checkout_table_asset(&env.store, &env.duckdb, &archived.resource_id)
            .await
            .expect("checkout again");
        abandon_table_checkout(&env.store, &env.duckdb, &archived.resource_id)
            .await
            .expect("abandon");
        assert!(!exists(
            &env.duckdb,
            &abandoned.work_schema,
            &abandoned.work_table
        )
        .await);

        let restored = restore_table_asset_version(&env.store, &archived.resource_id, 1)
            .await
            .expect("restore");
        assert_eq!(restored.version, 3);
        assert!(restored.created_new_version);
        let row = env
            .store
            .get_resource_by_id(&archived.resource_id)
            .await
            .expect("row");
        let restored_table = snapshot_table_name(&archived.snapshot_id);
        assert_eq!(row.analytics_table.as_deref(), Some(restored_table.as_str()));
        let versions = env
            .store
            .table_asset_versions(&archived.resource_id)
            .await
            .expect("versions");
        assert_eq!(versions.len(), 3, "v1/v2/v3 都保留");
        assert_eq!(versions[2].manifest_id, versions[0].manifest_id, "恢复共享 v1 manifest");
        cleanup(env);
    }
}

//! 文件资产的 CAS 版本索引与取回租约。
//!
//! 这里只读写 `project.db`：manifest 内容在 `.RSmeta/cas/`，本模块把
//! `manifest_id / content_hash / meta_hash / parent_version` 与资产版本号绑定。

use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use shared::error::{CoreError, StorageError};

use super::AnalyticsResourceStore;

/// 文件资产的一个版本索引。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAssetVersion {
    pub resource_id: String,
    pub version: i32,
    pub manifest_id: String,
    pub content_hash: String,
    pub meta_hash: Option<String>,
    pub parent_version: Option<i32>,
    pub content_pruned: bool,
    pub created_at: DateTime<Utc>,
}

/// 写入文件版本所需的最小输入（与 manifest 同批落地）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFileAssetVersion {
    pub manifest_id: String,
    pub content_hash: String,
    pub meta_hash: Option<String>,
    pub parent_version: Option<i32>,
}

/// 一次取回租约。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutLease {
    pub lease_id: String,
    pub resource_id: String,
    pub base_scope: String,
    pub base_version: i32,
    pub dest_rel: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub consumed_version: Option<i32>,
}

impl AnalyticsResourceStore {
    /// 在调用方事务里写一条文件版本索引（供首次归档与再归档复用）。
    pub(crate) fn insert_file_asset_version_on(
        conn: &rusqlite::Connection,
        resource_id: &str,
        version: i32,
        file: &NewFileAssetVersion,
    ) -> Result<(), CoreError> {
        conn.execute(
            r#"
            INSERT INTO file_asset_versions (
                resource_id, version, manifest_id, content_hash, meta_hash,
                parent_version, content_pruned, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, 0, ?)
            "#,
            rusqlite::params![
                resource_id,
                version,
                &file.manifest_id,
                &file.content_hash,
                &file.meta_hash,
                file.parent_version,
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(|error| fav_err("insert", error))?;
        Ok(())
    }

    /// 某资产某版本的文件索引；旧资产没有这一行时返回 `None`。
    pub async fn file_asset_version(
        &self,
        resource_id: &str,
        version: i32,
    ) -> Result<Option<FileAssetVersion>, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                r#"
                SELECT resource_id, version, manifest_id, content_hash, meta_hash,
                       parent_version, content_pruned, created_at
                FROM file_asset_versions
                WHERE resource_id = ? AND version = ?
                "#,
                rusqlite::params![resource_id, version],
                map_file_version,
            )
            .optional()
            .map_err(|error| fav_err("select_one", error))
    }

    /// 某资产的版本索引（升序），供迁移 / 诊断。
    pub async fn list_file_asset_versions(
        &self,
        resource_id: &str,
    ) -> Result<Vec<FileAssetVersion>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
                SELECT resource_id, version, manifest_id, content_hash, meta_hash,
                       parent_version, content_pruned, created_at
                FROM file_asset_versions
                WHERE resource_id = ?
                ORDER BY version ASC
                "#,
            )
            .map_err(|error| fav_err("prepare_list", error))?;
        let rows = stmt
            .query_map(rusqlite::params![resource_id], map_file_version)
            .map_err(|error| fav_err("query_list", error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| fav_err("collect_list", error))
    }

    /// 当前 active 租约（同资产同 scope 最多一条，由唯一索引兜底）。
    pub async fn active_lease(
        &self,
        resource_id: &str,
    ) -> Result<Option<CheckoutLease>, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                r#"
                SELECT lease_id, resource_id, base_scope, base_version, dest_rel,
                       state, created_at, closed_at, consumed_version
                FROM checkout_leases
                WHERE resource_id = ? AND state = 'active'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                rusqlite::params![resource_id],
                map_lease,
            )
            .optional()
            .map_err(|error| lease_err("active", error))
    }

    /// 一次取出所有 active 租约：刷新快照时按资源 id 建映射，避免详情区逐行 N+1。
    ///
    /// 同一资产同 scope 至多一条（唯一索引），所以这里直接按资源 id 收成映射；
    /// 历史 / 已消费 / 已放弃的租约留在表里做审计，不出现在这份结果里。
    pub async fn active_leases(
        &self,
    ) -> Result<std::collections::HashMap<String, CheckoutLease>, CoreError> {
        let conn = self.get_conn().await?;
        let mut stmt = conn
            .inner()?
            .prepare(
                r#"
                SELECT lease_id, resource_id, base_scope, base_version, dest_rel,
                       state, created_at, closed_at, consumed_version
                FROM checkout_leases
                WHERE state = 'active'
                ORDER BY created_at DESC
                "#,
            )
            .map_err(|error| lease_err("active_list_prepare", error))?;
        let rows = stmt
            .query_map([], map_lease)
            .map_err(|error| lease_err("active_list_query", error))?;
        let mut leases = std::collections::HashMap::new();
        for row in rows {
            let lease = row.map_err(|error| lease_err("active_list_collect", error))?;
            // 同 id 只保留最新一条（理论上不会有第二条，唯一索引已经挡住）。
            leases.entry(lease.resource_id.clone()).or_insert(lease);
        }
        Ok(leases)
    }

    /// 按工作副本相对路径反查 active 租约（再归档时确认 base）。
    pub async fn lease_by_dest(
        &self,
        dest_rel: &str,
    ) -> Result<Option<CheckoutLease>, CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .query_row(
                r#"
                SELECT lease_id, resource_id, base_scope, base_version, dest_rel,
                       state, created_at, closed_at, consumed_version
                FROM checkout_leases
                WHERE dest_rel = ? AND state = 'active'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                rusqlite::params![dest_rel],
                map_lease,
            )
            .optional()
            .map_err(|error| lease_err("by_dest", error))
    }

    /// 创建取回租约；同资产已有 active 时由唯一索引拒绝。
    pub async fn create_lease(
        &self,
        resource_id: &str,
        base_scope: &str,
        base_version: i32,
        dest_rel: &str,
    ) -> Result<CheckoutLease, CoreError> {
        let conn = self.get_conn().await?;
        let lease_id = format!("lease_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        conn.inner()?
            .execute(
                r#"
                INSERT INTO checkout_leases (
                    lease_id, resource_id, base_scope, base_version,
                    dest_rel, state, created_at
                ) VALUES (?, ?, ?, ?, ?, 'active', ?)
                "#,
                rusqlite::params![&lease_id, resource_id, base_scope, base_version, dest_rel, now],
            )
            .map_err(|error| {
                let text = error.to_string();
                if text.contains("UNIQUE") || text.contains("unique") {
                    lease_err(
                        "create",
                        format!(
                            "该资产已有活跃取回（{}）；请先打开已有草稿、结束取回或显式 fork",
                            resource_id
                        ),
                    )
                } else {
                    lease_err("create", error)
                }
            })?;
        self.active_lease(resource_id)
            .await?
            .ok_or_else(|| lease_err("create", "租约写入后读不到 active 行"))
    }

    /// 消费租约：再归档成功时调用。
    pub async fn consume_lease(
        &self,
        lease_id: &str,
        consumed_version: i32,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                r#"
                UPDATE checkout_leases
                SET state = 'consumed', closed_at = ?, consumed_version = ?
                WHERE lease_id = ? AND state = 'active'
                "#,
                rusqlite::params![Utc::now().to_rfc3339(), consumed_version, lease_id],
            )
            .map_err(|error| lease_err("consume", error))?;
        if affected == 0 {
            return Err(lease_err("consume", "租约不存在或已经结束"));
        }
        Ok(())
    }

    /// 放弃取回：草稿可以由用户继续保留/删除，但版本链不再认它。
    pub async fn abandon_lease(&self, lease_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                r#"
                UPDATE checkout_leases
                SET state = 'abandoned', closed_at = ?
                WHERE lease_id = ? AND state = 'active'
                "#,
                rusqlite::params![Utc::now().to_rfc3339(), lease_id],
            )
            .map_err(|error| lease_err("abandon", error))?;
        if affected == 0 {
            return Err(lease_err("abandon", "租约不存在或已经结束"));
        }
        Ok(())
    }
}

fn map_file_version(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileAssetVersion> {
    let created_at: String = row.get(7)?;
    Ok(FileAssetVersion {
        resource_id: row.get(0)?,
        version: row.get(1)?,
        manifest_id: row.get(2)?,
        content_hash: row.get(3)?,
        meta_hash: row.get(4)?,
        parent_version: row.get(5)?,
        content_pruned: row.get::<_, i64>(6)? != 0,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(created_at)?,
    })
}

fn map_lease(row: &rusqlite::Row<'_>) -> rusqlite::Result<CheckoutLease> {
    let created_at: String = row.get(6)?;
    let closed_at: Option<String> = row.get(7)?;
    Ok(CheckoutLease {
        lease_id: row.get(0)?,
        resource_id: row.get(1)?,
        base_scope: row.get(2)?,
        base_version: row.get(3)?,
        dest_rel: row.get(4)?,
        state: row.get(5)?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(created_at)?,
        closed_at: closed_at
            .and_then(|text| AnalyticsResourceStore::parse_datetime(text).ok()),
        consumed_version: row.get(8)?,
    })
}

fn fav_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "file_asset_versions".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

fn lease_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "checkout_leases".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

//! 取回凭据的读写（迁移 030）。
//!
//! 这是一条**回连**，不是登记行：它只回答「这份草稿是从哪条存档的哪个版本取回的」。
//! 有了它，「取回 → 改 → 再归档」才能回到原存档升版本，而不是另起一条新存档。

use std::path::Path;

use chrono::{DateTime, Utc};
use shared::error::{CoreError, StorageError};

use super::AnalyticsResourceStore;

/// 一条取回凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutReceipt {
    pub dest_path: String,
    pub resource_id: String,
    /// 取回时的存档版本（回溯起点，不随后续升版改写）。
    pub from_version: i32,
    pub created_at: DateTime<Utc>,
    /// 最近一次凭此凭据再归档的时间 / 生成的新版本；`None` = 还没再归档过。
    pub consumed_at: Option<DateTime<Utc>>,
    pub consumed_version: Option<i32>,
}

fn receipt_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resource_checkouts".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 路径规范化：取回与归档两侧必须用同一口径，否则 Windows 反斜杠会让凭据永远查不到。
pub fn normalize_dest(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn map_receipt(row: &rusqlite::Row<'_>) -> Result<CheckoutReceipt, rusqlite::Error> {
    let created: String = row.get(3)?;
    let consumed: Option<String> = row.get(4)?;
    let parse = |text: &str| {
        DateTime::parse_from_rfc3339(text)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now())
    };
    Ok(CheckoutReceipt {
        dest_path: row.get(0)?,
        resource_id: row.get(1)?,
        from_version: row.get(2)?,
        created_at: parse(&created),
        consumed_at: consumed.as_deref().map(parse),
        consumed_version: row.get(5)?,
    })
}

const RECEIPT_COLUMNS: &str =
    "dest_path, resource_id, from_version, created_at, consumed_at, consumed_version";

impl AnalyticsResourceStore {
    /// 记一次取回。同一路径重复取回覆盖为最新一次（旧草稿若还在，本就该跟着新版本走）。
    pub async fn record_checkout(
        &self,
        dest: &Path,
        resource_id: &str,
        from_version: i32,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute(
                "INSERT INTO analytics_resource_checkouts \
                 (dest_path, resource_id, from_version, created_at) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(dest_path) DO UPDATE SET \
                   resource_id = excluded.resource_id, \
                   from_version = excluded.from_version, \
                   created_at = excluded.created_at, \
                   consumed_at = NULL, consumed_version = NULL",
                rusqlite::params![
                    normalize_dest(dest),
                    resource_id,
                    from_version,
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|e| receipt_err("insert", e))?;
        Ok(())
    }

    /// 按工作副本路径反查（已消费的凭据仍然有效：同一份工作副本可以连续归档多次）。
    pub async fn find_checkout(&self, dest: &Path) -> Result<Option<CheckoutReceipt>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let mut stmt = inner
            .prepare(&format!(
                "SELECT {RECEIPT_COLUMNS} FROM analytics_resource_checkouts WHERE dest_path = ?1"
            ))
            .map_err(|e| receipt_err("prepare", e))?;
        match stmt.query_row(rusqlite::params![normalize_dest(dest)], map_receipt) {
            Ok(receipt) => Ok(Some(receipt)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(receipt_err("select", error)),
        }
    }

    /// 再归档成功后结案：记录本次生成的新版本，供资产履历合成「取回 → 升版」节点。
    pub async fn consume_checkout(&self, dest: &Path, new_version: i32) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute(
                "UPDATE analytics_resource_checkouts \
                 SET consumed_at = ?2, consumed_version = ?3 WHERE dest_path = ?1",
                rusqlite::params![normalize_dest(dest), Utc::now().to_rfc3339(), new_version],
            )
            .map_err(|e| receipt_err("update", e))?;
        Ok(())
    }

    /// 一条存档的全部取回凭据（时间倒序，时间线用）。
    pub async fn checkouts_for_resource(
        &self,
        resource_id: &str,
    ) -> Result<Vec<CheckoutReceipt>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let mut stmt = inner
            .prepare(&format!(
                "SELECT {RECEIPT_COLUMNS} FROM analytics_resource_checkouts \
                 WHERE resource_id = ?1 ORDER BY created_at DESC"
            ))
            .map_err(|e| receipt_err("prepare", e))?;
        let rows = stmt
            .query_map(rusqlite::params![resource_id], map_receipt)
            .map_err(|e| receipt_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| receipt_err("select", e))
    }
}

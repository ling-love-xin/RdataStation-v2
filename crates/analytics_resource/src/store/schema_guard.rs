//! M6 语义层表结构的幂等自检（迁移 027–030 的兜底）。
//!
//! 迁移账本 `get_current_version()` 取的是 `MAX(version)`，`manager.rs` 只跑
//! `version > current_version` 的文件。于是存在一个真实窗口：某项目库的账本里
//! 已经出现 ≥30 的版本号（手工修库 / 并行分支 / 未来的迁移号复用）时，
//! 027–030 会被**整批跳过**，而 `events` / `assertions` / `refs` / `checkouts`
//! 的 INSERT 没有 `table_exists` 保护，第一次写就会报 `no such table`。
//!
//! 这里不改动全局迁移执行器（那会牵动所有 crate 的迁移历史），只在 M6 自己
//! 打开项目库后用**与迁移文件同源**的 `IF NOT EXISTS` 补建一遍：
//! 幂等、无数据迁移、缺表时能自愈，已有表时是几次 no-op。
//!
//! 刻意不把失败升级成硬错误：只读项目本来就不该写 DDL，让它继续能浏览。

use shared::error::{CoreError, StorageError};

use super::AnalyticsResourceStore;

/// 与迁移文件同源，避免 schema 在两处各写一份而漂移。
const M6_TABLES_SQL: &str = concat!(
    include_str!("../../../store/migrations/project_meta/027_analytics_resource_timeline.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/028_analytics_resource_semantics.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/029_analytics_resource_refs.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/030_analytics_resource_checkouts.sql"),
);

impl AnalyticsResourceStore {
    /// 打开项目库后调用：确保 M6 的四组扩展表都在。
    pub(crate) async fn ensure_m6_tables(&self) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute_batch(M6_TABLES_SQL)
            .map_err(|error| CoreError::storage(StorageError::Persistence {
                store: "analytics_resource_schema".to_string(),
                operation: "ensure_m6_tables".to_string(),
                reason: error.to_string(),
            }))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// 故障注入：删掉一张扩展表后自检应把它补回来（模拟迁移被 MAX 账本跳过）。
    #[tokio::test]
    async fn ensure_m6_tables_recreates_a_missing_table() {
        let (store, dir) = crate::tests::create_test_store().await;
        {
            let conn = store.get_conn().await.expect("conn");
            conn.inner()
                .expect("inner")
                .execute_batch("DROP TABLE analytics_resource_checkouts;")
                .expect("drop");
        }
        let missing = {
            let conn = store.get_conn().await.expect("conn");
            conn.inner()
                .expect("inner")
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='analytics_resource_checkouts'",
                    [],
                    |row| row.get::<_, i32>(0),
                )
                .expect("count")
        };
        assert_eq!(missing, 0, "前置条件：表确实被删了");

        store.ensure_m6_tables().await.expect("ensure");
        let restored = {
            let conn = store.get_conn().await.expect("conn");
            conn.inner()
                .expect("inner")
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='analytics_resource_checkouts'",
                    [],
                    |row| row.get::<_, i32>(0),
                )
                .expect("count")
        };
        assert_eq!(restored, 1, "自检应补回缺表");
        // 再跑一次：幂等，不报错、不丢已有数据。
        store.ensure_m6_tables().await.expect("ensure twice");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}

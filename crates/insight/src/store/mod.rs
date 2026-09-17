//! 洞察快照的**领域存储**与装配点（M8）。
//!
//! # 布局
//!
//! | 子模块 | 内容 | 物理位置 |
//! | --- | --- | --- |
//! | [`body`] | 正文仓库：列快照 / 表质量报告 / Schema 报告 + `InsightStorage` 门面 | 项目 `analytics.duckdb` |
//! | [`meta`] | 元数据仓库：实体归属、行数、耗时、版本链 | 项目 `project.db` |
//! | 本文件 | 装配点 [`ProjectInsightStores`]（把「项目库」翻译成「洞察存储」） | — |
//!
//! # 为什么需要装配点
//!
//! 「保存快照 / 历史版本 / 版本对比 / 清理 / 存储统计」这五条链路在 v2 一期虽然
//! **全部编译通过**，却**一行都不可达**：`InsightStorage` 与 `InsightMetaStore`
//! 只在函数签名里作为参数出现，全仓没有任何地方构造它们。
//!
//! 根因是这两个门面都要求「项目库连接」——连接由项目打开流程持有，而洞察侧没有
//! 自己的项目句柄。本模块就是那个缺失的装配点。
//!
//! # 与 engine 的分工
//!
//! engine 提供**连接与迁移**（`ProjectDatabaseManager` 及其 SQLite/DuckDB 句柄），
//! 洞察侧提供**领域存储与装配**。表结构由 `ProjectDatabaseManager::open` 的迁移保证
//! （`project_analysis/002_insight_storage.sql` 建正文表，`project_meta/008_insight_snapshots.sql`
//! 建元数据表）。
//!
//! # 用法
//!
//! 优先用 [`ProjectInsightStores::from_project_db`] 复用调用方**已打开**的项目库，
//! 避免对同一个 `analytics.duckdb` 重复开连接；[`ProjectInsightStores::open`] 是
//! 给「手头只有项目根」的调用方的便捷入口。
//!
//! 归属变更（M8 Phase 0 / 0.2）：`body` / `meta` 两个仓库自 `engine/src/persistence/` 迁入。

pub mod body;
pub mod meta;

pub use body::{
    snapshot_checksum, InsightColumnStore, InsightSchemaReportStore, InsightStorage,
    InsightStorageStats, InsightTableReportStore, InsightVersionEntry,
};
pub use meta::{InsightMetaStore, InsightSnapshotMeta};

use std::path::Path;

use engine::persistence::project_db::ProjectDatabaseManager;
use shared::error::CoreError;

use crate::model::types::ColumnInsightFull;

/// 打开项目库时申请的 SQLite 连接池大小。
///
/// 洞察快照的 SQLite 访问是低频串行写 + 偶尔读，2 个连接足够；
/// 具体值只影响并发余量，不影响正确性。
const INSIGHT_SQLITE_POOL_SIZE: usize = 2;

/// 一个项目的洞察快照存储组合。
///
/// 两半是**成对**的：正文（DuckDB）与元数据（SQLite）通过 `snapshot_id` / `version_id`
/// 互相引用，只写一半会造成「历史列表有记录但读不出正文」这类不一致。
/// 因此本结构不提供分割访问的口径，要一起传。
pub struct ProjectInsightStores {
    /// 正文：列快照 / 表探查报告 / Schema 报告（后两者为 Phase 4 预留）。
    pub storage: InsightStorage,
    /// 元数据：实体归属（`entity_type` / `entity_name` / `entity_source`）+ 版本链。
    pub meta: InsightMetaStore,
}

impl ProjectInsightStores {
    /// 复用调用方**已打开**的项目库装配（推荐）。
    pub fn from_project_db(db: &ProjectDatabaseManager) -> Self {
        Self {
            storage: InsightStorage::new(db.duckdb_conn()),
            meta: InsightMetaStore::new(db.sqlite_pool()),
        }
    }

    /// 打开项目库并装配。
    ///
    /// 注意：`ProjectDatabaseManager::open` 会**新建**一份项目库连接。调用方若已持有
    /// 项目库句柄，请改用 [`Self::from_project_db`]，避免对同一 DuckDB 文件重复开连接。
    pub async fn open(project_root: &Path) -> Result<Self, CoreError> {
        let db = ProjectDatabaseManager::open(project_root, INSIGHT_SQLITE_POOL_SIZE).await?;
        Ok(Self::from_project_db(&db))
    }

    /// 保存一次列画像快照（**正文 + 元数据双写**），返回 `(snapshot_id, version_id)`。
    ///
    /// 版本链：`parent_version_id` 取该列当前最新元数据的 `version_id`，首次保存为 `None`。
    /// 供 `entity_source` 记录来源（连接 / 库 / schema / 表），便于「这份快照是哪来的」。
    pub async fn save_column_snapshot(
        &self,
        insight: &ColumnInsightFull,
        entity_source: Option<&str>,
        row_count: Option<i32>,
        elapsed_ms: Option<i32>,
    ) -> Result<(String, String), CoreError> {
        let column_name = insight.stats.column_name.clone();

        let parent_version_id = self
            .meta
            .get_latest_meta("column", &column_name)
            .await
            .ok()
            .flatten()
            .map(|m| m.version_id);

        let (snapshot_id, version_id) = self
            .storage
            .columns
            .save_snapshot(insight, parent_version_id.as_deref())
            .await?;

        // 正文写入成功后必须把元数据也写成功；任一失败都向上抛，
        // 由调用方决定是否重试（不做「只写一半也算成功」的静默降级）。
        let checksum = checksum_of(insight)?;
        self.meta
            .save_meta(
                "column",
                &column_name,
                entity_source,
                &snapshot_id,
                row_count,
                elapsed_ms,
                &version_id,
                parent_version_id.as_deref(),
                &checksum,
            )
            .await?;

        Ok((snapshot_id, version_id))
    }
}

/// 正文的 SHA256（复用 [`body::snapshot_checksum`] 的唯一算法实现，不在此另写一份）。
fn checksum_of(insight: &ColumnInsightFull) -> Result<String, CoreError> {
    snapshot_checksum(insight)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{
        ColumnInsightFull, ColumnStats, ColumnStatsDetail, DistributionBin, NumericStats,
    };

    /// 本次调用专属的临时项目目录。
    ///
    /// 不能复用别的测试目录：`ProjectDatabaseManager::open` 会跑迁移并独占
    /// DuckDB 文件锁，两个测试同时开同一个 `analytics.duckdb` 会互相失败。
    fn temp_project_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_insight_store_{}_{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp project dir");
        dir
    }

    fn sample_insight(column_name: &str) -> ColumnInsightFull {
        ColumnInsightFull {
            stats: ColumnStats {
                column_name: column_name.to_string(),
                data_type: "DOUBLE".to_string(),
                total_count: 10_000,
                null_count: 500,
                null_rate: 0.05,
                unique_count: Some(8_214),
                stats_detail: ColumnStatsDetail::Numeric(NumericStats {
                    min: 0.0,
                    max: 9_999.0,
                    avg: 127.35,
                    median: 98.0,
                    p25: 42.0,
                    p75: 188.0,
                    sum: 1_273_500.0,
                    stddev: Some(231.7),
                    skewness: Some(3.42),
                    kurtosis: None,
                    is_extreme: vec![],
                }),
            },
            sample: vec![serde_json::json!(127.35), serde_json::Value::Null],
            histogram: Some(vec![DistributionBin {
                label: "0–999".to_string(),
                count: 5_820,
                ratio: 0.582,
            }]),
        }
    }

    /// 0.7 的核心诉求：整条快照链路**可达**且正文/元数据**双写一致**。
    #[tokio::test]
    async fn test_snapshot_roundtrip_is_reachable() -> Result<(), CoreError> {
        let root = temp_project_dir("roundtrip");
        let stores = ProjectInsightStores::open(&root).await?;

        let insight = sample_insight("amount");
        let (snapshot_id, version_id) = stores
            .save_column_snapshot(&insight, Some("conn=c1,db=analytics"), Some(10_000), Some(42))
            .await?;
        assert!(!snapshot_id.is_empty(), "应返回 snapshot_id");
        assert!(!version_id.is_empty(), "应返回 version_id");

        // 正文回读（DuckDB）
        let latest = stores
            .storage
            .columns
            .get_latest_snapshot("amount")
            .await?
            .expect("应能读回刚保存的正文");
        assert_eq!(latest.stats.column_name, "amount");
        assert_eq!(latest.stats.total_count, 10_000);
        assert_eq!(latest.sample.len(), 2);

        // 元数据回读（SQLite）：版本号与来源必须对得上
        let meta = stores
            .meta
            .get_latest_meta("column", "amount")
            .await?
            .expect("应能读回刚保存的元数据");
        assert_eq!(meta.version_id, version_id);
        assert_eq!(meta.snapshot_id, snapshot_id);
        assert_eq!(meta.entity_source.as_deref(), Some("conn=c1,db=analytics"));
        assert_eq!(meta.row_count, Some(10_000));
        assert_eq!(meta.elapsed_ms, Some(42));

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    /// 版本链：第二次保存的 `parent_version_id` 必须指向上一次的 `version_id`。
    #[tokio::test]
    async fn test_version_chain_links_to_previous() -> Result<(), CoreError> {
        let root = temp_project_dir("chain");
        let stores = ProjectInsightStores::open(&root).await?;

        let first = sample_insight("amount");
        let (_, first_version) = stores
            .save_column_snapshot(&first, None, None, None)
            .await?;

        let second = sample_insight("amount");
        let (_, second_version) = stores
            .save_column_snapshot(&second, None, None, None)
            .await?;
        assert_ne!(first_version, second_version, "两次保存应是不同版本");

        let meta = stores
            .meta
            .get_latest_meta("column", "amount")
            .await?
            .expect("最新元数据");
        assert_eq!(meta.version_id, second_version);
        assert_eq!(
            meta.parent_version_id.as_deref(),
            Some(first_version.as_str()),
            "第二次保存应挂在上一次版本之后"
        );

        let history = stores.storage.columns.get_history("amount", Some(10)).await?;
        assert_eq!(history.len(), 2, "历史应有两条" );
        // 历史条目能解析回完整画像（版本对比的前提）
        let parsed = history[0].parse_insight()?;
        assert_eq!(parsed.stats.column_name, "amount");

        let stats = stores.storage.columns.get_storage_stats().await?;
        assert_eq!(stats.total_snapshots, 2);
        assert_eq!(stats.unique_columns, 1);

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    /// 无项目库时的行为：目录不存在也能开出来（迁移会建目录与表）。
    #[tokio::test]
    async fn test_open_creates_missing_project_layout() -> Result<(), CoreError> {
        let root = temp_project_dir("fresh");
        // 只给一个空目录，项目库结构应由迁移建立
        assert!(root.join(".RSmeta").exists() == false);

        let stores = ProjectInsightStores::open(&root).await?;
        assert!(
            root.join(".RSmeta").join("project.db").exists(),
            "应建立项目 SQLite"
        );

        // 尚无快照时，读操作应返回空而不是报错
        assert!(
            stores
                .storage
                .columns
                .get_latest_snapshot("whatever")
                .await?
                .is_none()
        );
        assert!(stores.meta.get_latest_meta("column", "whatever").await?.is_none());

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    /// 把快照的 `created_at` 统一推到 N 天前。
    ///
    /// 快照接口只会写「现在」，而「清理旧快照」的判据就是时间，所以测试得自己动手改时间。
    /// 两侧都改：只改一边就测不出「成对删」了（而那正是这条用例要钉的东西）。
    async fn backdate(db: &ProjectDatabaseManager, days: i64) {
        {
            let handle = db.duckdb_conn();
            let handle = handle.acquire().await.expect("DuckDB 句柄");
            let mut guard = handle.lock().await;
            let conn = guard.as_mut().expect("DuckDB 连接");
            conn.execute_batch(&format!(
                "UPDATE insight_column_snapshots
                 SET created_at = CURRENT_TIMESTAMP - INTERVAL {days} DAY"
            ))
            .expect("回填正文时间");
        }
        {
            let pool = db.sqlite_pool();
            let conn = pool.acquire().await.expect("SQLite 句柄");
            conn.inner()
                .expect("SQLite 连接")
                .execute(
                    &format!(
                        "UPDATE insight_snapshots
                         SET created_at = datetime('now', '-{days} days')"
                    ),
                    [],
                )
                .expect("回填元数据时间");
        }
    }

    /// 清理旧快照：**正文与元数据必须成对删**（D16），两侧条数对不上就是半写。
    ///
    /// 这里将旧时间回填到两侧（接口只写「现在」），再用 30 天的档位删——判据不在
    /// 测试里重写一遍，而是走两条真实的 `cleanup_older_than`。
    #[tokio::test]
    async fn test_cleanup_deletes_both_sides_in_pairs() -> Result<(), CoreError> {
        let root = temp_project_dir("cleanup");
        let db = ProjectDatabaseManager::open(&root, 2).await?;
        let stores = ProjectInsightStores::from_project_db(&db);

        stores
            .save_column_snapshot(&sample_insight("amount"), None, None, None)
            .await?;
        stores
            .save_column_snapshot(&sample_insight("amount"), None, None, None)
            .await?;
        backdate(&db, 40).await;

        let (body, meta) =
            crate::service::cleanup_old_insight_snapshots(30, &stores.storage, &stores.meta).await?;
        assert_eq!((body, meta), (2, 2), "两侧删的条数必须一致（成对删）");
        assert!(
            stores
                .storage
                .columns
                .get_history("amount", Some(10))
                .await?
                .is_empty(),
            "旧快照应从历史里消失"
        );
        assert_eq!(
            stores.storage.columns.get_storage_stats().await?.total_snapshots,
            0
        );

        // 新写的版本不受 30 天档影响（闸是时间，不是条数）
        stores
            .save_column_snapshot(&sample_insight("amount"), None, None, None)
            .await?;
        let (body, meta) =
            crate::service::cleanup_old_insight_snapshots(30, &stores.storage, &stores.meta).await?;
        assert_eq!((body, meta), (0, 0), "刚存的快照不该被 30 天档删掉");
        assert_eq!(
            stores
                .storage
                .columns
                .get_history("amount", Some(10))
                .await?
                .len(),
            1
        );

        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}

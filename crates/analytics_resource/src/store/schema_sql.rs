//! 项目库 / 全局库建表 SQL 的**单一来源**。
//!
//! 背景：这套迁移清单此前在六处各抄一遍（`tests/mod.rs`、`global/tests.rs`、
//! `indexer/tests.rs`、`service/tests/mod.rs`、`global/mod.rs`，外加迁移执行器自己的
//! `include_dir`）。加一个新迁移要改六处，漏一处也不会报编译错——只会在某个用例里
//! 以 `no such column` 冒出来，排查成本极高（本轮踩过一次）。
//!
//! 这里收成两个常量：测试辅助与 `GlobalInstanceStore::open_at` 都从这里取。
//! 注意两份都是**新建库**用的整段 SQL，含 `ALTER TABLE` / `UPDATE` 这类**非幂等**
//! 语句，只能跑一次；幂等补建走 [`super::schema_guard`]（那里只放 `IF NOT EXISTS`）。

/// 项目库：`analytics_resources` 从 007 起的全部相关迁移（按版本顺序）。
///
/// 007 建表 → 020 归档语义 → 023 copy/reference → 024 关联表去 tag 外键 →
/// 027–030 时间线 / 断言 / 引用 / 取回 → 031 表型本体列 → 032 kind 归一。
#[allow(dead_code)] // 只在测试建库路径用：生产建库走 `store::migration` 执行器。
pub(crate) const PROJECT_MIGRATIONS_SQL: &str = concat!(
    include_str!("../../../store/migrations/project_meta/007_analytics_resources.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/020_analytics_resource_archive.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql"),
    "\n",
    include_str!(
        "../../../store/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
    ),
    "\n",
    include_str!("../../../store/migrations/project_meta/027_analytics_resource_timeline.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/028_analytics_resource_semantics.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/029_analytics_resource_refs.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/030_analytics_resource_checkouts.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/031_analytics_table_payload.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/032_archive_kind_two_tiers.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/036_file_asset_cas.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/037_checkout_lease_global_scope.sql"),
    "\n",
    include_str!("../../../store/migrations/project_meta/038_table_asset_snapshots.sql")
);

/// 全局库：全局镜像表与版本表、分发回执、表型本体列、kind 归一。
///
/// 026 全局镜像表 → 033 分发回执 → 034 表型本体列 → 035 kind 归一
/// → 036 全局行表名回填（一版一张表）。
pub(crate) const GLOBAL_MIGRATIONS_SQL: &str = concat!(
    include_str!("../../../store/migrations/global/026_analytics_resource_global.sql"),
    "\n",
    include_str!("../../../store/migrations/global/033_analytics_resource_dispatch.sql"),
    "\n",
    include_str!("../../../store/migrations/global/034_analytics_table_payload.sql"),
    "\n",
    include_str!("../../../store/migrations/global/035_archive_kind_two_tiers.sql"),
    "\n",
    include_str!("../../../store/migrations/global/036_global_version_table_names.sql"),
    "\n",
    include_str!("../../../store/migrations/global/038_analytics_global_refs.sql"),
    "\n",
    include_str!("../../../store/migrations/global/040_global_file_asset_cas.sql"),
    "\n",
    include_str!("../../../store/migrations/global/041_table_asset_snapshots.sql"),
    "\n",
    include_str!("../../../store/migrations/global/042_global_ref_asset_id.sql")
);

//! 表资产的对外入口：冻结快照、检出与恢复。
//!
//! 表资产不再把“活表名”当版本：一版对应一份不可变快照（`rds_archive` 下的
//! `arc_*`），manifest 记逻辑内容，`table_asset_versions` 记版本链。
//! 旧“升级为资产 / 登记已有表”的活表指针模型已整体删除。

pub mod manifest;

pub use crate::service::table_asset::{
    abandon_table_checkout, archive_table_at, checkout_table_asset, consume_table_checkout,
    manifest_for_table, restore_table_asset_version, ArchiveTableOutcome, ArchiveTableRequest,
    TableCheckoutOutcome, TableConsumeOutcome,
};

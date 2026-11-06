//! 视图模型层：面板与对话框共用的人读快照，只依赖领域模型，不依赖任何视图模块。
//!
//! 旧路径（`resource_view::ArchiveRow`、`detail_view::ArchiveDetail` 等）由各模块
//! `pub use` 兜住，外部契约不变。

pub mod detail;
pub mod row;

pub use self::detail::{ArchiveDetail, ArchiveTagChip};
pub use self::row::{
    ArchiveCounts, ArchiveRow, GroupOption, InstanceFilter, ResourcesSnapshot, TagOption,
};

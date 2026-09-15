//! RdataStation v2 草稿箱 crate（scratchpad，M5）
//!
//! 类似 VS Code 文件管理的草稿工作区：
//! - `models`：草稿条目/搜索/差异/替换/外部引用模型
//! - `state`：草稿箱状态机
//! - `store`：草稿箱存储（文件系统 + 超时锁）
//! - `trash`：项目级回收站（草稿 / 资源共用）
//! - `watch`：模块目录文件监控（外部改动 → 变更标记，供视图去抖重拉）
//!
//! 依赖方向：scratchpad → shared。

pub mod models;
pub mod state;
pub mod store;
pub mod trash;
pub mod watch;

pub use models::{
    AnalyzableFile, DiffLine, DiffLineKind, DiffResult, ExternalReference, ExternalReferenceStatus,
    FileMeta, ReplaceResult, ScratchpadChangeEntry, ScratchpadChangeEvent, ScratchpadConfig,
    ScratchpadEntry, ScratchpadEntryKind, ScratchpadResponse, SearchMatch, SearchResult,
};
pub use state::ScratchpadState;
pub use store::{ScratchpadStore, MODULE_DIR_NAME, ORIGIN_SCRATCHPAD};
pub use trash::{ProjectTrash, TrashEntry, TrashManifest};
pub use watch::{ChangeFlag, ScratchpadWatcher};

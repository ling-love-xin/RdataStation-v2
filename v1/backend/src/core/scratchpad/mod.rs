pub mod models;
pub mod state;
pub mod store;

pub use models::{
    AnalyzableFile, DiffLine, DiffLineKind, DiffResult, ExternalReference, FileMeta, ReplaceResult,
    ScratchpadChangeEntry, ScratchpadChangeEvent, ScratchpadConfig, ScratchpadEntry,
    ScratchpadEntryKind, ScratchpadResponse, SearchMatch, SearchResult,
};
pub use state::ScratchpadState;
pub use store::ScratchpadStore;

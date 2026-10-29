//! 编辑器宿主面板 —— 工具栏域（`impl EditorHostPanel` 方法块，按能力域拆分）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：单一 impl 块按职责切分为 6 个子模块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。

mod render;
mod render_buttons;
mod icons;
mod actions;
mod toolbar_ops;
mod comments;

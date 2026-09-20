//! 编辑器动作（A10 / A14）
//!
//! **只在事件路径上**：动作由面板的 `on_action` 处理，改 `EditorService` 里的文档或向
//! 编辑内核写文本；渲染路径不认识这些动作（对齐项目「事件 / Action 与焦点」规范）。
//!
//! 命名空间是 `editor`，键位在 `crates/app` 统一注册（**注册了才宣传**：没实现的动作
//! 不绑键，也不写进文档的快捷键表）。
//!
//! 键位的 key context 是 `editor`（面板根元素的 `key_context`）。焦点在编辑内核内时，
//! 内核自己的 `Input` context 更深、会先被尝试；内核没处理的键（如 `Ctrl+S`）逐级冒泡
//! 到这里的动作。若某键内核已经绑了（如 `Ctrl+F` → `input::Search`），
//! 冒泡仍会继续到本命名空间——因此**改键位前先看 `gpui-base/src/input/base/state.rs`
//! 的 `init`**，否则会出现"看着注册了、实际被内核吃掉"的静默失效。
//!
//! 结果网格是**第二个上下文**（[`RESULT_GRID_CONTEXT`]）：它只挂在结果网格那层，
//! 所以同一个 `Ctrl+C` 在编辑区里仍是内核的文本复制、在网格里是复制选中的一格 / 一整行。
//!
//! 需要宿主（Dock / 对话框 / 系统文件对话框）的动作也在这里声明，由 `WorkbenchView`
//! 处理：`CloseDocument` · `SaveDocumentAs` · `OpenDocument`（面板自己发起 Dock 移除是重入）。

use gpui_kit::*;

actions!(
    editor,
    [
        SaveDocument,
        SaveDocumentAs,
        OpenDocument,
        ToggleComment,
        FormatDocument,
        CloseDocument,
        ExecuteSql,
        ExecuteAll,
        TriggerCompletion,
        CopyGridSelection
    ]
);

/// 结果网格的 key context（`crates/app` 把 `Ctrl+C` 绑在它上面，两边成对）
///
/// 与面板根的 `editor` 刻意分开：这个上下文只挂在结果网格那层元素上，所以
/// **编辑区里 `Ctrl+C` 仍是复制 SQL 文本、网格里才是复制选中的一格 / 一整行**。
/// 组件库的表格自己用的是 `DataTable`（方向键 / 翻页 / `Esc` 清选）——两个上下文
/// 会同时出现在焦点路径上，各管各的键。
pub const RESULT_GRID_CONTEXT: &str = "editor-result-grid";

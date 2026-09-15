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

use gpui_kit::*;

actions!(
    editor,
    [
        SaveDocument,
        ToggleComment,
        CloseDocument,
        ExecuteSql,
        ExecuteAll
    ]
);

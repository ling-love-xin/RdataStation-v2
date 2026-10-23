//! rds-connection-dialog — commands。
//!
//! 连接对话框局部动作（自 rds-workbench `commands.rs` 下沉，2026-10-15）：
//! - `SaveConnection`：Ctrl+Enter 保存（焦点在输入框内时由 Enter action 冒泡兜底）；
//! - `TestConnection`：Ctrl+T 测试连接；
//! - `DraftPrev` / `DraftNext`：↑↓ 切换暂存列表条目（输入框内 ↑↓ 仍由 Input 处理）。
//! 仅绑定在为对话框容器声明的 key context 上；
//! workbench 侧 `commands.rs` 经 `pub use` 重导保持 `workbench::commands::*` 路径不变。

use gpui_kit::*;

actions!(
    connection_dialog,
    [SaveConnection, TestConnection, DraftPrev, DraftNext]
);

//! RdataStation v2 连接对话框 crate（rds-connection-dialog）
//!
//! 自 rds-workbench 下沉（2026-10-15，见 `.rds/tmp/connection-dialog-split-plan.md`）：
//! - `services`：连接服务层（connection_service / driver_service / data_source_service /
//!   workspace_loader / secret_integration）——只依赖 engine / connection / shared / workbench_shell，
//!   不引用任何特性 crate，可独立编译。
//! - `components/connection_dialog`：连接对话框（步 3 迁入，宿主泛型化 `ConnectionDialogState<H>`）
//! - `host`：`ConnectionDialogHost` trait（宿主解耦，步 2）
//! - `commands`：对话框命令 Action（步 3 迁入）
//!
//! 依赖方向：rds-workbench → 本 crate → {engine, connection, shared, project, workbench_shell, gpui-kit}。

pub mod commands;
pub mod components;
pub mod host;
pub mod services;
// 根级 re-export：宿主（workbench editor.rs）以 `connection_dialog::ConnectionDialogState` 直连，
// 避免穿透三层模块路径（components::connection_dialog::…）。
pub use components::connection_dialog::ConnectionDialogState;
// 尺寸常量已下沉 `rds-workbench-shell`：这里重导保持 `crate::ui::*` 路径不变（与 workbench 同惯例）。
pub use workbench_shell::ui;

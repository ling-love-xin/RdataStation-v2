//! RdataStation v2 工作台 crate（workbench）
//!
//! 承载 M5 查询工作台：连接管理、结果集服务、SQL 执行编排入口。
//!
//! ## 迁移进度
//! - ✅ Round 8：`services/`（connection_service / result_service / persistence_service / driver_service，自 v1 `core/services`）
//! - ✅ Round 19：`view/`（WorkbenchView 首个可交互工作台骨架：活动栏 + 侧边栏 + 内容区 + 状态栏）
//! - ⏳ 后续：查询视图（query_view）、DockArea 布局系统、命令（commands）等 GPUI 层
//!
//! 依赖方向：workbench → insight → engine → shared；workbench → engine → connection → shared。

pub mod commands;
pub mod components;
pub mod panels;
pub mod services;
pub mod view;

// 重新导出服务层（与 v1 core/mod.rs 顶层 re-export 对齐）
pub use commands::{
    CloseProject, HideSidebars, RestoreSidebars, SwitchProject, ToggleQuickOpen,
};
pub use services::connection_service::{ConnectRequest, ConnectionService, SaveGlobalConnectionInput};
pub use services::driver_service::DriverService;
pub use services::result_service::ResultService;
pub use view::{ConnectionItem, LeftPanel, RightPanel, SidebarMode, WorkbenchView};

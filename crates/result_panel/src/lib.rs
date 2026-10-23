//! 结果集面板（DBeaver 式 Data Editor 方向）
//!
//! 定位：结果区从编辑器宿主拆出独立 crate（任务书「结果集表格」并行开发线）。
//! 结果域自洽——**结果存储（唯一权威）+ 网格视图 + 结果工具栏 / 状态行 + 导出**都在本
//! crate 内，editor 只保留访问口（面板实体 + 端口接线）。
//!
//! ## 依赖方向（硬约束）
//!
//! `editor → result_panel → engine / gpui-kit / workbench_shell`；本 crate **绝不反向
//! 依赖 editor**。共享模型（`DocumentId` / `ExecChannel` / `ColumnAnnotation`）与尺寸
//! 常量从 `workbench_shell` 命名。
//!
//! ## 模块
//!
//! - [`store`]：结果存储（`ResultStore` 按文档分槽 + `ResultEntry` 结果集 + 列类型/列注释）
//! - [`changes`]：结果集就地编辑的变更集（pending / committed / conflict 三态）
//! - [`grid`]：结果网格（`ResultGridDelegate` + DataTable）与结果工具栏 / 状态行 / 渲染
//! - [`sets`]：结果集标签条（TabBar 投影，不持有数据）
//! - [`error_card`] / [`script_card`]：结果区两张卡片（失败 / 脚本输出）
//! - [`export`]：结果导出五档（TSV / JSON / CSV / SQL Updates / Parquet / XLSX）
//! - [`analysis`]：结果分析菜单（对已抓行跑聚合 SQL 的预置项）
//! - [`panel`]：独立 `ResultPanel` 实体（状态 + 视图 + 动作闭环；由编辑器宿主嵌入）
//! - [`ports`]：宿主端口（执行 / 导出写文件 / 值预览 / 列统计 / 洞察 / 连接解析）

pub mod analysis;
pub mod changes;
pub mod error_card;
pub mod export;
pub mod grid;
pub mod script_card;
pub mod sets;
pub mod status;
pub mod store;
pub mod ui;

//! RdataStation v2 SQL 编辑器 crate（editor）
//!
//! **一个内核 + 三档能力**：
//! - **文本模式**：带高亮的记事本，**不与数据库通信**（无连接、无执行、无结果区）
//! - **SQL 模式**：DBeaver 一档的脚本窗口（连接绑定 + 执行族 + 结果集 + 历史）
//! - **分析模式**：单元（Cell）+ 会话（Session）+ 输出（Output）的可执行笔记
//!
//! 设计文档：`docs/architecture/editor/`（原型设计 / 架构与决策 / 开发方案 / 交互稿）。
//!
//! ## 依赖方向（硬约束）
//!
//! `workbench → editor → engine / database / shared`；editor **不得**依赖 workbench。
//!
//! ## 当前状态（Phase 0 地基 + 1a 进行中）
//!
//! - ✅ `model`：文档 / 模式 / 只读 / 能力表
//! - ✅ `mode`：模式判定规则表（纯函数 + 表驱动测试）
//! - ✅ `service`：文档集合与生命周期（打开/关闭/激活/重命名/脏状态，A1）
//! - ✅ `shared` + `view/host`：宿主面板（一面板 = 一标签 = 一份文档；标题 / 脏点 / 编辑内核 / 状态栏，A2+A3+A6+A7）
//! - ✅ `view/highlight`：SQL 语义着色（A4）· ✅ `mode` 切换矩阵（A5）· ✅ `persist` 打开/保存与外部修改检测（A9）
//! - ✅ `edit` 文本操作纯函数（行注释开关）· ✅ `commands` 动作声明 + 面板 `key_context("editor")`（A10）
//! - ✅ 已注册键位（`crates/app`）：`ctrl-s` 保存 · `ctrl-/` 行注释 · `ctrl-w` 关闭当前文档
//!   （`ctrl-w` 的处理器在宿主 workbench：面板在自己的 `update` 里让 Dock 移除自己会重入）
//! - ✅ `ui`：本 crate 的结构尺寸常量（不反向依赖 workbench）
//! - ⬜ 1a 待做：`execution`（执行目标解析 + 后台任务 + 回填）· `store`（结果单权威）·
//!   `completion` · `view/` 其余（查找 A11）· 另存为 / 关闭三态 / 模式切换确认对话框（A9 收尾）· 打开与另存为的系统文件对话框
//! - ⬜ 1c：`session` / `notebook`（Cell / Output / Session）
//!
//! 语句切分（「执行当前语句」与「批量执行」的基础）落在 `engine::sql::split`：它是不带编辑器
//! 状态的 SQL 文本原语，属 engine（`core/sql` 是 sqlglot 的唯一接入点），editor 直接消费，
//! 不在本 crate 重复实现。

pub mod commands;
pub mod edit;
pub mod mode;
pub mod model;
pub mod persist;
pub mod service;
pub mod shared;
pub mod ui;
pub mod view;

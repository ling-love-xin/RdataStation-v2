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
//! - ✅ `shared` + `view/host`：宿主面板（一面板 = 一标签 = 一份文档；标题 / 脏点 / 编辑内核，A2+A3）
//! - ✅ `ui`：本 crate 的结构尺寸常量（不反向依赖 workbench）
//! - ⬜ 1a 待做：`execution`（执行目标解析 + 后台任务 + 回填）· `store`（结果单权威）·
//!   `completion` · `persist` · `view/` 其余（高亮 A4 / 状态栏 A7 / 查找 A11）· 标签条接入 workbench Dock
//! - ⬜ 1c：`session` / `notebook`（Cell / Output / Session）
//!
//! 语句切分（「执行当前语句」与「批量执行」的基础）落在 `engine::sql::split`：它是不带编辑器
//! 状态的 SQL 文本原语，属 engine（`core/sql` 是 sqlglot 的唯一接入点），editor 直接消费，
//! 不在本 crate 重复实现。

pub mod mode;
pub mod model;
pub mod service;
pub mod shared;
pub mod ui;
pub mod view;

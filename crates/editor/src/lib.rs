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
//! ## 当前状态（Phase 0：地基）
//!
//! - ✅ `model`：文档 / 模式 / 只读 / 能力表
//! - ✅ `mode`：模式判定规则表（纯函数 + 表驱动测试）
//! - ⬜ 1a：`service`（唯一执行入口）· `execution`（后台任务 + 回填）· `store`（结果单权威）·
//!   `completion` · `persist` · `view/*`
//! - ⬜ 1c：`session` / `notebook`（Cell / Output / Session）
//!
//! 语句切分（「执行当前语句」与「批量执行」的基础）落在 `engine::sql::split`：它是不带编辑器
//! 状态的 SQL 文本原语，属 engine（`core/sql` 是 sqlglot 的唯一接入点），editor 直接消费，
//! 不在本 crate 重复实现。

pub mod mode;
pub mod model;

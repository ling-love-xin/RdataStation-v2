//! SQL 处理基础能力统一封装模块
//!
//! `core/sql/` 是 sqlglot-rust 在 RdataStation 中的唯一接入点。
//! 所有对底层 SQL 解析、生成、构建、优化、方言转换能力的调用，
//! 均通过此模块间接进行。业务模块不直接依赖 sqlglot-rust 的 API。
//!
//! # 子模块
//!
//! - `engine` — SqlEngine 结构体定义与公开方法
//! - `parser` — SQL 解析与特征检测（parse_and_route）
//! - `builder` — Expression Builder 封装（DDL/DML 生成）
//! - `formatter` — SQL 格式化
//! - `transpiler` — 方言转换

mod builder;
mod engine;
mod formatter;
mod parser;
mod transpiler;

pub use engine::{AlterOperation, ColumnDefInfo, DdlInfo, SqlDialect, SqlEngine, SqlStatementType};

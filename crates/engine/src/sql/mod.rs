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
//! - `split` — 词法级语句切分（执行当前语句 / 批量执行 / 语句数的基础，不依赖解析器）
//! - `highlight` — 词法高亮区间（tokenizer 驱动，视图层只负责上色）
//! - `builder` — Expression Builder 封装（DDL/DML 生成）
//! - `formatter` — SQL 格式化
//! - `transpiler` — 方言转换
//! - `filter` — 下发源库的筛选改写（包一层子查询 + WHERE + ORDER BY 提到外层）

mod builder;
mod engine;
mod filter;
mod formatter;
mod highlight;
mod parser;
mod split;
mod transpiler;

pub use builder::QualifiedTable;
pub use engine::{AlterOperation, ColumnDefInfo, DdlInfo, SqlDialect, SqlEngine, SqlStatementType};
pub use formatter::FormatReport;
pub use filter::{Rewrite, rewrite_with_filter, rewrite_with_order};
pub use highlight::{highlight_spans, HighlightSpan, TokenClass};
pub use split::{split_statements, SqlStatement};

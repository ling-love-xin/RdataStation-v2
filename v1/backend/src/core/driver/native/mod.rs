//! Native 数据库驱动实现层
//!
//! 每个驱动通过实现 `driver::traits::Database` trait 接入系统。
//!
//! ## list_* 方法设计模式
//!
//! 每个驱动包含两套元数据浏览方法：
//! | 方法 | 返回类型 | 用途 |
//! |------|---------|------|
//! | `get_*` | Vec<NodeInfo> / TableDetail | 完整元数据，DriverEngine 调用 |
//! | `list_*` | Vec<String> / SchemaObject / ColumnDetail | 精简版，MetadataBrowser trait |
//!
//! `list_*` 是 `get_*` 的薄封装，将完整结果转换为前端友好的结构。
//! 4 个驱动中该模式存在结构性重复（~120 行），因 `#[async_trait]` 添加的
//! 隐式生命周期约束阻止了 macro 自动生成，属于已知的可接受架构折衷。

pub mod duckdb;
pub mod duckdb_pool;
pub mod mysql;
pub mod mysql_native;
pub mod mysql_pool;
pub mod postgres;
pub mod postgres_native;
pub mod postgres_pool;
pub mod sqlite;
pub mod sqlite_pool;

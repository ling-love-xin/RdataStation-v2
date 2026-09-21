//! Native 数据库驱动实现层
//!
//! 每个驱动通过实现 `driver::traits::Database` trait 接入系统。
//!
//! ## list_* 与 get_* 的关系
//!
//! 每个驱动包含两套元数据浏览方法：
//! | 方法 | 返回类型 | 用途 |
//! |------|---------|------|
//! | `get_*` | Vec<NodeInfo> / NodeDetail | 完整元数据，`MetadataBrowser` trait |
//! | `list_*` | Vec<NodeInfo> / Vec<String> / ColumnDetail | `Database` trait 的对象树能力 |
//!
//! 两套返回**同一套结构对象**（`NodeInfo` / `ColumnDetail`）：实现了浏览器的驱动
//! 直接转发（`list_tables` → `get_tables`），方言特有的类别（例程 / 序列 / 触发器）
//! 才写独立查询。2026-09-19 统一前，`list_*` 返回的是另一套 `SchemaObject`，
//! 每个驱动都要写一遍「`NodeInfo` 降级」的空转映射（约 120 行），现已消除。

pub mod duckdb;
pub mod duckdb_pool;
pub mod mysql;
pub mod mysql_native;
pub mod mysql_pool;
pub mod pg_wire;
pub mod postgres;
pub mod postgres_native;
pub mod postgres_pool;
pub mod sqlite;
pub mod sqlite_pool;

//! 驱动侧的**纯数据层**：元数据 SQL 常量 / PG wire 解码 / 枚举码→文案。
//!
//! ## 为什么独立成 crate
//!
//! 这些内容是「一份逻辑喂多个实现」的：`mysql`(sqlx) 与 `mysql_native` 用同一批
//! MySQL 元数据 SQL，`postgres`(sqlx) 与 `postgres_native` 用同一批 PG 元数据 SQL 和
//! **同一套 wire 解码**。sqlx 实现正在从主程序抽出去（插件
//! `tools/plugin-drivers/sqlx-rust`，见 `docs/architecture/plugin/sqlx-driver-extraction-plan.md`
//! §3），如果直接把它抄一份进插件，两份 SQL 必然漂移（本仓 2026-09-21 刚把七份
//! `is_read_only_sql` 收敛成一份）。
//!
//! 所以：**纯字符串、纯函数、零依赖**放这里，内核驱动与插件二进制都 path 依赖它。
//!
//! ## 边界（重要）
//!
//! 这里**不放**任何需要驱动 trait / Arrow / sqlx 类型的代码：
//! * `columns_from_detail_rows`（要吃 `QueryResult` + `ColumnDetail`）留在内核；
//! * `sqlx_cell_as_text!` 宏（吃 `sqlx::Row`）留在内核；
//! * URL 拼装 / 驱动属性校验留在内核。
//!
//! 判据：**插件二进制用得上吗？** 用不上就别进来——那会把插件的依赖树拖大。

pub mod codes;
pub mod meta_sql;
pub mod wire;

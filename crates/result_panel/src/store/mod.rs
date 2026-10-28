//! 结果存储（**结果集的唯一权威**）
//!
//! 视图只是投影：结果网格从 `ResultStore` 读列与行，自己不缓存数据（架构 §4 状态地图、
//! 不变式"结果只有 `ResultStore`"）。这样将来加第二个视图（内联输出 / 导出 / 洞察）
//! 时不会出现"两个视图各存一份、对不上"。
//!
//! ## 每份文档一份**结果集列表**（B2）
//!
//! 原型 §2.4：「每次执行产生一个结果集」——批量执行每句一个、重查与分析派生新结果集
//! 而不就地覆盖。因此每份文档记的是 `Vec<ResultEntry>` + **当前选中项**：
//!
//! - [`ResultPlacement::Replace`]：替换**当前选中**的那一份（「刷新」等显式重查路径）；
//! - [`ResultPlacement::NewSet`]：追加一份新的并**切到新结果**（每次执行 = 一个新 tab，看
//!   最新结果；2026-10-27 起默认执行 / 批量 / 新标签全部走这里）。
//!
//! 上限 [`MAX_RESULT_SETS`]（原型 §2.4：上限 5、超出淘汰最旧）淘汰的是**最旧的未选中项**：
//! 正在看的那一份永远不会被自己的下一个结果挤掉。
//!
//! ## 列类型（本切片）
//!
//! [`ResultEntry::column_types`] 记**每列的语义类型**（[`ColumnKind`]）：渲染据此决定数字右对齐、
//! 布尔 / 时间用等宽字体、表头显示类型小标签。三条口径：
//!
//! - **值本身不动**：仍按 B5 的展示文本（`NULL` 是字面量 `"NULL"`），类型只**追加**信息，
//!   TSV / 导出 / 筛选 / 排序 / 复制全部照旧；
//! - **缺类型 = 与今天完全一致**：类型缺失时渲染走「无类型」这一档（左对齐、不等宽、表头不加标签），
//!   不会因为“不知道类型”就错位；
//! - **今天驱动还填不上**：整条执行路径（`QueryData` → 这里的 `rows`）目前只带列名与行，驱动的
//!   `column_types` 在 `workbench::services::editor_exec::to_data` 就被丢掉了，所以真实结果集
//!   现在一律是**空 vec**。形状先立住：上游接上之后只需 [`ResultEntry::with_column_types`] 一处填值。
//!
//! ## 模块拆分（2026-10-29）
//!
//! 原单文件 2272 行按职责拆成三个子模块，**对外 API 不变**（`crate::store::*` 路径由本文件
//! 重导出保持；editor 侧经 `crate::store` 引用的 `ResultPlacement` 等不受影响）：
//!
//! - [`entry`]：结果条目（`ResultEntry`）+ 列类型（`ColumnKind` / `ColumnType`）+ 落位语义
//!   （`ResultPlacement` / `Pushdown`）+ 上限常量（`MAX_RESULT_SETS`），含条目相关测试；
//! - [`fact`]：事实档载体状态（`FactState` 三态状态机，L0 事实层）；
//! - [`store`]：`ResultStore` 集合与逐文档结果槽（`DocumentResults`）、淘汰 / 追加合并
//!   （`merge_fact_state` / `evict_oldest`），含集合相关测试。

mod entry;
mod fact;
mod store;

pub use entry::{ColumnKind, ColumnType, Pushdown, ResultEntry, ResultPlacement, MAX_RESULT_SETS};
pub use fact::FactState;
pub use store::ResultStore;

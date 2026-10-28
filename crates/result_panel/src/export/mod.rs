//! 结果集导出（B7：CSV / JSON / INSERT 走文本编码；Parquet / XLSX 经 DuckDB）
//!
//! ## 四条口径
//!
//! - **值按展示文本导出**：`ResultEntry` 里的值就是网格里那些字符串，`NULL` 也是字面 `NULL`
//!   （B5 定的字符串化口径）。所以 CSV 里 NULL 与字符串 `"NULL"` 同形、JSON 里给 `null`
//!   （与网格的斜体判据同源）——要类型化得先有列类型，那是另一笔工作（计划 B7 余项）。
//! - **只导出已抓到的行**：分段抓取只把窗口里的行拿回来（`ExportScope::Fetched`）；「抓全量后
//!   导出」是另一档，取数由面板循环取段（`fetch_next`），本模块只管编码与命名。
//! - **两条落盘路径**：文本档（CSV / JSON / INSERT，以及【B7 切片三】的 SQL Updates /
//!   `WHERE 子句`）都是**纯函数编码**（本模块 + `fs::write`）；
//!   Parquet / XLSX 不是“拼字符串”写得出来的（前者是带 schema 的列式格式，后者是 OOXML 包），
//!   交给 DuckDB 的 `COPY … TO …`——那条路要把行围成临时表，属 I/O，所以走**后台线程**
//!   （见 `execution::DuckDbExportRequest`）。
//! - **纯函数**：编码、默认表名、默认文件名都在这里，写成能逐条断言的形状；落盘在面板里。
//!
//! ## 【B7 切片三】改库用的两档（`SQL Updates` / `WHERE 子句`）
//!
//! 这两档不是数据快照，是**拿去改数据的文本**（同类产品 sqlab 的 8 档里有它们，我们的
//! 菜单里到此为止只有 CSV / JSON / INSERT 三档文本）：
//!
//! - **`SQL Updates`**：每行一条 `UPDATE <表> SET <非键列> = <值>, … WHERE <键列> = <值>;`。
//!   它比 INSERT 多一个前提——**得知道哪几列能唯一定位一行**。我们手里没有主键元数据
//!   （`ResultEntry` 只有列名与展示文本），所以键列**必须由调用方给**（[`RowKey`]）：
//!   给不出就不生成——`UPDATE … WHERE 随便一列` 猜错的代价是**改错行**，而 INSERT 猜错只是
//!   插错值，两者不是一个量级。这就是本模块的“宁可回绝也不猜”。
//! - **`WHERE 子句`**：只出谓词（`WHERE` 一行 + 每行一个括号 + `OR` 串起来），
//!   贴进 `DELETE` / `SELECT` / `UPDATE` 都对得上。
//!   条件列：给了键就用键列（短、能命中索引），没给就**用全部列**（每行一个括号）。
//!   `NULL` 写成 `IS NULL`——`= NULL` 永远不成立，那是错的不是“差不多”。
//!
//! 两档进不进菜单由 [`ExportExtras`] 说（[`menu_items`] 仍只给“有网格就能给”的那几档，
//! 菜单要长出这两档就调 [`menu_items_for`]）——摆出来等于替用户表态“我要拿它去改库”，
//! 得由调用方（面板 + 它手里的表结构信息）说，界面不逐档硬编码。
//!
//! ## 模块拆分（2026-10-29）
//!
//! 原单文件 1411 行按职责拆成三个子模块，**对外 API 不变**（`crate::export::*` 路径由本文件
//! 重导出保持；`editor` 侧 `exec/export.rs` 的 `pub use result_panel::export::*` 通配重导出
//! 不受影响）：
//!
//! - [`formats`]：格式枚举 / 行定位依据 / 导出范围 / 菜单纯函数 / 表结构端口；
//! - [`encode`]：各档文本编码（CSV / JSON / INSERT / SQL Updates / WHERE）与值字面量启发式；
//! - [`table_name`]：默认表名 / 默认文件名（词法级 `FROM` 解析）。

mod encode;
mod formats;
mod table_name;

pub use encode::{encode, encode_rows, encode_rows_with_key, encode_updates, encode_where};
pub(crate) use encode::{
    cell_literal, is_null_cell, quote_ident, row_to_insert, row_to_json, value_to_json,
};
pub use formats::{
    ExportExtras, ExportFormat, ExportMenuItem, ExportScope, RowKey, TableKeysHandle,
    TableKeysPort, menu_items, menu_items_for,
};
pub use table_name::{default_file_name, default_table_name, table_from_sql};

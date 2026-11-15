//! 结果集「再分析」的宿主侧出口：把一张已存在的临时表（`tmp_q_` 结果集 / `tmp_i_` 洞察产物）
//! 直接送进洞察取样。
//!
//! 为什么是「已存在的表」而不是行数据：结果集的默认物化就是 `tmp_q_`（`analysis::result_set`），
//! 洞察取样落在 `tmp_i_`；两者都在**进程内存单例**里，同一条连接可见。所以再分析只需
//! 一句 `SELECT * FROM <表>`，不需要经过 Rust 搬运行，也不需要再建第二张表
//! （链式的关键：分析产物本身就是结果集，能继续被分析）。
//!
//! 分层：编辑器 / 结果面板只报「表名 + 展示名」（它不依赖 insight），翻译成
//! `SampleSource` 在这里做——与 `shared.open_insight_column` 同一分工。

use gpui_kit::App;

use insight::SampleSource;

use crate::panels::Shared;

/// 把表名拼成取样 SQL：表名走**双引号转义**（表名可能带引号，`tmp_i_` 的描述段来自用户）。
///
/// 这与 `engine::duckdb::temp_table::quote_ident` 同一规则，但那是 `pub(crate)`，
/// 跨 crate 拿不到，所以宿主侧留一份唯一实现（不要在各调用点各拼一次）。
pub fn select_sql_for_table(table: &str) -> String {
    format!("SELECT * FROM \"{}\"", table.replace('"', "\"\""))
}

/// 打开洞察面板并把这张表当作取样来源（链式再分析）。
pub fn analyze_table(shared: &Shared, table: &str, label: &str, cx: &mut App) {
    if table.trim().is_empty() {
        shared.say_error("分析：目标表名为空", cx);
        return;
    }
    let source = SampleSource::on_duckdb(select_sql_for_table(table), label.to_string());
    shared.open_insight_source_table(source, label.to_string(), cx);
}

#[cfg(test)]
mod tests {
    use super::select_sql_for_table;

    #[test]
    fn table_name_is_quoted_and_escaped() {
        assert_eq!(
            select_sql_for_table("tmp_q_orders_20261115120000_ab12cd34"),
            "SELECT * FROM \"tmp_q_orders_20261115120000_ab12cd34\""
        );
        // 内部引号翻倍：否则一句「表名里带引号」就能把取样 SQL 拼坏
        assert_eq!(
            select_sql_for_table("weird\"name"),
            "SELECT * FROM \"weird\"\"name\""
        );
    }
}

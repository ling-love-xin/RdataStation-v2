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

/// 面板行「洞察」：把这张表当取样来源（与结果集右键「再分析」同一出口）。
///
/// `table` 传**裸表名**（不带引号）；引号与转义由 [`select_sql_for_table`] 统一负责。
pub fn open_insight_table(shared: &Shared, table: &str, label: &str, cx: &mut App) {
    analyze_table(shared, table, label, cx);
}

/// 面板行「作为 Mock 结构来源」：**只出结构**，不碰数据。
///
/// 铁律 E 组：「mock 只出 schema」。这里只读项目分析库的
/// `information_schema.columns`（见 [`crate::services::mock_generator::import_columns_from_analytics`]），
/// 不预览、不抽行；列拿到就预填 Mock 草稿，生成多少行由用户在 Mock 面板决定。
pub fn open_mock_from_analytics(shared: &Shared, table: &str, cx: &mut App) {
    if table.trim().is_empty() {
        shared.say_error("Mock 结构：目标表名为空", cx);
        return;
    }
    let root = shared.project_root();
    match crate::services::mock_generator::import_columns_from_analytics(root.as_deref(), table) {
        Ok(columns) => shared.open_mock_panel_with_columns(
            table.to_string(),
            columns,
            &format!("项目分析库 {table}"),
            cx,
        ),
        Err(error) => shared.say_error(format!("Mock 结构：{error}"), cx),
    }
}

/// 把表名拼成取样 SQL：表名走**双引号转义**（表名可能带引号，`tmp_i_` 的描述段来自用户）。
///
/// 这与 `engine::duckdb::temp_table::quote_ident` 同一规则，但那是 `pub(crate)`，
/// 跨 crate 拿不到，所以宿主侧留一份唯一实现（不要在各调用点各拼一次）。
pub fn select_sql_for_table(table: &str) -> String {
    let qualified = table
        .split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".");
    format!("SELECT * FROM {qualified}")
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

/// 把项目分析库里**已经存在**的普通表显式归档为表资产（数据对象面板「归档为表资产」）。
///
/// 与 [`persist_to_project`] 的分工：那个把结果集落成普通表；这个冻结快照并消费源表。
/// 已经在库里、只是尚未归档的表走这条。
pub fn archive_table_asset(shared: &Shared, table: &str, label: &str, cx: &mut App) {
    if table.trim().is_empty() {
        shared.say_error("归档为表资产：源表名为空", cx);
        return;
    }
    let Some(root) = shared.project_root() else {
        shared.say_error("归档为表资产需要打开项目", cx);
        return;
    };
    let request = crate::services::persist_jobs::ArchiveTableJobRequest {
        project_root: root,
        label: label.to_string(),
        table: table.to_string(),
    };
    match crate::services::persist_jobs::submit_archive_table(request) {
        Ok(()) => shared.say(format!("正在把「{label}」归档为表资产…"), cx),
        Err(error) => shared.say_error(format!("归档为表资产失败：{error}"), cx),
    }
}

/// 把这份结果**持久化**成项目分析库里的表（`res_<描述>_<时间戳>`）。
///
/// 与 [`analyze_table`] 的分工：那个把 `tmp_q_` 当取样来源（临时、随 TTL 走），
/// 这个把它**落成项目库的正式表**（跨会话、可归档）。
///
/// 写库在 worker 线程（见 [`crate::services::persist_jobs`]）：事件路径上只入队，
/// 结果由工作台泵取后经状态栏告知。
pub fn persist_to_project(shared: &Shared, table: &str, label: &str, cx: &mut App) {
    if table.trim().is_empty() {
        shared.say_error("持久化：目标表名为空", cx);
        return;
    }
    let Some(root) = shared.project_root() else {
        shared.say_error(
            "持久化需要打开项目：Mock / 结果集只写项目分析库（{项目}/.RSmeta/analytics.duckdb）",
            cx,
        );
        return;
    };
    let request = crate::services::persist_jobs::PersistRequest {
        project_root: root,
        source_table: table.to_string(),
        label: label.to_string(),
        // 描述参与表名：用结果集的展示名（如「结果 3」），出的名字是 res_结果_3_<时间戳>
        description: label.to_string(),
    };
    match crate::services::persist_jobs::submit(request) {
        Ok(()) => shared.say(format!("正在把「{label}」写入项目分析库…"), cx),
        Err(error) => shared.say_error(format!("持久化失败：{error}"), cx),
    }
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

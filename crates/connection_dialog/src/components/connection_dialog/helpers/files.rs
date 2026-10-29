
// ===== 文件型「新建文件…」的确定性逻辑（纯函数，便于单测）=====

/// 「新建文件…」的默认文件名（按**驱动 id** 判族，与 `is_file_db` 同一来源）。
pub(crate) fn new_db_file_suggested_name(driver_id: &str) -> &'static str {
    if driver_id.eq_ignore_ascii_case("duckdb") {
        "new_database.duckdb"
    } else {
        "new_database.db"
    }
}

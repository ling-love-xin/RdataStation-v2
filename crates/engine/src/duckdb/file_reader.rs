//! 文件 → DuckDB 读取函数的**唯一定义处**。
//!
//! 谁在用：`analytics_resource`（归档文件能否看统计）、`insight`（文件源取样）、
//! `workbench`（草稿箱文件能否看统计）——三处都只要「这个扩展名该用哪个表函数」这一件事。
//!
//! 2026-09-19：本函数原先挂在 `dbi::engine::duckdb_engine::DuckDBEngine` 上。`dbi` 层
//! 整体零调用（该层 2124 行里只有它是活的，台账见
//! `docs/architecture/data-layer-wiring-matrix.md` §5.3）——于是把它从那个 700 行的
//! 死类里摘出来，不再让三个 crate 为一个函数去依赖一条废弃的抽象链。

/// 按扩展名选 DuckDB 的读取函数（CSV / Parquet / Excel / JSON）。
///
/// 支持的文件类型就这一处定义。`None` = 这个格式没有对应的读取函数，
/// 调用方应当直接报「不支持」而不是猜一个读取器。
pub fn file_reader_function(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".csv") || lower.ends_with(".tsv") || lower.ends_with(".txt") {
        Some("read_csv_auto")
    } else if lower.ends_with(".parquet") {
        Some("read_parquet")
    } else if lower.ends_with(".xlsx") || lower.ends_with(".xls") {
        // 需要 excel 扩展（`INSTALL excel; LOAD excel;`）；没装时由 DuckDB 报错
        Some("read_excel_auto")
    } else if lower.ends_with(".json") || lower.ends_with(".ndjson") {
        Some("read_json_auto")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions_map_to_their_readers() {
        assert_eq!(file_reader_function("a.csv"), Some("read_csv_auto"));
        assert_eq!(
            file_reader_function("a.TSV"),
            Some("read_csv_auto"),
            "扩展名匹配不看大小写"
        );
        assert_eq!(file_reader_function("a.parquet"), Some("read_parquet"));
        assert_eq!(file_reader_function("a.xlsx"), Some("read_excel_auto"));
        assert_eq!(file_reader_function("a.json"), Some("read_json_auto"));
    }

    #[test]
    fn unknown_extensions_return_none() {
        assert!(file_reader_function("a.sql").is_none());
        assert!(file_reader_function("noext").is_none());
    }
}

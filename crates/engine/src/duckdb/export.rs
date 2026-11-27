//! 分析实例里的一张表 → 落盘文件（`COPY … TO`）。
//!
//! 导出与「保存到草稿」是同一件事的两个目的地（铁律 E 组第 24 条）：
//! 同一个写盘动作，`dest` 指向全局资源目录还是项目的 `scratchpad/`。
//! 格式只收 csv / parquet / json（第 25 条）——DuckDB 原生 `COPY` 就这三类最稳，
//! 其它格式没有一致口径，不放进来。
//!
//! 为什么不用 `workbench::services::query_export`：那条是"行已经取到内存里再拼 CSV"，
//! 适合查询结果的小结果集；这里要导的是**库里的整张表**，走 `COPY` 不经过 Rust 搬运。

use std::path::Path;

use duckdb::Connection;
use shared::error::CommonError;

use crate::CoreError;

/// 支持的导出格式（只这三种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Parquet,
    Json,
}

impl ExportFormat {
    /// 按扩展名推断；不认识的扩展名返回 `None`（界面据此拒）
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())?;
        match ext.as_str() {
            "csv" => Some(ExportFormat::Csv),
            "parquet" => Some(ExportFormat::Parquet),
            "json" => Some(ExportFormat::Json),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Parquet => "parquet",
            ExportFormat::Json => "json",
        }
    }

    /// `COPY` 的格式子句
    fn options(self) -> &'static str {
        match self {
            // 带表头：没有表头的 CSV 重新读回来只能靠猜列名
            ExportFormat::Csv => "FORMAT CSV, HEADER",
            ExportFormat::Parquet => "FORMAT PARQUET",
            // 数组形：一行一条对象，再读回来是同一张表（行式 JSON 读回来会碎）
            ExportFormat::Json => "FORMAT JSON, ARRAY",
        }
    }
}

/// 表名转义（与 `result_set_actions::select_sql_for_table` 同一规则：
/// `schema.table` 逐段加引号，普通表名只加一层）。
fn quote_ident(name: &str) -> String {
    name.split('.')
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(".")
}

/// 在一条连接上把 `table` 写成 `path`（扩展名决定格式）。
///
/// 表名走标识符转义；路径只做单引号转义（Windows 反斜杠照原样交给 DuckDB）。
pub fn export_table(conn: &Connection, table: &str, path: &str) -> Result<ExportFormat, CoreError> {
    let format = ExportFormat::from_path(path).ok_or_else(|| {
        CoreError::common(CommonError::General(
            "导出只支持 .csv / .parquet / .json".to_string(),
        ))
    })?;
    let quoted_path = path.replace('\'', "''");
    let sql = format!(
        "COPY (SELECT * FROM {}) TO '{}' ({})",
        quote_ident(table),
        quoted_path,
        format.options()
    );
    conn.execute_batch(&sql).map_err(|error| {
        CoreError::common(CommonError::General(format!("导出失败：{error}")))
    })?;
    Ok(format)
}

/// 在**进程单例**上导出（事件路径调用）。锁被占时返回 `Err`，由调用方回执。
pub fn export_table_global(table: &str, path: &str) -> Result<ExportFormat, CoreError> {
    let conn = crate::duckdb::manager::DuckDBManager::global()
        .lock()
        .map_err(|_| CoreError::common(CommonError::General("内存库连接锁被污染".to_string())))?;
    export_table(&conn, table, path)
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ExportFormat, export_table};
    use std::path::PathBuf;

    fn workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_export_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    #[test]
    fn only_three_formats_are_accepted() {
        assert_eq!(ExportFormat::from_path("D:/out/a.CSV"), Some(ExportFormat::Csv));
        assert_eq!(
            ExportFormat::from_path("a.parquet"),
            Some(ExportFormat::Parquet)
        );
        assert_eq!(ExportFormat::from_path("a.json"), Some(ExportFormat::Json));
        assert_eq!(ExportFormat::from_path("a.xlsx"), None, "xlsx 不在口径里");
        assert_eq!(ExportFormat::from_path("a"), None);
    }

    /// 三种格式都真能写出来、并且能被 DuckDB 读回来（不是"文件存在"了事）
    #[test]
    fn a_table_lands_in_all_three_formats() {
        let dir = workdir("formats");
        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        conn.execute_batch("CREATE TABLE t AS SELECT i AS id, 'x' || i AS s FROM range(3) AS r(i)")
            .expect("建表");

        for (name, format) in [
            ("out.csv", ExportFormat::Csv),
            ("out.parquet", ExportFormat::Parquet),
            ("out.json", ExportFormat::Json),
        ] {
            let path = dir.join(name);
            let path = path.to_str().expect("路径");
            assert_eq!(export_table(&conn, "t", path).expect("导出"), format);
            let reader = match format {
                ExportFormat::Csv => format!("read_csv_auto('{}')", path.replace('\'', "''")),
                ExportFormat::Parquet => format!("read_parquet('{}')", path.replace('\'', "''")),
                ExportFormat::Json => format!("read_json_auto('{}')", path.replace('\'', "''")),
            };
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {reader}"), [], |row| row.get(0))
                .expect("读回来");
            assert_eq!(count, 3, "{name} 应当能读回同样的 3 行");
        }
    }

    /// 表名带引号也不该把 SQL 拼坏
    #[test]
    fn weird_table_names_are_quoted() {
        let dir = workdir("quote");
        let conn = duckdb::Connection::open_in_memory().expect("内存库");
        conn.execute_batch("CREATE TABLE \"odd\"\"name\" AS SELECT 1 AS id")
            .expect("建表");
        let path = dir.join("out.csv");
        let path = path.to_str().expect("路径");
        export_table(&conn, "odd\"name", path).expect("带引号的表名也该导得出");
    }
}

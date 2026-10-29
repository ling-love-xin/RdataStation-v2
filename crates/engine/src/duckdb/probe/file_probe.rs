//! 探一个**数据文件**的结构（列名 + 类型）与行数：M6 `Analysis` 档的采集侧。
//!
//! 为什么落在 engine：这要连 DuckDB、要读文件——是**数据访问**能力，与
//! [`file_reader_function`](super::file_reader_function) 同一层。M6 只认它回给的形状
//! （列 + 行数），自己不碰 DuckDB（依赖方向：`analytics_resource → engine`）。
//!
//! 指纹口径不在这里：**「指纹 = 定义 + 列结构，行数只作元信息」** 是 M6 的裁决
//! （`analytics_resource::analysis`，开发方案 R4）——本模块只负责把事实读出来。

use duckdb::Connection;
use shared::error::{CommonError, CoreError};

use super::file_reader_function;

/// 探到的文件事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileProbe {
    /// 列（列名 + 类型，**保持文件的声明顺序**）。
    pub columns: Vec<(String, String)>,
    /// 行数（读一次 `count(*)`）。读不到给 `None`——不假装 0。
    pub row_count: Option<i64>,
}

/// 数据文件的 `SELECT` 表达式（= 最小配方）：按扩展名选读取器，路径里的单引号转义。
///
/// `None` = 这个格式没有读取器（调用方应当报“不支持”，而不是猜一个）。
pub fn file_select_sql(path: &str) -> Option<String> {
    let reader = file_reader_function(path)?;
    Some(format!(
        "SELECT * FROM {reader}('{}')",
        path.replace('\'', "''")
    ))
}

/// 探一个数据文件：先 `DESCRIBE` 拿列与类型，再 `count(*)` 拿行数。
///
/// 两件事都**只读**文件；调用方（宿主工作线程）负责拿连接——用
/// [`DuckDBManager::global`](super::DuckDBManager::global) 那个内存实例即可：
/// 它已经配好扩展目录与文件读取器。
pub fn probe_file(conn: &Connection, path: &str) -> Result<FileProbe, CoreError> {
    let select = file_select_sql(path).ok_or_else(|| {
        CoreError::common(CommonError::General(format!(
            "没有可用于 `{path}` 的读取器（不支持的数据文件类型）"
        )))
    })?;
    let columns = describe_columns(conn, &select)?;
    // 行数只作元信息：读不到（权限 / 解析中断）也给 `None`，不让探测整体失败——
    // 结构才是指纹要看的东西。
    let row_count = conn
        .query_row(
            &format!("SELECT count(*) FROM ({select}) AS t"),
            [],
            |row| row.get::<_, i64>(0),
        )
        .ok();
    Ok(FileProbe { columns, row_count })
}

/// `DESCRIBE <select>` → 列名与类型（DuckDB 的前两列）。
fn describe_columns(conn: &Connection, select: &str) -> Result<Vec<(String, String)>, CoreError> {
    let mut stmt = conn
        .prepare(&format!("DESCRIBE {select}"))
        .map_err(|e| engine_err("describe", e))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| engine_err("describe", e))?;
    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(|e| engine_err("describe", e))?);
    }
    if columns.is_empty() {
        return Err(CoreError::common(CommonError::General(
            "探测结果没有列：这个文件可能不是可读的数据文件".to_string(),
        )));
    }
    Ok(columns)
}

fn engine_err(action: &str, error: impl std::fmt::Display) -> CoreError {
    CoreError::common(CommonError::General(format!("{action} 失败: {error}")))
}

#[cfg(test)]
mod tests {
    use super::{file_select_sql, probe_file};
    use duckdb::Connection;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_probe_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// 配方表达式：按扩展名选读取器；不支持的格式给 `None`（不猜一个读取器出来）。
    #[test]
    fn select_sql_follows_the_reader_table_and_escapes_quotes() {
        assert_eq!(
            file_select_sql("resources/dau.csv").as_deref(),
            Some("SELECT * FROM read_csv_auto('resources/dau.csv')")
        );
        // 文件名里带单引号：转义成两个（否则拼出来的 SQL 会散架）。
        assert!(
            file_select_sql("resources/o'brien.csv")
                .expect("csv 有读取器")
                .contains("o''brien.csv")
        );
        assert_eq!(
            file_select_sql("resources/notes.md"),
            None,
            "文本不是数据文件"
        );
        assert_eq!(file_select_sql("resources/a.parquet").is_some(), true);
    }

    /// 真探一个 CSV：列名与顺序来自文件，行数来自 `count(*)`。
    ///
    /// 类型**只断言非空**：DuckDB 的自动推断（`BIGINT` / `VARCHAR` / `DOUBLE`…）会随版本变，
    /// 把它写死在测试里会在无关升级时变红；结构指纹在乎的是“同一份文件两次探到同一串”。
    #[test]
    fn probe_reads_columns_and_row_count_from_a_real_csv() {
        let dir = temp_dir("csv");
        let path = dir.join("dau.csv");
        std::fs::write(&path, "id,name\n1,a\n2,b\n3,c\n").expect("write csv");
        let conn = Connection::open_in_memory().expect("内存连接");

        let probe = probe_file(&conn, &path.to_string_lossy()).expect("探测");
        assert_eq!(
            probe
                .columns
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "name"],
            "列名与顺序来自文件"
        );
        assert!(
            probe.columns.iter().all(|(_, ty)| !ty.is_empty()),
            "每列都要有类型"
        );
        assert_eq!(probe.row_count, Some(3));

        // 同一份文件再探一次：结果完全一致（结构指纹的稳定性靠这条）。
        let again = probe_file(&conn, &path.to_string_lossy()).expect("再探");
        assert_eq!(probe, again, "同样的文件探两次要一模一样");

        // 不支持的扩展名：报错而不是给一个空结果（空结构会变成一个假指纹）。
        let text = dir.join("notes.md");
        std::fs::write(&text, "hello").expect("write md");
        assert!(probe_file(&conn, &text.to_string_lossy()).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

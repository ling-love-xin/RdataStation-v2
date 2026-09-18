use shared::error::{CommonError, CoreError};
use crate::get_connection_manager;
use crate::services::duckdb_service::{self, extract_rows_from_serialized};
use crate::services::result_types::ResultSet;
use crate::services::sql_service::SqlExecuteOptions;
use crate::SqlService;
use crate::DuckDBManager;

pub async fn re_execute_with_filter(
    conn_id: String,
    original_sql: &str,
    where_clause: &str,
    order_clause: &str,
) -> Result<ResultSet, CoreError> {
    let start = std::time::Instant::now();
    let manager = get_connection_manager().clone();
    let service = SqlService::new(manager);

    let base_sql = original_sql.trim().trim_end_matches(';');
    let mut filtered_sql = format!("SELECT * FROM ({}) AS _result", base_sql);
    if !where_clause.trim().is_empty() {
        filtered_sql.push_str(&format!(" WHERE {}", where_clause));
    }
    if !order_clause.trim().is_empty() {
        filtered_sql.push_str(&format!(" ORDER BY {}", order_clause));
    }

    let options = SqlExecuteOptions {
        channel: None,
        record_history: false,
        use_transaction: false,
        timeout_ms: None,
        use_cache: false,
    };

    let result = service
        .execute(Some(conn_id), &filtered_sql, options)
        .await?;
    let elapsed = start.elapsed().as_millis() as u64;

    let json_value = serde_json::to_value(&result.result)
        .map_err(|e| CoreError::common(CommonError::General(format!("Serialize error: {}", e))))?;

    let columns = json_value["columns"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let rows = extract_rows_from_serialized(&json_value);
    let temp_table = duckdb_service::DuckDbService::create_duckdb_temp_table(&columns, &rows)?;

    Ok(ResultSet {
        row_count: rows.len() as u32,
        columns,
        rows,
        elapsed_ms: elapsed as u32,
        temp_table,
    })
}

pub fn execute_duckdb_analysis(
    temp_table: &str,
    sql: &str,
    columns: Option<Vec<String>>,
    rows: Option<Vec<Vec<serde_json::Value>>>,
) -> Result<ResultSet, CoreError> {
    let start = std::time::Instant::now();
    let duckdb = duckdb_service::DuckDbService::get_or_create_duckdb()?;
    let mut conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    let actual_table = if temp_table.is_empty() {
        if let (Some(cols), Some(rws)) = (columns, rows) {
            duckdb_service::DuckDbService::create_temp_table_internal(&mut conn, &cols, &rws)?
        } else {
            return Err(CoreError::common(CommonError::General(
                "No temp table or data provided".to_string(),
            )));
        }
    } else {
        temp_table.to_string()
    };

    let analysis_sql = sql
        .replace("{table}", &actual_table)
        .replace("result_temp", &actual_table);

    DuckDBManager::validate_analysis_sql(&analysis_sql)?;

    let (cols_out, rws_out) =
        duckdb_service::DuckDbService::query_duckdb(&mut conn, &analysis_sql)?;
    let elapsed = start.elapsed().as_millis() as u64;
    let row_count = rws_out.len();

    Ok(ResultSet {
        columns: cols_out,
        rows: rws_out,
        row_count: row_count as u32,
        elapsed_ms: elapsed as u32,
        temp_table: actual_table,
    })
}

/// 【B7 切片二】DuckDB 导出格式（引擎只认这两个名字，不依赖编辑器的 `ExportFormat`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuckDbExportFormat {
    /// 列式二进制（Parquet 是**内核自带**的，不需要任何扩展）
    Parquet,
    /// Excel 工作簿（要 `excel` 扩展）
    Xlsx,
}

impl DuckDbExportFormat {
    /// 传进 SQL 的格式名（也是文件扩展名）
    pub fn name(self) -> &'static str {
        match self {
            Self::Parquet => "parquet",
            Self::Xlsx => "xlsx",
        }
    }

    fn needs_excel_extension(self) -> bool {
        matches!(self, Self::Xlsx)
    }
}

/// 一次 DuckDB 导出的结果（回执用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportStats {
    pub rows: usize,
    pub elapsed_ms: u64,
}

/// 【B7 切片二】把结果行经 DuckDB 落成 Parquet / XLSX（`COPY … TO …`）
///
/// ## 为什么要过 DuckDB
///
/// 这两种格式都不是“拼字符串”能写的：Parquet 是带 schema 的列式格式，XLSX 是要写 OOXML 包的
/// 工作簿。DuckDB 既能把行围成表（和本地分析同一套 `create_temp_table_internal`），又能 `COPY`
/// 出去——省掉两个手写编码器（手写 Parquet 几乎必然写出一个别人读不了的文件）。
///
/// ## 三条口径
///
/// - **值仍按展示文本进库**（与本地分析同一口径）：`NULL`→真 NULL，其余是字符串；
///   列类型由引擎按整列推断，所以数字列在 Parquet / XLSX 里就是数（这是比 CSV / JSON 多出来的好处），
///   代价是 `007` 这类前导零文本会被推成数字。
/// - **临时表必收**：无论 `COPY` 成不成功都 `DROP`（`with_analysis_temp_table` 同款承诺）——
///   否则每次导出都在进程级内存库里漏一张 `tmp_q_*`。
/// - **不静默联网**：连接建时就关了 `autoinstall_known_extensions`（见 `configure_connection`），
///   所以 XLSX 缺扩展时在这里**显式**试一次 `LOAD`→`INSTALL`→`LOAD`，失败就把原话（连扩展目录）
///   交回去，而不是让 DuckDB 在 `COPY` 里报一句看不懂的错。
pub fn export_rows_via_duckdb(
    columns: &[String],
    rows: &[Vec<serde_json::Value>],
    path: &std::path::Path,
    format: DuckDbExportFormat,
) -> Result<ExportStats, CoreError> {
    let start = std::time::Instant::now();
    let duckdb = duckdb_service::DuckDbService::get_or_create_duckdb()?;
    let mut conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    if format.needs_excel_extension() {
        ensure_excel_extension(&conn)?;
    }

    let table = duckdb_service::DuckDbService::create_temp_table_internal(&mut conn, columns, rows)?;
    let outcome = copy_table_to_file(&conn, &table, path, format);
    // 表是纯中间产物：成败都要收（收不掉只记日志，不盖主流程的错）
    if let Err(error) = crate::duckdb::drop_temp_table(
        &conn,
        crate::duckdb::TempTableSource::Query,
        &table,
    ) {
        tracing::warn!("[export] 收掉临时表 {table} 失败: {error}");
    }
    outcome?;

    Ok(ExportStats {
        rows: rows.len(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

/// `COPY (SELECT * FROM <表>) TO '<文件>' (FORMAT <格式>)`
///
/// **XLSX 要写表头**：探针实测（2026-09-18）不传 `HEADER` 时写出来的工作簿第一行就是数据，
/// 用 `read_xlsx(…, header = true)` 读回来会把 `1` 当列名。表头是表格文件的基本预期，
/// 所以显式传；Parquet 是列名自带在 schema 里的，不需要这个选项。
fn copy_table_to_file(
    conn: &duckdb::Connection,
    table: &str,
    path: &std::path::Path,
    format: DuckDbExportFormat,
) -> Result<(), CoreError> {
    // 路径按 SQL 字面量转义：单引号双写；反斜杠在 DuckDB 字面量里不是转义符，但统一给正斜杠
    // （Windows 上 DuckDB 两种都认，正斜杠少一层“这里到底要不要再转义”的怀疑）
    let file = path.to_string_lossy().replace('\\', "/").replace('\'', "''");
    let options = match format {
        DuckDbExportFormat::Parquet => "FORMAT parquet".to_string(),
        DuckDbExportFormat::Xlsx => "FORMAT xlsx, HEADER true".to_string(),
    };
    let sql = format!(
        "COPY (SELECT * FROM {}) TO '{}' ({})",
        crate::duckdb::quote_ident(table),
        file,
        options
    );
    conn.execute_batch(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB 导出 {} 失败：{}",
            format.name(),
            e
        )))
    })
}

/// 确保 `excel` 扩展能用：先 `LOAD`（离线路径），不得已才 `INSTALL`（联网一次）
///
/// 与本地加速那套（`accel::install_and_load`）同一取舍：失败原因里必须带**扩展目录**，
/// 否则用户只看得到一句“Extension not found”，不知道该把文件放哪。
fn ensure_excel_extension(conn: &duckdb::Connection) -> Result<(), CoreError> {
    if conn.execute_batch("LOAD excel").is_ok() {
        return Ok(());
    }
    conn.execute_batch("INSTALL excel; LOAD excel").map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "XLSX 导出需要 DuckDB 的 excel 扩展，安装失败：{e}（首次需要能访问 DuckDB 扩展源；离线时请先把 excel.duckdb_extension 放进 {}）",
            crate::duckdb::DuckDBManager::extensions_dir().display()
        )))
    })
}

#[cfg(test)]
mod export_tests {
    // 安全模式：**不通配导入**
    use super::{DuckDbExportFormat, export_rows_via_duckdb};
    use crate::services::duckdb_service::DuckDbService;
    use serde_json::{Value, json};

    /// Parquet 导出**不需要任何服务器也不需要扩展**（内核自带）：写出去再读回来，
    /// 值的类型与 NULL 都要对上——这是本地分析那条桥接口径在导出侧的回归。
    #[test]
    fn parquet_export_round_trips_through_duckdb() {
        let dir = std::env::temp_dir().join(format!("rds_export_probe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("out.parquet");
        std::fs::remove_file(&path).ok();

        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec![json!(1), json!("alpha")],
            vec![json!(2), json!("beta")],
            // 展示文本里的 `NULL` 到这一层已经是真 null（编辑器侧桥接时判的）
            vec![json!(3), Value::Null],
        ];
        let stats = export_rows_via_duckdb(&columns, &rows, &path, DuckDbExportFormat::Parquet)
            .expect("Parquet 导出应当成功");
        assert_eq!(stats.rows, 3);
        assert!(
            std::fs::metadata(&path).expect("文件应当写出来了").len() > 0,
            "Parquet 文件不该是空的"
        );

        // 读回来：列名、行的先后（按 id 排）、NULL 与数字都对得上
        let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
        let mut conn = duckdb.lock().expect("锁");
        let file = path.to_string_lossy().replace('\\', "/");
        let (cols, got) = DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT * FROM read_parquet('{file}') ORDER BY id"),
        )
        .expect("读回 Parquet 应当成功");
        assert_eq!(cols, vec!["id".to_string(), "name".to_string()]);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], vec![json!(1), json!("alpha")]);
        assert_eq!(got[2], vec![json!(3), Value::Null], "NULL 要真是 NULL");

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }
}

use shared::error::{CommonError, CoreError};
use crate::get_connection_manager;
use crate::services::duckdb_service;
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
        // DuckDB 分析路径：无源库 schema 概念，不注入会话上下文
        schema: None,
    };

    let result = service
        .execute(Some(conn_id), &filtered_sql, options)
        .await?;
    let elapsed = start.elapsed().as_millis() as u64;

    // 直读 `batches`（**不经 JSON 契约往返**）：契约序列化不含 Arrow，而驱动只填 `batches`
    // —— 旧写法 `to_value(...) + ["batches"]` 拿到的永远是零行，临时表建出来是空的
    // （明细见 `duckdb_service::rows_as_json`）。
    let columns = result.result.columns.clone();
    let rows = duckdb_service::rows_as_json(&result.result);
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

/// 【B4】事实档 Arrow batches 直灌建表后跑分析（**保真档**：类型来自 Arrow schema，不猜型）。
///
/// 与 [`execute_duckdb_analysis`] 的分工：
/// - `temp_table` 非空 = 复用已有常驻表（表档重复分析，**不重复建表**，batches 参数只作留位）；
/// - `None` = 用 `batches` 经 `create_temp_table_from_batches` 直灌新建（列类型来自 schema，
///   不支持的类型直接报错，不静默落 VARCHAR）；
/// - 无事实档 / 显示矩阵猜型降级档仍走 [`execute_duckdb_analysis`]，两条路径互不替代。
///
/// 回执 `temp_table` 带回实际使用的表名：上层据此把表名回挂原结果集槽位。
pub fn execute_duckdb_analysis_from_batches(
    temp_table: Option<&str>,
    sql: &str,
    batches: &[shared::models::ArrowBatch],
) -> Result<ResultSet, CoreError> {
    let start = std::time::Instant::now();
    let duckdb = duckdb_service::DuckDbService::get_or_create_duckdb()?;
    let mut conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    let actual_table = match temp_table {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => duckdb_service::DuckDbService::create_temp_table_from_batches(&mut conn, batches)?,
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
/// - **值仍按展示文本进库**：`NULL`→真 NULL，其余由编辑器侧 `cell_json` 一律包成字符串，
///   `infer_type` 只认 JSON 值类型、不 parse 文本内容，所以整列落 `VARCHAR`——数字列在
///   Parquet / XLSX 里也是文本（数字精度 / 类型在这一跳收窄失真）；前导零文本反而保持原样。
///   带类型档（Parquet / XLSX / typed JSON）将在 D 批切事实档 batches 直灌建表，本 internal
///   路径只作无事实档回退（方案 §7-D）。
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

/// 【D1】事实档 batches 直灌建表后 `COPY` 落盘（保真档：列类型来自 Arrow schema，不猜型）。
///
/// 与 [`export_rows_via_duckdb`] 的分工：本路径类型保真；临时表是导出专用中间产物，
/// 成败都收、即用即收，不回挂事实档句柄（导出临时表与分析常驻表生命周期不同，方案 §7-D）。
pub fn export_batches_via_duckdb(
    batches: &[shared::models::ArrowBatch],
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

    let table = duckdb_service::DuckDbService::create_temp_table_from_batches(&mut conn, batches)?;
    let row_count = batches.iter().map(|batch| batch.num_rows()).sum();
    let outcome = copy_table_to_file(&conn, &table, path, format);
    // 临时表成败都收（与 internal 路径同一承诺）；收不掉只记日志，不盖主流程的错
    if let Err(error) = crate::duckdb::drop_temp_table(
        &conn,
        crate::duckdb::TempTableSource::Query,
        &table,
    ) {
        tracing::warn!("[export] 收掉直灌临时表 {table} 失败: {error}");
    }
    outcome?;

    Ok(ExportStats {
        rows: row_count,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

/// 【D1】表档常驻表直接 `COPY` 落盘（保真档：复用常驻表，不新建、不回收）。
///
/// 常驻表归事实档生命周期管（槽位淘汰 / 文档关闭 / 项目切换清场），本路径只借表导出，
/// 不 `DROP`；行数由调用方从事实帧行数缓存带入（表档 batches 已释放，不回读 Rust 堆计数）。
pub fn export_existing_table_via_duckdb(
    table: &str,
    row_count: usize,
    path: &std::path::Path,
    format: DuckDbExportFormat,
) -> Result<ExportStats, CoreError> {
    let start = std::time::Instant::now();
    let duckdb = duckdb_service::DuckDbService::get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;

    if format.needs_excel_extension() {
        ensure_excel_extension(&conn)?;
    }

    copy_table_to_file(&conn, table, path, format)?;

    Ok(ExportStats {
        rows: row_count,
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
    use super::{
        DuckDbExportFormat, export_batches_via_duckdb, export_existing_table_via_duckdb,
        export_rows_via_duckdb,
    };
    use crate::duckdb::{TempTableSource, drop_temp_table};
    use crate::services::duckdb_service::DuckDbService;
    use arrow::array::{ArrayRef, Int64Array, RecordBatch, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use serde_json::{Value, json};
    use shared::models::ArrowBatch;
    use std::sync::Arc;

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

    /// 混合列事实档批（id Int64 + name Utf8，第三行 name 为 NULL）。
    fn mixed_batch() -> ArrowBatch {
        let id: ArrayRef = Arc::new(Int64Array::from(vec![Some(1i64), Some(2), None]));
        let name: ArrayRef = Arc::new(StringArray::from(vec![Some("alpha"), None, Some("gamma")]));
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(schema, vec![id, name]).expect("批能建出来")
    }

    /// 【D1】内存档 batches 直灌导出 Parquet：列类型来自 Arrow schema，数字列落 BIGINT，
    /// 不是 internal 猜型档的 VARCHAR；临时表成败都收、即用即收。
    #[test]
    fn batches_parquet_export_keeps_native_integer_type() {
        let dir = std::env::temp_dir().join(format!("rds_export_batches_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("batches.parquet");
        std::fs::remove_file(&path).ok();

        let batch = mixed_batch();
        let stats = export_batches_via_duckdb(&[batch], &path, DuckDbExportFormat::Parquet)
            .expect("batches 直灌导出应当成功");
        assert_eq!(stats.rows, 3);

        let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
        let mut conn = duckdb.lock().expect("锁");
        let file = path.to_string_lossy().replace('\\', "/");

        // 类型保真：id 是 BIGINT、name 是 VARCHAR（对照 internal 猜型档整列落 VARCHAR）
        let (cols, types) = DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT typeof(id) AS id_type, typeof(name) AS name_type FROM read_parquet('{file}') LIMIT 1"),
        )
        .expect("读回类型应当成功");
        assert_eq!(cols, vec!["id_type".to_string(), "name_type".to_string()]);
        assert_eq!(types[0][0].as_str(), Some("BIGINT"), "数字列要保留数值类型");
        assert_eq!(types[0][1].as_str(), Some("VARCHAR"), "文本列仍是文本");

        // 值与 NULL 对上（按 id 排，NULL 行在最后）
        let (_, got) = DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT id, name FROM read_parquet('{file}') ORDER BY id NULLS LAST"),
        )
        .expect("读回值应当成功");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], vec![json!(1), json!("alpha")]);
        assert_eq!(got[1], vec![json!(2), Value::Null]);

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 【D1】表档常驻表复用导出：直接 `COPY` 常驻表，不新建、不 DROP；导出后表仍可查询。
    #[test]
    fn existing_table_parquet_export_reuses_without_dropping_table() {
        let dir = std::env::temp_dir().join(format!("rds_export_table_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("reused.parquet");
        std::fs::remove_file(&path).ok();

        let duckdb = DuckDbService::get_or_create_duckdb().expect("内存库");
        let batch = mixed_batch();
        // 先建常驻表（函数内部取锁，建完即放，不跨调用持锁避免死锁）
        let table = {
            let mut conn = duckdb.lock().expect("锁");
            DuckDbService::create_temp_table_from_batches(&mut conn, &[batch])
                .expect("常驻表应当建出来")
        };

        let stats = export_existing_table_via_duckdb(
            &table,
            3,
            &path,
            DuckDbExportFormat::Parquet,
        )
        .expect("常驻表复用导出应当成功");
        assert_eq!(stats.rows, 3);

        // 导出后常驻表仍在（本路径不回收表，归事实档生命周期管）
        let mut conn = duckdb.lock().expect("锁");
        let (cols, count_rows) = DuckDbService::query_duckdb(
            &mut conn,
            &format!("SELECT count(*) AS n FROM \"{table}\""),
        )
        .expect("复用表应当还能查");
        assert_eq!(cols, vec!["n".to_string()]);
        assert_eq!(count_rows[0][0].as_i64(), Some(3), "表没被导出路径 DROP");

        drop_temp_table(&conn, TempTableSource::Query, &table).expect("测试收尾清场");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod analysis_batches_tests {
    // 安全模式：**不通配导入**
    use super::execute_duckdb_analysis_from_batches;

    use arrow::array::{ArrayRef, Int64Array, RecordBatch};
    use arrow::datatypes::{Field, Schema, DataType};
    use shared::models::ArrowBatch;
    use std::sync::Arc;

    /// 单列 Int64、两行的事实档批（类型来自 schema，不猜型）。
    fn id_batch() -> ArrowBatch {
        let array: ArrayRef = Arc::new(Int64Array::from(vec![1i64, 2]));
        let schema = Arc::new(Schema::new(vec![Field::new(
            "id",
            DataType::Int64,
            false,
        )]));
        RecordBatch::try_new(schema, vec![array]).expect("批能建出来")
    }

    /// 【B4】事实档 batches 直灌新建表跑聚合：类型保真、表名回传、结果行数对上。
    #[test]
    fn batches_land_and_aggregate_on_a_fresh_table() {
        let batch = id_batch();
        let result = execute_duckdb_analysis_from_batches(
            None,
            "SELECT count(*) AS \"行数\" FROM {table}",
            &[batch],
        )
        .expect("直灌建表 + 聚合应当成功");

        assert_eq!(result.columns, vec!["行数".to_string()]);
        assert_eq!(result.rows.len(), 1);
        let count = result.rows[0][0].as_i64().expect("count 是整数");
        assert_eq!(count, 2, "两行事实档聚合出 2");
        assert!(!result.temp_table.is_empty(), "回执带回实际表名");
    }

    /// 【B4】已有常驻表名直接复用：不重复建表，表名原样返回、结果一致。
    ///
    /// 刻意用 `count(*)`（BIGINT，已在 `duckdb_value_to_json` 映射表内）：`sum(BIGINT)` 在
    /// DuckDB 返回 HUGEINT，尚落在"其余变体回 Null"的遗留 backlog（方案 §10.2 #12），
    /// 本用例不扩面修它，只钉住复用链路本身。
    #[test]
    fn existing_temp_table_is_reused_without_rebuilding() {
        let batch = id_batch();
        let first = execute_duckdb_analysis_from_batches(
            None,
            "SELECT count(*) AS \"行数\" FROM {table}",
            &[batch],
        )
        .expect("首轮直灌成功");

        // 复用上一轮回传的表名：传入 `Some(...)`，函数不新建表
        let reused = execute_duckdb_analysis_from_batches(
            Some(first.temp_table.as_str()),
            "SELECT count(*) AS \"计数\" FROM {table}",
            &[],
        )
        .expect("复用常驻表聚合应当成功");

        assert_eq!(reused.temp_table, first.temp_table, "复用同一表名");
        let count = reused.rows[0][0].as_i64().expect("count 是整数");
        assert_eq!(count, 2, "复用表里的两行没丢");
    }
}

use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

use crate::DuckDBManager;
use shared::error::{CommonError, CoreError};
use shared::models::ArrowBatch;

pub struct DuckDbService;

impl DuckDbService {
    pub fn get_or_create_duckdb() -> Result<Arc<std::sync::Mutex<duckdb::Connection>>, CoreError> {
        DuckDBManager::get_or_create_in_memory()
    }

    /// 运行时调整内存库上限（MB）——转发到 [`DuckDBManager::set_memory_limit_mb`]。
    ///
    /// 设置页经装配层 sink 调用（`settings` 不依赖 `engine`，出口由装配层注册）。
    pub fn set_memory_limit_mb(mb: u64) -> Result<(), CoreError> {
        DuckDBManager::set_memory_limit_mb(mb)
    }

    /// 运行时调整**每句语句超时**（毫秒；`0` = 不限时）——设置页改「语句超时」走这里。
    ///
    /// 超时由实例的看门狗落实（超时只中断本会话的语句，会话本身继续可用）；
    /// 改完只影响之后开始的语句。值落盘在 `settings`，装配层负责在启动与变更时调这里。
    pub fn apply_statement_timeout_ms(ms: u64) -> Result<(), CoreError> {
        let instance = crate::duckdb::instance::instance().map_err(|error| {
            CoreError::common(CommonError::General(format!("取分析实例失败: {error}")))
        })?;
        instance.set_timeout_ms(ms);
        Ok(())
    }

    /// 内存库可观测读数（上限 / 实际大小 / 临时表数）——转发到
    /// [`DuckDBManager::memory_stats`]。洞察工具栏的轮询采样走这里。
    pub fn memory_stats() -> Result<crate::duckdb::MemoryStats, CoreError> {
        DuckDBManager::memory_stats()
    }

    // 2026-09-19 删除 `accelerate_query`：它零调用，且依赖的 `dbi` 层整体已废弃
    // （2124 行里只有 `DuckDBEngine::file_reader_function` 是活的，现挂在
    // `crate::duckdb::probe::file_reader`，经 `duckdb/mod.rs` 的 `pub use probe::*` 传导到
    // `crate::duckdb::file_reader_function`）。加速档的实际实现是 `crate::duckdb::accel`，
    // 它自己拼 `ATTACH … (READ_ONLY)`，不经过这里。

    pub fn create_duckdb_temp_table(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
        owner: &str,
    ) -> Result<String, CoreError> {
        Self::create_duckdb_temp_table_named(columns, rows, "result", owner)
    }

    /// 带**描述**的版本：描述会进表名（`tmp_q_<描述>_<时间戳>_<随机>`）。
    ///
    /// 用户给结果集起名后再物化就走这里——名字要能在面板里认出来。注意临时表名的
    /// 尺子只留 ASCII（[`crate::duckdb::generate_unique_name`] 的既有口径），中文会被
    /// 折成下划线：界面必须展示**实际生成的表名**，不要拿用户输入直接当表名显示。
    ///
    /// `owner` 是这张表该由谁回收（编辑器传文档 id，模块自己传模块名）。
    pub fn create_duckdb_temp_table_named(
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
        description: &str,
        owner: &str,
    ) -> Result<String, CoreError> {
        let duckdb = Self::get_or_create_duckdb()?;
        let mut conn = duckdb.lock().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
        })?;
        Self::create_temp_table_internal(&mut conn, columns, rows, description, owner)
    }

    /// 建一张**查询结果**临时表（表名带 `tmp_q_` 前缀、建完即登记）并回填数据。
    ///
    /// 命名走 [`crate::duckdb::generate_unique_name`]：前缀不是装饰——
    /// TTL / 上限 / 按来源清理 / 关项目清场全靠它识别；历史上的 `rs_<uuid>` 不属于任何前缀，
    /// 于是那些机制对它全部失效（K16）。
    ///
    /// 回收责任在**建表方**：结果集被丢弃 / 替换 / 关文档时调
    /// [`crate::duckdb::drop_temp_table`]（定向），项目切换 / 关闭时调
    /// [`crate::duckdb::DuckDBManager::drop_in_memory_temp_tables`]（清场）。
    pub fn create_temp_table_internal(
        conn: &mut duckdb::Connection,
        columns: &[String],
        rows: &[Vec<serde_json::Value>],
        description: &str,
        owner: &str,
    ) -> Result<String, CoreError> {
        let table_name = crate::duckdb::generate_unique_name(
            crate::duckdb::TempTableSource::Query,
            description,
        );
        // 第 3b 项：结果集落 **`rds_tmp`**（跨会话共享的唯一去处），名字仍按裸名返回——
        // `search_path` 让裸名与 `"双引号名"` 都解析到那里（见 `duckdb/temp_table/shared.rs`）。
        crate::duckdb::temp_table::ensure_shared_schema(conn).map_err(|error| {
            CoreError::common(CommonError::General(error))
        })?;
        let target = crate::duckdb::temp_table::qualified(&table_name);
        let col_defs: Vec<String> = columns
            .iter()
            .enumerate()
            .map(|(i, col)| {
                let dtype = if rows.is_empty() {
                    "VARCHAR"
                } else {
                    infer_type(rows, i)
                };
                format!("\"{}\" {}", col, dtype)
            })
            .collect();

        conn.execute_batch(&format!(
            "CREATE TABLE {} ({})",
            target,
            col_defs.join(", ")
        ))
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to create DuckDB table: {}",
                e
            )))
        })?;

        if !rows.is_empty() {
            let placeholders: Vec<String> = (0..columns.len()).map(|_| "?".to_string()).collect();
            let insert_sql = format!(
                "INSERT INTO {} VALUES ({})",
                target,
                placeholders.join(", ")
            );
            let mut stmt = conn.prepare(&insert_sql).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Prepare insert failed: {}",
                    e
                )))
            })?;

            for row in rows {
                let params: Vec<duckdb::types::Value> =
                    row.iter().map(json_to_duckdb_value).collect();
                let params_refs: Vec<&dyn duckdb::types::ToSql> = params
                    .iter()
                    .map(|p| p as &dyn duckdb::types::ToSql)
                    .collect();
                stmt.execute(&params_refs[..]).map_err(|e| {
                    CoreError::common(CommonError::General(format!("Insert row failed: {}", e)))
                })?;
            }
        }

        DuckDBManager::register_temp_table(&table_name, owner);
        Ok(table_name)
    }

    /// 用一个 Arrow 批**直接**建一张查询结果临时表。
    ///
    /// 表名与回收口径同 [Self::create_temp_table_internal]（前缀 tmp_q_ + 登记），
    /// 区别是数据**不过 Rust 的值层**：批经 DuckDB 的 Appender 按 Arrow 数据块直灌。
    ///
    /// 与逐行 INSERT 那条路真正的差别不只是快，而是**类型保真**：列类型来自 Arrow schema
    /// （DECIMAL(38,10) 还是 DECIMAL(38,10)，带时区的时间还是带时区的时间），不是从值里猜出来的。
    /// 二次分析（洞察 / 自定义分析）就卡在这里：结果集本来就是 Arrow，绕成字符串再猜回去，
    /// 精度与时间语义全丢在半路上。
    ///
    /// 三条口径（都是「不得静默退化」）：
    /// - 所有批**同一个形状**（列名与类型逐一对上）：不一致直接报错点名，中间层不做隐式转换；
    /// - 不支持的 Arrow 类型**报错**，不悄悄落成 VARCHAR（那会让人以为数据本身就是文本）；
    /// - 列类型与 duckdb-rs 的写入层**对齐**（见 [map_arrow_type]）：声明成 DECIMAL 而写入按
    ///   DOUBLE 转换，会在 append 那一步炸得莫名其妙；
    /// - 超出 DuckDB 表达范围的（`Decimal256` 宽度 > 38）**降级成 DOUBLE 并记降级说明**：
    ///   数据能读出来，但「读得准」不成立——原因带列名进日志，不静默（见 [`map_arrow_type`]）。
    pub fn create_temp_table_from_batches(
        conn: &mut duckdb::Connection,
        batches: &[ArrowBatch],
        owner: &str,
    ) -> Result<String, CoreError> {
        let raw_schema = batches
            .first()
            .map(|batch| batch.schema())
            .ok_or_else(|| general_error("没有批可以建表：Arrow 直灌至少要一批（哪怕 0 行）"))?;

        // Decimal256 在 DuckDB 的 Arrow→DuckDB 转换里**没有路**（2026-11-27 在 2.0.1 实测原话：
        // `Data type "Decimal256(65, 30)" not yet supported by Arrow-to-DuckDB conversion`），
        // 声明成 DOUBLE 也过不了 append。所以在 Arrow 层先 cast 成 Float64：数据能读出来，
        // 小数位精度会丢——降级说明带列名进日志，不静默。
        let mut downgrades: Vec<(String, String)> = Vec::new();
        let schema = Arc::new(Schema::new(
            raw_schema
                .fields()
                .iter()
                .map(|field| {
                    if let DataType::Decimal256(width, scale) = field.data_type() {
                        downgrades.push((
                            field.name().to_string(),
                            format!(
                                "DECIMAL({width},{scale}) 超出 DuckDB 的 DECIMAL 宽度上限 38，且 DuckDB 的 Arrow 转换不认 Decimal256，已按 DOUBLE 承接：整数位可读，小数位精度会丢"
                            ),
                        ));
                        Field::new(field.name().clone(), DataType::Float64, field.is_nullable())
                    } else {
                        (**field).clone()
                    }
                })
                .collect::<Vec<_>>(),
        ));

        let casted: Vec<ArrowBatch> = batches
            .iter()
            .map(|batch| cast_decimal256_columns(batch, &schema))
            .collect::<Result<Vec<_>, CoreError>>()?;

        for (i, batch) in casted.iter().enumerate() {
            if !same_shape(&schema, &batch.schema()) {
                return Err(general_error(format!(
                    "第 {i} 个批的形状与第一个不一致（列名或类型变了）：直灌不做隐式转换，请在上游对齐"
                )));
            }
        }

        let col_defs: Vec<String> = schema
            .fields()
            .iter()
            .map(|field| {
                let (ty, _) = map_arrow_type(field.data_type()).map_err(|reason| {
                    general_error(format!("列 {} 的类型没法直灌：{reason}", field.name()))
                })?;
                Ok(format!("{} {}", quote_ident(field.name()), ty))
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        for (column, note) in &downgrades {
            tracing::warn!(column = %column, "{note}");
        }

        // 命名口径与逐行那条路一致：前缀是 TTL / 上限 / 按来源清理 / 关项目清场唯一的识别依据
        let table_name =
            crate::duckdb::generate_unique_name(crate::duckdb::TempTableSource::Query, "result");
        // 第 3b 项：落 `rds_tmp`、返回裸名（同逐行那条路）
        crate::duckdb::temp_table::ensure_shared_schema(conn)
            .map_err(|error| general_error(error))?;
        let target = crate::duckdb::temp_table::qualified(&table_name);

        conn.execute_batch(&format!(
            "CREATE TABLE {} ({})",
            target,
            col_defs.join(", ")
        ))
        .map_err(|e| general_error(format!("建直灌临时表失败: {e}")))?;

        {
            // appender 认默认 schema、不吃 `search_path` → 显式给 schema
            let mut appender = conn
                .appender_to_db(
                    &table_name,
                    crate::duckdb::temp_table::SHARED_TEMP_SCHEMA,
                )
                .map_err(|e| general_error(format!("打开直灌 Appender 失败: {e}")))?;
            for batch in &casted {
                if batch.num_rows() == 0 {
                    // 空批没有要写的数据；它仍然有用（建表的 schema 可能是从它来的）
                    continue;
                }
                let batch = decode_dictionary_columns(batch)?;
                appender
                    .append_record_batch(batch)
                    .map_err(|e| general_error(format!("Arrow 批直灌失败: {e}")))?;
            }
            appender
                .flush()
                .map_err(|e| general_error(format!("直灌收尾失败: {e}")))?;
        }

        DuckDBManager::register_temp_table(&table_name, owner);
        Ok(table_name)
    }

    pub fn query_duckdb(
        conn: &mut duckdb::Connection,
        sql: &str,
    ) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>), CoreError> {
        let mut stmt = conn.prepare(sql).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB prepare failed: {}",
                e
            )))
        })?;

        // 顺序不能反：duckdb-rs 1.10505 的 `Statement::column_names()` / `schema()` 读的是
        // **执行结果**（`raw_statement.rs` 的 `executed()`），prepare 之后、执行之前调会直接
        // panic（`The statement was not executed yet`）。所以先 `query` 把语句跑起来，再从
        // `Rows::as_ref()` 取回那条已执行的语句读列名——这也是 duckdb-rs 文档推荐的写法。
        // （1.20001.0 / DuckDB 2.0.1 复核：prepare 后执行前取列名仍 panic，此顺序不能改）
        let mut query_rows = stmt.query([]).map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB query failed: {}", e)))
        })?;
        let col_names = query_rows
            .as_ref()
            .map(|statement| statement.column_names())
            .unwrap_or_default();
        let col_count = col_names.len();

        let mut rows = Vec::new();
        while let Some(row) = query_rows.next().map_err(|e| {
            CoreError::common(CommonError::General(format!("DuckDB row error: {}", e)))
        })? {
            let mut values = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let v: duckdb::types::Value = row.get(i).map_err(|e| {
                    CoreError::common(CommonError::General(format!("DuckDB cell error: {}", e)))
                })?;
                values.push(duckdb_value_to_json(&v));
            }
            rows.push(values);
        }

        Ok((col_names, rows))
    }
}

/// `QueryResult` → 建 DuckDB 临时表用的行（**进程内直读 Arrow `batches`**）。
///
/// 为什么不走 `serde_json::to_value(&QueryResult)` 再读 `["batches"]`：契约序列化只输出
/// `columns` / `rows` / …，Arrow `batches` 不参与，而 native 驱动（MySQL / PostgreSQL /
/// SQLite / DuckDB）**只填 `batches`**、`rows` 字段恒为空 —— 那条路读到的永远是
/// 「有列名、零行」，临时表于是建出一张空表。洞察侧踩过同一个坑并留了回归
/// （`insight::service::result_columns_and_rows`），这里补上引擎侧的那一处。
pub fn rows_as_json(result: &shared::models::QueryResult) -> Vec<Vec<serde_json::Value>> {
    result
        .to_rows()
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(shared::models::Value::to_json)
                .collect()
        })
        .collect()
}

/// JSON 行 → DuckDB 列类型。
///
/// 同 crate 内公开：`duckdb::analysis`（分析临时表）灌样本时要与结果集那条路径
/// **用同一套打型规则**，否则同一个 `serde_json` 值在两处会变成不同类型。
pub(crate) fn infer_type(rows: &[Vec<serde_json::Value>], col_idx: usize) -> &str {
    for row in rows {
        if col_idx < row.len() {
            match &row[col_idx] {
                serde_json::Value::Null => continue,
                serde_json::Value::Number(n) => {
                    return if n.is_f64() { "DOUBLE" } else { "BIGINT" };
                }
                serde_json::Value::Bool(_) => return "BOOLEAN",
                _ => return "VARCHAR",
            }
        }
    }
    "VARCHAR"
}

/// JSON 值 → DuckDB 值（插入参数）。
///
/// 同 `infer_type`：与 `duckdb::analysis` 共用一份转换。
pub(crate) fn json_to_duckdb_value(v: &serde_json::Value) -> duckdb::types::Value {
    match v {
        serde_json::Value::Null => duckdb::types::Value::Null,
        serde_json::Value::Bool(b) => duckdb::types::Value::Boolean(*b),
        serde_json::Value::Number(n) => n
            .as_f64()
            .map(duckdb::types::Value::Double)
            .or_else(|| n.as_i64().map(duckdb::types::Value::BigInt))
            .unwrap_or(duckdb::types::Value::Text(n.to_string())),
        serde_json::Value::String(s) => duckdb::types::Value::Text(s.clone()),
        _ => duckdb::types::Value::Text(v.to_string()),
    }
}

pub fn duckdb_value_to_json(v: &duckdb::types::Value) -> serde_json::Value {
    match v {
        duckdb::types::Value::Null => serde_json::Value::Null,
        duckdb::types::Value::Boolean(b) => serde_json::json!(b),
        duckdb::types::Value::TinyInt(n) => serde_json::json!(n),
        duckdb::types::Value::SmallInt(n) => serde_json::json!(n),
        duckdb::types::Value::Int(n) => serde_json::json!(n),
        duckdb::types::Value::BigInt(n) => serde_json::json!(n),
        duckdb::types::Value::Float(f) => serde_json::json!(f),
        duckdb::types::Value::Double(f) => serde_json::json!(f),
        duckdb::types::Value::Text(s) => serde_json::Value::String(s.clone()),
        _ => serde_json::Value::Null,
    }
}

// 列类型族判定（`is_numeric_type` / `is_datetime_type` / …）已移入 `crates/insight`：
// 它们唯一的调用方是洞察的类型分派，且编码的是「哪种类型用哪些统计量」的业务语义。

#[allow(dead_code)]
pub fn is_json_type(dt_lower: &str) -> bool {
    matches!(dt_lower, "json" | "jsonb")
}
// ─── 数据导出 ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub enum ExportFormat {
    #[serde(rename = "csv")]
    Csv,
    #[serde(rename = "parquet")]
    Parquet,
    #[serde(rename = "xlsx")]
    Xlsx,
}

impl ExportFormat {
    fn sql_format(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "FORMAT CSV, HEADER true",
            ExportFormat::Parquet => "FORMAT PARQUET",
            ExportFormat::Xlsx => "FORMAT XLSX, HEADER true",
        }
    }

    fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Parquet => "parquet",
            ExportFormat::Xlsx => "xlsx",
        }
    }
}

impl DuckDbService {
    pub fn export_temp_table(
        temp_table: &str,
        file_path: &str,
        format: ExportFormat,
    ) -> Result<String, shared::error::CoreError> {
        use crate::duckdb::DuckDBManager;
        use shared::error::CommonError;

        let arc = DuckDBManager::get_or_create_in_memory()?;
        let conn = arc.lock().map_err(|e| {
            shared::error::CoreError::common(CommonError::General(format!(
                "DuckDB lock error during export: {}",
                e
            )))
        })?;

        let escaped_path = file_path.replace('\'', "''");
        let sql = format!(
            "COPY \"{}\" TO '{}' ({})",
            temp_table,
            escaped_path,
            format.sql_format()
        );
        conn.execute_batch(&sql).map_err(|e| {
            shared::error::CoreError::common(CommonError::General(format!(
                "导出 {} 失败: {}",
                format.extension(),
                e
            )))
        })?;

        Ok(file_path.to_string())
    }
}

/// 这一层的错误口径（与同文件其它函数一致：General 带一句能读的话）。
fn general_error(message: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::General(message.into()))
}

/// 两个 schema 是不是同一个形状（列名与类型逐一对上）。
///
/// **不比 metadata**：rds.* 那些键是给面板看的，不影响列类型；拿它们判等会把
/// 「同一份结果的第二页」误判成形状不一致（对端每页都带 metadata 的话更是必然）。
fn same_shape(a: &Schema, b: &Schema) -> bool {
    a.fields().len() == b.fields().len()
        && a.fields()
            .iter()
            .zip(b.fields().iter())
            .all(|(x, y)| x.name() == y.name() && x.data_type() == y.data_type())
}

/// SQL 标识符加引号（引号翻倍转义）。
///
/// 列名来自结果集（用户 SQL 的别名也在里面），直接拼进 DDL 就是注入面；
/// 引号本身也要处理，否则一个带引号的别名就能把 DDL 拼坏。
fn quote_ident(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for ch in name.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// Arrow 类型 → DuckDB 列类型（建表用）。
///
/// 映射与 duckdb-rs 的写入层对齐（它的 arrow_interop/schema.rs 把 Arrow 类型映射成
/// DuckDB 逻辑类型；这里写的是同一套的 SQL 写法）——两边不一致就会「声明成 A、写进去按 B」，
/// 在 append 那一步报一句谁都看不懂的错。
///
/// 不支持的类型**明确报错**：退化成 VARCHAR 是最坏的一种「成功」，用户会以为数据本身是文本。
/// 把批里的 Decimal256 列 cast 成 Float64——DuckDB 的 Arrow→DuckDB 转换不认 Decimal256
/// （2026-11-27 在 2.0.1 实测原话："not yet supported by Arrow-to-DuckDB conversion"）。
///
/// 没有 Decimal256 列时原样返回（批的克隆是廉价的，缓冲都是 Arc）。
fn cast_decimal256_columns(
    batch: &ArrowBatch,
    target: &Arc<Schema>,
) -> Result<ArrowBatch, CoreError> {
    let has_decimal256 = batch
        .schema()
        .fields()
        .iter()
        .any(|f| matches!(f.data_type(), DataType::Decimal256(..)));
    if !has_decimal256 {
        return Ok(batch.clone());
    }

    let mut columns: Vec<arrow::array::ArrayRef> = Vec::with_capacity(batch.num_columns());
    for (i, field) in batch.schema().fields().iter().enumerate() {
        let column = batch.column(i).clone();
        if matches!(field.data_type(), DataType::Decimal256(..)) {
            columns.push(
                arrow::compute::cast(&column, &DataType::Float64).map_err(|e| {
                    general_error(format!(
                        "列 {} 的 Decimal256 转 DOUBLE 失败: {e}",
                        field.name()
                    ))
                })?,
            );
        } else {
            columns.push(column);
        }
    }
    RecordBatch::try_new(target.clone(), columns)
        .map_err(|e| general_error(format!("Decimal256 降级后重建批失败: {e}")))
}

/// 把 Arrow 类型映射成 DuckDB 列类型；`Some` = 这一列是有损承接（降级说明），`None` = 无损。
///
/// 只有确实超出 DuckDB 表达范围的情况才降级；不支持的 Arrow 类型仍**明确报错**
/// （退化成 VARCHAR 是最坏的一种「成功」，用户会以为数据本身是文本）。
fn map_arrow_type(data_type: &DataType) -> Result<(String, Option<String>), String> {
    fn plain(ty: &str) -> Result<(String, Option<String>), String> {
        Ok((ty.to_string(), None))
    }

    match data_type {
        DataType::Boolean => plain("BOOLEAN"),
        DataType::Int8 => plain("TINYINT"),
        DataType::Int16 => plain("SMALLINT"),
        DataType::Int32 => plain("INTEGER"),
        DataType::Int64 => plain("BIGINT"),
        DataType::UInt8 => plain("UTINYINT"),
        DataType::UInt16 => plain("USMALLINT"),
        DataType::UInt32 => plain("UINTEGER"),
        DataType::UInt64 => plain("UBIGINT"),
        DataType::Float32 => plain("FLOAT"),
        DataType::Float64 => plain("DOUBLE"),
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => plain("VARCHAR"),
        DataType::Binary
        | DataType::LargeBinary
        | DataType::BinaryView
        | DataType::FixedSizeBinary(_) => plain("BLOB"),
        DataType::Date32 | DataType::Date64 => plain("DATE"),
        DataType::Time32(_) | DataType::Time64(_) => plain("TIME"),
        DataType::Timestamp(unit, None) => plain(match unit {
            TimeUnit::Second => "TIMESTAMP_S",
            TimeUnit::Millisecond => "TIMESTAMP_MS",
            TimeUnit::Microsecond => "TIMESTAMP",
            TimeUnit::Nanosecond => "TIMESTAMP_NS",
        }),
        DataType::Timestamp(_, Some(_)) => plain("TIMESTAMPTZ"),
        DataType::Duration(_) | DataType::Interval(_) => plain("INTERVAL"),
        DataType::Decimal32(width, scale)
        | DataType::Decimal64(width, scale)
        | DataType::Decimal128(width, scale) => plain(&format!("DECIMAL({width},{scale})")),
        // 256 位宽：DuckDB 的 DECIMAL 宽度上限是 38（2026-11-27 在 2.0.1 实测：
        // `CREATE TABLE t(x DECIMAL(39,30))` 直接 Binder Error "DECIMAL type width
        // must be between 1 and 38"），duckdb-rs 的写入层对 256 位十进制也只按 DOUBLE
        // 走。承接成 DOUBLE 而不是报错：跨源（MySQL `DECIMAL(65,30)` 这类）至少能查
        // 出来，代价写进降级说明——「能读」与「读得准」之间的差别要说清楚。
        DataType::Decimal256(width, scale) => Ok((
            "DOUBLE".to_string(),
            Some(format!(
                "DECIMAL({width},{scale}) 超出 DuckDB 的 DECIMAL 宽度上限 38，已按 DOUBLE 承接：整数位可读，小数位精度会丢"
            )),
        )),
        // 词典编码走的是值类型（写入前会被解码成值类型，见 decode_dictionary_columns）
        DataType::Dictionary(_, value_type) => map_arrow_type(value_type),
        other => Err(format!("{other:?} 还没接（直灌前请先转成支持的类型）")),
    }
}

/// 词典编码的列先解成值类型。
///
/// 不是优化：duckdb-rs 的写入层按值类型逐列写数据块，没有 Dictionary 那条路，
/// 而协议**允许**对端用词典编码（§4.2.4 的硬性约束里写着「dictionary encoding 允许」）。
fn decode_dictionary_columns(batch: &ArrowBatch) -> Result<ArrowBatch, CoreError> {
    let schema = batch.schema();
    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut columns = Vec::with_capacity(schema.fields().len());
    let mut decoded_any = false;

    for (i, field) in schema.fields().iter().enumerate() {
        let column = batch.column(i);
        if let DataType::Dictionary(_, value_type) = field.data_type() {
            decoded_any = true;
            let value_type = value_type.as_ref().clone();
            let decoded = arrow::compute::cast(column, &value_type)
                .map_err(|e| general_error(format!("列 {} 的词典编码解不开: {e}", field.name())))?;
            fields.push(Field::new(field.name(), value_type, field.is_nullable()));
            columns.push(decoded);
        } else {
            fields.push(field.as_ref().clone());
            columns.push(column.clone());
        }
    }

    if !decoded_any {
        // 没有词典列就原样传：批的克隆是廉价的（缓冲是 Arc），不必复制数据
        return Ok(batch.clone());
    }
    RecordBatch::try_new(std::sync::Arc::new(Schema::new(fields)), columns)
        .map_err(|e| general_error(format!("解完词典编码后重建批失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::DuckDbService;
    use arrow::array::Array;
    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use arrow::record_batch::RecordBatch;
    use shared::models::ArrowBatch;

    use crate::DuckDBManager;
    use crate::duckdb::{TempTableSource, drop_temp_table};
    use serde_json::json;

    /// 一份类型齐全的批：整数 / 高精度小数 / 带时区时间 / 长文本 / 二进制 / 布尔 / 可空。
    fn typed_batch() -> ArrowBatch {
        use arrow::array::{
            BooleanArray, Decimal128Array, Int64Array, LargeBinaryArray, LargeStringArray,
            TimestampMicrosecondArray,
        };

        let schema = std::sync::Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("amount", DataType::Decimal128(38, 10), true),
            Field::new(
                "created_at",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".to_string().into())),
                false,
            ),
            Field::new("note", DataType::LargeUtf8, true),
            Field::new("payload", DataType::LargeBinary, true),
            Field::new("flag", DataType::Boolean, true),
        ]));

        RecordBatch::try_new(
            schema,
            vec![
                std::sync::Arc::new(Int64Array::from(vec![1, 2])),
                std::sync::Arc::new(
                    Decimal128Array::from(vec![Some(12345678901234567890i128), None])
                        .with_precision_and_scale(38, 10)
                        .expect("精度合法"),
                ),
                std::sync::Arc::new(
                    TimestampMicrosecondArray::from(vec![
                        1_700_000_000_000_000i64,
                        1_700_000_001_000_000i64,
                    ])
                    .with_timezone("UTC"),
                ),
                std::sync::Arc::new(LargeStringArray::from(vec![Some("订单"), None])),
                std::sync::Arc::new(LargeBinaryArray::from(vec![Some(&b"ab"[..]), None])),
                std::sync::Arc::new(BooleanArray::from(vec![Some(true), None])),
            ],
        )
        .expect("批能建出来")
    }

    /// 一个值的文本形态（typeof / 单元格都用它取，省得为每个断言写一次 prepare）。
    fn scalar_text(conn: &duckdb::Connection, sql: &str) -> String {
        conn.query_row(sql, [], |row| row.get::<_, String>(0))
            .expect("取一个值")
    }

    fn scalar_i64(conn: &duckdb::Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .expect("取一个整数")
    }

    /// Arrow 直灌：列类型来自 schema（不是从值猜的），值也原样过来。
    #[test]
    fn arrow_batches_land_with_their_own_types() {
        let mut conn = duckdb::Connection::open_in_memory().expect("内存连接");
        let batch = typed_batch();
        let table =
            DuckDbService::create_temp_table_from_batches(&mut conn, &[batch], "test").expect("直灌建表");

        assert!(
            table.starts_with("tmp_q_"),
            "结果集临时表名要在 tmp_q_ 前缀下: {table}"
        );
        assert_eq!(
            DuckDBManager::temp_table_manager().count_by_prefix(&table),
            1,
            "建完应当被登记（回收机制全靠这个前缀）"
        );

        // 类型保真：这几条正是「逐行 INSERT + 猜类型」那条路做不到的
        let ty =
            |col: &str| scalar_text(&conn, &format!("SELECT typeof({col}) FROM {table} LIMIT 1"));
        assert_eq!(ty("id"), "BIGINT");
        assert_eq!(ty("amount"), "DECIMAL(38,10)");
        assert!(
            ty("created_at").contains("TIMESTAMP"),
            "{}",
            ty("created_at")
        );
        assert!(
            ty("created_at").contains("WITH TIME ZONE"),
            "{}",
            ty("created_at")
        );
        assert_eq!(ty("note"), "VARCHAR");
        assert_eq!(ty("payload"), "BLOB");
        assert_eq!(ty("flag"), "BOOLEAN");

        // 值也对：高精度小数没被压成 f64，时间没被字符串化
        assert_eq!(
            scalar_i64(&conn, &format!("SELECT count(*) FROM {table}")),
            2
        );
        assert_eq!(
            scalar_text(
                &conn,
                &format!("SELECT CAST(amount AS VARCHAR) FROM {table} WHERE id = 1")
            ),
            "1234567890.1234567890"
        );
        assert_eq!(
            scalar_text(&conn, &format!("SELECT note FROM {table} WHERE id = 1")),
            "订单"
        );
        assert_eq!(
            scalar_text(
                &conn,
                &format!("SELECT CAST(payload AS VARCHAR) FROM {table} WHERE id = 1")
            ),
            "ab"
        );

        drop_temp_table(&conn, TempTableSource::Query, &table).expect("删表");
    }

    /// 多批直灌：行数累加，空批不干扰（它对建表的 schema 仍然有用）。
    #[test]
    fn several_batches_accumulate_and_empty_ones_are_harmless() {
        let mut conn = duckdb::Connection::open_in_memory().expect("内存连接");
        let batch = typed_batch();
        let empty = batch.slice(0, 0);
        let table = DuckDbService::create_temp_table_from_batches(
            &mut conn,
            &[empty, batch.slice(0, 1), batch.slice(1, 1)],
            "test",
        )
        .expect("多批直灌");

        assert_eq!(
            scalar_i64(&conn, &format!("SELECT count(*) FROM {table}")),
            2
        );
        // 空批在前时，建表用的是它的 schema（列照样齐）
        assert_eq!(
            scalar_text(
                &conn,
                &format!("SELECT typeof(amount) FROM {table} LIMIT 1")
            ),
            "DECIMAL(38,10)"
        );
        drop_temp_table(&conn, TempTableSource::Query, &table).expect("删表");
    }

    /// 词典编码的列：进去之前解成值类型（协议允许对端用词典编码）。
    #[test]
    fn a_dictionary_column_is_decoded_on_the_way_in() {
        use arrow::array::{DictionaryArray, Int32Array, StringArray};

        let mut conn = duckdb::Connection::open_in_memory().expect("内存连接");
        let keys = Int32Array::from(vec![0, 1, 0]);
        let values = StringArray::from(vec!["甲", "乙"]);
        let dict = DictionaryArray::new(keys, std::sync::Arc::new(values));
        let schema = std::sync::Arc::new(Schema::new(vec![Field::new(
            "name",
            dict.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![std::sync::Arc::new(dict)]).expect("批");

        let table = DuckDbService::create_temp_table_from_batches(&mut conn, &[batch], "test")
            .expect("词典编码也该能直灌");
        assert_eq!(
            scalar_text(&conn, &format!("SELECT typeof(name) FROM {table} LIMIT 1")),
            "VARCHAR",
            "词典编码解成值类型后落成文本列"
        );
        assert_eq!(
            scalar_i64(&conn, &format!("SELECT count(*) FROM {table}")),
            3
        );
        assert_eq!(
            scalar_text(
                &conn,
                &format!("SELECT name FROM {table} WHERE name = '甲' LIMIT 1")
            ),
            "甲"
        );
        drop_temp_table(&conn, TempTableSource::Query, &table).expect("删表");
    }

    /// 不支持的形状与类型：**报错点名**，不悄悄退化成文本列。
    #[test]
    fn unsupported_shapes_and_types_are_refused() {
        let mut conn = duckdb::Connection::open_in_memory().expect("内存连接");

        // 形状不一致（列数不同）
        let batch = typed_batch();
        let narrow = batch.slice(0, 1).project(&[0]).expect("只留一列");
        let err = DuckDbService::create_temp_table_from_batches(
            &mut conn,
            &[batch.clone(), narrow],
            "test",
        )
        .expect_err("形状不一致要报错");
        assert!(format!("{err:?}").contains("形状"), "{err:?}");

        // 嵌套类型（协议里这类是 JSON 字符串，真来了也先说明白）
        let list = std::sync::Arc::new(arrow::array::ListArray::from_iter_primitive::<
            arrow::datatypes::Int64Type,
            _,
            _,
        >(vec![Some(vec![Some(1i64)]), None]));
        let list_batch = RecordBatch::try_new(
            std::sync::Arc::new(Schema::new(vec![Field::new(
                "tags",
                list.data_type().clone(),
                true,
            )])),
            vec![list],
        )
        .expect("批");
        let err = DuckDbService::create_temp_table_from_batches(&mut conn, &[list_batch], "test")
            .expect_err("嵌套类型要报错");
        let message = format!("{err:?}");
        assert!(message.contains("tags"), "要点名是哪一列：{message}");

        // 一批都没有
        assert!(DuckDbService::create_temp_table_from_batches(&mut conn, &[], "test").is_err());
    }

    /// 两条路（Arrow 直灌 / 逐行 INSERT）在建出来的**数据**上必须一致。
    #[test]
    fn the_arrow_path_and_the_row_path_agree() {
        let mut conn = duckdb::Connection::open_in_memory().expect("内存连接");
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![json!(1), json!("甲")], vec![json!(2), json!("乙")]];

        let row_table = DuckDbService::create_temp_table_internal(&mut conn, &columns, &rows, "result", "test")
            .expect("逐行建表");
        let arrow_batch = {
            use arrow::array::{Int64Array, LargeStringArray};
            let schema = std::sync::Arc::new(Schema::new(vec![
                Field::new("id", DataType::Int64, false),
                Field::new("name", DataType::LargeUtf8, true),
            ]));
            RecordBatch::try_new(
                schema,
                vec![
                    std::sync::Arc::new(Int64Array::from(vec![1, 2])),
                    std::sync::Arc::new(LargeStringArray::from(vec![Some("甲"), Some("乙")])),
                ],
            )
            .expect("批")
        };
        let arrow_table = DuckDbService::create_temp_table_from_batches(&mut conn, &[arrow_batch], "test")
            .expect("直灌建表");

        let count = |table: &str| scalar_i64(&conn, &format!("SELECT count(*) FROM {table}"));
        assert_eq!(count(&row_table), count(&arrow_table));

        let names = |table: &str| {
            scalar_text(
                &conn,
                &format!("SELECT string_agg(name, ',' ORDER BY id) FROM {table}"),
            )
        };
        assert_eq!(names(&row_table), names(&arrow_table));

        drop_temp_table(&conn, TempTableSource::Query, &row_table).expect("删行表");
        drop_temp_table(&conn, TempTableSource::Query, &arrow_table).expect("删列表");
    }

    /// Decimal256（源侧 MySQL `DECIMAL(65,30)` 这类）超出 DuckDB 的 38 位上限时，
    /// 按 DOUBLE 承接并给出降级说明；128 位以内的仍是原样 DECIMAL（无损）。
    #[test]
    fn decimal256_is_taken_as_double_with_a_downgrade_note() {
        let (ty, note) =
            super::map_arrow_type(&DataType::Decimal256(65, 30)).expect("Decimal256 不再拒收");
        assert_eq!(ty, "DOUBLE");
        let note = note.expect("要带降级说明");
        assert!(note.contains("38"), "说明里要写清上限：{note}");
        assert!(note.contains("精度"), "说明里要写清代价：{note}");

        let (ty, note) = super::map_arrow_type(&DataType::Decimal128(38, 10)).expect("128 位");
        assert_eq!(ty, "DECIMAL(38,10)");
        assert!(note.is_none(), "38 位以内是无损的");
    }

    /// 直灌链路：Decimal256 列现在能建表、能写一整行 null，落成 DOUBLE。
    #[test]
    fn decimal256_column_survives_the_arrow_passthrough() {
        let mut conn = duckdb::Connection::open_in_memory().expect("内存连接");
        let dt = DataType::Decimal256(65, 30);
        let array = arrow::array::new_null_array(&dt, 1);
        let batch = RecordBatch::try_new(
            std::sync::Arc::new(Schema::new(vec![Field::new("amount", dt, true)])),
            vec![array],
        )
        .expect("批");
        let table = DuckDbService::create_temp_table_from_batches(&mut conn, &[batch], "test")
            .expect("Decimal256 直灌建表");
        assert_eq!(
            scalar_text(
                &conn,
                &format!("SELECT typeof(amount) FROM {table} LIMIT 1")
            ),
            "DOUBLE"
        );
        assert_eq!(
            scalar_i64(&conn, &format!("SELECT count(*) FROM {table}")),
            1
        );
        drop_temp_table(&conn, TempTableSource::Query, &table).expect("删表");
    }

    /// K16：结果集临时表必须带 `tmp_q_` 前缀并登记——前缀是 TTL / 上限 /
    /// 按来源清理 / 关项目清场唯一的识别依据（历史上的 `rs_<uuid>` 什么机制都识别不了）。
    #[test]
    fn test_create_temp_table_uses_query_prefix_and_registers() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec![json!(1), json!("a")]];

        let table = DuckDbService::create_duckdb_temp_table(&columns, &rows, "test").expect("建表");
        assert!(
            table.starts_with("tmp_q_"),
            "结果集临时表名要在 tmp_q_ 前缀下: {table}"
        );
        assert_eq!(
            DuckDBManager::temp_table_manager().count_by_prefix(&table),
            1,
            "建完应当被登记（按来源清理才看得见它）"
        );

        // 定向回收口能用（建表方在结果集丢弃 / 替换时调它）
        let conn = DuckDbService::get_or_create_duckdb().expect("内存连接");
        let guard = conn.lock().expect("锁");
        drop_temp_table(&guard, TempTableSource::Query, &table).expect("删表");
    }

    /// 建表即登到**分析实例**的登记册（第 3 项：`rds_tmp` 自动登记）。
    ///
    /// 登到实例的那一份才是"用户看得见、点得掉"的（面板 → `AnalysisInstance::drop_temp`）；
    /// 原来的管理器登记册继续保留（TTL / 上限 / 惰性清理仍按它算）。
    #[test]
    fn a_result_set_registers_itself_in_the_instance() {
        let table = DuckDbService::create_duckdb_temp_table(
            &["id".to_string()],
            &[vec![json!(1)]],
            "test",
        )
        .expect("建表");
        let instance = crate::duckdb::instance::current().expect("进程单例就是实例的 process 会话");
        assert!(
            instance.temps().iter().any(|entry| entry.name == table),
            "实例登记册里应有 {table}：{:?}",
            instance.temps()
        );
        assert!(
            instance.drop_temp(&table).expect("手动清理"),
            "该能销掉登记"
        );
        assert!(
            !instance.temps().iter().any(|entry| entry.name == table),
            "清理后登记不该还在"
        );
    }

    /// `rows_as_json` 直读 Arrow `batches`。
    ///
    /// 回归：旧实现走 `serde_json::to_value(&QueryResult)` 再读 `["batches"]`，
    /// 而契约序列化不含 Arrow —— 这里是「只有 batches」的最普通形态，旧路给出零行，
    /// 结果筛选建出来的临时表就是空的。
    #[test]
    fn rows_as_json_reads_arrow_batches() {
        use arrow::array::Int64Array;
        use shared::models::QueryResult;

        let batch = RecordBatch::try_new(
            std::sync::Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, false)])),
            vec![std::sync::Arc::new(Int64Array::from(vec![1, 2, 3]))],
        )
        .expect("构造 RecordBatch");
        let mut result = QueryResult::empty();
        result.columns = vec!["id".to_string()];
        result.batches = vec![batch];

        let rows = super::rows_as_json(&result);
        assert_eq!(rows.len(), 3, "行在 batches 里，直读要拿得到");
        assert_eq!(rows[0][0], serde_json::json!(1));
        assert_eq!(rows[2][0], serde_json::json!(3));
    }
}

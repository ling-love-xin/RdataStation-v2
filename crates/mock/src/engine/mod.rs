use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// 子模块
mod helpers;
mod sinks;
mod scenario;

// 导入 impl 块（Rust 允许跨文件的 impl 块，不需要 use）
// 导入辅助函数
use helpers::*;
// 对外 re-export
pub use helpers::sanitize_identifier;

use fake::Fake;
use fake::rand::SeedableRng;
use fake::rand::rngs::StdRng;

use super::generators::generate_cell;
use crate::error::{MockError, MockResult};
use crate::generators::parse_hour_minutes;
use crate::models::{
    ColumnDef, ColumnDependency, ColumnMappingResponse, GeneratorConfig, Locale, MockConfig,
    MockExportFormat, MockGenerateResult, MockScenarioResult, MockScenarioTableResult,
    ReferenceDomain, ScenarioTemplate,
};
use crate::schema_map::{ColumnMapper, parse_data_type};
use engine::duckdb::row_to_arrow::duckdb_rows_to_arrow;
use engine::duckdb::{DuckDBManager, TempTableSource};
use engine::sql::{ColumnDefInfo, QualifiedTable, SqlEngine};
use shared::models::QueryResult;

/// Mock 数据引擎 —— 在 DuckDB 内存表中生成模拟数据集
///
/// # SQL 安全性
///
/// 本模块通过 [SqlEngine](engine::sql::SqlEngine) 构造所有 DDL/DML SQL 语句，
/// 彻底消除 `format!()` 字符串拼接：
/// - **DDL**（CREATE TABLE / DROP TABLE）：通过 `SqlEngine::build_create_table` /
///   `SqlEngine::build_drop_table` 生成
/// - **DML**（INSERT）：通过 `SqlEngine::build_insert` 生成，值由 SqlEngine 负责转义
/// - **DQL**（SELECT）：通过 `SqlEngine::build_select_all` / `SqlEngine::build_select` 生成
///
/// 仅 DuckDB 专有的 `COPY` 命令保留 `format!()` 拼接（非标准 SQL）。
pub struct MockEngine;

const TEMP_MOCK_PREFIX: &str = "temp_mock_";

/// 跨库直写时的 `ATTACH` 别名（固定值：内存库里只有 mock 会挂载目标库，不会撞名）。
const ATTACH_ALIAS: &str = "rds_mock_sink";

/// 跨库直写的模式。
#[derive(Debug, Clone)]
pub enum TempTableWriteMode {
    /// 新建表：用给定列定义建表（列类型 / 唯一 / 非空是产品语义，由调用方给）
    Create(Vec<ColumnDefInfo>),
    /// 追加到既有表：调用方负责先校验「目标表存在且含临时表全部列」（报错更早、更好懂）。
    ///
    /// `unique_columns` 是本批次要保**全局唯一**的列（= 临时表里 `unique: true` 的列，
    /// 通常是隐含主键）：写入前引擎对目标表同名列做冲突预检，有冲突**整批拒绝**
    /// （DuckDB 单条 `INSERT SELECT` 本身是原子的，这里再给一次**可读**的冲突原因，
    /// 而不是让约束错误原样冒到界面上）。
    Append { unique_columns: Vec<String> },
}

/// 临时表的列名（与 `insert_statements` 同一取法：`query` 之后再读 `column_names`）。
fn temp_table_columns(conn: &duckdb::Connection, temp_table: &str) -> MockResult<Vec<String>> {
    let select_sql = SqlEngine::build_select_all(temp_table, Some(0));
    let mut stmt = conn.prepare(&select_sql)?;
    let _rows = stmt.query([])?;
    Ok(stmt.column_names().iter().map(|c| c.to_string()).collect())
}
const BATCH_SIZE: usize = 10_000;
const PREVIEW_ROWS: usize = 10;

/// 新建一个「未取消」的任务级取消令牌。
///
/// S4：取消从**进程级全局标志**改为**任务级令牌**——每个生成任务持自己的
/// `Arc<AtomicBool>`，`cancel()` 只影响当前这一个任务，不会误伤并行 / 后续任务。
pub fn new_cancel_token() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

impl MockEngine {
    // ==================== 数据生成 ====================

    /// 生成 Mock 数据（无进度回调）
    ///
    /// 根据 `MockConfig` 配置在 DuckDB 内存临时表中生成指定行数/列数的模拟数据。
    /// 内部调用 `generate_with_progress`，回调为空，取消令牌用默认（新建、未取消）。
    pub async fn generate(config: MockConfig) -> MockResult<MockGenerateResult> {
        Self::generate_with_progress(config, new_cancel_token(), |_, _| {}).await
    }

    /// 生成 Mock 数据（带进度回调）
    ///
    /// 分批次生成数据（每批 BATCH_SIZE 行），每完成一批调用 `on_progress(batch_idx, total_batches)`。
    /// 结果存储在 DuckDB 内存临时表 `temp_mock_{table_name}` 中。
    /// 返回 `MockGenerateResult` 包含预览 `QueryResult` 和耗时。
    ///
    /// S4：`cancel` 是本任务的取消令牌——宿主置位后，生成在**批次边界**响应
    /// （返回 `Err("生成已取消")`）。任务结束即可丢弃该令牌，不影响别的任务。
    pub async fn generate_with_progress<F>(
        config: MockConfig,
        cancel: Arc<AtomicBool>,
        on_progress: F,
    ) -> MockResult<MockGenerateResult>
    where
        F: Fn(usize, usize) + Send + 'static,
    {
        Self::generate_table(config, &[], cancel, on_progress).await
    }

    /// 生成一张表（内部实现）：`domains` 是本次多表生成的父表取值域，供声明了跨表引用的列采样。
    ///
    /// 引用的**校验**在 [`Self::resolve_reference_domains`]（场景级，错就报）；到了生成单表这一步，
    /// 域清单里没有匹配项只意味着一件事：**这是一次单表生成**，没有父表上下文。
    /// 此时按该列自己的 `generator` 取值（模板与面板都保证它的域落在父表域内），
    /// 而不是报错——单表生成本来就没有“对齐哪张父表”可言。
    async fn generate_table<F>(
        config: MockConfig,
        domains: &[ReferenceDomain],
        cancel: Arc<AtomicBool>,
        on_progress: F,
    ) -> MockResult<MockGenerateResult>
    where
        F: Fn(usize, usize) + Send + 'static,
    {
        if config.row_count == 0 {
            return Err(MockError::InvalidRowCount(0));
        }
        if config.columns.is_empty() {
            return Err(MockError::InvalidColumn("无列定义".to_string()));
        }

        for col in &config.columns {
            if col.nullable_ratio < 0.0 || col.nullable_ratio > 1.0 {
                return Err(MockError::InvalidColumn(format!(
                    "列 '{}' 的 nullable_ratio 必须介于 0.0~1.0，当前: {}",
                    col.name, col.nullable_ratio
                )));
            }
            if let Some(reason) = generator_param_problem(&col.generator) {
                return Err(MockError::InvalidColumn(format!(
                    "列 '{}' {reason}",
                    col.name
                )));
            }
            if col.unique {
                if let Some(reason) = unique_capacity_problem(col, config.row_count) {
                    return Err(MockError::InvalidColumn(reason));
                }
            }
        }

        let start = Instant::now();

        let mut rng: StdRng = match config.seed {
            Some(s) => StdRng::seed_from_u64(s as u64),
            None => StdRng::seed_from_u64(rand::random()),
        };

        let safe_name = sanitize_table_name(&config.table_name);
        let table_name = format!("{}{}", TEMP_MOCK_PREFIX, safe_name);

        let db = Self::get_db()?;
        let conn = Self::get_conn(&db)?;

        let drop_sql = SqlEngine::build_drop_table(&table_name, true);
        conn.execute_batch(&drop_sql)?;

        let ddl = Self::build_create_table_ddl(&table_name, &config.columns)?;
        conn.execute_batch(&ddl)?;

        let total_batches = config.row_count.div_ceil(BATCH_SIZE as u32);
        let mut unique_sets: Vec<HashSet<String>> = config
            .columns
            .iter()
            .map(|c| {
                if c.unique {
                    HashSet::with_capacity(config.row_count as usize)
                } else {
                    HashSet::new()
                }
            })
            .collect();

        let safe_col_names: Vec<String> = config
            .columns
            .iter()
            .map(|c| {
                let safe = sanitize_identifier(&c.name);
                if safe.is_empty() {
                    return Err(MockError::Generation(format!(
                        "column name '{}' resolves to empty after sanitization",
                        c.name
                    )));
                }
                Ok(safe)
            })
            .collect::<MockResult<Vec<_>>>()?;

        for batch_idx in 0..total_batches {
            if cancel.load(Ordering::Relaxed) {
                return Err(MockError::Generation("生成已取消".to_string()));
            }
            let start_row = batch_idx * BATCH_SIZE as u32;
            let count = std::cmp::min(BATCH_SIZE as u32, config.row_count - start_row);

            // 每格要么有值（`Some`），要么是 SQL NULL（`None`）。
            // 不用 `"NULL"` 字符串当哨兵：那样生成器恰好产出这四个字母时会被吞成空值。
            let mut all_values: Vec<Vec<Option<String>>> = Vec::with_capacity(count as usize);

            for row_idx in 0..count {
                let global_row = start_row + row_idx;
                let mut row_vals = Vec::with_capacity(config.columns.len());

                for (col_idx, col) in config.columns.iter().enumerate() {
                    // 空值先定：这一格要写成 NULL 就**不参与**唯一性去重——
                    // SQL 里多个 NULL 不违反唯一约束；若在取值之后才掷骰子，抽到的那个值
                    // 会白白占掉一个唯一槽，让后面本可以取到它的行反而生成失败。
                    let nullable_ratio = config.columns[col_idx].nullable_ratio;
                    if nullable_ratio > 0.0 {
                        let rand_val: f64 = (0.0..1.0).fake_with_rng(&mut rng);
                        if rand_val < nullable_ratio {
                            row_vals.push(None);
                            continue;
                        }
                    }

                    // 跨表引用列：值从父表的主键域里取（域由父列的自增参数与行数算出，不读数据）
                    let domain = col
                        .dependency
                        .as_ref()
                        .and_then(|dep| Self::domain_for(dep, domains));
                    let mut attempts = 0;
                    let value = loop {
                        let val = match domain {
                            Some(domain) if domain.count > 0 => {
                                let index = (0..domain.count).fake_with_rng::<u32, _>(&mut rng);
                                domain.value_at(index.min(domain.count - 1)).to_string()
                            }
                            Some(_) => {
                                return Err(MockError::Generation(format!(
                                    "引用目标 '{}' 的父表行数为 0，没有可采样的主键值",
                                    col.name
                                )));
                            }
                            None => generate_cell(
                                &col.generator,
                                &mut rng,
                                global_row as usize,
                                &config.locale,
                            ),
                        };
                        if !col.unique || !unique_sets[col_idx].contains(&val) {
                            if col.unique {
                                unique_sets[col_idx].insert(val.clone());
                            }
                            break val;
                        }
                        attempts += 1;
                        if attempts > 100 {
                            return Err(MockError::Generation(format!(
                                "无法为唯一列'{}'生成不重复的值",
                                col.name
                            )));
                        }
                    };

                    row_vals.push(Some(value));
                }

                all_values.push(row_vals);
            }

            // 借一层引用交给构造器：`build_insert` 只读取值，不需要所有权。
            let batch_rows: Vec<Vec<Option<&str>>> = all_values
                .iter()
                .map(|row| row.iter().map(|v| v.as_deref()).collect())
                .collect();
            let insert_sql = SqlEngine::build_insert(&table_name, &safe_col_names, &batch_rows);
            conn.execute_batch(&insert_sql)?;
            on_progress(batch_idx as usize + 1, total_batches as usize);
        }

        DuckDBManager::register_temp_table(&table_name);

        let preview = Self::read_preview(&conn, &table_name, PREVIEW_ROWS)?;
        let elapsed_ms = start.elapsed().as_millis() as u64;

        Ok(MockGenerateResult {
            table_name: config.table_name.clone(),
            temp_table_name: table_name,
            row_count: config.row_count,
            preview,
            columns: config.columns.iter().map(|c| c.name.clone()).collect(),
            elapsed_ms: elapsed_ms as u32,
        })
    }

    fn get_db() -> MockResult<Arc<Mutex<duckdb::Connection>>> {
        DuckDBManager::get_or_create_in_memory()
            .map_err(|e| MockError::Generation(format!("DuckDB error: {}", e)))
    }

    /// 取内存库连接。
    ///
    /// **锁被毒化时不报错、直接复用**：毒化只可能是上一次任务在持锁期间失败（例如生成器参数造成的空区间）。
    /// DuckDB 连接本身仍然可用（那些失败发生在拼值 / 取数之间，语句未半途提交），
    /// 而把「上一次任务的失败」升级成「本进程后续每次生成都失败」对用户毫无帮助——
    /// 实测后果就是面板之后每个任务都拿到 `poisoned lock`，只能重启应用。
    fn get_conn(
        db: &Arc<Mutex<duckdb::Connection>>,
    ) -> MockResult<std::sync::MutexGuard<'_, duckdb::Connection>> {
        Ok(db.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("Mock: 内存库连接锁曾被毒化（上一次任务在持锁期间失败），已恢复复用");
            poisoned.into_inner()
        }))
    }

    // ==================== 预览刷新 ====================

    /// 刷新临时表预览数据
    ///
    /// 从指定的 DuckDB 临时表中读取前 `limit` 行，转换为 Arrow `RecordBatch`
    /// 并封装为 `QueryResult` 返回。用于前端 ag-Grid 二次渲染。
    pub fn preview(temp_table_name: &str, limit: usize) -> MockResult<QueryResult> {
        let db = Self::get_db()?;
        let conn = Self::get_conn(&db)?;
        Self::read_preview(&conn, temp_table_name, limit)
            .map_err(|e| MockError::Preview(e.to_string()))
    }

    /// 预览的**重查取样**（只读）：取临时表的前 `limit` 行；`order` 给出按哪一列排序
    /// （`None` = 不排序，只多要几行）。
    ///
    /// 为何是重查而不是就地改：预览只装了前 N 行的取样，就地重排只会把这 N 行换个顺序
    ///（看着像「按值排过」，实际不是这一列的前 N 名）；而「多要几行」也只能由库给——
    /// 生成时只读了 `PREVIEW_ROWS` 行。重查拿到的才是全局前 N 行，与标题里的「前 N 行」同一个意思。
    ///
    /// **非阻塞**：拿不到内存库连接锁（有任务在跑 / 别的调用持锁）时返回 `Ok(None)`，
    /// 由调用方保持现状——预览取样是随手动作，不值得为它冻住 UI 等一个不可取消的出口任务。
    pub fn try_preview_sample(
        temp_table_name: &str,
        order: Option<(&str, bool)>,
        limit: usize,
    ) -> MockResult<Option<QueryResult>> {
        let db = Self::get_db()?;
        let conn = match db.try_lock() {
            Ok(guard) => guard,
            // 与 `get_conn` 同一口径：毒化只说明上一次任务在持锁期间失败过，连接本身仍可用
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                tracing::warn!("Mock: 内存库连接锁曾被毒化（预览重查），已恢复复用");
                poisoned.into_inner()
            }
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
        };
        Self::read_sample_preview(&conn, temp_table_name, order, limit)
            .map(Some)
            .map_err(|e| MockError::Preview(e.to_string()))
    }

    /// 工具栏「刷新」：重读内存临时表的行数。
    ///
    /// 测试过程中可能改过这张表（加字段 / 增删行 / 外部写入），清单行尾的数字
    /// 是生成时的快照；刷新用**当前实际行数**把清单与预览对齐。
    ///
    /// **非阻塞**：拿不到内存库连接锁（有任务在跑）时返回 `Ok(None)`，由调用方保持现状，
    /// 口径与 [`Self::try_preview_sample`] 一致（刷新是随手动作，不值得为它冻住 UI）。
    pub fn try_temp_table_row_count(temp_table_name: &str) -> MockResult<Option<i64>> {
        let db = Self::get_db()?;
        let conn = match db.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                tracing::warn!("Mock: 内存库连接锁曾被毒化（刷新重查），已恢复复用");
                poisoned.into_inner()
            }
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
        };
        let sql = format!("SELECT COUNT(*) FROM \"{temp_table_name}\"");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| MockError::Preview(e.to_string()))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| MockError::Preview(e.to_string()))?;
        let count = match rows.next().map_err(|e| MockError::Preview(e.to_string()))? {
            Some(row) => row.get(0).map_err(|e| MockError::Preview(e.to_string()))?,
            None => return Ok(None),
        };
        Ok(Some(count))
    }

    // ==================== 导出 ====================

    /// 导出临时表数据到文件
    ///
    /// 支持 CSV / Parquet / Xlsx / SQL INSERT 四种格式。
    /// 使用 DuckDB `COPY` 命令直接导出，利用原生 I/O 优化。
    /// Xlsx 格式通过 JSON 中转（DuckDB 不原生支持 xlsx）。
    pub fn export(
        temp_table_name: &str,
        format: &MockExportFormat,
        output_path: Option<&str>,
        table_name: Option<&str>,
    ) -> MockResult<String> {
        // SQL INSERT 分支走 `insert_statements`（它自己取连接）：不能在持锁期间调用它——
        // 内存库连接是 `Mutex<Connection>`，同线程重复加锁会直接死锁。
        if matches!(format, MockExportFormat::SqlInsert) {
            let path = output_path.ok_or_else(|| {
                MockError::Config("SQL INSERT export requires output_path".to_string())
            })?;
            let text = Self::insert_statements(temp_table_name, table_name)?;
            std::fs::write(path, text).map_err(|e| MockError::Export {
                format: "SQL INSERT".to_string(),
                reason: format!("Write file failed: {}", e),
            })?;
            return Ok(format!("Exported to SQL INSERT: {}", path));
        }

        let db = Self::get_db()?;
        let conn = Self::get_conn(&db)?;

        match format {
            MockExportFormat::Csv | MockExportFormat::Parquet | MockExportFormat::Xlsx => {
                let ext = match format {
                    MockExportFormat::Csv => "CSV",
                    MockExportFormat::Parquet => "PARQUET",
                    MockExportFormat::Xlsx => "XLSX",
                    _ => unreachable!(),
                };
                let path = output_path.ok_or_else(|| {
                    MockError::Config(format!("{} export requires output_path", ext))
                })?;
                let sql = format!(
                    "COPY \"{}\" TO '{}' (FORMAT {}, HEADER)",
                    temp_table_name,
                    path.replace('\\', "/"),
                    ext
                );
                conn.execute_batch(&sql)?;
                Ok(format!("Exported to {}: {}", ext, path))
            }
            MockExportFormat::Table => {
                let name = table_name.unwrap_or(temp_table_name);
                let new_name = name.trim_start_matches(TEMP_MOCK_PREFIX);
                // `build_create_table_as_select` 的第二个参数是**源表名**（内部拼 `SELECT * FROM …`），
                // 传整条 SELECT 会生成 `CREATE TABLE t AS SELECT * FROM SELECT * FROM …`（v1 遗留缺陷）。
                let create_sql = SqlEngine::build_create_table_as_select(new_name, temp_table_name);
                conn.execute_batch(&create_sql)?;
                let drop_sql = SqlEngine::build_drop_table(temp_table_name, true);
                conn.execute_batch(&drop_sql)?;
                Ok(format!("Persisted as table: {}", new_name))
            }
            // 上面已提前返回
            MockExportFormat::SqlInsert => unreachable!(),
        }
    }

    /// 生成全量 `INSERT` 语句文本（每行一条，目标表名为 `table_name` 去临时前缀）。
    ///
    /// 专给「导出 SQL 文件」用（产出一份可在别处执行的脚本）。**落库不走它**：
    /// 落库用 [`MockEngine::write_temp_table_to_database`]（ATTACH 直写，省掉文本中转）。
    ///
    /// 目标表须已存在（本函数只产 `INSERT`，不产 `CREATE TABLE`）。
    ///
    /// **调用方不得持有内存库连接锁**（`get_conn`）后再调本函数：内存库是
    /// `Mutex<Connection>`，同线程重入加锁会死锁。
    pub fn insert_statements(
        temp_table_name: &str,
        table_name: Option<&str>,
    ) -> MockResult<String> {
        let db = Self::get_db()?;
        let conn = Self::get_conn(&db)?;
        let select_sql = SqlEngine::build_select_all(temp_table_name, None);
        let mut stmt = conn.prepare(&select_sql)?;
        // 列元数据必须在 query 之后读取（duckdb-rs 时序要求）。
        let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
        {
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let mut vals = Vec::new();
                for i in 0.. {
                    match row.get::<usize, duckdb::types::Value>(i) {
                        Ok(v) => vals.push(v),
                        Err(_) => break,
                    }
                }
                data.push(vals);
            }
        }
        let columns: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect();
        let col_list = columns.join(", ");
        let target_name = table_name
            .unwrap_or(temp_table_name)
            .trim_start_matches(TEMP_MOCK_PREFIX);

        let mut statements = Vec::with_capacity(data.len());
        for vals in &data {
            let values: Vec<String> = vals.iter().map(value_to_sql_literal).collect();
            statements.push(format!(
                "INSERT INTO \"{}\" ({}) VALUES ({});",
                target_name,
                col_list,
                values.join(", ")
            ));
        }
        Ok(statements.join("\n"))
    }

    // ==================== 跨库直写（落库新路径） ====================

    /// 把内存临时表**直写**到 DuckDB 文件库的表（`ATTACH` → [建表] → `INSERT SELECT` → `DETACH`）。
    ///
    /// 与 [`MockEngine::insert_statements`]（文本中转）的区别：数据全程在 DuckDB 内部流动，
    /// 不经过 Rust 字符串，省掉「读全量 → 拼 INSERT 文本 → 目标库再解析」两跳（架构 §9-I3）。
    ///
    /// 约束与细节：
    /// - 会话开在**内存库连接**上（临时表在那边，跨库才看得见）；写入期间该连接持有目标文件锁；
    /// - 列清单取自**临时表自身**的列元数据，目标表多出的列走默认值（与旧文本路径语义一致）；
    /// - `Create` 模式插入失败时删掉刚建的表（不留半成品空表），然后无论成败都 `DETACH`；
    /// - 目标表名可以是用户给的任意名字（写 SQL 时带引号）；临时表列名留 `sanitize_identifier` 口径。
    ///
    /// **调用方不得持有内存库连接锁**后再调本函数（同 `insert_statements` 的死锁教训）。
    pub fn write_temp_table_to_database(
        db_path: &Path,
        temp_table: &str,
        target_table: &str,
        mode: TempTableWriteMode,
    ) -> MockResult<()> {
        let db = Self::get_db()?;
        let conn = Self::get_conn(&db)?;
        let target = QualifiedTable {
            catalog: ATTACH_ALIAS,
            schema: "main",
            table: target_table,
        };
        let columns = temp_table_columns(&conn, temp_table)?;

        // 上一次异常退出可能留下同名挂载：先尽力解挂（失败忽略，ATTACH 会报真错）
        let _ = conn.execute_batch(&SqlEngine::build_detach_database(ATTACH_ALIAS));
        let path_text = db_path.to_string_lossy().to_string();
        conn.execute_batch(&SqlEngine::build_attach_database(&path_text, ATTACH_ALIAS))
            .map_err(|e| MockError::Generation(format!("挂载分析库失败: {e}")))?;

        let written = (|| -> MockResult<()> {
            if let TempTableWriteMode::Create(defs) = &mode {
                // 建表失败（含同名已存在）→ 直接返回：**不能**走回滚删表分支，
                // 那时表可能是别人的（同名表已在），删掉就是数据丢失
                conn.execute_batch(&SqlEngine::build_create_table_in(&target, defs, false))?;
            }
            // S3：追加前对「要保全局唯一的列」做冲突预检——目标表同名列里若已有本次
            // 生成的值，整批拒绝（可读原因；DuckDB 单条 INSERT SELECT 本身也会整语句
            // 失败，这里把「撞了哪个值」讲清楚，而不是把约束错误原样抛出去）。
            if let TempTableWriteMode::Append { unique_columns } = &mode {
                for col in unique_columns {
                    // 列名是 `sanitize_identifier` 后的产物（不可能是空 / 含引号），
                    // 表名带引号：这里拼 SQL 是安全的。
                    let conflict = format!(
                        "SELECT 1 FROM \"{target_table}\" WHERE \"{col}\" IN \
                         (SELECT \"{col}\" FROM \"{temp_table}\") LIMIT 1"
                    );
                    let hit = conn
                        .query_row(&conflict, [], |row| row.get::<_, i32>(0))
                        .map_err(|e| {
                            MockError::Generation(format!("追加冲突预检失败（{col}）：{e}"))
                        })?;
                    if hit == 1 {
                        return Err(MockError::Generation(format!(
                            "追加被拒绝：目标表已有 {col} 与本次生成相同的值（唯一列不能重复）——\
                             请调整列参数或删掉目标表里的冲突行"
                        )));
                    }
                }
            }
            if let Err(e) = conn.execute_batch(&SqlEngine::build_insert_select(
                &target, temp_table, &columns,
            )) {
                if matches!(mode, TempTableWriteMode::Create(_)) {
                    // 建表成功但写入失败 → 回滚刚建的**空表**（不留半成品）
                    let _ = conn.execute_batch(&SqlEngine::build_drop_table_in(&target, true));
                }
                return Err(e.into());
            }
            Ok(())
        })();

        let detached = conn.execute_batch(&SqlEngine::build_detach_database(ATTACH_ALIAS));
        written?; // 写入失败优先报写入（解挂失败不拖淡根因）
        detached.map_err(|e| MockError::Generation(format!("解挂分析库失败: {e}")))?;
        Ok(())
    }

    // ==================== 临时表生命周期 ====================

    /// 删掉本进程里全部 mock 临时表（切换 / 关闭项目时由宿主调用）。返回被删表名。
    ///
    /// 为什么需要显式清：临时表建在 engine 的**进程级内存库**（`GLOBAL_DUCKDB`）里，
    /// 切项目不会自动释放。同一个目标表名重复生成会 DROP + 重建，**换名字就会逐张累积**，
    /// 长会话下一直占内存（架构 §9-I1/I2）。
    ///
    /// **调用方不得持有内存库连接锁**（本函数内部要取）。
    pub fn clear_temp_tables() -> MockResult<Vec<String>> {
        DuckDBManager::drop_in_memory_temp_tables(TempTableSource::Mock)
            .map_err(|e| MockError::Generation(e.to_string()))
    }

    /// **非阻塞**版清理：拿不到内存库连接锁时返回 `Ok(None)`（调用方稍后重试）。
    ///
    /// 用途：切项目时宿主在 **UI 线程**上清理——而出口任务（落库 / 导出）持有连接锁且不可取消
    /// （D23），同步等锁会把界面冻到任务结束。拿到锁就清，拿不到就留给「任务收尾」那一拍重试
    /// （`MockHost::take_job_done`）。
    pub fn try_clear_temp_tables() -> MockResult<Option<Vec<String>>> {
        DuckDBManager::try_drop_in_memory_temp_tables(TempTableSource::Mock)
            .map_err(|e| MockError::Generation(e.to_string()))
    }

    /// 当前进程里还留着的 mock 临时表（只读观察：测试与面板用）。
    pub fn temp_tables() -> MockResult<Vec<String>> {
        DuckDBManager::in_memory_temp_tables(TempTableSource::Mock)
            .map_err(|e| MockError::Generation(e.to_string()))
    }

    // ==================== 列名智能映射 ====================

    /// 单列智能映射
    ///
    /// 根据列名和数据类型的语义分析，自动推荐最合适的 `GeneratorConfig`。
    /// 返回 `ColumnMappingResponse` 含置信度和示例值。
    pub fn map_column(column_name: &str, data_type: &str) -> MockResult<ColumnMappingResponse> {
        let dt = parse_data_type(data_type)?;
        Ok(ColumnMapper::infer(column_name, &dt))
    }

    // ==================== 私有方法 ====================

    /// 临时表 DDL。
    ///
    /// 列名走 [`sanitize_identifier`]——插值时的列清单是**同一算法**得出的（`generate_table` 里的
    /// `safe_col_names`）。两边不一致时：源库列名里的空格 / 连字符（`Order Date`）会让 DDL 直接
    /// 语法错误，就算能建出来，`INSERT` 也会报「列不存在」。落地路径（`write_temp_table_to_database`
    /// 的 `Create`）早就按净化名建表，这里补齐最后一块。
    ///
    /// 净化后为空 / 与另一列重名在**这里**报可读错误：否则用户看到的是 DuckDB 原始报错。
    fn build_create_table_ddl(table_name: &str, columns: &[ColumnDef]) -> MockResult<String> {
        let mut col_infos: Vec<ColumnDefInfo> = Vec::with_capacity(columns.len());
        for column in columns {
            let name = sanitize_identifier(&column.name);
            if name.is_empty() {
                return Err(MockError::InvalidColumn(format!(
                    "列名 '{}' 去掉符号后为空：列名需含字母 / 数字 / 下划线",
                    column.name
                )));
            }
            if col_infos.iter().any(|def| def.name == name) {
                return Err(MockError::InvalidColumn(format!(
                    "列名 '{}' 与另一列同名（都归一到 {name}）：改一个名字",
                    column.name
                )));
            }
            col_infos.push(ColumnDefInfo {
                name,
                data_type: column.data_type.to_duckdb_type(),
                unique: column.unique,
                nullable: column.nullable_ratio > 0.0,
            });
        }

        Ok(SqlEngine::build_create_table(table_name, &col_infos, false))
    }

    /// 预览：临时表的前 `limit` 行（`SELECT *`，不排序）。
    fn read_preview(
        conn: &duckdb::Connection,
        table_name: &str,
        limit: usize,
    ) -> MockResult<QueryResult> {
        let sql = SqlEngine::build_select_all(table_name, Some(limit as i64));
        Self::read_select(conn, &sql)
    }

    /// 预览的「按列重排」：临时表**按该列排序后的前 `limit` 行**。
    ///
    /// 「排序后取前 N」而非「取前 N 再排序」：只有前者是全局视图——
    /// 「这一列最大的 10 行长什么样」答得上问题，「把手上那 10 行换个个儿」答不上。
    fn read_sample_preview(
        conn: &duckdb::Connection,
        table_name: &str,
        order: Option<(&str, bool)>,
        limit: usize,
    ) -> MockResult<QueryResult> {
        let sql = match order {
            Some((column, descending)) => {
                SqlEngine::build_select_ordered(table_name, column, descending, Some(limit as i64))
            }
            // 不排序 = 「只想看更多行」：与生成时那份取样同一条语句，只是 LIMIT 更大
            None => SqlEngine::build_select_all(table_name, Some(limit as i64)),
        };
        Self::read_select(conn, &sql)
    }

    /// 跑一条 `SELECT`，把结果整成 `QueryResult`（列名取语句的列标签）。
    ///
    /// 列名走 `stmt.column_name`（取实际结果的标签，不是我们拼的列清单）：
    /// `SELECT *` 与 `SELECT * ORDER BY …` 两条路得到同一口径的列名。
    fn read_select(conn: &duckdb::Connection, sql: &str) -> MockResult<QueryResult> {
        let mut stmt = conn.prepare(sql)?;

        let row_data: Vec<Vec<duckdb::types::Value>>;
        {
            let mut rows = stmt.query([])?;
            let mut data: Vec<Vec<duckdb::types::Value>> = Vec::new();
            while let Some(row) = rows.next()? {
                let mut cols: Vec<duckdb::types::Value> = Vec::new();
                for i in 0.. {
                    match row.get::<usize, duckdb::types::Value>(i) {
                        Ok(v) => cols.push(v),
                        Err(_) => break,
                    }
                }
                data.push(cols);
            }
            row_data = data;
        }

        let column_count = if let Some(first) = row_data.first() {
            first.len()
        } else {
            stmt.column_count()
        };

        let columns: Vec<String> = if column_count > 0 {
            (0..column_count)
                .map(|i| stmt.column_name(i).map_or("unknown", |v| v).to_string())
                .collect()
        } else {
            Vec::new()
        };

        let arrow_batch = duckdb_rows_to_arrow(&columns, &row_data)?;
        let _total = arrow_batch.num_rows();

        Ok(QueryResult {
            columns,
            batches: vec![arrow_batch],
            ..Default::default()
        })
    }
}

